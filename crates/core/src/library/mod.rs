//! The game library: what is installed where, how to start it, and how long it
//! has been played.
//!
//! One `games` row is one title. One `installs` row is one place that title can
//! be started from (this machine now; a desktop to stream from later). A card
//! in the interface therefore only ever offers places where the game really is.

pub mod downloads;
pub mod launch;
pub mod owned;
pub mod emulation;
pub mod epic;
pub mod gog;
pub mod steam;
mod store;
pub mod vdf;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod achievements;
pub mod bvdf;
pub mod saves;
pub mod sync;
pub mod meta;
pub mod art;
pub mod ports;
pub mod shortcut;

pub use store::Store;

/// The place name for "installed on this machine".
pub const LOCAL: &str = "local";

/// The stable name of a game added by hand, from its program's path. The same
/// on every machine, so the catalogue id is `manual:<this>` everywhere.
pub fn manual_key(exe: &str) -> String {
    let hash = exe.to_lowercase().bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{hash:016x}")
}

/// Which section of the interface a title belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Game,
    Emulated,
    /// A native version of a game that also exists on a console. Never an emulator.
    Port,
    Homebrew,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Game => "game",
            Kind::Emulated => "emulated",
            Kind::Port => "port",
            Kind::Homebrew => "homebrew",
        }
    }

    pub(crate) fn parse(s: &str) -> Kind {
        match s {
            "emulated" => Kind::Emulated,
            "port" => Kind::Port,
            "homebrew" => Kind::Homebrew,
            _ => Kind::Game,
        }
    }
}

/// How far a store's update of an installed game has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    /// The store knows of a newer version and has not started on it.
    Needed,
    /// Being downloaded or put in place now.
    Running,
    /// Started, then paused.
    Paused,
}

/// An update a store has to finish before it will start a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub state: UpdateState,
    /// Bytes to download in all. 0 when the store has not said.
    #[serde(default)]
    pub bytes: u64,
    /// Bytes downloaded so far.
    #[serde(default)]
    pub done: u64,
}

impl Update {
    /// How far the download is, when that is known.
    pub fn percent(&self) -> Option<u8> {
        (self.bytes > 0).then(|| (self.done.min(self.bytes) * 100 / self.bytes) as u8)
    }

    /// What is left to download, in words: "1.2 GB", "340 MB".
    pub fn left(&self) -> Option<String> {
        let left = self.bytes.checked_sub(self.done).filter(|_| self.bytes > 0)?;
        Some(if left >= 1_000_000_000 { format!("{:.1} GB", left as f64 / 1e9) } else { format!("{} MB", (left / 1_000_000).max(1)) })
    }
}

/// One installed game as a scanner found it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scanned {
    /// The store's own id for the game (a Steam app id, for example).
    pub source_id: String,
    pub title: String,
    pub install_path: PathBuf,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    /// The program to start, when the game starts without its store.
    pub exe: Option<PathBuf>,
    /// Options for that program; for a store that starts games through an
    /// address of its own, that address.
    pub args: String,
    /// What the badge says the game is from, when the store's name is not it.
    pub platform: String,
    /// Played through an emulator.
    pub emulated: bool,
    /// The store has an update to finish before the game starts.
    pub update: Option<Update>,
}

/// A game its store could install on one machine: in the account's library
/// there, and not installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offered {
    pub source_id: String,
    pub title: String,
    pub cover: Option<PathBuf>,
}

/// The stores CarlaOS looks through by itself.
pub const STORES: [&str; 3] = ["steam", "epic", "gog"];

/// What one store has installed on this machine.
pub fn scan_store(store: &str, steam_root: Option<&std::path::Path>) -> Vec<Scanned> {
    match store {
        "steam" => steam_root.map(steam::scan).unwrap_or_default(),
        "epic" => epic::manifests().as_deref().map(epic::scan).unwrap_or_default(),
        "gog" => gog::scan(),
        _ => Vec::new(),
    }
}

/// Whether a store has an update to finish for one installed game, read
/// afresh. For the moment just before a game starts, and while it does.
pub fn update_of(source: &str, source_id: &str, steam_root: Option<&std::path::Path>) -> Option<Update> {
    match source {
        "steam" => steam::update_of(steam_root?, source_id),
        "epic" => epic::manifests().and_then(|dir| epic::update_of(&dir, source_id)),
        _ => None,
    }
}

/// A game the administrator adds by pointing at its program.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManualGame {
    pub title: String,
    pub exe: PathBuf,
    #[serde(default)]
    pub args: String,
    pub kind: Kind,
    #[serde(default)]
    pub cover: Option<PathBuf>,
    /// A sentence or two about the game, for its page.
    #[serde(default)]
    pub description: String,
    /// The console the game first came out on, for a port: "Nintendo 64".
    #[serde(default)]
    pub origin: String,
}

/// An update that has to finish in one place before the game starts there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Waiting {
    pub place: String,
    #[serde(flatten)]
    pub update: Update,
}

/// One title as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GameView {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Steam, Standalone, ...
    pub platform: String,
    pub kind: Kind,
    /// Places it can be started from, [`LOCAL`] first.
    pub places: Vec<String>,
    /// Places where its store could install it and has not, [`LOCAL`] first.
    pub installable: Vec<String>,
    /// The places where a store has an update to finish first.
    pub updates: Vec<Waiting>,
    pub playtime_minutes: u64,
    /// Unix seconds.
    pub last_played: Option<i64>,
    pub added: i64,
    pub favourite: bool,
    pub cover: Option<String>,
    pub hero: Option<String>,
    /// Added by hand, so the administrator can remove it.
    pub manual: bool,
    /// The console a port first came out on; empty for anything else.
    pub origin: String,
    /// The administrator chose its pictures, so they can be given back.
    pub own_art: bool,
}

/// How to start one install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub game_id: String,
    pub title: String,
    pub source: String,
    pub source_id: String,
    pub install_path: PathBuf,
    pub exe: Option<PathBuf>,
    pub args: String,
}

/// The last time one game's saves were brought in step with one desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    pub host_id: String,
    /// What both machines held when they last agreed.
    pub digest: Option<String>,
    pub at: i64,
    /// "same", "sent", "fetched" or "problem".
    pub outcome: String,
    /// Both had new saves; the older ones are in a safe copy.
    pub both_changed: bool,
    pub files: u32,
    pub problem: Option<String>,
}

/// The picture a stream from a desktop got, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Streamed {
    pub choice: crate::link::quality::Choice,
    /// Unix seconds.
    pub at: i64,
}

/// A linked desktop as the catalogue stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRow {
    /// The desktop's own id, which is also the place name of its installs.
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    /// What the desktop calls this Ally.
    pub device_id: String,
    /// The device key, sealed with [`crate::secret::seal`].
    pub key: Vec<u8>,
    pub hardware: String,
    /// Unix seconds of the last answer.
    pub last_seen: Option<i64>,
    /// Every address the desktop said it has; `address` is the one that
    /// answered last.
    pub addresses: Vec<String>,
    /// Hardware addresses of its network adapters, for waking it.
    pub macs: Vec<String>,
}
