//! The link between the Ally and a desktop companion.
//!
//! A desktop is linked once, with a six-digit code shown on the desktop. The
//! two sides run a password-authenticated key exchange (SPAKE2) over that code,
//! so the code is never sent and someone listening on the network gets one
//! guess per attempt, not a million. Each side keeps the resulting device key.
//! Every later request is signed with it, and so is every reply, so the desktop
//! only obeys linked devices and the Ally only believes the desktop it linked.
//!
//! Requests are signed, not encrypted: what travels is game names, status and
//! save files. Between locations the traffic goes through Tailscale, which
//! encrypts it.

use serde::{Deserialize, Serialize};

pub mod client;
pub mod discover;
pub mod enrol;
pub mod moonlight;
pub mod quality;
pub mod sign;
pub mod wake;

/// TCP port of the companion API and UDP port for finding companions.
pub const PORT: u16 = 47600;

/// The single Sunshine application every stream goes through.
pub const SUNSHINE_APP: &str = "CarlaOS";

/// What a companion says about itself to anyone on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub name: String,
    /// Stable id of this desktop, chosen at set-up.
    pub id: String,
    pub version: String,
    pub api_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// A game can be streamed now.
    Ready,
    /// A game is already running there.
    Busy,
    /// The desktop is on but streaming is not working. See `problem`.
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    pub id: String,
    pub version: String,
    pub state: State,
    /// Title of the game running there, if any.
    pub playing: Option<String>,
    /// One line for the administrator when `state` is `not_ready`.
    pub problem: Option<String>,
    /// Processor and memory, for the desktop's card.
    pub hardware: String,
    /// Every address the desktop can be reached at: home network, Tailscale.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Hardware addresses of its network adapters, for waking it.
    #[serde(default)]
    pub macs: Vec<String>,
}

/// What a desktop answers when it is asked to update itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Updating {
    /// False when the desktop already has the newest version.
    pub updating: bool,
    /// The version it is moving to, or the one it has.
    pub version: String,
}

/// One game installed on a desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteGame {
    /// Catalogue id, the same on every machine (`steam:<app id>`, or
    /// `manual:<key>` for a program the administrator added there).
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub has_cover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolStart {
    pub device_name: String,
    /// The Ally's key-exchange message, hex.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolOffer {
    pub enrol_id: String,
    /// The desktop's key-exchange message, hex.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolFinish {
    pub enrol_id: String,
    /// Proof that the Ally derived the same key, hex.
    pub proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolDone {
    pub device_id: String,
    /// Proof that the desktop derived the same key, hex.
    pub proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prepare {
    pub game_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    /// The four digits Moonlight is pairing with.
    pub pin: String,
    /// Name the Ally gets in Sunshine's list of clients.
    pub name: String,
}

/// Asks a desktop for its copy of one game's saves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavesFind {
    pub game_id: String,
    /// A game added by hand has a different id on each machine, so the name
    /// is the fallback.
    pub title: String,
}

/// A desktop's save folder for a game, as it is now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavesFound {
    /// Names the folder in the requests that follow.
    pub key: String,
    pub manifest: crate::library::sync::Manifest,
}

/// Names one file of a save folder. Also the first line of a file being
/// written, followed by a line break and the contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub path: String,
    /// Unix seconds it was saved, when writing.
    #[serde(default)]
    pub modified: i64,
}

/// Body of every reply that is not a success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub error: String,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// Random bytes from the operating system.
pub fn random(len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    bytes
}

pub fn random_hex(len: usize) -> String {
    hex(&random(len))
}
