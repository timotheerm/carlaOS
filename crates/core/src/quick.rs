//! The controller side of the quick menu: the menu that opens over a running
//! game. While a game is in front, CarlaOS itself does not have the
//! controller, so the buttons are read directly and turned into the shortcut
//! that opens the menu and the presses that move through it.
//!
//! This module only decides; reading the controller is the shell's job. The
//! button bits are the ones Windows reports for an Xbox-style controller.

use std::time::{Duration, Instant};

use serde::Serialize;

pub const UP: u16 = 0x0001;
pub const DOWN: u16 = 0x0002;
pub const LEFT: u16 = 0x0004;
pub const RIGHT: u16 = 0x0008;
/// The button with three lines, right of the middle.
pub const MENU: u16 = 0x0010;
/// The button with two squares, left of the middle.
pub const VIEW: u16 = 0x0020;
pub const A: u16 = 0x1000;
pub const B: u16 = 0x2000;

/// How long View and Menu are held together before the menu opens. Long
/// enough that pressing both by accident in a game does nothing.
pub const HOLD: Duration = Duration::from_millis(500);

/// What the controller is doing at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u16,
    /// Left stick, right and up positive, full range of an `i16`.
    pub stick: (i16, i16),
}

impl Pad {
    /// Several controllers count as one: anyone may open the menu.
    pub fn merged(pads: impl IntoIterator<Item = Pad>) -> Pad {
        pads.into_iter().fold(Pad::default(), |all, pad| Pad {
            buttons: all.buttons | pad.buttons,
            stick: if pad.stick.0.unsigned_abs().max(pad.stick.1.unsigned_abs()) > all.stick.0.unsigned_abs().max(all.stick.1.unsigned_abs()) { pad.stick } else { all.stick },
        })
    }
}

/// Notices View and Menu being held together.
#[derive(Debug, Default)]
pub struct Chord {
    since: Option<Instant>,
    fired: bool,
}

impl Chord {
    /// True once per hold, at the moment it has lasted [`HOLD`].
    pub fn observe(&mut self, buttons: u16, now: Instant) -> bool {
        if buttons & (VIEW | MENU) != (VIEW | MENU) {
            *self = Self::default();
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.duration_since(since) >= HOLD {
            self.fired = true;
            return true;
        }
        false
    }
}

/// One step through the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Press {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
}

const FIRST_REPEAT: Duration = Duration::from_millis(380);
const REPEAT: Duration = Duration::from_millis(130);
const STICK: i16 = 18_000;

/// Turns what is held into presses: once per press for A and B, repeating
/// for a direction that stays held.
#[derive(Debug)]
pub struct Presses {
    was: u16,
    direction: Option<(Press, Instant)>,
}

impl Presses {
    /// For a menu that has just opened. Whatever is held at that moment (the
    /// shortcut itself, or a direction the game was using) does nothing until
    /// it has been let go.
    pub fn opened() -> Self {
        Self { was: u16::MAX, direction: None }
    }

    fn direction(pad: Pad) -> Option<(Press, u16)> {
        let (x, y) = pad.stick;
        let stick = if x.unsigned_abs() > y.unsigned_abs() {
            (x > STICK).then_some(Press::Right).or((x < -STICK).then_some(Press::Left))
        } else {
            (y > STICK).then_some(Press::Up).or((y < -STICK).then_some(Press::Down))
        };
        [(UP, Press::Up), (DOWN, Press::Down), (LEFT, Press::Left), (RIGHT, Press::Right)]
            .into_iter()
            .find(|(bit, _)| pad.buttons & bit != 0)
            .map(|(bit, press)| (press, bit))
            // A stick counts as the same four buttons.
            .or(stick.map(|press| (press, 0x8000)))
    }

    pub fn observe(&mut self, pad: Pad, now: Instant) -> Vec<Press> {
        let mut out = Vec::new();
        let held = Self::direction(pad);
        let directions = UP | DOWN | LEFT | RIGHT | 0x8000;
        let mut down = pad.buttons & !directions;
        match held {
            Some((press, bit)) => {
                down |= bit;
                if self.was & directions != 0 && self.direction.is_none() {
                    // Still held from before the menu opened.
                } else {
                    match self.direction {
                        Some((last, due)) if last == press => {
                            if now >= due {
                                out.push(press);
                                self.direction = Some((press, now + REPEAT));
                            }
                        }
                        _ => {
                            out.push(press);
                            self.direction = Some((press, now + FIRST_REPEAT));
                        }
                    }
                }
            }
            None => self.direction = None,
        }
        for (bit, press) in [(A, Press::Select), (B, Press::Back)] {
            if pad.buttons & bit != 0 && self.was & bit == 0 {
                out.push(press);
            }
        }
        self.was = down;
        out
    }
}

/// Move a level (volume, brightness) one notch, staying between 0 and 100.
pub fn step(level: u8, up: bool) -> u8 {
    const NOTCH: u8 = 10;
    if up {
        // From 5 the next notch is 10, not 15.
        ((level / NOTCH + 1) * NOTCH).min(100)
    } else {
        (level.saturating_sub(1) / NOTCH) * NOTCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn the_menu_opens_once_when_view_and_menu_are_held_together() {
        let t = Instant::now();
        let mut chord = Chord::default();
        assert!(!chord.observe(VIEW, t), "one button alone is the game's");
        assert!(!chord.observe(VIEW | MENU, at(t, 100)));
        assert!(!chord.observe(VIEW | MENU | A, at(t, 400)), "not yet");
        assert!(chord.observe(VIEW | MENU, at(t, 650)));
        assert!(!chord.observe(VIEW | MENU, at(t, 2000)), "holding on does not open and close it");
        assert!(!chord.observe(0, at(t, 2100)));
        // A quick tap of both, as a game might ask for, does nothing.
        assert!(!chord.observe(VIEW | MENU, at(t, 2200)));
        assert!(!chord.observe(MENU, at(t, 2400)));
        assert!(!chord.observe(VIEW | MENU, at(t, 2500)));
        assert!(!chord.observe(VIEW | MENU, at(t, 2900)));
        assert!(chord.observe(VIEW | MENU, at(t, 3000)));
    }

    #[test]
    fn what_was_held_when_the_menu_opened_does_nothing_until_let_go() {
        let t = Instant::now();
        let mut presses = Presses::opened();
        let pad = |buttons| Pad { buttons, stick: (0, 0) };
        assert!(presses.observe(pad(VIEW | MENU | DOWN | A), t).is_empty());
        assert!(presses.observe(pad(DOWN | A), at(t, 50)).is_empty());
        assert!(presses.observe(pad(0), at(t, 100)).is_empty());
        assert_eq!(presses.observe(pad(DOWN), at(t, 150)), [Press::Down]);
        assert!(presses.observe(pad(DOWN), at(t, 300)).is_empty());
        assert_eq!(presses.observe(pad(DOWN), at(t, 540)), [Press::Down], "a held direction repeats");
        assert_eq!(presses.observe(pad(DOWN), at(t, 680)), [Press::Down]);
        assert_eq!(presses.observe(pad(RIGHT), at(t, 700)), [Press::Right], "a new direction acts at once");
        assert_eq!(presses.observe(pad(A), at(t, 750)), [Press::Select]);
        assert!(presses.observe(pad(A), at(t, 2000)).is_empty(), "A never repeats");
        assert_eq!(presses.observe(pad(B), at(t, 2050)), [Press::Back]);
    }

    #[test]
    fn the_stick_moves_through_the_menu_like_the_cross() {
        let t = Instant::now();
        let mut presses = Presses::opened();
        assert!(presses.observe(Pad::default(), t).is_empty());
        assert!(presses.observe(Pad { buttons: 0, stick: (3000, -9000) }, at(t, 10)).is_empty(), "a resting stick is never quite still");
        assert_eq!(presses.observe(Pad { buttons: 0, stick: (2000, -30_000) }, at(t, 20)), [Press::Down]);
        assert_eq!(presses.observe(Pad { buttons: 0, stick: (i16::MIN, 0) }, at(t, 30)), [Press::Left]);
        let both = Pad::merged([Pad { buttons: A, stick: (0, 100) }, Pad { buttons: B, stick: (0, 30_000) }]);
        assert_eq!(both, Pad { buttons: A | B, stick: (0, 30_000) });
    }

    #[test]
    fn levels_move_in_notches_and_stay_in_range() {
        assert_eq!((step(50, true), step(50, false)), (60, 40));
        assert_eq!((step(55, true), step(55, false)), (60, 50));
        assert_eq!((step(100, true), step(0, false)), (100, 0));
        assert_eq!((step(95, true), step(5, false)), (100, 0));
    }
}
