//! Noticing that the machine has been asleep.
//!
//! A handheld is put to sleep in the middle of everything: with a game open,
//! with a stream running, on the home page. CarlaOS has to notice on the way
//! back, so that time asleep is not counted as time played, a stream that
//! was cut is picked up again, and the desktops are asked afresh how they
//! are.
//!
//! Sleep is recognised the plain way: something that looks at the clock
//! every second, and finds that far more than a second went by, did not run
//! in between. That holds for classic sleep, for hibernation and for the
//! "modern standby" handhelds use, where programs are simply frozen.

/// A gap in the ticks longer than this was sleep, not a busy machine.
pub const SHORTEST: i64 = 8_000;

/// One sleep, as noticed on waking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nap {
    /// Unix milliseconds of the last tick before it.
    pub from: i64,
    /// Unix milliseconds of the first tick after it.
    pub until: i64,
}

impl Nap {
    pub fn seconds(&self) -> i64 {
        (self.until - self.from) / 1000
    }
}

/// Looks at the clock tick by tick.
#[derive(Debug)]
pub struct Napper {
    last: i64,
}

impl Napper {
    pub fn new(now_ms: i64) -> Self {
        Self { last: now_ms }
    }

    /// Call about once a second with the wall clock. Says when the time since
    /// the call before can only have been sleep.
    pub fn observe(&mut self, now_ms: i64) -> Option<Nap> {
        let from = std::mem::replace(&mut self.last, now_ms);
        // A clock that was set back is not sleep, and neither is one tick
        // that came late.
        (now_ms - from >= SHORTEST).then_some(Nap { from, until: now_ms })
    }
}

/// How much of a nap falls inside something that began at `started` (Unix
/// seconds): the part to take off its length.
pub fn inside(nap: Nap, started: i64) -> i64 {
    (nap.until / 1000 - (nap.from / 1000).max(started)).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_ticks_are_not_sleep() {
        let mut napper = Napper::new(10_000);
        assert_eq!(napper.observe(11_000), None);
        assert_eq!(napper.observe(12_050), None);
        // A machine too busy to tick for a few seconds was not asleep.
        assert_eq!(napper.observe(17_000), None);
    }

    #[test]
    fn a_long_gap_is_sleep_and_is_said_once() {
        let mut napper = Napper::new(10_000);
        napper.observe(11_000);
        let nap = napper.observe(11_000 + 3_600_000).expect("an hour went by");
        assert_eq!((nap.from, nap.until, nap.seconds()), (11_000, 3_611_000, 3600));
        assert_eq!(napper.observe(3_612_000), None);
    }

    #[test]
    fn a_clock_set_back_is_not_sleep() {
        let mut napper = Napper::new(100_000);
        assert_eq!(napper.observe(40_000), None);
        assert_eq!(napper.observe(41_000), None);
    }

    #[test]
    fn only_the_part_of_a_nap_inside_a_game_is_taken_off() {
        let nap = Nap { from: 1_000_000, until: 1_600_000 };
        assert_eq!(inside(nap, 500), 600, "the game was open the whole time");
        assert_eq!(inside(nap, 1_300), 300, "the game began during the gap");
        assert_eq!(inside(nap, 2_000), 0, "the game began afterwards");
    }
}
