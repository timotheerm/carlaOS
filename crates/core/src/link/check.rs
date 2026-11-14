//! Checking a linked desktop from the Ally, step by step, and saying what
//! would fix each problem.
//!
//! Streaming needs a chain of things to hold at once: the desktop answers,
//! it still accepts this Ally, Sunshine runs there, Moonlight is installed
//! here and paired with that Sunshine, and the connection carries a picture.
//! When a stream does not start, the player only sees that it did not. The
//! check walks the chain in order, so the first broken link is named in
//! words, with the one action that repairs it when CarlaOS can do that itself.
//!
//! The steps are worked out here from answers the caller gathers, so the same
//! report serves the Ally's screen and `carla-companion ally check` on a desktop.

use serde::{Deserialize, Serialize};

use super::client::Error;
use super::quality::{self, Measure, Setting};
use super::{State, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    /// Streams work, but something is worth knowing or doing.
    Warn,
    /// Streams cannot work until this is fixed.
    Fail,
    /// Not looked at, because an earlier step failed.
    Skipped,
}

/// What CarlaOS can do about a step by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    /// Pair Moonlight with the desktop's Sunshine again, over the link.
    Pair,
    /// Send the wake-up packet.
    Wake,
    /// Update the companion on the desktop.
    Update,
    /// The link itself is gone: a new code from the desktop is needed.
    Relink,
}

impl Fix {
    /// The button, or the command on a desktop.
    pub fn words(self) -> &'static str {
        match self {
            Fix::Pair => "Pair streaming again",
            Fix::Wake => "Wake it",
            Fix::Update => "Update the desktop",
            Fix::Relink => "Link it again",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Stable name of the step, for the interface.
    pub key: String,
    pub title: String,
    pub outcome: Outcome,
    /// One or two sentences for the administrator.
    pub detail: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub name: String,
    pub steps: Vec<Step>,
    /// What was repaired on the way, in words, when repairs were asked for.
    #[serde(default)]
    pub repaired: Vec<String>,
}

impl Report {
    /// The worst outcome among the steps.
    pub fn outcome(&self) -> Outcome {
        let steps = || self.steps.iter().map(|s| s.outcome);
        if steps().any(|o| o == Outcome::Fail) {
            Outcome::Fail
        } else if steps().any(|o| o == Outcome::Warn) {
            Outcome::Warn
        } else {
            Outcome::Ok
        }
    }

    /// The first thing CarlaOS could fix by itself, if any.
    pub fn first_fix(&self) -> Option<Fix> {
        self.steps.iter().find(|s| s.outcome == Outcome::Fail).or_else(|| self.steps.iter().find(|s| s.outcome == Outcome::Warn)).and_then(|s| s.fix)
    }

    /// The report as lines of text, for a terminal.
    pub fn words(&self) -> String {
        let mut lines = vec![format!("{}:", self.name)];
        for step in &self.steps {
            let mark = match step.outcome {
                Outcome::Ok => "ok  ",
                Outcome::Warn => "note",
                Outcome::Fail => "FAIL",
                Outcome::Skipped => "  - ",
            };
            lines.push(format!("  {mark}  {}: {}", step.title, step.detail));
        }
        for done in &self.repaired {
            lines.push(format!("  done  {done}"));
        }
        lines.push(
            match self.outcome() {
                Outcome::Ok => "Everything needed for streaming is in place.".to_string(),
                _ if self.repaired.is_empty() => match self.first_fix() {
                    Some(Fix::Relink) => "Link the desktop again: a new code on the desktop (carla-companion code), then Settings, Desktops, Add a desktop on the Ally.".to_string(),
                    Some(fix) => format!("CarlaOS can fix this itself: {}. Add --repair to do it now.", fix.words().to_lowercase()),
                    None => "This needs fixing on the desktop or the Ally itself; the line marked above says how.".to_string(),
                },
                _ => "Some of it is still not right; the lines above say what.".to_string(),
            }
            .to_string(),
        );
        lines.join("\n")
    }
}

/// What the caller found out about a desktop, gathered before the steps are
/// worked out. Only what can be known is filled in: after the desktop failed
/// to answer, nothing past `answer` is asked.
pub struct Facts<'a> {
    pub ally_version: &'a str,
    /// The desktop's answer: its status, where it answered, how quickly.
    pub answer: Result<(Status, String, u64), Error>,
    /// The desktop said once how to wake it.
    pub can_wake: bool,
    /// Moonlight is installed on this Ally.
    pub moonlight: bool,
    /// Moonlight is paired with the desktop's Sunshine. `None` when it was
    /// not asked.
    pub paired: Option<bool>,
    pub measure: Option<Measure>,
    pub setting: Setting,
}

fn step(key: &str, title: &str, outcome: Outcome, detail: impl Into<String>, fix: Option<Fix>) -> Step {
    Step { key: key.to_string(), title: title.to_string(), outcome, detail: detail.into(), fix }
}

fn skipped(key: &str, title: &str, why: &str) -> Step {
    step(key, title, Outcome::Skipped, why, None)
}

/// Delay past which a home network is worth a note: a stream still plays
/// but feels slower than it should.
const SLOW_AT_HOME_MS: u32 = 20;

/// Work out the steps from what was found.
pub fn steps(facts: &Facts) -> Vec<Step> {
    let mut out = Vec::new();
    let status = match &facts.answer {
        Ok((status, address, ms)) => {
            let route = if super::wake::is_tailscale(address) { "through Tailscale, away from home" } else { "on the home network" };
            out.push(step("answer", "Answers", Outcome::Ok, format!("At {address}, {route}, in {ms} ms."), None));
            Some(status)
        }
        Err(Error::Unreachable) if facts.can_wake => {
            out.push(step("answer", "Answers", Outcome::Fail, "It did not answer. It is probably asleep or off; CarlaOS can wake it.", Some(Fix::Wake)));
            None
        }
        Err(Error::Unreachable) => {
            out.push(step(
                "answer",
                "Answers",
                Outcome::Fail,
                "It did not answer, and it has not said yet how to wake it. Turn it on by hand and make sure the companion runs there.",
                None,
            ));
            None
        }
        Err(Error::Refused) => {
            out.push(step(
                "answer",
                "Answers",
                Outcome::Fail,
                "It answered but no longer accepts this Ally: it was forgotten there, or set up again. It needs a new code.",
                Some(Fix::Relink),
            ));
            None
        }
        Err(Error::Untrusted) => {
            out.push(step(
                "answer",
                "Answers",
                Outcome::Fail,
                "Something answered at its address, but not the desktop this Ally linked. The router may have given its address to another computer.",
                Some(Fix::Relink),
            ));
            None
        }
        Err(Error::Host(text)) => {
            out.push(step("answer", "Answers", Outcome::Fail, text.clone(), None));
            None
        }
    };
    let Some(status) = status else {
        let why = "Needs the desktop to answer first.";
        out.push(skipped("version", "Same CarlaOS", why));
        out.push(skipped("sunshine", "Sunshine", why));
        out.push(moonlight_step(facts));
        out.push(skipped("paired", "Paired for streaming", why));
        out.push(waking_step(facts));
        out.push(skipped("connection", "Connection", why));
        return out;
    };

    out.push(if status.version == facts.ally_version {
        step("version", "Same CarlaOS", Outcome::Ok, format!("Both have {}.", status.version), None)
    } else {
        step(
            "version",
            "Same CarlaOS",
            Outcome::Warn,
            format!("The desktop has {}, the Ally {}. They still work together; updating keeps them in step.", status.version, facts.ally_version),
            Some(Fix::Update),
        )
    });

    out.push(match status.state {
        State::Ready => step("sunshine", "Sunshine", Outcome::Ok, "Running and accepting the companion.", None),
        State::Busy => step(
            "sunshine",
            "Sunshine",
            Outcome::Ok,
            format!("Running. {} is being played there now.", status.playing.as_deref().unwrap_or("A game")),
            None,
        ),
        State::NotReady => step(
            "sunshine",
            "Sunshine",
            Outcome::Fail,
            format!("{} This is fixed on the desktop: carla-companion status says more there.", status.problem.as_deref().unwrap_or("Not ready.")),
            None,
        ),
    });

    out.push(moonlight_step(facts));
    out.push(match (facts.moonlight, facts.paired) {
        (false, _) => skipped("paired", "Paired for streaming", "Needs Moonlight on the Ally first."),
        (true, Some(true)) => step("paired", "Paired for streaming", Outcome::Ok, "Moonlight on the Ally and Sunshine on the desktop know each other.", None),
        (true, Some(false)) => step(
            "paired",
            "Paired for streaming",
            Outcome::Fail,
            "Moonlight on the Ally is not paired with Sunshine there, so streams cannot start. CarlaOS pairs them over the link; nobody types a PIN.",
            Some(Fix::Pair),
        ),
        (true, None) => skipped("paired", "Paired for streaming", "Not asked."),
    });

    out.push(waking_step(facts));

    out.push(match facts.measure {
        Some(m) => {
            let away = super::wake::is_tailscale(facts.answer.as_ref().map(|(_, a, _)| a.as_str()).unwrap_or_default());
            let choice = quality::choose(facts.setting, Some(m), away);
            let measured = choice.measured();
            let slow_at_home = !m.away && m.latency_ms > SLOW_AT_HOME_MS;
            let picture = if choice.automatic { format!("A stream now would get {}.", choice.level.words()) } else { format!("Streams are set to {}.", choice.level.words()) };
            if slow_at_home {
                step(
                    "connection",
                    "Connection",
                    Outcome::Warn,
                    format!("Measured {measured}. {picture} That is slow for a home network; a cable to the desktop, or the Ally nearer the router, helps."),
                    None,
                )
            } else {
                step("connection", "Connection", Outcome::Ok, format!("Measured {measured}. {picture}"), None)
            }
        }
        None => step("connection", "Connection", Outcome::Warn, "Could not be measured. Streams use a careful picture until it can be.", None),
    });
    out
}

fn moonlight_step(facts: &Facts) -> Step {
    if facts.moonlight {
        step("moonlight", "Moonlight on the Ally", Outcome::Ok, "Installed.", None)
    } else {
        step(
            "moonlight",
            "Moonlight on the Ally",
            Outcome::Fail,
            "Not installed. Install Moonlight (moonlight-stream.org) on the Ally once; CarlaOS does the rest.",
            None,
        )
    }
}

fn waking_step(facts: &Facts) -> Step {
    if facts.can_wake {
        step("waking", "Waking from the Ally", Outcome::Ok, "The Ally can wake it when a game from it is chosen.", None)
    } else {
        step(
            "waking",
            "Waking from the Ally",
            Outcome::Warn,
            "It has not said how to wake it yet. It does the first time it answers with a newer companion; until then it has to be turned on by hand.",
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: State, version: &str) -> Status {
        Status {
            name: "HOME PC".into(),
            id: "home".into(),
            version: version.into(),
            state,
            playing: None,
            problem: (state == State::NotReady).then(|| "Sunshine is not running on this desktop.".to_string()),
            hardware: String::new(),
            addresses: vec![],
            macs: vec![],
        }
    }

    fn facts(answer: Result<(Status, String, u64), Error>) -> Facts<'static> {
        Facts {
            ally_version: "0.5.0",
            answer,
            can_wake: true,
            moonlight: true,
            paired: Some(true),
            measure: Some(Measure { latency_ms: 3, jitter_ms: 1, kbps: Some(200_000), away: false }),
            setting: Setting::default(),
        }
    }

    fn report(steps: Vec<Step>) -> Report {
        Report { id: "home".into(), name: "HOME PC".into(), steps, repaired: vec![] }
    }

    fn outcome(steps: &[Step], key: &str) -> Outcome {
        steps.iter().find(|s| s.key == key).unwrap().outcome
    }

    #[test]
    fn a_healthy_desktop_passes_every_step() {
        let steps = steps(&facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4))));
        assert_eq!(steps.len(), 7);
        assert!(steps.iter().all(|s| s.outcome == Outcome::Ok), "{steps:#?}");
        let r = report(steps);
        assert_eq!(r.outcome(), Outcome::Ok);
        assert_eq!(r.first_fix(), None);
        assert!(r.words().contains("Everything needed for streaming is in place."));
    }

    #[test]
    fn an_unpaired_moonlight_is_named_and_pairing_is_offered() {
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4)));
        f.paired = Some(false);
        let steps = steps(&f);
        assert_eq!(outcome(&steps, "paired"), Outcome::Fail);
        let r = report(steps);
        assert_eq!(r.first_fix(), Some(Fix::Pair));
        assert!(r.words().contains("--repair"));
    }

    #[test]
    fn a_silent_desktop_skips_what_needs_its_answer_but_still_checks_the_ally() {
        let mut f = facts(Err(Error::Unreachable));
        f.moonlight = false;
        let steps = steps(&f);
        assert_eq!(outcome(&steps, "answer"), Outcome::Fail);
        assert_eq!(outcome(&steps, "version"), Outcome::Skipped);
        assert_eq!(outcome(&steps, "paired"), Outcome::Skipped);
        assert_eq!(outcome(&steps, "moonlight"), Outcome::Fail, "known without the desktop");
        assert_eq!(report(steps).first_fix(), Some(Fix::Wake));
        let mut f = facts(Err(Error::Unreachable));
        f.can_wake = false;
        assert_eq!(report(super::steps(&f)).first_fix(), None);
    }

    #[test]
    fn a_desktop_that_forgot_the_ally_needs_a_new_code() {
        for e in [Error::Refused, Error::Untrusted] {
            let r = report(steps(&facts(Err(e))));
            assert_eq!(r.first_fix(), Some(Fix::Relink));
            assert!(r.words().contains("carla-companion code"));
        }
    }

    #[test]
    fn sunshine_down_is_a_failure_that_is_fixed_on_the_desktop() {
        let steps = steps(&facts(Ok((status(State::NotReady, "0.5.0"), "192.168.1.20".into(), 4))));
        let sunshine = steps.iter().find(|s| s.key == "sunshine").unwrap();
        assert_eq!(sunshine.outcome, Outcome::Fail);
        assert!(sunshine.detail.contains("Sunshine is not running"));
        assert_eq!(sunshine.fix, None);
    }

    #[test]
    fn different_versions_and_a_slow_home_network_are_notes_not_failures() {
        let mut f = facts(Ok((status(State::Busy, "0.4.0"), "192.168.1.20".into(), 40)));
        f.measure = Some(Measure { latency_ms: 35, jitter_ms: 5, kbps: Some(30_000), away: false });
        let steps = steps(&f);
        assert_eq!(outcome(&steps, "version"), Outcome::Warn);
        assert_eq!(outcome(&steps, "connection"), Outcome::Warn);
        let r = report(steps);
        assert_eq!(r.outcome(), Outcome::Warn);
        assert_eq!(r.first_fix(), Some(Fix::Update));
    }

    #[test]
    fn a_delay_through_tailscale_is_expected() {
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "100.101.1.2".into(), 45)));
        f.measure = Some(Measure { latency_ms: 45, jitter_ms: 5, kbps: Some(30_000), away: true });
        let steps = steps(&f);
        assert_eq!(outcome(&steps, "connection"), Outcome::Ok);
        assert!(steps[0].detail.contains("Tailscale"));
    }

    #[test]
    fn the_report_travels_as_json() {
        let r = report(steps(&facts(Err(Error::Refused))));
        let back: Report = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(serde_json::to_value(Fix::Relink).unwrap(), "relink");
    }
}
