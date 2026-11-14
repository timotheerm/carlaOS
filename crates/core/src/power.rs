//! How hard the Ally works for each game.
//!
//! A handheld lives on its battery. A small game, a stream or a cloud game
//! needs little of the processor, and a heavy game wants all of it. So each
//! game has a power level and a screen speed, chosen once (on the game's
//! page, or in the quick menu while it runs) and used every time it starts.
//! Between games CarlaOS rests in the quietest level, since its own pages
//! need almost nothing.
//!
//! Three levels, the same three ASUS gives the Ally (Silent, Performance,
//! Turbo), named for what the player feels:
//!
//! - **Quiet**: the fan stays down and the battery lasts longest.
//! - **Balanced**: most games.
//! - **Turbo**: the most demanding games, best when plugged in.
//!
//! This module only decides. Telling the Ally is the shell's job: ASUS's own
//! power mode through its ACPI device, the same one Armoury Crate uses, and
//! the matching Windows power mode. See [`asus_request`].

use serde::{Deserialize, Serialize};

use crate::library::{cloud::CLOUD, Store, LOCAL};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    Quiet,
    Balanced,
    Turbo,
}

impl Power {
    pub const ALL: [Power; 3] = [Power::Quiet, Power::Balanced, Power::Turbo];

    pub fn name(self) -> &'static str {
        match self {
            Power::Quiet => "quiet",
            Power::Balanced => "balanced",
            Power::Turbo => "turbo",
        }
    }

    pub fn parse(name: &str) -> Option<Power> {
        Power::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }

    /// ASUS's number for it: Balanced (they call it Performance) 0, Turbo 1,
    /// Silent 2.
    pub fn asus(self) -> u32 {
        match self {
            Power::Balanced => 0,
            Power::Turbo => 1,
            Power::Quiet => 2,
        }
    }

    /// The Windows power mode that goes with it: "best power efficiency",
    /// "balanced" (no overlay at all) and "best performance".
    pub fn windows_overlay(self) -> u128 {
        match self {
            Power::Quiet => 0x961cc777_2547_4f9d_8174_7d86181b8a7a,
            Power::Balanced => 0,
            Power::Turbo => 0xded574b5_45a0_4f42_8737_46345c09c238,
        }
    }
}

/// How often the screen is redrawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    /// As fast as the screen goes (120 times a second on the Ally).
    Smooth,
    /// 60 times a second: plenty for most games, and the screen uses less.
    Saver,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Smooth => "smooth",
            Screen::Saver => "saver",
        }
    }

    pub fn parse(name: &str) -> Option<Screen> {
        [Screen::Smooth, Screen::Saver].into_iter().find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The refresh rate to ask for, from the ones the screen offers at its
    /// current size. `None` keeps whatever it has.
    pub fn rate(self, offered: &[u32]) -> Option<u32> {
        let usable = offered.iter().copied().filter(|&hz| hz >= 30);
        match self {
            Screen::Smooth => usable.max(),
            // 60, or the closest the screen has above it.
            Screen::Saver => usable.filter(|&hz| hz >= 59).min(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub power: Power,
    pub screen: Screen,
}

/// Where a game is played, as far as power goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// On the Ally itself.
    Local,
    /// Streamed from a desktop: the Ally only shows a video.
    Stream,
    /// On a cloud gaming service, in a browser.
    Cloud,
}

impl Kind {
    pub fn of(place: &str) -> Kind {
        if place == LOCAL {
            Kind::Local
        } else if place == CLOUD {
            Kind::Cloud
        } else {
            Kind::Stream
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Local => "local",
            Kind::Stream => "stream",
            Kind::Cloud => "cloud",
        }
    }

    /// What a game gets until someone chooses. A stream or a cloud game is a
    /// video: the processor has almost nothing to do.
    pub fn default_profile(self) -> Profile {
        match self {
            Kind::Local => Profile { power: Power::Balanced, screen: Screen::Smooth },
            Kind::Stream | Kind::Cloud => Profile { power: Power::Quiet, screen: Screen::Smooth },
        }
    }

    /// Streams leave the screen to the stream itself, which picks its own
    /// rate to match the desktop's.
    pub fn sets_screen(self) -> bool {
        self != Kind::Stream
    }
}

/// What CarlaOS rests in between games.
pub const RESTING: Power = Power::Quiet;

fn key(game_id: &str, kind: Kind) -> String {
    format!("power:{}:{game_id}", kind.name())
}

/// The game's own choice, if one was made.
pub fn chosen(store: &Store, game_id: &str, kind: Kind) -> Result<Option<Profile>, String> {
    Ok(store.setting(&key(game_id, kind))?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// What the game will be played with.
pub fn profile(store: &Store, game_id: &str, kind: Kind) -> Result<Profile, String> {
    Ok(chosen(store, game_id, kind)?.unwrap_or_else(|| kind.default_profile()))
}

/// Remember a choice for the game, or forget it with `None`.
pub fn choose(store: &mut Store, game_id: &str, kind: Kind, profile: Option<Profile>) -> Result<(), String> {
    let bytes = profile.map(|p| serde_json::to_vec(&p).expect("a profile is always JSON"));
    store.set_setting(&key(game_id, kind), bytes.as_deref())
}

/// What the ASUS ACPI device (`\\.\ATKACPI`) is sent to change its power
/// mode: the "set device" method, the length of what follows, then the
/// power mode's device number and its new value, all little-endian. It is
/// sent with the control code [`ASUS_CONTROL`].
pub fn asus_request(power: Power) -> [u8; 16] {
    const DEVS: u32 = 0x5356_4544; // "DEVS"
    const PERFORMANCE_MODE: u32 = 0x0012_0075;
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&DEVS.to_le_bytes());
    out[4..8].copy_from_slice(&8u32.to_le_bytes());
    out[8..12].copy_from_slice(&PERFORMANCE_MODE.to_le_bytes());
    out[12..16].copy_from_slice(&power.asus().to_le_bytes());
    out
}

pub const ASUS_CONTROL: u32 = 0x0022_240C;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_game_keeps_its_own_choice_for_each_way_it_is_played() {
        let mut store = Store::in_memory().unwrap();
        assert_eq!(profile(&store, "steam:1", Kind::Local).unwrap(), Profile { power: Power::Balanced, screen: Screen::Smooth });
        assert_eq!(profile(&store, "steam:1", Kind::of("desk-1")).unwrap().power, Power::Quiet);
        assert_eq!(Kind::of(CLOUD), Kind::Cloud);
        let turbo = Profile { power: Power::Turbo, screen: Screen::Saver };
        choose(&mut store, "steam:1", Kind::Local, Some(turbo)).unwrap();
        assert_eq!(profile(&store, "steam:1", Kind::Local).unwrap(), turbo);
        assert_eq!(profile(&store, "steam:1", Kind::Stream).unwrap().power, Power::Quiet, "a stream of it is not changed");
        assert_eq!(chosen(&store, "steam:2", Kind::Local).unwrap(), None);
        choose(&mut store, "steam:1", Kind::Local, None).unwrap();
        assert_eq!(chosen(&store, "steam:1", Kind::Local).unwrap(), None);
    }

    #[test]
    fn the_screen_goes_as_fast_as_it_can_or_to_sixty() {
        let ally = [48, 60, 120];
        assert_eq!(Screen::Smooth.rate(&ally), Some(120));
        assert_eq!(Screen::Saver.rate(&ally), Some(60));
        assert_eq!(Screen::Saver.rate(&[50, 75, 144]), Some(75));
        assert_eq!(Screen::Smooth.rate(&[]), None);
        assert!(!Kind::Stream.sets_screen());
    }

    #[test]
    fn names_and_the_asus_request() {
        assert_eq!(Power::parse(" Turbo "), Some(Power::Turbo));
        assert_eq!(Power::parse("max"), None);
        assert_eq!(Screen::parse("saver"), Some(Screen::Saver));
        assert_eq!(serde_json::to_string(&Profile { power: Power::Quiet, screen: Screen::Smooth }).unwrap(), r#"{"power":"quiet","screen":"smooth"}"#);
        let r = asus_request(Power::Quiet);
        assert_eq!(&r[0..4], b"DEVS");
        assert_eq!(u32::from_le_bytes(r[4..8].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(r[8..12].try_into().unwrap()), 0x0012_0075);
        assert_eq!(u32::from_le_bytes(r[12..16].try_into().unwrap()), 2);
        assert_eq!(Power::Balanced.windows_overlay(), 0);
    }
}
