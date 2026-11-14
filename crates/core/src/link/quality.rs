//! Choosing the picture of a stream from what the connection can carry.
//!
//! Just before a stream starts the Ally times a few signed questions to the
//! desktop: empty ones for the delay, then one that carries a known amount of
//! data back. From that it picks the sharpest picture the connection carries
//! with room to spare, so the same game looks its best at home and still
//! plays on a hotel connection, without anyone choosing a setting.
//!
//! The measurement is a short download over the same path the stream takes.
//! It is cautious rather than exact, which is the right way round: a picture
//! that is a little soft plays, one that is too heavy stutters.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::moonlight::Picture;
use crate::display::{self, Mode};

/// The pictures CarlaOS streams, lightest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Lightest,
    Light,
    Balanced,
    Sharp,
    /// 120 pictures a second: the Ally's screen shows them, but only when
    /// the desktop's screen runs that fast too.
    Smooth,
}

pub const LEVELS: [Level; 5] = [Level::Lightest, Level::Light, Level::Balanced, Level::Sharp, Level::Smooth];

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Lightest => "lightest",
            Level::Light => "light",
            Level::Balanced => "balanced",
            Level::Sharp => "sharp",
            Level::Smooth => "smooth",
        }
    }

    pub fn picture(self) -> Picture {
        let (width, height, fps, kbps) = match self {
            Level::Lightest => (1280, 720, 30, 3_000),
            Level::Light => (1280, 720, 60, 7_000),
            Level::Balanced => (1920, 1080, 60, 12_000),
            Level::Sharp => (1920, 1080, 60, 25_000),
            Level::Smooth => (1920, 1080, 120, 40_000),
        };
        Picture { width, height, fps, kbps }
    }

    /// The picture made for one screen: its own shape, and its own size
    /// when that is no larger than this level carries. A screen that does
    /// not show 120 pictures a second is not sent them.
    pub fn picture_on(self, screen: Option<Mode>) -> Picture {
        let base = self.picture();
        let Some(screen) = screen.filter(|s| s.width > 0 && s.height > 0) else { return base };
        // Room for a 16:10 screen of the same width.
        let most = (base.width, base.width * 5 / 8);
        let (width, height) = display::fit((screen.width, screen.height), most);
        let slow = base.fps > 60 && screen.hz < 100;
        let (fps, kbps) = if slow { (60, Level::Sharp.picture().kbps) } else { (base.fps, base.kbps) };
        // A larger picture needs more to look the same, a smaller one less.
        let scaled = u64::from(kbps) * u64::from(width) * u64::from(height) / (u64::from(base.width) * u64::from(base.height));
        Picture { width, height, fps, kbps: (scaled as u32).clamp(kbps / 2, kbps * 3 / 2) }
    }

    /// The picture in words, for the administrator.
    pub fn words(self) -> String {
        let p = self.picture();
        format!("{}p, {} pictures a second, {} Mbit/s", p.height, p.fps, p.kbps / 1000)
    }

    /// What the connection must carry for this picture to play calmly. Video
    /// comes in bursts and the measurement is short, so a stream gets a
    /// little over half of what was measured.
    fn needs_kbps(self) -> u32 {
        self.picture().kbps * 9 / 5
    }
}

/// What the administrator chose for one desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Measure and choose. `smooth` allows 120 pictures a second.
    Auto { smooth: bool },
    Fixed(Level),
}

impl Default for Setting {
    fn default() -> Self {
        Setting::Auto { smooth: false }
    }
}

impl Setting {
    pub fn name(self) -> &'static str {
        match self {
            Setting::Auto { smooth: false } => "auto",
            Setting::Auto { smooth: true } => "auto120",
            Setting::Fixed(level) => level.name(),
        }
    }

    /// The setting in words, for the administrator.
    pub fn words(self) -> String {
        match self {
            Setting::Auto { smooth: false } => "chosen by measuring the connection".to_string(),
            Setting::Auto { smooth: true } => "chosen by measuring the connection, up to 120 pictures a second".to_string(),
            Setting::Fixed(level) => format!("always {}", level.words()),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Setting::Auto { smooth: false }),
            "auto120" => Some(Setting::Auto { smooth: true }),
            other => LEVELS.into_iter().find(|l| l.name() == other).map(Setting::Fixed),
        }
    }

    /// Every setting, in the order the interface steps through them.
    pub fn all() -> Vec<Setting> {
        let auto = [Setting::Auto { smooth: false }, Setting::Auto { smooth: true }];
        auto.into_iter().chain(LEVELS.into_iter().rev().map(Setting::Fixed)).collect()
    }
}

/// What the connection to a desktop was like when it was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    /// The quickest answer to an empty question, there and back.
    pub latency_ms: u32,
    /// How far the slowest answer was from the quickest.
    pub jitter_ms: u32,
    /// What the connection carried towards the Ally. `None` when the desktop
    /// answered but could not be timed carrying data.
    pub kbps: Option<u32>,
    /// Through Tailscale rather than the home network.
    pub away: bool,
}

/// The largest answer a desktop is asked to send for a measurement.
pub const LARGEST_PROBE: usize = 2 * 1024 * 1024;
const FIRST_PROBE: usize = 128 * 1024;
const EMPTY_PROBES: usize = 4;
/// About how long the second, larger download should take.
const SECOND_AIM: Duration = Duration::from_millis(800);

fn rate_kbps(bytes: usize, took: Duration, latency: Duration) -> u32 {
    // The delay of the question itself is not time spent carrying data.
    let carrying = took.saturating_sub(latency).max(Duration::from_millis(1));
    let kbps = bytes as f64 * 8.0 / 1000.0 / carrying.as_secs_f64();
    kbps.min(10_000_000.0) as u32
}

/// Measure the connection. `probe(n)` asks the desktop for `n` bytes and says
/// how long the answer took, or `None` when it did not come.
///
/// `None` when the desktop cannot be measured at all, which is the case for a
/// companion from before this existed.
pub fn measure(probe: &dyn Fn(usize) -> Option<Duration>, away: bool) -> Option<Measure> {
    let empty: Vec<Duration> = (0..EMPTY_PROBES).filter_map(|_| probe(0)).collect();
    let latency = *empty.iter().min()?;
    let slowest = *empty.iter().max()?;
    let mut kbps = probe(FIRST_PROBE).map(|took| rate_kbps(FIRST_PROBE, took, latency));
    // A short download says little about a fast connection: it is over
    // before the connection is up to speed. Ask for more when there is room.
    if let Some(first) = kbps {
        let fits = (first as f64 * 1000.0 / 8.0 * SECOND_AIM.as_secs_f64()) as usize;
        let second = fits.min(LARGEST_PROBE);
        if second >= FIRST_PROBE * 2 {
            if let Some(took) = probe(second) {
                kbps = Some(rate_kbps(second, took, latency).max(first));
            }
        }
    }
    Some(Measure {
        latency_ms: latency.as_millis().min(u32::MAX as u128) as u32,
        jitter_ms: (slowest - latency).as_millis().min(u32::MAX as u128) as u32,
        kbps,
        away,
    })
}

/// An unsteady connection gets a lighter picture than its speed alone says.
const UNSTEADY_MS: u32 = 40;
/// Past this delay 120 pictures a second cannot be felt.
const SMOOTH_LATENCY_MS: u32 = 15;

/// What was decided for one stream, and from what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub level: Level,
    /// Chosen by measuring, not fixed by the administrator.
    pub automatic: bool,
    pub measure: Option<Measure>,
}

impl Choice {
    pub fn picture(&self) -> Picture {
        self.level.picture()
    }

    /// The picture for the screen it will be shown on.
    pub fn picture_on(&self, screen: Option<Mode>) -> Picture {
        self.level.picture_on(screen)
    }

    /// What was measured, in words. Empty when nothing was.
    pub fn measured(&self) -> String {
        match self.measure {
            None => String::new(),
            Some(m) => {
                let speed = match m.kbps {
                    Some(kbps) if kbps >= 10_000 => format!("{} Mbit/s", kbps / 1000),
                    Some(kbps) => format!("{:.1} Mbit/s", kbps as f64 / 1000.0),
                    None => "speed unknown".to_string(),
                };
                format!("{speed}, {} ms{}", m.latency_ms, if m.away { ", away from home" } else { "" })
            }
        }
    }
}

/// The picture for a stream. `away` matters when nothing could be measured.
pub fn choose(setting: Setting, measure: Option<Measure>, away: bool) -> Choice {
    let smooth = match setting {
        Setting::Fixed(level) => return Choice { level, automatic: false, measure },
        Setting::Auto { smooth } => smooth,
    };
    let level = match measure {
        // What CarlaOS did before it could measure: full picture at home,
        // a careful one elsewhere.
        None | Some(Measure { kbps: None, .. }) => {
            if away {
                Level::Balanced
            } else {
                Level::Sharp
            }
        }
        Some(Measure { kbps: Some(kbps), jitter_ms, latency_ms, away }) => {
            let mut top = if smooth && !away && latency_ms <= SMOOTH_LATENCY_MS { Level::Smooth } else { Level::Sharp };
            if jitter_ms > UNSTEADY_MS {
                top = top.min(Level::Balanced);
            }
            LEVELS.into_iter().rev().find(|level| *level <= top && kbps >= level.needs_kbps()).unwrap_or(Level::Lightest)
        }
    };
    Choice { level, automatic: true, measure }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A connection with a fixed delay and speed.
    fn line(latency_ms: u64, kbps: u64) -> impl Fn(usize) -> Option<Duration> {
        move |bytes| Some(Duration::from_millis(latency_ms) + Duration::from_secs_f64(bytes as f64 * 8.0 / 1000.0 / kbps as f64))
    }

    fn auto(measure: Measure) -> Level {
        choose(Setting::default(), Some(measure), measure.away).level
    }

    #[test]
    fn a_connection_is_measured_close_to_what_it_carries() {
        for kbps in [2_000u64, 15_000, 60_000, 400_000] {
            let m = measure(&line(6, kbps), false).unwrap();
            assert_eq!(m.latency_ms, 6);
            assert_eq!(m.jitter_ms, 0);
            let got = m.kbps.unwrap() as f64;
            assert!((got - kbps as f64).abs() / (kbps as f64) < 0.1, "{kbps} measured as {got}");
        }
    }

    #[test]
    fn a_slow_connection_is_not_kept_waiting_for_a_large_download() {
        let asked = RefCell::new(Vec::new());
        let slow = line(40, 1_500);
        let m = measure(
            &|bytes| {
                asked.borrow_mut().push(bytes);
                slow(bytes)
            },
            true,
        )
        .unwrap();
        assert!(m.kbps.unwrap() < 2_000);
        assert_eq!(asked.borrow().iter().filter(|b| **b > 0).count(), 1, "only the small download");
        // A fast one is asked for more, but never more than the limit.
        asked.borrow_mut().clear();
        let fast = line(2, 900_000);
        measure(
            &|bytes| {
                asked.borrow_mut().push(bytes);
                fast(bytes)
            },
            false,
        );
        assert_eq!(asked.borrow().last(), Some(&LARGEST_PROBE));
    }

    #[test]
    fn a_desktop_that_cannot_be_measured_gets_the_picture_it_always_had() {
        assert_eq!(measure(&|_| None, false), None);
        assert_eq!(choose(Setting::default(), None, false).level, Level::Sharp);
        assert_eq!(choose(Setting::default(), None, true).level, Level::Balanced);
        // Answers, but no data could be timed.
        let m = measure(&|bytes| (bytes == 0).then_some(Duration::from_millis(5)), true).unwrap();
        assert_eq!(m.kbps, None);
        assert_eq!(choose(Setting::default(), Some(m), true).level, Level::Balanced);
    }

    #[test]
    fn the_picture_follows_the_connection() {
        let on = |kbps| Measure { latency_ms: 5, jitter_ms: 2, kbps: Some(kbps), away: false };
        assert_eq!(auto(on(300_000)), Level::Sharp);
        assert_eq!(auto(on(45_000)), Level::Sharp);
        assert_eq!(auto(on(30_000)), Level::Balanced);
        assert_eq!(auto(on(14_000)), Level::Light);
        assert_eq!(auto(on(6_000)), Level::Lightest);
        assert_eq!(auto(on(500)), Level::Lightest, "still tries");
        // Every picture leaves room on the connection that earned it.
        for level in LEVELS {
            assert!(level.needs_kbps() > level.picture().kbps * 3 / 2);
        }
    }

    #[test]
    fn an_unsteady_connection_gets_a_lighter_picture() {
        let shaky = Measure { latency_ms: 20, jitter_ms: 90, kbps: Some(200_000), away: false };
        assert_eq!(auto(shaky), Level::Balanced);
    }

    #[test]
    fn a_hundred_and_twenty_only_when_allowed_at_home_on_a_quick_connection() {
        let allowed = Setting::Auto { smooth: true };
        let home = Measure { latency_ms: 3, jitter_ms: 1, kbps: Some(500_000), away: false };
        assert_eq!(choose(allowed, Some(home), false).level, Level::Smooth);
        assert_eq!(choose(Setting::default(), Some(home), false).level, Level::Sharp);
        assert_eq!(choose(allowed, Some(Measure { away: true, ..home }), true).level, Level::Sharp);
        assert_eq!(choose(allowed, Some(Measure { latency_ms: 30, ..home }), false).level, Level::Sharp);
        assert_eq!(choose(allowed, Some(Measure { kbps: Some(60_000), ..home }), false).level, Level::Sharp);
    }

    #[test]
    fn the_administrator_can_fix_the_picture() {
        let poor = Measure { latency_ms: 80, jitter_ms: 100, kbps: Some(1_000), away: true };
        let fixed = choose(Setting::Fixed(Level::Sharp), Some(poor), true);
        assert_eq!((fixed.level, fixed.automatic), (Level::Sharp, false));
    }

    #[test]
    fn the_picture_is_made_for_the_screen_it_is_shown_on() {
        let on = |level: Level, w, h, hz| level.picture_on(Some(Mode::new(w, h, hz)));
        // The Ally: what every level always was.
        for level in LEVELS {
            assert_eq!(on(level, 1920, 1080, 120), level.picture());
            assert_eq!(level.picture_on(None), level.picture());
        }
        // A 16:10 laptop keeps its shape and gets a little more to fill it.
        let laptop = on(Level::Sharp, 2560, 1600, 60);
        assert_eq!((laptop.width, laptop.height, laptop.fps), (1920, 1200, 60));
        assert_eq!(laptop.kbps, 27_777);
        assert_eq!((on(Level::Light, 2560, 1600, 60).width, on(Level::Light, 2560, 1600, 60).height), (1280, 800));
        // A small screen is not sent more than it shows.
        let small = on(Level::Sharp, 1366, 768, 60);
        assert_eq!((small.width, small.height), (1366, 768));
        assert!(small.kbps < Level::Sharp.picture().kbps && small.kbps >= Level::Sharp.picture().kbps / 2);
        // 120 a second is wasted on a screen that shows 60.
        let slow = on(Level::Smooth, 1920, 1080, 60);
        assert_eq!((slow.fps, slow.kbps), (60, 25_000));
    }

    #[test]
    fn settings_keep_their_names() {
        for setting in Setting::all() {
            assert_eq!(Setting::parse(setting.name()), Some(setting));
        }
        assert_eq!(Setting::all().len(), 7);
        assert_eq!(Setting::parse("ultra"), None);
    }
}
