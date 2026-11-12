//! Signing requests and replies with a device key (HMAC-SHA256).

use std::collections::HashMap;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{hex, unhex};

pub const DEVICE: &str = "x-carla-device";
pub const TIME: &str = "x-carla-time";
pub const NONCE: &str = "x-carla-nonce";
pub const SIGN: &str = "x-carla-sign";

/// How far apart the two clocks may be, and how long a nonce is remembered.
pub const WINDOW: i64 = 300;

fn mac(key: &[u8], text: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(text.as_bytes());
    mac
}

fn request_text(method: &str, path: &str, time: i64, nonce: &str, body: &[u8]) -> String {
    format!("carla-v1\n{}\n{path}\n{time}\n{nonce}\n{}", method.to_ascii_uppercase(), hex(&Sha256::digest(body)))
}

fn reply_text(nonce: &str, status: u16, body: &[u8]) -> String {
    format!("carla-v1-reply\n{nonce}\n{status}\n{}", hex(&Sha256::digest(body)))
}

pub fn request(key: &[u8], method: &str, path: &str, time: i64, nonce: &str, body: &[u8]) -> String {
    hex(&mac(key, &request_text(method, path, time, nonce, body)).finalize().into_bytes())
}

pub fn request_ok(key: &[u8], method: &str, path: &str, time: i64, nonce: &str, body: &[u8], sign: &str) -> bool {
    unhex(sign).is_some_and(|s| mac(key, &request_text(method, path, time, nonce, body)).verify_slice(&s).is_ok())
}

/// A reply is bound to the nonce of its request, so an old reply cannot be
/// played back as the answer to a new question.
pub fn reply(key: &[u8], nonce: &str, status: u16, body: &[u8]) -> String {
    hex(&mac(key, &reply_text(nonce, status, body)).finalize().into_bytes())
}

pub fn reply_ok(key: &[u8], nonce: &str, status: u16, body: &[u8], sign: &str) -> bool {
    unhex(sign).is_some_and(|s| mac(key, &reply_text(nonce, status, body)).verify_slice(&s).is_ok())
}

/// Remembers recent nonces so a captured request cannot be sent again.
#[derive(Default)]
pub struct Replay {
    seen: HashMap<String, i64>,
}

impl Replay {
    /// True the first time a nonce is seen with a time close to ours.
    pub fn fresh(&mut self, nonce: &str, time: i64, now: i64) -> bool {
        if (now - time).abs() > WINDOW || nonce.len() < 16 || nonce.len() > 64 {
            return false;
        }
        self.seen.retain(|_, t| (now - *t).abs() <= WINDOW);
        self.seen.insert(nonce.to_string(), time).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_covers_every_part_of_the_request() {
        let key = b"device key";
        let sign = request(key, "post", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{}");
        assert!(request_ok(key, "POST", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{}", &sign));
        assert!(!request_ok(b"other key", "POST", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{}", &sign));
        assert!(!request_ok(key, "GET", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{}", &sign));
        assert!(!request_ok(key, "POST", "/v1/status", 100, "nonce-nonce-nonce", b"{}", &sign));
        assert!(!request_ok(key, "POST", "/v1/stream/prepare", 101, "nonce-nonce-nonce", b"{}", &sign));
        assert!(!request_ok(key, "POST", "/v1/stream/prepare", 100, "nonce-nonce-nonc2", b"{}", &sign));
        assert!(!request_ok(key, "POST", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{ }", &sign));
        assert!(!request_ok(key, "POST", "/v1/stream/prepare", 100, "nonce-nonce-nonce", b"{}", "zz"));
    }

    #[test]
    fn a_reply_only_answers_its_own_request() {
        let key = b"device key";
        let sign = reply(key, "nonce-a", 200, b"ok");
        assert!(reply_ok(key, "nonce-a", 200, b"ok", &sign));
        assert!(!reply_ok(key, "nonce-b", 200, b"ok", &sign));
        assert!(!reply_ok(key, "nonce-a", 500, b"ok", &sign));
        assert!(!reply_ok(key, "nonce-a", 200, b"no", &sign));
    }

    #[test]
    fn a_request_cannot_be_sent_twice_or_late() {
        let mut replay = Replay::default();
        assert!(replay.fresh("0123456789abcdef", 1000, 1000));
        assert!(!replay.fresh("0123456789abcdef", 1000, 1001));
        assert!(!replay.fresh("fedcba9876543210", 1000, 1000 + WINDOW + 1));
        assert!(!replay.fresh("short", 1000, 1000));
        assert!(replay.fresh("fedcba9876543210", 2000, 2000));
        assert_eq!(replay.seen.len(), 1, "old nonces are forgotten");
    }
}
