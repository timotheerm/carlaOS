//! Linking an Ally to a desktop with a six-digit code.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

use super::{hex, random, unhex};

pub const CODE_LENGTH: usize = 6;
/// How long a code shown on the desktop can be used.
pub const CODE_LIFETIME: i64 = 600;
/// Wrong tries before a code is thrown away.
pub const CODE_TRIES: u32 = 5;

const ALLY: &[u8] = b"carlaos-ally";
const DESKTOP: &[u8] = b"carlaos-desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ally,
    Desktop,
}

pub fn valid_code(code: &str) -> bool {
    code.len() == CODE_LENGTH && code.bytes().all(|b| b.is_ascii_digit())
}

pub fn new_code() -> String {
    let bytes = random(8);
    let n = u64::from_le_bytes(bytes.try_into().expect("eight bytes"));
    format!("{:06}", n % 1_000_000)
}

/// One side's half of the exchange, waiting for the other's message.
pub struct Exchange {
    state: Spake2<Ed25519Group>,
    side: Side,
    mine: Vec<u8>,
}

impl Exchange {
    pub fn start(side: Side, code: &str) -> Self {
        let (password, ally, desktop) = (Password::new(code.as_bytes()), Identity::new(ALLY), Identity::new(DESKTOP));
        let (state, mine) = match side {
            Side::Ally => Spake2::<Ed25519Group>::start_a(&password, &ally, &desktop),
            Side::Desktop => Spake2::<Ed25519Group>::start_b(&password, &ally, &desktop),
        };
        Self { state, side, mine }
    }

    /// The message to send to the other side, hex.
    pub fn message(&self) -> String {
        hex(&self.mine)
    }

    /// Combine with the other side's message. The result only matches the
    /// other side's when both used the same code; [`Shared::proof`] shows it.
    pub fn finish(self, theirs: &str) -> Option<Shared> {
        let theirs = unhex(theirs)?;
        let key = self.state.finish(&theirs).ok()?;
        let (ally, desktop) = match self.side {
            Side::Ally => (self.mine, theirs),
            Side::Desktop => (theirs, self.mine),
        };
        Some(Shared { key, transcript: format!("{}:{}", hex(&ally), hex(&desktop)) })
    }
}

/// What both sides hold after the exchange.
pub struct Shared {
    key: Vec<u8>,
    transcript: String,
}

impl Shared {
    fn derive(&self, label: &str) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC takes a key of any length");
        mac.update(format!("carlaos-enrol-v1:{label}:{}", self.transcript).as_bytes());
        mac
    }

    /// What `side` sends to show it holds the key.
    pub fn proof(&self, side: Side) -> String {
        hex(&self.derive(if side == Side::Ally { "proof-ally" } else { "proof-desktop" }).finalize().into_bytes())
    }

    pub fn proof_ok(&self, side: Side, proof: &str) -> bool {
        let label = if side == Side::Ally { "proof-ally" } else { "proof-desktop" };
        unhex(proof).is_some_and(|p| self.derive(label).verify_slice(&p).is_ok())
    }

    /// The key every later request is signed with.
    pub fn device_key(&self) -> Vec<u8> {
        self.derive("device-key").finalize().into_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(ally_code: &str, desktop_code: &str) -> (Shared, Shared) {
        let ally = Exchange::start(Side::Ally, ally_code);
        let desktop = Exchange::start(Side::Desktop, desktop_code);
        let (a, d) = (ally.message(), desktop.message());
        (ally.finish(&d).unwrap(), desktop.finish(&a).unwrap())
    }

    #[test]
    fn the_same_code_gives_both_sides_the_same_key() {
        let (ally, desktop) = exchange("482913", "482913");
        assert!(desktop.proof_ok(Side::Ally, &ally.proof(Side::Ally)));
        assert!(ally.proof_ok(Side::Desktop, &desktop.proof(Side::Desktop)));
        assert_eq!(ally.device_key(), desktop.device_key());
        assert_eq!(ally.device_key().len(), 32);
        // A proof cannot be bounced back as the other side's.
        assert!(!ally.proof_ok(Side::Desktop, &ally.proof(Side::Ally)));
    }

    #[test]
    fn a_wrong_code_proves_nothing() {
        let (ally, desktop) = exchange("482913", "482914");
        assert!(!desktop.proof_ok(Side::Ally, &ally.proof(Side::Ally)));
        assert_ne!(ally.device_key(), desktop.device_key());
    }

    #[test]
    fn nonsense_messages_are_refused() {
        assert!(Exchange::start(Side::Ally, "000000").finish("not hex").is_none());
        assert!(Exchange::start(Side::Ally, "000000").finish("00ff").is_none());
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..50 {
            assert!(valid_code(&new_code()));
        }
        assert!(!valid_code("12345"));
        assert!(!valid_code("12345a"));
    }
}
