//! Measuring the path a stream takes, with the kind of packets a stream is
//! made of.
//!
//! Sunshine sends its picture as small UDP packets, and what a player feels
//! is how long those take, how evenly they arrive and how many are lost. A
//! web request says little about that: it rides on TCP, which hides loss by
//! sending again and adds a handshake to every delay. So the companion
//! answers two kinds of UDP packet on its own port:
//!
//! - **Echo.** The Ally sends numbered packets the size of a video packet,
//!   the desktop sends each straight back. From the times: the delay there
//!   and back, how much it wavers, and how many never returned.
//! - **Burst.** The Ally asks, over the signed link, for a second of packets
//!   at the rate a picture would need. The desktop sends them and the Ally
//!   counts what arrives. That is the nearest thing to a stream without
//!   starting one.
//!
//! An echo is answered for anyone, like the search for desktops: the answer
//! is exactly as large as the question, so it cannot be used to flood a
//! third party. A burst is many packets for one request, so it is only sent
//! to a linked Ally, to the address its signed request came from.

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Starts every echo packet. The desktop sends it back unchanged.
pub const ECHO: &[u8; 8] = b"carla~e1";
/// Starts every packet of a burst.
pub const BURST: &[u8; 8] = b"carla~b1";
/// The size of the packets: about a video packet, and under the smallest
/// packet size Tailscale carries whole.
pub const PACKET: usize = 1200;
const HEADER: usize = 16;

/// The most a burst may ask for.
pub const MOST_KBPS: u32 = 150_000;
pub const LONGEST_MS: u32 = 1_500;

fn packet(magic: &[u8; 8], seq: u32, total: u32) -> [u8; PACKET] {
    let mut out = [0u8; PACKET];
    out[..8].copy_from_slice(magic);
    out[8..12].copy_from_slice(&seq.to_be_bytes());
    out[12..16].copy_from_slice(&total.to_be_bytes());
    // Filling that does not shrink if something on the way compresses.
    let mut state = 0x9E37_79B9u32 ^ seq.wrapping_mul(0x85EB_CA6B);
    for byte in &mut out[HEADER..] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = state as u8;
    }
    out
}

fn numbers(bytes: &[u8], magic: &[u8; 8]) -> Option<(u32, u32)> {
    if bytes.len() < HEADER || &bytes[..8] != magic {
        return None;
    }
    let seq = u32::from_be_bytes(bytes[8..12].try_into().ok()?);
    let total = u32::from_be_bytes(bytes[12..16].try_into().ok()?);
    Some((seq, total))
}

/// Is this a packet the desktop should send straight back?
pub fn is_echo(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && bytes.len() <= PACKET && &bytes[..8] == ECHO
}

/// What came of sending numbered packets and timing their return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Echo {
    pub sent: u32,
    pub lost: u32,
    /// There and back, in millionths of a second: the quickest packet, the
    /// middle one, and the one only a twentieth were slower than.
    pub best_us: u32,
    pub median_us: u32,
    pub p95_us: u32,
    /// How much the delay changes from one packet to the next, on average.
    /// This is what makes a stream stutter.
    pub jitter_us: u32,
}

impl Echo {
    /// Lost packets in every thousand.
    pub fn loss_permille(&self) -> u32 {
        if self.sent == 0 {
            0
        } else {
            (u64::from(self.lost) * 1000 / u64::from(self.sent)) as u32
        }
    }
}

fn micros(d: Duration) -> u32 {
    d.as_micros().min(u128::from(u32::MAX)) as u32
}

/// Work out the figures from each packet's time there and back, in the
/// order they were sent; `None` for one that never returned.
pub fn summarise(times: &[Option<Duration>]) -> Option<Echo> {
    let mut got: Vec<u32> = times.iter().flatten().map(|d| micros(*d)).collect();
    if got.is_empty() {
        return None;
    }
    // Between packets that both returned, in the order they were sent.
    let steps: Vec<u32> = times.windows(2).filter_map(|w| Some(micros(w[0]?).abs_diff(micros(w[1]?)))).collect();
    let jitter = if steps.is_empty() { 0 } else { (steps.iter().map(|s| u64::from(*s)).sum::<u64>() / steps.len() as u64) as u32 };
    got.sort_unstable();
    let at = |part: usize| got[((got.len() - 1) * part + 50) / 100];
    Some(Echo { sent: times.len() as u32, lost: (times.len() - got.len()) as u32, best_us: got[0], median_us: at(50), p95_us: at(95), jitter_us: jitter })
}

/// Send `count` packets, one every `gap`, and time their return. `None`
/// when not one came back: the desktop is from before this existed, or
/// something on the way drops the packets.
pub fn echo(target: SocketAddr, count: u32, gap: Duration) -> Option<Echo> {
    let socket = UdpSocket::bind(if target.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).ok()?;
    echo_on(&socket, target, count, gap)
}

/// The same on a socket the caller keeps, so a burst can follow on it.
pub fn echo_on(socket: &UdpSocket, target: SocketAddr, count: u32, gap: Duration) -> Option<Echo> {
    let count = count.clamp(1, 1000);
    let mut sent: Vec<Instant> = Vec::with_capacity(count as usize);
    let mut times: Vec<Option<Duration>> = vec![None; count as usize];
    let mut buffer = [0u8; PACKET + 64];
    let started = Instant::now();
    // After the last packet, wait for stragglers: longer than any delay a
    // stream could be played over.
    let patience = Duration::from_millis(500);
    let mut arrived = 0u32;
    loop {
        let now = Instant::now();
        let next_due = started + gap * sent.len() as u32;
        if (sent.len() as u32) < count && now >= next_due {
            let _ = socket.send_to(&packet(ECHO, sent.len() as u32, count), target);
            sent.push(Instant::now());
            continue;
        }
        let wait = if (sent.len() as u32) < count {
            next_due.saturating_duration_since(now)
        } else {
            let end = *sent.last()? + patience;
            if arrived == count || now >= end {
                break;
            }
            end - now
        };
        let _ = socket.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
        if let Ok((len, from)) = socket.recv_from(&mut buffer) {
            let back = Instant::now();
            if from.ip() != target.ip() {
                continue;
            }
            if let Some((seq, _)) = numbers(&buffer[..len], ECHO) {
                if let (Some(at), Some(slot)) = (sent.get(seq as usize), times.get_mut(seq as usize)) {
                    if slot.is_none() {
                        *slot = Some(back.duration_since(*at));
                        arrived += 1;
                    }
                }
            }
        }
    }
    summarise(&times)
}

/// What a linked Ally asks for: packets at this rate for this long, sent
/// to the port its echo packets came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstAsk {
    pub port: u16,
    pub kbps: u32,
    pub ms: u32,
}

/// The desktop's answer: what it is about to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstPlan {
    pub packets: u32,
    pub ms: u32,
}

/// What arrived of a burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Burst {
    /// The rate that was asked for, and the rate that arrived.
    pub asked_kbps: u32,
    pub got_kbps: u32,
    pub sent: u32,
    pub lost: u32,
}

impl Burst {
    pub fn loss_permille(&self) -> u32 {
        if self.sent == 0 {
            0
        } else {
            (u64::from(self.lost) * 1000 / u64::from(self.sent)) as u32
        }
    }
}

/// How many packets make up a burst.
pub fn plan(ask: BurstAsk) -> Option<BurstPlan> {
    if ask.port == 0 || ask.kbps == 0 || ask.kbps > MOST_KBPS || ask.ms == 0 || ask.ms > LONGEST_MS {
        return None;
    }
    let bytes = u64::from(ask.kbps) * 1000 / 8 * u64::from(ask.ms) / 1000;
    Some(BurstPlan { packets: (bytes / PACKET as u64).max(1) as u32, ms: ask.ms })
}

/// The desktop's side: send the packets evenly over the time. Blocks for
/// that long.
pub fn send_burst(socket: &UdpSocket, to: SocketAddr, plan: BurstPlan) {
    let started = Instant::now();
    let length = Duration::from_millis(u64::from(plan.ms));
    let mut sent = 0u32;
    while sent < plan.packets {
        // Everything that is due by now. A sleep is longer than it says on
        // some machines, which makes the sending come in small bunches: so
        // does video, a picture at a time.
        let due = if started.elapsed() >= length { plan.packets } else { (u128::from(plan.packets) * started.elapsed().as_micros() / length.as_micros().max(1)) as u32 + 1 };
        while sent < due.min(plan.packets) {
            let _ = socket.send_to(&packet(BURST, sent, plan.packets), to);
            sent += 1;
        }
        if sent < plan.packets {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The Ally's side: count what arrives of a burst the desktop said it is
/// sending. `latency` is the delay there and back, for knowing when to stop
/// waiting.
pub fn receive_burst(socket: &UdpSocket, from: SocketAddr, plan: BurstPlan, asked_kbps: u32, latency: Duration) -> Burst {
    let length = Duration::from_millis(u64::from(plan.ms));
    let end = Instant::now() + length + latency + Duration::from_millis(400);
    let mut seen = vec![false; plan.packets as usize];
    let mut buffer = [0u8; PACKET + 64];
    let (mut got, mut first, mut last) = (0u32, None::<Instant>, None::<Instant>);
    while got < plan.packets {
        let now = Instant::now();
        // Nothing for a while after the last one: the rest is lost.
        let quiet = last.is_some_and(|l| now.duration_since(l) > latency + Duration::from_millis(300)) && first.is_some_and(|f| now.duration_since(f) >= length);
        if now >= end || quiet {
            break;
        }
        let _ = socket.set_read_timeout(Some((end - now).min(Duration::from_millis(100))));
        if let Ok((len, sender)) = socket.recv_from(&mut buffer) {
            if sender.ip() != from.ip() {
                continue;
            }
            if let Some((seq, total)) = numbers(&buffer[..len], BURST) {
                if total == plan.packets && seen.get(seq as usize) == Some(&false) {
                    seen[seq as usize] = true;
                    got += 1;
                    let at = Instant::now();
                    first.get_or_insert(at);
                    last = Some(at);
                }
            }
        }
    }
    // What arrived, over the time the desktop took to send it. A burst that
    // arrived whole therefore comes out at what was asked for.
    let got_kbps = (u64::from(got) * PACKET as u64 * 8 / u64::from(plan.ms.max(1))) as u32;
    Burst { asked_kbps, got_kbps: got_kbps.min(asked_kbps), sent: plan.packets, lost: plan.packets - got }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// A desktop that echoes, dropping every packet whose number `drop` likes.
    fn desktop(drop: fn(u32) -> bool) -> SocketAddr {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 2048];
            while let Ok((len, from)) = socket.recv_from(&mut buffer) {
                if is_echo(&buffer[..len]) && !drop(numbers(&buffer[..len], ECHO).unwrap().0) {
                    let _ = socket.send_to(&buffer[..len], from);
                }
            }
        });
        address
    }

    #[test]
    fn packets_are_timed_there_and_back() {
        let got = echo(desktop(|_| false), 30, Duration::from_millis(2)).unwrap();
        assert_eq!((got.sent, got.lost, got.loss_permille()), (30, 0, 0));
        assert!(got.best_us <= got.median_us && got.median_us <= got.p95_us);
        assert!(got.median_us < 50_000, "this machine answers itself quickly: {got:?}");
    }

    #[test]
    fn lost_packets_are_counted_and_silence_is_no_measurement() {
        let got = echo(desktop(|seq| seq % 4 == 0), 20, Duration::from_millis(2)).unwrap();
        assert_eq!((got.sent, got.lost, got.loss_permille()), (20, 5, 250));
        assert_eq!(echo(desktop(|_| true), 3, Duration::from_millis(1)), None);
    }

    #[test]
    fn the_figures_come_from_the_times() {
        let ms = |n: u64| Some(Duration::from_millis(n));
        let e = summarise(&[ms(4), ms(6), None, ms(5), ms(5), ms(20)]).unwrap();
        assert_eq!((e.sent, e.lost), (6, 1));
        assert_eq!((e.best_us, e.median_us, e.p95_us), (4_000, 5_000, 20_000));
        // Steps of 2, 0 and 15 between neighbours that both returned.
        assert_eq!(e.jitter_us, 17_000 / 3);
        assert_eq!(summarise(&[None, None]), None);
        assert_eq!(summarise(&[ms(3)]).unwrap().jitter_us, 0);
    }

    #[test]
    fn only_echo_packets_of_a_sane_size_are_sent_back() {
        assert!(is_echo(&packet(ECHO, 1, 2)));
        assert!(!is_echo(&packet(BURST, 1, 2)));
        assert!(!is_echo(b"carla~e1"), "too short to carry a number");
        assert!(!is_echo(&[&ECHO[..], &[0u8; 2000][..]].concat()), "never more than a packet");
        assert!(!is_echo(crate::link::discover::QUESTION));
    }

    #[test]
    fn a_burst_arrives_at_the_rate_that_was_asked_for() {
        let ally = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let desktop = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let (to, from) = (ally.local_addr().unwrap(), desktop.local_addr().unwrap());
        let plan = plan(BurstAsk { port: to.port(), kbps: 8_000, ms: 200 }).unwrap();
        assert_eq!(plan.packets, 8_000 * 1000 / 8 / 5 / PACKET as u32);
        let sending = std::thread::spawn(move || {
            let started = Instant::now();
            send_burst(&desktop, to, plan);
            started.elapsed()
        });
        let got = receive_burst(&ally, from, plan, 8_000, Duration::from_millis(1));
        assert!(sending.join().unwrap() >= Duration::from_millis(190), "spread over the time, not all at once");
        assert_eq!((got.sent, got.lost), (plan.packets, 0), "{got:?}");
        assert!(got.got_kbps > 7_500 && got.got_kbps <= 8_000, "{got:?}");
    }

    #[test]
    fn a_burst_that_is_partly_lost_says_so() {
        let ally = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let desktop = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let (to, from) = (ally.local_addr().unwrap(), desktop.local_addr().unwrap());
        let whole = BurstPlan { packets: 40, ms: 50 };
        // The desktop sends only the first half of what it announced.
        for seq in 0..20 {
            let _ = desktop.send_to(&packet(BURST, seq, 40), to);
        }
        let got = receive_burst(&ally, from, whole, 7_680, Duration::from_millis(1));
        assert_eq!((got.sent, got.lost, got.loss_permille()), (40, 20, 500));
        assert_eq!(got.got_kbps, 3_840);
    }

    #[test]
    fn nobody_can_ask_for_a_flood() {
        assert!(plan(BurstAsk { port: 1, kbps: MOST_KBPS, ms: LONGEST_MS }).is_some());
        assert_eq!(plan(BurstAsk { port: 1, kbps: MOST_KBPS + 1, ms: 100 }), None);
        assert_eq!(plan(BurstAsk { port: 1, kbps: 1000, ms: LONGEST_MS + 1 }), None);
        assert_eq!(plan(BurstAsk { port: 0, kbps: 1000, ms: 100 }), None);
        assert_eq!(plan(BurstAsk { port: 1, kbps: 0, ms: 100 }), None);
    }
}
