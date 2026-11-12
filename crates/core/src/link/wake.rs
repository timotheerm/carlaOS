//! Waking a sleeping desktop, and choosing which of its addresses to use.
//!
//! Waking is Wake-on-LAN: one "magic packet" shouted on the local network,
//! which a sleeping network adapter recognises as its own name repeated. It
//! only works from the same network as the desktop, on a wired adapter that
//! has waking switched on. Nothing in it is secret or trusted.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// Parse `aa:bb:cc:dd:ee:ff` (or with dashes).
pub fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let mut mac = [0u8; 6];
    let mut parts = text.split([':', '-']);
    for byte in &mut mac {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    (parts.next().is_none() && mac != [0; 6]).then_some(mac)
}

/// Six bytes of 0xFF, then the adapter's address sixteen times.
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0xFF; 102];
    for chunk in packet[6..].chunks_exact_mut(6) {
        chunk.copy_from_slice(&mac);
    }
    packet
}

/// Shout the packet for every adapter. Returns how many could be sent.
pub fn wake(macs: &[String]) -> usize {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else { return 0 };
    if socket.set_broadcast(true).is_err() {
        return 0;
    }
    macs.iter()
        .filter_map(|m| parse_mac(m))
        .filter(|mac| {
            let packet = magic_packet(*mac);
            // Both customary ports, twice: a lost packet costs the player a minute.
            let sent: usize = [9u16, 7, 9, 7].iter().filter(|port| socket.send_to(&packet, (Ipv4Addr::BROADCAST, **port)).is_ok()).count();
            sent > 0
        })
        .count()
}

/// Tailscale gives every device an address in 100.64.0.0/10.
pub fn is_tailscale(address: &str) -> bool {
    matches!(address.parse::<IpAddr>(), Ok(IpAddr::V4(ip)) if ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
}

/// Is this an address another machine could use to reach the desktop?
pub fn usable(address: &str) -> bool {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => !(ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast()),
        // IPv6 is left out for now: Moonlight and Sunshine pair by IPv4 here.
        _ => false,
    }
}

/// The addresses to try, best first: the one that worked last, then the home
/// network, then Tailscale. No address appears twice.
pub fn candidates(last: &str, reported: &[String]) -> Vec<String> {
    let mut out = vec![last.to_string()];
    let mut rest: Vec<&String> = reported.iter().filter(|a| usable(a)).collect();
    rest.sort_by_key(|a| is_tailscale(a));
    for address in rest {
        if !out.contains(address) {
            out.push(address.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_magic_packet_is_the_address_sixteen_times() {
        let mac = parse_mac("AA:bb:0c:1D:2e:3F").unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0x0C, 0x1D, 0x2E, 0x3F]);
        assert_eq!(parse_mac("aa-bb-0c-1d-2e-3f"), Some(mac));
        let packet = magic_packet(mac);
        assert_eq!(packet[..6], [0xFF; 6]);
        assert!(packet[6..].chunks(6).all(|c| c == mac));
        assert_eq!(packet[6..].chunks(6).count(), 16);
    }

    #[test]
    fn nonsense_addresses_wake_nothing() {
        for bad in ["", "aa:bb:cc:dd:ee", "aa:bb:cc:dd:ee:ff:00", "aa:bb:cc:dd:ee:zz", "aabb:cc:dd:ee:ff", "00:00:00:00:00:00"] {
            assert_eq!(parse_mac(bad), None, "{bad}");
        }
        assert_eq!(wake(&["nonsense".to_string()]), 0);
    }

    #[test]
    fn the_home_network_is_tried_before_tailscale() {
        assert!(is_tailscale("100.101.102.103"));
        assert!(!is_tailscale("100.1.2.3"));
        assert!(!is_tailscale("192.168.1.20"));
        let reported = ["127.0.0.1", "100.90.1.2", "169.254.3.4", "192.168.1.20", "fe80::1", "10.0.0.7"].map(String::from);
        assert_eq!(candidates("192.168.1.20", &reported), ["192.168.1.20", "10.0.0.7", "100.90.1.2"]);
        // Away from home the Tailscale address worked last, so it goes first.
        assert_eq!(candidates("100.90.1.2", &reported), ["100.90.1.2", "192.168.1.20", "10.0.0.7"]);
        assert_eq!(candidates("home-pc.local", &[]), ["home-pc.local"]);
    }
}
