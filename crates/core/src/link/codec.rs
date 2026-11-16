//! Which video codec a stream between two machines can be expected to use.
//!
//! Sunshine encodes on the desktop's graphics chip and Moonlight decodes on
//! the Ally's; they settle the codec between themselves when a stream starts.
//! CarlaOS does not choose it. This only reads the names of the graphics
//! chips and says what that settlement will most likely be, so the check can
//! warn before a stream that the desktop is left with H.264, which needs
//! about half as much again of the connection for the same picture.
//!
//! The answer comes from the chip's name, so it is a well-informed guess.

use serde::{Deserialize, Serialize};

/// Oldest and heaviest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::Av1 => "AV1",
        }
    }
}

fn has(name: &str, parts: &[&str]) -> bool {
    parts.iter().any(|p| name.contains(p))
}

/// Not a chip that draws games: remote-desktop and virtual screens.
fn real(name: &str) -> bool {
    !has(&name.to_lowercase(), &["virtual", "basic display", "basic render", "remote", "parsec", "mirror", "idd", "displaylink"])
}

/// The newest codec a graphics chip encodes for Sunshine. `None` when the
/// name says nothing.
pub fn encodes(gpu: &str) -> Option<Codec> {
    let name = gpu.to_lowercase();
    if has(&name, &["nvidia", "geforce", "quadro", "rtx", "gtx"]) {
        Some(if has(&name, &["rtx 40", "rtx 50", "rtx40", "rtx50"]) {
            Codec::Av1
        } else if has(&name, &["rtx", "gtx 16", "gtx 10", "gtx 9"]) {
            Codec::Hevc
        } else {
            Codec::H264
        })
    } else if has(&name, &["radeon", "amd"]) {
        Some(if has(&name, &["rx 7", "rx 9", "rx7", "rx9"]) { Codec::Av1 } else { Codec::Hevc })
    } else if has(&name, &["intel", "arc(", "arc "]) {
        Some(if name.contains("arc") { Codec::Av1 } else { Codec::Hevc })
    } else {
        None
    }
}

/// The newest codec a graphics chip decodes for Moonlight, read carefully:
/// AV1 only where the name is sure of it.
pub fn decodes(gpu: &str) -> Option<Codec> {
    let name = gpu.to_lowercase();
    if has(&name, &["nvidia", "geforce", "rtx", "gtx"]) {
        Some(if has(&name, &["rtx 30", "rtx 40", "rtx 50"]) { Codec::Av1 } else { Codec::Hevc })
    } else if has(&name, &["radeon", "amd"]) {
        Some(if has(&name, &["rx 6", "rx 7", "rx 9"]) { Codec::Av1 } else { Codec::Hevc })
    } else if has(&name, &["intel", "arc"]) {
        Some(if has(&name, &["arc", "xe"]) { Codec::Av1 } else { Codec::Hevc })
    } else {
        None
    }
}

/// The chip that matters among a machine's: the one with the newest codec.
pub fn pick(names: &[String], can: fn(&str) -> Option<Codec>) -> Option<(String, Codec)> {
    names.iter().filter(|n| real(n)).filter_map(|n| can(n).map(|c| (n.clone(), c))).max_by_key(|(_, c)| *c)
}

/// What a stream from one machine to the other can be expected to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fit {
    pub codec: Codec,
    /// False when one of the chips was not recognised and H.264, which
    /// everything handles, is assumed.
    pub known: bool,
    pub desktop: String,
    pub ally: String,
}

impl Fit {
    pub fn words(&self) -> String {
        let chips = match (self.desktop.is_empty(), self.ally.is_empty()) {
            (false, false) => format!(" ({} to {})", self.desktop, self.ally),
            (false, true) => format!(" ({})", self.desktop),
            _ => String::new(),
        };
        if self.known {
            format!("{} expected{chips}", self.codec.name())
        } else {
            format!("H.264 assumed{chips}")
        }
    }
}

/// `desktop` and `ally` are the names of each machine's graphics chips.
pub fn fit(desktop: &[String], ally: &[String]) -> Fit {
    let (encoder, decoder) = (pick(desktop, encodes), pick(ally, decodes));
    let name = |found: &Option<(String, Codec)>, all: &[String]| found.as_ref().map(|(n, _)| n.clone()).or_else(|| all.iter().find(|n| real(n)).cloned()).unwrap_or_default();
    let (desktop, ally) = (name(&encoder, desktop), name(&decoder, ally));
    match (encoder, decoder) {
        // HEVC is what Moonlight settles on by itself when both handle it;
        // AV1 only when both are sure to.
        (Some((_, e)), Some((_, d))) => Fit { codec: e.min(d), known: true, desktop, ally },
        _ => Fit { codec: Codec::H264, known: false, desktop, ally },
    }
}

/// The names of this machine's graphics chips.
#[cfg(windows)]
pub fn graphics() -> Vec<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    // The class of display adapters; one numbered key for each.
    const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(CLASS) else { return Vec::new() };
    let mut names: Vec<String> = class
        .enum_keys()
        .flatten()
        .filter(|key| key.len() == 4 && key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|key| class.open_subkey(key).ok()?.get_value::<String, _>("DriverDesc").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(not(windows))]
pub fn graphics() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_home_desktop_and_the_ally_settle_on_hevc() {
        let got = fit(&names(&["AMD Radeon RX 9070 XT"]), &names(&["AMD Radeon(TM) Graphics"]));
        assert_eq!((got.codec, got.known), (Codec::Hevc, true));
        assert_eq!(got.words(), "HEVC expected (AMD Radeon RX 9070 XT to AMD Radeon(TM) Graphics)");
    }

    #[test]
    fn av1_only_when_both_are_sure_to_handle_it() {
        assert_eq!(fit(&names(&["NVIDIA GeForce RTX 4070"]), &names(&["AMD Radeon RX 7600"])).codec, Codec::Av1);
        assert_eq!(fit(&names(&["NVIDIA GeForce RTX 4070"]), &names(&["Intel(R) UHD Graphics 630"])).codec, Codec::Hevc);
        assert_eq!(fit(&names(&["NVIDIA GeForce GT 710"]), &names(&["AMD Radeon RX 7600"])).codec, Codec::H264);
    }

    #[test]
    fn the_chip_that_draws_games_counts_not_the_others() {
        let desktop = names(&["Virtual Display Driver", "Intel(R) UHD Graphics 770", "NVIDIA GeForce RTX 5080", "Microsoft Basic Display Adapter"]);
        assert_eq!(pick(&desktop, encodes), Some(("NVIDIA GeForce RTX 5080".to_string(), Codec::Av1)));
        assert_eq!(pick(&names(&["Parsec Virtual Display Adapter"]), encodes), None);
    }

    #[test]
    fn an_unknown_chip_is_said_to_be_unknown() {
        let got = fit(&names(&["Matrox G200"]), &names(&["AMD Radeon(TM) Graphics"]));
        assert_eq!((got.codec, got.known), (Codec::H264, false));
        assert_eq!(got.words(), "H.264 assumed (Matrox G200 to AMD Radeon(TM) Graphics)");
        assert_eq!(fit(&[], &[]).words(), "H.264 assumed");
    }
}
