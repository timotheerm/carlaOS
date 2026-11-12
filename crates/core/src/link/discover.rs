//! Finding companions on the local network.
//!
//! The Ally shouts one small packet to everyone on the network; each companion
//! answers with its name. Nothing here is trusted: it only saves typing an
//! address. Trust comes from the code entered afterwards.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use super::{Hello, PORT};

pub const QUESTION: &[u8] = b"carlaos? v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Found {
    pub address: String,
    pub hello: Hello,
}

/// Ask who is there and collect answers for `wait`.
pub fn search(wait: Duration) -> Vec<Found> {
    search_on(SocketAddr::from((Ipv4Addr::BROADCAST, PORT)), wait)
}

pub fn search_on(target: SocketAddr, wait: Duration) -> Vec<Found> {
    let mut found = BTreeMap::new();
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else { return Vec::new() };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let end = Instant::now() + wait;
    let mut asked = None::<Instant>;
    let mut buffer = [0u8; 1024];
    while Instant::now() < end {
        // Ask again every second: a single packet on Wi-Fi is easily lost.
        if asked.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            let _ = socket.send_to(QUESTION, target);
            asked = Some(Instant::now());
        }
        if let Ok((len, from)) = socket.recv_from(&mut buffer) {
            if let Ok(hello) = serde_json::from_slice::<Hello>(&buffer[..len]) {
                found.insert(hello.id.clone(), Found { address: from.ip().to_string(), hello });
            }
        }
    }
    found.into_values().collect()
}

/// The companion's side: answer every question with `hello`, for ever.
pub fn answer(socket: &UdpSocket, hello: &Hello) {
    let reply = serde_json::to_vec(hello).expect("Hello is always serialisable");
    let mut buffer = [0u8; 64];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((len, from)) if &buffer[..len] == QUESTION => {
                let _ = socket.send_to(&reply, from);
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::ConnectionReset) => {}
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_companion_answers_a_search() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let target = socket.local_addr().unwrap();
        let hello = Hello { name: "HOME PC".into(), id: "abc".into(), version: "0.1.0".into(), api_version: 1 };
        let theirs = hello.clone();
        std::thread::spawn(move || answer(&socket, &theirs));
        let found = search_on(target, Duration::from_millis(600));
        assert_eq!(found, [Found { address: "127.0.0.1".into(), hello }]);
    }
}
