//! The screen: what it shows now, what it can show, and changing it for the
//! length of a stream.
//!
//! A stream looks right when the desktop's screen has the shape and size of
//! the Ally's. Otherwise the picture is squeezed or has black bars, and text
//! is scaled twice. So the Ally asks for a picture made for its own screen
//! ([`fit`]), and the desktop switches its screen to the nearest mode it has
//! ([`choose`]) until the stream is over.
//!
//! The switch is never written to the Windows settings. It lasts until it is
//! taken back, the program that made it ends, or the user signs out, so a
//! desktop cannot be left in the wrong mode for good.

use serde::{Deserialize, Serialize};

/// One way a screen can be driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Pictures a second. 0 when not known.
    pub hz: u32,
}

impl Mode {
    pub const fn new(width: u32, height: u32, hz: u32) -> Self {
        Self { width, height, hz }
    }

    /// "1920x1080 at 60".
    pub fn words(self) -> String {
        format!("{}x{} at {}", self.width, self.height, self.hz)
    }
}

/// The size of the picture to stream to a screen: the screen's own size when
/// it is no larger than `most`, otherwise the largest size of the same shape
/// that is. Sides are even, as video needs.
pub fn fit(screen: (u32, u32), most: (u32, u32)) -> (u32, u32) {
    let (w, h) = (u64::from(screen.0), u64::from(screen.1));
    let (mw, mh) = (u64::from(most.0), u64::from(most.1));
    if w == 0 || h == 0 {
        return most;
    }
    let even = |n: u64| ((n.max(2) / 2) * 2) as u32;
    if w <= mw && h <= mh {
        return (even(w), even(h));
    }
    // Whichever side runs out first decides.
    if w * mh >= h * mw {
        (even(mw), even((h * mw + w / 2) / w))
    } else {
        (even((w * mh + h / 2) / h), even(mh))
    }
}

fn same_shape(a: Mode, b: Mode) -> bool {
    let (x, y) = (u64::from(a.width) * u64::from(b.height), u64::from(b.width) * u64::from(a.height));
    x.abs_diff(y) * 100 <= x.max(y)
}

/// The refresh rate to use for `fps` pictures a second, from those a size
/// offers: the same rate, else the lowest whole multiple (120 for 60 shows
/// every picture for the same time), else the lowest that is faster, else
/// the fastest there is.
fn rate(mut offered: Vec<u32>, fps: u32) -> u32 {
    offered.sort_unstable();
    offered.dedup();
    let fps = fps.max(1);
    // Windows calls 59.94 a second "59": as good, when the round rate is missing.
    let even = (1..=4).flat_map(|n| [fps * n, (fps * n).saturating_sub(1)]).find(|hz| offered.contains(hz));
    let faster = || offered.iter().copied().find(|hz| *hz > fps);
    even.or_else(faster).or(offered.last().copied()).unwrap_or(0)
}

/// The mode a screen should take to show a stream made for `want`, out of
/// the modes it has. `None` when it has nothing suitable: then it is better
/// left alone.
pub fn choose(modes: &[Mode], want: Mode) -> Option<Mode> {
    let sized = |w: u32, h: u32| modes.iter().any(|m| m.width == w && m.height == h).then_some((w, h));
    let exact = sized(want.width, want.height);
    // The smallest mode of the same shape that loses nothing.
    let same_shape = || {
        modes
            .iter()
            .filter(|m| same_shape(**m, want) && m.height >= want.height)
            .min_by_key(|m| (m.height, m.width))
            .map(|m| (m.width, m.height))
    };
    // As wide, and as near in height as there is: black bars, but no blur.
    let same_width = || {
        modes
            .iter()
            .filter(|m| m.width == want.width)
            .min_by_key(|m| m.height.abs_diff(want.height))
            .map(|m| (m.width, m.height))
    };
    let (width, height) = exact.or_else(same_shape).or_else(same_width)?;
    let offered = modes.iter().filter(|m| m.width == width && m.height == height).map(|m| m.hz).collect();
    Some(Mode { width, height, hz: rate(offered, want.hz) })
}

/// What to switch a screen to for a stream, or `None` when it is already
/// right or has nothing suitable.
pub fn plan(current: Option<Mode>, modes: &[Mode], want: Mode) -> Option<Mode> {
    let chosen = choose(modes, want)?;
    (Some(chosen) != current).then_some(chosen)
}

#[cfg(windows)]
mod os {
    use super::Mode;
    use windows_sys::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_FULLSCREEN, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
        ENUM_CURRENT_SETTINGS,
    };

    fn blank() -> DEVMODEW {
        // SAFETY: DEVMODEW is plain data for which all zeroes is valid.
        let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        mode
    }

    fn read(index: u32) -> Option<Mode> {
        let mut mode = blank();
        // SAFETY: a null name means the main screen; `mode` is a valid place
        // of the size it says it has.
        let found = unsafe { EnumDisplaySettingsW(std::ptr::null(), index, &mut mode) } != 0;
        found.then_some(Mode { width: mode.dmPelsWidth, height: mode.dmPelsHeight, hz: mode.dmDisplayFrequency })
    }

    pub fn current() -> Option<Mode> {
        read(ENUM_CURRENT_SETTINGS)
    }

    pub fn modes() -> Vec<Mode> {
        let mut found: Vec<Mode> = Vec::new();
        // Windows lists each mode several times over (colour depth, scaling).
        for mode in (0..).map_while(read) {
            if mode.width > 0 && mode.height > 0 && !found.contains(&mode) {
                found.push(mode);
            }
        }
        found
    }

    pub fn apply(to: Mode) -> Result<(), String> {
        let mut mode = blank();
        mode.dmPelsWidth = to.width;
        mode.dmPelsHeight = to.height;
        mode.dmDisplayFrequency = to.hz;
        mode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | if to.hz > 0 { DM_DISPLAYFREQUENCY } else { 0 };
        // CDS_FULLSCREEN: for now only, never written to the settings.
        // SAFETY: a null name means the main screen; `mode` is valid.
        let result = unsafe { ChangeDisplaySettingsExW(std::ptr::null(), &mode, std::ptr::null_mut(), CDS_FULLSCREEN, std::ptr::null()) };
        if result == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!("Windows did not switch the screen to {} (answer {result}).", to.words()))
        }
    }

    pub fn restore() -> Result<(), String> {
        // SAFETY: no mode at all means "what the settings say".
        let result = unsafe { ChangeDisplaySettingsExW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null()) };
        if result == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!("Windows did not put the screen back (answer {result})."))
        }
    }
}

#[cfg(not(windows))]
mod os {
    use super::Mode;

    pub fn current() -> Option<Mode> {
        None
    }

    pub fn modes() -> Vec<Mode> {
        Vec::new()
    }

    pub fn apply(_to: Mode) -> Result<(), String> {
        Err("The screen can only be switched on Windows.".to_string())
    }

    pub fn restore() -> Result<(), String> {
        Ok(())
    }
}

/// What the main screen shows now.
pub fn current() -> Option<Mode> {
    os::current()
}

/// Every mode the main screen has.
pub fn modes() -> Vec<Mode> {
    os::modes()
}

/// Switch the main screen, for now only.
pub fn apply(to: Mode) -> Result<(), String> {
    os::apply(to)
}

/// Put the main screen back to what the Windows settings say.
pub fn restore() -> Result<(), String> {
    os::restore()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn m(width: u32, height: u32, hz: u32) -> Mode {
        Mode::new(width, height, hz)
    }

    /// A 1440p desktop monitor that runs at up to 165.
    fn desktop() -> Vec<Mode> {
        let sizes = [(2560, 1440), (1920, 1080), (1680, 1050), (1600, 900), (1280, 720), (1280, 800), (1024, 768)];
        sizes.into_iter().flat_map(|(w, h)| [59, 60, 120, 144, 165].map(|hz| m(w, h, hz))).collect()
    }

    #[test]
    fn a_stream_is_made_for_the_screen_it_is_shown_on() {
        let most = (1920, 1200);
        assert_eq!(fit((1920, 1080), most), (1920, 1080), "the Ally's own screen");
        assert_eq!(fit((2560, 1600), most), (1920, 1200), "a 16:10 laptop keeps its shape");
        assert_eq!(fit((3840, 2160), most), (1920, 1080));
        assert_eq!(fit((2880, 1800), most), (1920, 1200));
        assert_eq!(fit((3440, 1440), most), (1920, 804), "an ultrawide keeps its shape too");
        assert_eq!(fit((1366, 768), most), (1366, 768), "a small screen gets its own size");
        assert_eq!(fit((1600, 1200), most), (1600, 1200));
        assert_eq!(fit((2048, 1536), most), (1600, 1200), "height decides for a tall screen");
        assert_eq!(fit((1920, 1080), (1280, 800)), (1280, 720));
        assert_eq!(fit((0, 0), most), most);
        assert_eq!(fit((1367, 769), most), (1366, 768), "sides are even");
    }

    #[test]
    fn the_desktop_takes_the_allys_size_when_it_has_it() {
        assert_eq!(choose(&desktop(), m(1920, 1080, 60)), Some(m(1920, 1080, 60)));
        assert_eq!(choose(&desktop(), m(1920, 1080, 120)), Some(m(1920, 1080, 120)));
        assert_eq!(choose(&desktop(), m(1280, 720, 30)), Some(m(1280, 720, 60)), "60 shows 30 evenly");
    }

    #[test]
    fn without_the_exact_size_it_takes_the_nearest_of_the_same_shape() {
        // 1366x768 is not offered; 1600x900 is the smallest 16:9 that loses nothing.
        assert_eq!(choose(&desktop(), m(1366, 768, 60)), Some(m(1600, 900, 60)));
        // A 16:10 laptop on a 16:9 monitor: no 1920x1200, so as wide as asked.
        assert_eq!(choose(&desktop(), m(1920, 1200, 60)), Some(m(1920, 1080, 60)));
        // Nothing of that shape or width at all: leave the screen alone.
        assert_eq!(choose(&desktop(), m(1920, 804, 60)), Some(m(1920, 1080, 60)));
        assert_eq!(choose(&desktop(), m(900, 1600, 60)), None);
        assert_eq!(choose(&[], m(1920, 1080, 60)), None);
    }

    #[test]
    fn the_refresh_rate_shows_every_picture_evenly_when_it_can() {
        assert_eq!(rate(vec![59, 60, 144], 60), 60, "60 before 59.94");
        assert_eq!(rate(vec![59, 144], 60), 59, "59.94 counts as 60");
        assert_eq!(rate(vec![144, 120, 75], 60), 120, "twice as fast shows each picture twice");
        assert_eq!(rate(vec![75, 144], 60), 75);
        assert_eq!(rate(vec![50], 60), 50);
        assert_eq!(rate(vec![60, 90, 120], 30), 60);
        assert_eq!(rate(vec![], 60), 0);
    }

    #[test]
    fn a_screen_that_is_already_right_is_not_touched() {
        let modes = desktop();
        assert_eq!(plan(Some(m(1920, 1080, 60)), &modes, m(1920, 1080, 60)), None);
        assert_eq!(plan(Some(m(2560, 1440, 165)), &modes, m(1920, 1080, 60)), Some(m(1920, 1080, 60)));
        // Same size but the wrong rate is worth a switch: 165 shows 60 unevenly.
        assert_eq!(plan(Some(m(1920, 1080, 165)), &modes, m(1920, 1080, 60)), Some(m(1920, 1080, 60)));
        assert_eq!(plan(None, &modes, m(1920, 1080, 60)), Some(m(1920, 1080, 60)));
    }
}
