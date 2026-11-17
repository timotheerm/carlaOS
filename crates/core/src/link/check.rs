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
use super::codec;
use super::health::{Grade, Health};
use super::quality::{self, Measure, Setting};
use super::{ScreenFit, State, Status};
use crate::display::Mode;

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
    /// Install Moonlight on the Ally.
    Moonlight,
}

impl Fix {
    /// The button, or the command on a desktop.
    pub fn words(self) -> &'static str {
        match self {
            Fix::Pair => "Pair streaming again",
            Fix::Wake => "Wake it",
            Fix::Update => "Update the desktop",
            Fix::Relink => "Link it again",
            Fix::Moonlight => "Install Moonlight",
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
    /// The connection in figures, when it could be measured.
    #[serde(default)]
    pub health: Option<Health>,
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
        if let Some(health) = &self.health {
            lines.push(format!("  {}", health.words()));
        }
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
    /// Whether each of Sunshine's ports opened from the Ally. `None` when
    /// they were not tried.
    pub ports: Option<Vec<(u16, bool)>>,
    /// The Ally's own screen, and what the desktop would give a stream of
    /// its size. `None` when the desktop is too old to say.
    pub screen: Option<Mode>,
    pub fit: Option<ScreenFit>,
    /// The names of the Ally's graphics chips.
    pub graphics: Vec<String>,
    /// Unix seconds, for dating the measurement.
    pub now: i64,
}

impl Facts<'_> {
    fn address(&self) -> &str {
        self.answer.as_ref().map(|(_, a, _)| a.as_str()).unwrap_or_default()
    }

    /// The codec the two machines can be expected to settle on.
    pub fn codec(&self) -> Option<codec::Fit> {
        let status = self.answer.as_ref().ok().map(|(s, ..)| s)?;
        (!status.graphics.is_empty() || !self.graphics.is_empty()).then(|| codec::fit(&status.graphics, &self.graphics))
    }

    /// The connection in figures, when it was measured.
    pub fn health(&self) -> Option<Health> {
        let m = self.measure?;
        let choice = quality::choose(self.setting, Some(m), super::wake::is_tailscale(self.address()));
        Some(Health::of(m, choice, self.screen, self.address(), self.codec(), self.now))
    }
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

fn ports_step(facts: &Facts, sunshine_up: bool) -> Step {
    let title = "Sunshine can be reached";
    match &facts.ports {
        _ if !sunshine_up => skipped("ports", title, "Needs Sunshine running on the desktop first."),
        None => skipped("ports", title, "Not tried."),
        Some(ports) if ports.iter().all(|(_, open)| *open) => step("ports", title, Outcome::Ok, "Its ports open from the Ally, so nothing on the way blocks a stream from starting.", None),
        Some(ports) => {
            let shut: Vec<String> = ports.iter().filter(|(_, open)| !open).map(|(port, _)| port.to_string()).collect();
            step(
                "ports",
                title,
                Outcome::Fail,
                format!(
                    "Sunshine runs on the desktop, but its port {} cannot be reached from the Ally. The desktop's firewall is in the way: run carla-companion firewall there once.",
                    shut.join(" and ")
                ),
                None,
            )
        }
    }
}

fn picture_step(facts: &Facts) -> Step {
    let title = "Picture fit";
    let mut outcome = Outcome::Ok;
    let mut parts = Vec::new();
    match (facts.fit, facts.screen) {
        (Some(ScreenFit { virtual_screen: true, .. }), Some(want)) => {
            parts.push(format!("The desktop gives each stream a screen of its own at the Ally's size, {} by {}.", want.width, want.height));
        }
        (Some(ScreenFit { mode: Some(mode), .. }), Some(want)) if (mode.width, mode.height) == (want.width, want.height) => {
            parts.push(format!("The desktop's screen switches to the Ally's size, {}, for a stream.", mode.words()));
        }
        (Some(ScreenFit { mode: Some(mode), .. }), Some(_)) => {
            outcome = Outcome::Warn;
            parts.push(format!(
                "The desktop's screen does not have the Ally's size; the nearest is {}, so the picture is scaled. A virtual screen fits exactly: carla-companion stream virtual install on the desktop.",
                mode.words()
            ));
        }
        (Some(ScreenFit { mode: None, .. }), Some(_)) => {
            outcome = Outcome::Warn;
            parts.push(
                "The desktop's screen has no size of the Ally's shape, so the picture is scaled and may have bars. A virtual screen fits exactly: carla-companion stream virtual install on the desktop."
                    .to_string(),
            );
        }
        _ => {}
    }
    match facts.codec() {
        Some(fit) if fit.known => parts.push(format!("{}; Moonlight and Sunshine settle that themselves.", fit.words())),
        Some(fit) => {
            outcome = Outcome::Warn;
            parts.push(format!("{}: a graphics chip was not recognised. H.264 needs about half as much again of the connection for the same picture.", fit.words()));
        }
        None => {}
    }
    if parts.is_empty() {
        return skipped("picture", title, "The desktop is too old to say. Update it.");
    }
    step("picture", title, outcome, parts.join(" "), None)
}

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
        out.push(skipped("ports", "Sunshine can be reached", why));
        out.push(moonlight_step(facts));
        out.push(skipped("paired", "Paired for streaming", why));
        out.push(waking_step(facts));
        out.push(skipped("connection", "Connection", why));
        out.push(skipped("picture", "Picture fit", why));
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

    out.push(ports_step(facts, status.state != State::NotReady));
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

    out.push(match facts.health() {
        Some(health) => {
            let fixed = if health.picture.automatic { String::new() } else { " The picture is fixed by the administrator.".to_string() };
            let advice = health.advice.first().map(|a| format!(" {a}")).unwrap_or_default();
            // A slow home network still plays, but is worth a note.
            let slow_at_home = facts.measure.is_some_and(|m| !m.away && m.latency_ms > SLOW_AT_HOME_MS);
            let outcome = if health.grade <= Grade::Playable || slow_at_home { Outcome::Warn } else { Outcome::Ok };
            step("connection", "Connection", outcome, format!("{}{fixed}{advice}", health.summary), None)
        }
        None => step("connection", "Connection", Outcome::Warn, "Could not be measured. Streams use a careful picture until it can be.", None),
    });
    out.push(picture_step(facts));
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
            "Not installed. CarlaOS can install it on the Ally; Windows asks once to allow that.",
            Some(Fix::Moonlight),
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
            graphics: vec!["AMD Radeon RX 9070 XT".into()],
        }
    }

    fn facts(answer: Result<(Status, String, u64), Error>) -> Facts<'static> {
        Facts {
            ally_version: "0.5.0",
            answer,
            can_wake: true,
            moonlight: true,
            paired: Some(true),
            measure: Some(Measure::plain(3, 1, Some(200_000), false)),
            setting: Setting::default(),
            ports: Some(vec![(47984, true), (47989, true)]),
            screen: Some(Mode::new(1920, 1080, 120)),
            fit: Some(ScreenFit { virtual_screen: true, mode: Some(Mode::new(1920, 1080, 120)) }),
            graphics: vec!["AMD Radeon(TM) Graphics".into()],
            now: 100,
        }
    }

    fn report(steps: Vec<Step>) -> Report {
        Report { id: "home".into(), name: "HOME PC".into(), steps, repaired: vec![], health: None }
    }

    fn outcome(steps: &[Step], key: &str) -> Outcome {
        steps.iter().find(|s| s.key == key).unwrap().outcome
    }

    #[test]
    fn a_healthy_desktop_passes_every_step() {
        let steps = steps(&facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4))));
        assert_eq!(steps.len(), 9);
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
        f.measure = Some(Measure::plain(35, 5, Some(30_000), false));
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
        f.measure = Some(Measure::plain(45, 5, Some(30_000), true));
        let steps = steps(&f);
        assert_eq!(outcome(&steps, "connection"), Outcome::Ok);
        assert!(steps[0].detail.contains("Tailscale"));
    }

    #[test]
    fn a_firewall_in_front_of_sunshine_is_named() {
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4)));
        f.ports = Some(vec![(47984, false), (47989, true)]);
        let steps = steps(&f);
        let ports = steps.iter().find(|s| s.key == "ports").unwrap();
        assert_eq!(ports.outcome, Outcome::Fail);
        assert!(ports.detail.contains("port 47984") && ports.detail.contains("carla-companion firewall"));
        // With Sunshine down the ports say nothing new.
        let mut f = facts(Ok((status(State::NotReady, "0.5.0"), "192.168.1.20".into(), 4)));
        f.ports = Some(vec![(47984, false), (47989, false)]);
        assert_eq!(outcome(&super::steps(&f), "ports"), Outcome::Skipped);
    }

    #[test]
    fn the_connection_is_given_in_figures_and_lost_packets_are_a_warning() {
        use crate::link::probe::Echo;
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4)));
        let echo = |lost| Echo { sent: 100, lost, best_us: 1_500, median_us: 2_100, p95_us: 4_000, jitter_us: 400 };
        f.measure = Some(Measure { echo: Some(echo(0)), ..Measure::plain(3, 1, Some(200_000), false) });
        let health = f.health().unwrap();
        assert_eq!((health.grade, health.ping_us, health.at), (Grade::Excellent, 2_100, 100));
        assert_eq!(health.codec.as_ref().map(|c| c.words()), Some("HEVC expected (AMD Radeon RX 9070 XT to AMD Radeon(TM) Graphics)".to_string()));
        let connection = steps(&f).into_iter().find(|s| s.key == "connection").unwrap();
        assert_eq!(connection.outcome, Outcome::Ok);
        assert!(connection.detail.starts_with("Excellent: 2.1 ms there and back"), "{}", connection.detail);

        f.measure = Some(Measure { echo: Some(echo(4)), ..Measure::plain(3, 1, Some(200_000), false) });
        let connection = steps(&f).into_iter().find(|s| s.key == "connection").unwrap();
        assert_eq!(connection.outcome, Outcome::Warn);
        assert!(connection.detail.contains("4.0 % of packets lost") && connection.detail.contains("network cable"), "{}", connection.detail);
    }

    #[test]
    fn a_screen_that_does_not_fit_and_an_unknown_chip_are_notes() {
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4)));
        let picture = |f: &Facts| steps(f).into_iter().find(|s| s.key == "picture").unwrap();
        assert_eq!(picture(&f).outcome, Outcome::Ok);
        assert!(picture(&f).detail.contains("a screen of its own") && picture(&f).detail.contains("HEVC expected"));
        f.fit = Some(ScreenFit { virtual_screen: false, mode: Some(Mode::new(1920, 1080, 60)) });
        assert_eq!(picture(&f).outcome, Outcome::Ok);
        f.fit = Some(ScreenFit { virtual_screen: false, mode: Some(Mode::new(2560, 1440, 60)) });
        assert_eq!(picture(&f).outcome, Outcome::Warn);
        f.fit = Some(ScreenFit { virtual_screen: false, mode: None });
        assert!(picture(&f).detail.contains("stream virtual install"));
        f.fit = Some(ScreenFit { virtual_screen: true, mode: None });
        f.graphics = vec!["Matrox G200".into()];
        assert_eq!(picture(&f).outcome, Outcome::Warn);
        assert!(picture(&f).detail.contains("H.264 assumed"));
    }

    #[test]
    fn a_missing_moonlight_can_be_installed_from_the_check() {
        let mut f = facts(Ok((status(State::Ready, "0.5.0"), "192.168.1.20".into(), 4)));
        f.moonlight = false;
        let r = report(steps(&f));
        assert_eq!(r.first_fix(), Some(Fix::Moonlight));
        assert_eq!(Fix::Moonlight.words(), "Install Moonlight");
    }

    #[test]
    fn the_report_travels_as_json() {
        let r = report(steps(&facts(Err(Error::Refused))));
        let back: Report = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(serde_json::to_value(Fix::Relink).unwrap(), "relink");
    }
}
