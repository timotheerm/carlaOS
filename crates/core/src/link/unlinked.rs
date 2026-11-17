//! When the desktop no longer knows this Ally: what the Ally shows, and how
//! seldom it asks again. Pure decisions, so that they can be tested.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::client::Error;

/// The one line the Ally shows for a desktop that has forgotten it.
pub const PROBLEM: &str = "This desktop does not know this Ally any more. Link it again.";

/// How often a desktop that forgot this Ally is asked again in the background,
/// so that one that comes back (linked again, or restored) is noticed without
/// asking every poll. Someone choosing "Check again" is not held to this.
pub const RECHECK: Duration = Duration::from_secs(5 * 60);

/// Whether an answer means the desktop does not accept this device any more.
pub fn is_unlinked(error: &Error) -> bool {
    matches!(error, Error::Refused)
}

/// What to make of every address of a desktop failing. A refusal counts only
/// when every address refused AND each refusal came from the desktop itself
/// (`from_desktop(i)`, confirmed by its own hello): a stale address now held
/// by another machine, or a server that is not CarlaOS, answers 401 as well
/// and says nothing about the link. Anything else is silence, or the
/// desktop's own problem in its words.
pub fn judge(errors: &[Error], from_desktop: impl Fn(usize) -> bool) -> Error {
    if !errors.is_empty() && errors.iter().all(is_unlinked) && (0..errors.len()).all(from_desktop) {
        return Error::Refused;
    }
    errors.iter().find(|e| matches!(e, Error::Host(_) | Error::Untrusted)).cloned().unwrap_or(Error::Unreachable)
}

/// What removing a desktop here told it, in a few words for the log.
pub fn leave_note(told: &Result<bool, Error>) -> &'static str {
    match told {
        Ok(true) => "told to forget this Ally",
        Ok(false) => "too old to be told",
        Err(Error::Refused) => "already did not know this Ally",
        Err(_) => "did not answer",
    }
}

/// The desktops that said they do not know this Ally. A refusal belongs to the
/// link (`identity`: its device id) that was refused, so one that finishes
/// after the desktop was linked again changes nothing.
#[derive(Debug, Default)]
pub struct Unlinked {
    /// Host id -> (identity refused, when asked).
    asked: HashMap<String, (String, Instant)>,
    /// Host id -> identity of the link kept now, once it was linked again.
    current: HashMap<String, String>,
}

impl Unlinked {
    /// Note that `id` refused the link `identity`, as of `now`. Ignored when
    /// that link has been replaced since.
    pub fn mark(&mut self, id: &str, identity: &str, now: Instant) {
        if self.current.get(id).is_some_and(|c| c != identity) {
            return;
        }
        self.asked.insert(id.to_string(), (identity.to_string(), now));
    }

    /// The desktop was linked again: `identity` is the new link.
    pub fn relinked(&mut self, id: &str, identity: &str) {
        self.asked.remove(id);
        self.current.insert(id.to_string(), identity.to_string());
    }

    /// The desktop answered (it accepts this Ally), or was removed.
    pub fn clear(&mut self, id: &str) {
        self.asked.remove(id);
    }

    /// The desktop was removed: nothing more is known about it.
    pub fn forget(&mut self, id: &str) {
        self.asked.remove(id);
        self.current.remove(id);
    }

    pub fn is_unlinked(&self, id: &str) -> bool {
        self.asked.contains_key(id)
    }

    /// Whether `id`, linked as `identity`, may be asked in the background now:
    /// always unless that link was refused, then only once `RECHECK` has passed.
    pub fn may_ask(&self, id: &str, identity: &str, now: Instant) -> bool {
        match self.asked.get(id) {
            Some((refused, at)) if refused == identity => now.saturating_duration_since(*at) >= RECHECK,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refusal_is_unlinked() {
        assert!(is_unlinked(&Error::Refused));
        assert!(!is_unlinked(&Error::Unreachable));
        assert!(!is_unlinked(&Error::Untrusted));
        assert!(!is_unlinked(&Error::Host("x".into())));
    }

    #[test]
    fn asks_at_most_every_five_minutes_after_a_refusal() {
        let start = Instant::now();
        let mut u = Unlinked::default();
        assert!(u.may_ask("a", "d1", start));
        u.mark("a", "d1", start);
        assert!(u.is_unlinked("a"));
        assert!(!u.may_ask("a", "d1", start + Duration::from_secs(60)));
        assert!(!u.may_ask("a", "d1", start + RECHECK - Duration::from_secs(1)));
        assert!(u.may_ask("a", "d1", start + RECHECK));
        // Another desktop is not held back.
        assert!(u.may_ask("b", "d1", start));
        u.clear("a");
        assert!(!u.is_unlinked("a"));
        assert!(u.may_ask("a", "d1", start));
    }

    #[test]
    fn a_refusal_that_finishes_after_relinking_changes_nothing() {
        let start = Instant::now();
        let mut u = Unlinked::default();
        u.relinked("a", "new");
        // The look that began with the old link ends late.
        u.mark("a", "old", start);
        assert!(!u.is_unlinked("a"));
        assert!(u.may_ask("a", "new", start));
        // A refusal of the link kept now still counts.
        u.mark("a", "new", start);
        assert!(!u.may_ask("a", "new", start));
        // A lock left from the old link never holds the new one.
        let mut v = Unlinked::default();
        v.mark("a", "old", start);
        assert!(!v.may_ask("a", "old", start));
        v.relinked("a", "new");
        assert!(v.may_ask("a", "new", start) && v.may_ask("a", "old", start));
        // Removed and linked from nothing: marks apply again.
        v.forget("a");
        v.mark("a", "x", start);
        assert!(v.is_unlinked("a"));
    }

    #[test]
    fn a_stale_address_or_a_stranger_is_not_a_refusal_by_the_desktop() {
        let yes = |_: usize| true;
        // Every address refused, each by the desktop itself: unlinked.
        assert_eq!(judge(&[Error::Refused, Error::Refused], yes), Error::Refused);
        // Another address is silent: the desktop may just be asleep.
        assert_eq!(judge(&[Error::Refused, Error::Unreachable], yes), Error::Unreachable);
        // The 401 came from something that is not this desktop.
        assert_eq!(judge(&[Error::Refused], |_| false), Error::Unreachable);
        assert_eq!(judge(&[Error::Refused, Error::Refused], |i| i == 0), Error::Unreachable);
        // The desktop's own words say more than silence.
        assert_eq!(judge(&[Error::Unreachable, Error::Host("busy".into())], yes), Error::Host("busy".into()));
        assert_eq!(judge(&[], yes), Error::Unreachable);
    }

    #[test]
    fn says_in_a_few_words_what_removing_told_the_desktop() {
        assert_eq!(leave_note(&Ok(true)), "told to forget this Ally");
        assert_eq!(leave_note(&Ok(false)), "too old to be told");
        assert_eq!(leave_note(&Err(Error::Refused)), "already did not know this Ally");
        assert_eq!(leave_note(&Err(Error::Unreachable)), "did not answer");
    }
}
