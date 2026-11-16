//! How good the connection between an Ally and a desktop is, in one shape
//! that every screen shows the same way: the Ally's own pages, the desktop's
//! dashboard and `carla-companion allies`.
//!
//! The Ally measures (see `probe` and `quality`), works out a [`Health`] and
//! hands it to the desktop with each [`Pulse`]. The desktop keeps the last
//! pulse of every linked Ally and gives them out together as an [`Overview`].
//! Everything is whole numbers in named units, so nothing has to be guessed
//! by whoever draws it.

use serde::{Deserialize, Serialize};

use super::codec;
use super::probe::Burst;
use super::quality::{ms_words, Choice, Level, Measure};
use crate::display::Mode;

/// Worst first, so the worse of two grades is the smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// A stream would stutter or lag.
    Poor,
    /// Plays, but a player notices the delay or the odd hitch.
    Playable,
    /// Plays well.
    Good,
    /// As good as a cable to the screen for most games.
    Excellent,
}

impl Grade {
    pub fn word(self) -> &'static str {
        match self {
            Grade::Poor => "Poor",
            Grade::Playable => "Playable",
            Grade::Good => "Good",
            Grade::Excellent => "Excellent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// The home network.
    Home,
    /// Through Tailscale, from somewhere else.
    Away,
}

/// The picture Moonlight is started with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureOut {
    /// "lightest", "light", "balanced", "sharp" or "smooth".
    pub level: String,
    /// Chosen by measuring, not fixed by the administrator.
    pub automatic: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// What the video may use, in kilobits a second.
    pub kbps: u32,
    /// Moonlight evens out when pictures are shown.
    pub pacing: bool,
}

/// One measurement of the connection and what follows from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// Unix seconds it was measured.
    pub at: i64,
    pub route: Route,
    /// The desktop's address that was measured.
    pub address: String,
    pub grade: Grade,
    /// One sentence: the grade and the figures.
    pub summary: String,
    /// The delay there and back, in millionths of a second ("ping").
    pub ping_us: u32,
    /// The quickest packet, and the one only a twentieth were slower than.
    /// `None` when packets could not be timed and `ping_us` is from a web
    /// request instead.
    pub best_us: Option<u32>,
    pub p95_us: Option<u32>,
    /// How much the delay wavers from one packet to the next.
    pub jitter_us: u32,
    /// Lost packets in every thousand. `None` when none could be sent.
    pub loss_permille: Option<u32>,
    /// What the connection carries towards the Ally, in kilobits a second.
    pub link_kbps: Option<u32>,
    /// A second of packets at a picture's rate, when that was tried.
    pub burst: Option<Burst>,
    pub picture: PictureOut,
    /// The codec the two graphics chips can be expected to settle on.
    pub codec: Option<codec::Fit>,
    /// What would make it better, most useful first. Empty when nothing would.
    pub advice: Vec<String>,
}

const fn ms(n: u32) -> u32 {
    n * 1000
}

/// The grade of a measurement. Each figure has its own limits and the worst
/// one decides: a quick connection that loses packets still stutters.
pub fn grade(m: &Measure, level: Level) -> Grade {
    let by = |value: u32, limits: [u32; 3]| match value {
        v if v <= limits[0] => Grade::Excellent,
        v if v <= limits[1] => Grade::Good,
        v if v <= limits[2] => Grade::Playable,
        _ => Grade::Poor,
    };
    // Away from home the delay of the journey itself is not a fault.
    let delay = if m.away { [ms(25), ms(50), ms(100)] } else { [ms(8), ms(20), ms(60)] };
    let picture = match level {
        Level::Smooth | Level::Sharp => Grade::Excellent,
        Level::Balanced => Grade::Good,
        Level::Light => Grade::Playable,
        Level::Lightest => Grade::Poor,
    };
    by(m.latency_us(), delay).min(by(m.jitter_us(), [ms(2), ms(6), ms(15)])).min(by(m.loss_permille().unwrap_or(0), [2, 8, 25])).min(picture)
}

fn advice(m: &Measure, choice: &Choice) -> Vec<String> {
    let mut out = Vec::new();
    let loss = m.loss_permille();
    if loss.is_some_and(|l| l > 8) {
        out.push("Packets are being lost on the way. A network cable to the desktop helps most; for the Ally, the 5 GHz or 6 GHz Wi-Fi band and a place nearer the router.".to_string());
    }
    if m.jitter_us() > ms(6) {
        out.push("The delay wavers, which shows as stutter. Other devices busy on the same Wi-Fi, or a crowded Wi-Fi channel, are the usual cause.".to_string());
    }
    if !m.away && m.latency_us() > ms(20) {
        out.push("That is slow for a home network. The desktop on a cable rather than Wi-Fi usually halves it.".to_string());
    }
    if choice.automatic && choice.level < Level::Sharp && loss.is_none_or(|l| l <= 8) {
        out.push(if m.away {
            "The connection carries less than the sharpest picture needs, so a lighter one is used. That is normal away from home.".to_string()
        } else {
            "The connection carries less than the sharpest picture needs, so a lighter one is used.".to_string()
        });
    }
    if m.echo.is_none() {
        out.push("The desktop did not answer the measuring packets, so lost packets could not be counted. Updating CarlaOS on the desktop, or running carla-companion firewall there, lets them through.".to_string());
    }
    out
}

impl Health {
    /// `screen` is the Ally's own, for the picture a stream would get on it.
    pub fn of(measure: Measure, choice: Choice, screen: Option<Mode>, address: &str, codec: Option<codec::Fit>, at: i64) -> Self {
        let picture = choice.picture_on(screen);
        let grade = grade(&measure, choice.level);
        let lost = match measure.loss_permille() {
            Some(0) => ", no packets lost".to_string(),
            Some(l) => format!(", {}.{} % of packets lost", l / 10, l % 10),
            None => String::new(),
        };
        let summary = format!(
            "{}: {} there and back, wavering by {}{lost}. Streams get {}p at {} pictures a second.",
            grade.word(),
            ms_words(measure.latency_us()),
            ms_words(measure.jitter_us()),
            picture.height,
            picture.fps
        );
        Health {
            at,
            route: if measure.away { Route::Away } else { Route::Home },
            address: address.to_string(),
            grade,
            summary,
            ping_us: measure.latency_us(),
            best_us: measure.echo.map(|e| e.best_us),
            p95_us: measure.echo.map(|e| e.p95_us),
            jitter_us: measure.jitter_us(),
            loss_permille: measure.loss_permille(),
            link_kbps: measure.carries_kbps(),
            burst: measure.burst,
            picture: PictureOut {
                level: choice.level.name().to_string(),
                automatic: choice.automatic,
                width: picture.width,
                height: picture.height,
                fps: picture.fps,
                kbps: picture.kbps,
                pacing: picture.pacing,
            },
            codec,
            advice: advice(&measure, &choice),
        }
    }

    /// The figures on one line, for a terminal.
    pub fn words(&self) -> String {
        let speed = self.link_kbps.map(|k| format!(", carries {} Mbit/s", k / 1000)).unwrap_or_default();
        let lost = self.loss_permille.map(|l| format!(", {}.{} % lost", l / 10, l % 10)).unwrap_or_default();
        format!("{}: ping {}, jitter {}{lost}{speed}", self.grade.word(), ms_words(self.ping_us), ms_words(self.jitter_us))
    }
}

/// What an Ally tells a desktop about itself every so often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pulse {
    /// The Ally's CarlaOS version.
    pub version: String,
    /// Moonlight is installed there.
    pub moonlight: bool,
    /// Moonlight is paired with this desktop's Sunshine. `None`: not asked yet.
    pub paired: Option<bool>,
    /// The game that is running on the Ally or streamed to it, if any.
    pub playing: Option<String>,
    /// Battery, in hundredths, and whether it is charging.
    #[serde(default)]
    pub battery: Option<u8>,
    #[serde(default)]
    pub charging: Option<bool>,
    /// The Ally's screen.
    #[serde(default)]
    pub screen: Option<Mode>,
    /// The latest measurement. `None` while a game runs: nothing is measured
    /// under a stream.
    pub health: Option<Health>,
}

/// An Ally that has not been heard from for this long is not connected.
pub const CONNECTED_FOR: i64 = 90;

/// One linked Ally as the desktop knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllyView {
    /// The id the desktop gave it when it was linked.
    pub id: String,
    pub name: String,
    /// Unix seconds it was linked.
    pub linked_at: i64,
    /// Unix seconds of its last signed request. `None`: not since the
    /// companion started keeping track.
    pub last_seen: Option<i64>,
    /// Heard from within the last minute and a half, or being streamed to.
    pub connected: bool,
    /// Where its last request came from.
    pub address: Option<String>,
    /// A stream to it is running now.
    pub streaming: bool,
    /// What it last said about itself and the connection.
    pub pulse: Option<Pulse>,
    /// The latest measurement that is known, kept when a later pulse
    /// carried none because a game was running.
    pub health: Option<Health>,
}

/// A device that is asking to be linked and waits for someone at the
/// desktop to allow it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asking {
    pub enrol_id: String,
    pub device_name: String,
    /// The address it asks from.
    pub from: String,
    /// The number both screens show. Allow only when they match.
    pub check: String,
    pub at: i64,
}

/// Everything a desktop dashboard shows, in one answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    pub name: String,
    pub id: String,
    pub version: String,
    /// "ready", "busy" or "not_ready".
    pub state: super::State,
    /// One line when streaming is not ready.
    pub problem: Option<String>,
    /// The game running here, if any.
    pub playing: Option<String>,
    /// A stream is running now.
    pub streaming: bool,
    pub hardware: String,
    pub graphics: Vec<String>,
    pub addresses: Vec<String>,
    /// Unix seconds until which an Ally can be linked without a code being
    /// typed. `None` when linking is closed.
    pub linking_until: Option<i64>,
    /// A device waiting to be allowed, if any.
    pub asking: Option<Asking>,
    pub allies: Vec<AllyView>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::probe::Echo;
    use crate::link::quality::{choose, Setting};

    fn measured(median_us: u32, jitter_us: u32, lost: u32, kbps: u32, away: bool) -> Measure {
        let echo = Echo { sent: 100, lost, best_us: median_us * 4 / 5, median_us, p95_us: median_us * 2, jitter_us };
        Measure { echo: Some(echo), ..Measure::plain(median_us.div_ceil(1000), jitter_us.div_ceil(1000), Some(kbps), away) }
    }

    fn health(m: Measure) -> Health {
        Health::of(m, choose(Setting::default(), Some(m), m.away), Some(Mode::new(1920, 1080, 120)), "192.168.1.20", None, 100)
    }

    #[test]
    fn a_cabled_home_network_is_excellent_and_says_so_in_figures() {
        let h = health(measured(1_800, 300, 0, 400_000, false));
        assert_eq!(h.grade, Grade::Excellent);
        assert_eq!(h.summary, "Excellent: 1.8 ms there and back, wavering by 0.3 ms, no packets lost. Streams get 1080p at 60 pictures a second.");
        assert_eq!((h.ping_us, h.best_us, h.p95_us, h.loss_permille), (1_800, Some(1_440), Some(3_600), Some(0)));
        assert_eq!((h.route, h.picture.level.as_str(), h.picture.kbps, h.picture.pacing), (Route::Home, "sharp", 25_000, false));
        assert!(h.advice.is_empty(), "{:?}", h.advice);
        assert_eq!(h.words(), "Excellent: ping 1.8 ms, jitter 0.3 ms, 0.0 % lost, carries 400 Mbit/s");
    }

    #[test]
    fn the_worst_figure_decides_and_the_advice_names_it() {
        // Quick, but one packet in thirty is lost.
        let lossy = health(measured(3_000, 1_000, 3, 200_000, false));
        assert_eq!(lossy.grade, Grade::Poor);
        assert!(lossy.advice[0].contains("Packets are being lost"));
        assert_eq!(lossy.picture.level, "balanced", "a lighter picture, so each loss spoils less");
        // Steady, but slow for a home.
        let slow = health(measured(35_000, 1_000, 0, 200_000, false));
        assert_eq!(slow.grade, Grade::Playable);
        assert!(slow.advice.iter().any(|a| a.contains("slow for a home network")));
        // A wavering delay.
        let shaky = health(measured(5_000, 9_000, 0, 200_000, false));
        assert_eq!(shaky.grade, Grade::Playable);
        assert!(shaky.picture.pacing, "Moonlight evens it out");
        // A thin connection.
        let thin = health(measured(4_000, 500, 0, 14_000, false));
        assert_eq!((thin.grade, thin.picture.level.as_str()), (Grade::Playable, "light"));
    }

    #[test]
    fn away_from_home_the_journey_is_not_a_fault() {
        let away = health(measured(32_000, 2_500, 0, 60_000, true));
        assert_eq!((away.grade, away.route), (Grade::Good, Route::Away));
        assert!(away.picture.pacing);
        assert!(away.advice.is_empty(), "{:?}", away.advice);
    }

    #[test]
    fn without_packets_the_figures_come_from_the_web_request_and_say_what_is_missing() {
        let m = Measure::plain(6, 2, Some(100_000), false);
        let h = health(m);
        assert_eq!((h.ping_us, h.best_us, h.loss_permille), (6_000, None, None));
        assert!(h.advice.iter().any(|a| a.contains("carla-companion firewall")));
    }

    #[test]
    fn the_shape_a_dashboard_reads_is_fixed() {
        let h = health(measured(1_800, 300, 0, 400_000, false));
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["grade"], "excellent");
        assert_eq!(json["route"], "home");
        assert_eq!(json["ping_us"], 1_800);
        assert_eq!(json["picture"]["fps"], 60);
        let pulse = Pulse { version: "0.17.0".into(), moonlight: true, paired: Some(true), playing: None, battery: Some(80), charging: Some(false), screen: None, health: Some(h.clone()) };
        let ally = AllyView { id: "d1".into(), name: "ROGALLY".into(), linked_at: 1, last_seen: Some(99), connected: true, address: Some("192.168.1.50".into()), streaming: false, pulse: Some(pulse), health: Some(h) };
        let back: AllyView = serde_json::from_value(serde_json::to_value(&ally).unwrap()).unwrap();
        assert_eq!(back, ally);
    }
}
