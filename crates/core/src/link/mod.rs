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

pub mod check;
pub mod client;
pub mod codec;
pub mod deadline;
pub mod discover;
pub mod enrol;
pub mod health;
pub mod moonlight;
pub mod probe;
pub mod quality;
pub mod sign;
pub mod unlinked;
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
    /// Someone at the desktop has opened linking: an Ally can be linked by
    /// being allowed there, without a code being typed.
    #[serde(default)]
    pub approve: bool,
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
    /// Names of its graphics chips, for knowing which codec a stream gets.
    #[serde(default)]
    pub graphics: Vec<String>,
}

/// Asks what screen the desktop would give a stream of this size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitAsk {
    pub screen: crate::display::Mode,
}

/// What screen a stream would be shown on at the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenFit {
    /// The stream gets a virtual screen of its own, of exactly its size.
    pub virtual_screen: bool,
    /// The mode the desktop's screen would take. `None`: it has nothing
    /// suitable and stays as it is, so the picture is scaled.
    pub mode: Option<crate::display::Mode>,
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
    /// Catalogue id, the same on every machine (`steam:<app id>`), or
    /// `manual:<key>` for a program the administrator added there, or
    /// `emu:<console>-<key>` for a game file of a console set up there.
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub has_cover: bool,
    /// The console's name for a game played through an emulator there
    /// ("GameCube"); empty for everything else.
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub emulated: bool,
    /// The store there has an update to finish before the game starts.
    #[serde(default)]
    pub update: Option<crate::library::Update>,
    /// What the administrator wrote (or looked up) there about a game added
    /// by hand; empty when there is none. Older desktops never send it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// What a desktop's stores could install there: games of the account's
/// library that are not installed on that desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLibrary {
    pub games: Vec<RemoteGame>,
    /// Room on the disk a new game would go to.
    pub free_bytes: Option<u64>,
    /// One line when the library could not be read.
    pub problem: Option<String>,
}

/// Asks a desktop to install a game of its library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Install {
    pub game_id: String,
}

/// How a desktop started an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installing {
    /// The store there is showing a window that someone has to answer at
    /// the desktop before anything is fetched.
    pub asked_store: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolStart {
    pub device_name: String,
    /// The Ally's key-exchange message, hex.
    pub message: String,
}

/// Starts a link that someone at the desktop allows, instead of a code.
/// The Ally first only commits to its key-exchange message, so that neither
/// side can steer the number both screens show.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolAsk {
    pub device_name: String,
    /// SHA-256 of the Ally's key-exchange message, hex.
    pub commit: String,
}

/// The Ally's message itself, once it has the desktop's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolReveal {
    pub enrol_id: String,
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
    /// The picture the Ally is about to ask for: its size, and pictures a
    /// second in `hz`. The desktop switches its screen to match.
    #[serde(default)]
    pub screen: Option<crate::display::Mode>,
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
    /// Where the game's saves are, in words that hold on any machine. Only a
    /// hint for a desktop that has no folder yet: it never names a folder by
    /// itself.
    #[serde(default)]
    pub place: Option<crate::library::saveplace::SavePlace>,
}

/// A desktop's save folder for a game, as it is now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavesFound {
    /// Names the folder in the requests that follow.
    pub key: String,
    pub manifest: crate::library::sync::Manifest,
    /// The folder's place, when it is below a well-known one.
    #[serde(default)]
    pub place: Option<crate::library::saveplace::SavePlace>,
    /// The desktop's administrator asked for this game's live saves to be
    /// cleared after a stream. Absent from older desktops, which never do.
    #[serde(default)]
    pub keep_clean: bool,
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

/// What a desktop removed from its own save folder after an Ally proved it
/// holds an identical copy (`POST /v1/saves/{key}/retire`, whose body is the
/// Ally's `Manifest` of its own folder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    pub files: u64,
    pub bytes: u64,
    /// Files that changed after the check and were therefore left in place.
    #[serde(default)]
    pub skipped: u64,
    /// The safe copy the desktop took first.
    pub backup: String,
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

#[cfg(test)]
mod description_tests {
    use super::RemoteGame;

    #[test]
    fn a_game_from_an_older_desktop_has_no_description_and_none_is_sent_when_empty() {
        let old = r#"{"id":"manual:ab","source":"manual","source_id":"ab","title":"Lanterns","has_cover":true}"#;
        let game: RemoteGame = serde_json::from_str(old).unwrap();
        assert_eq!(game.description, "");
        assert!(!serde_json::to_string(&game).unwrap().contains("description"), "an older Ally never sees the field");
        let told = RemoteGame { description: "A game.".into(), ..game };
        let back: RemoteGame = serde_json::from_str(&serde_json::to_string(&told).unwrap()).unwrap();
        assert_eq!(back.description, "A game.");
    }
}
