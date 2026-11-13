//! The game library: what is installed where, how to start it, and how long it
//! has been played.
//!
//! One `games` row is one title. One `installs` row is one place that title can
//! be started from (this machine now; a desktop to stream from later). A card
//! in the interface therefore only ever offers places where the game really is.

pub mod launch;
pub mod steam;
mod store;
pub mod vdf;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod achievements;
pub mod bvdf;
pub mod saves;
pub mod meta;

pub use store::Store;

/// The place name for "installed on this machine".
pub const LOCAL: &str = "local";

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

/// One installed game as a scanner found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// The store's own id for the game (a Steam app id, for example).
    pub source_id: String,
    pub title: String,
    pub install_path: PathBuf,
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
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
    pub playtime_minutes: u64,
    /// Unix seconds.
    pub last_played: Option<i64>,
    pub added: i64,
    pub favourite: bool,
    pub cover: Option<String>,
    pub hero: Option<String>,
    /// Added by hand, so the administrator can remove it.
    pub manual: bool,
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
