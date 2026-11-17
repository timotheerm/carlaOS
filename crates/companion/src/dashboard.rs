//! The desktop's own dashboard: one page, served to this machine only, that
//! says in plain words whether the Ally is connected, how good the connection
//! is, what is being streamed and whether this PC is ready.
//!
//! The page is for whoever sits at the desktop, so it is not part of the
//! signed API. It is trusted another way:
//!
//! - it is only answered when the request comes from this machine itself;
//! - the `Host` it was asked under must be this machine's own name, so a web
//!   page elsewhere cannot reach it by pointing a name of its own here;
//! - everything but the page itself needs a token that only the page holds,
//!   sent in a header no other site's page is allowed to send.
//!
//! What the connection is like is measured by the Ally and told to the desktop
//! with each pulse (`carla_core::link::health`). This page only shows that.

use std::collections::VecDeque;

use carla_core::display::Mode;
use carla_core::link::health::{AllyView as Linked, Grade as Rating, Pulse, Route, CONNECTED_FOR};
use carla_core::link::wake;
use serde::Deserialize;

use super::*;

const PAGE: &str = include_str!("dashboard.html");

/// A measurement older than this is shown as history, not as "now".
const LIVE_FOR: i64 = 120;
/// How many measurements the chart keeps.
const HISTORY: usize = 60;
/// How long the answer about paired Moonlight devices is reused.
const CLIENTS_AGE: Duration = Duration::from_secs(30);
/// How long the answer about a newer version is reused.
const NEWER_AGE: Duration = Duration::from_secs(6 * 3600);

/// The delays of one Ally's measurements, oldest first, for the chart.
#[derive(Debug, Clone, Default)]
pub struct Pings {
    /// When the last measurement that was counted was made.
    pub at: i64,
    pub values: VecDeque<u32>,
}

/// The stream an Ally last asked this desktop to get ready.
#[derive(Debug, Clone)]
pub(super) struct Asked {
    game_id: String,
    screen: Option<Mode>,
    at: i64,
}

/// How busy this PC is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Vitals {
    /// Processor use in percent, once it has been measured twice.
    pub cpu: Option<u8>,
    pub memory_used: u64,
    pub memory_total: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Headline {
    /// "streaming", "connected", "waiting", "link" or "attention".
    kind: &'static str,
    title: String,
    detail: String,
}

/// A connection in one word, with what to do about it when it is not good.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct GradeView {
    name: &'static str,
    /// "ok", "fair" or "poor".
    tone: &'static str,
    advice: String,
}

/// The colour a grade shows in on the page.
pub fn tone(grade: Rating) -> &'static str {
    match grade {
        Rating::Excellent | Rating::Good => "ok",
        Rating::Playable => "fair",
        Rating::Poor => "poor",
    }
}

#[derive(Debug, Serialize)]
struct StreamView {
    game: String,
    /// The Ally it goes to, when that is known.
    ally: Option<String>,
    minutes: Option<i64>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    /// What the video may use, in megabits a second.
    mbps: Option<u32>,
    level: Option<String>,
    automatic: bool,
    /// What the stream shows, in words.
    screen: String,
}

#[derive(Debug, Serialize)]
struct AllyView {
    id: String,
    name: String,
    /// "streaming", "connected" or "away".
    state: &'static str,
    /// Seconds since it was last heard from.
    heard_ago: Option<i64>,
    address: Option<String>,
    /// "home" or "tailscale".
    route: Option<&'static str>,
    ping_ms: Option<u32>,
    jitter_ms: Option<u32>,
    /// What the connection carries, in megabits a second.
    mbps: Option<f64>,
    /// Seconds since that was measured.
    measured_ago: Option<i64>,
    /// Whether that measurement is of right now.
    live: bool,
    pings: Vec<u32>,
    grade: Option<GradeView>,
    version: Option<String>,
    /// Whether Sunshine knows a Moonlight of that name. `None` when Sunshine did not say.
    paired: Option<bool>,
    /// It keeps asking for requests from the administrator.
    remote_admin: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Check {
    key: &'static str,
    title: &'static str,
    /// "ok", "warn" or "off".
    tone: &'static str,
    detail: String,
}

#[derive(Debug, Serialize)]
struct Fetching {
    title: String,
    percent: Option<u64>,
}

#[derive(Debug, Serialize)]
struct GamesView {
    total: usize,
    /// How many from each place, in the order they are shown.
    from: Vec<(&'static str, usize)>,
    fetching: Vec<Fetching>,
    free_gb: Option<u64>,
}

#[derive(Debug, Serialize)]
struct CodeView {
    code: String,
    seconds_left: i64,
}

#[derive(Debug, Serialize)]
struct Overview {
    name: String,
    version: &'static str,
    hardware: String,
    headline: Headline,
    stream: Option<StreamView>,
    allies: Vec<AllyView>,
    checks: Vec<Check>,
    vitals: Vitals,
    games: GamesView,
    code: Option<CodeView>,
}

#[derive(Debug, Serialize)]
struct Newer {
    /// The version that can be installed, when there is a newer one.
    newer: Option<String>,
    problem: Option<String>,
}

#[derive(Deserialize)]
struct Forget {
    id: String,
}

/// How many of the newest lines of the log the page shows.
const LOG_LINES: usize = 100;

/// The newest lines of `host.log`, for the page's "Log" section.
#[derive(Debug, Serialize)]
struct LogView {
    path: String,
    lines: Vec<String>,
}

/// How long a look for games can be added from.
const FOUND_AGE: Duration = Duration::from_secs(600);

/// The id of one program the last look found. Nothing else is accepted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    id: String,
}

/// What a file was like when it was found: its size and time, and for a
/// shortcut also the program it leads to.
type Stamp = Vec<(PathBuf, u64, Option<std::time::SystemTime>)>;

fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let one = |p: &std::path::Path| std::fs::metadata(p).ok().map(|m| (p.to_path_buf(), m.len(), m.modified().ok()));
    let mut all = vec![one(path)?];
    if carla_core::library::shortcut::is_shortcut(path) {
        all.push(one(&carla_core::library::shortcut::read(path).ok()?.target)?);
    }
    Some(all)
}

/// One program the last look found, as the server remembers it.
pub(super) struct Offered {
    id: String,
    found: Candidate,
    stamp: Stamp,
}

/// A program that may be a game, as the page shows it.
#[derive(Debug, Serialize)]
struct Offer {
    id: String,
    title: String,
    exe: String,
    folder: String,
    kind: carla_core::library::find::FoundKind,
    note: String,
    watch: Option<String>,
}

fn ok() -> Answer {
    json(&serde_json::json!({ "done": true }))
}

/// Whether `host` is how this machine names itself to its own browser. The
/// port does not matter: a name from elsewhere is what must not get in.
fn own_name(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let name = host.rsplit_once(':').map_or(host, |(name, port)| if port.bytes().all(|b| b.is_ascii_digit()) { name } else { host });
    name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")
}

/// A title as `Files::add_extra` accepts it: at most 80 bytes, cut at a letter's edge.
fn fit_title(title: &str) -> String {
    let mut end = title.trim().len().min(80);
    let title = title.trim();
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    title[..end].trim_end().to_string()
}

/// A name the Ally chose, shortened and cleaned for the page.
fn plain(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | ' ')).take(24).collect()
}

impl Server {
    /// Keep the delays of an Ally's measurements for the chart. Each one is
    /// counted once, however many pulses carry it.
    pub(super) fn pulsed(&self, device: &str, pulse: &Pulse) {
        let Some(health) = &pulse.health else { return };
        let mut all = locked(&self.pings);
        let known = all.entry(device.to_string()).or_default();
        if known.at == health.at {
            return;
        }
        known.at = health.at;
        known.values.push_back(health.ping_us / 1000);
        while known.values.len() > HISTORY {
            known.values.pop_front();
        }
    }

    /// Note which Ally asked for which game, for the dashboard.
    pub(super) fn asked_for(&self, prepare: &Prepare, now: i64) {
        let mut asked = locked(&self.asked);
        // The same game picked up again is still the same stream.
        if asked.as_ref().is_some_and(|a| a.game_id == prepare.game_id && self.files.playing(now).is_some()) {
            return;
        }
        *asked = Some(Asked { game_id: prepare.game_id.clone(), screen: prepare.screen, at: now });
    }

    /// The names Sunshine has paired a Moonlight under, when it says.
    pub(super) fn paired_names(&self) -> Option<Vec<String>> {
        let mut clients = locked(&self.clients);
        match clients.as_ref() {
            Some((at, names)) if at.elapsed() < CLIENTS_AGE => names.clone(),
            _ => {
                let names = self.streamer.as_deref().and_then(Streamer::paired);
                *clients = Some((Instant::now(), names.clone()));
                names
            }
        }
    }

    /// Every linked Ally as the page shows it, from what the desktop knows
    /// of each one (see `Files::allies`).
    fn allies(&self, now: i64, linked: &[Linked]) -> Vec<AllyView> {
        let paired = self.paired_names();
        let pings = locked(&self.pings).clone();
        linked
            .iter()
            .map(|ally| {
                let health = ally.health.as_ref();
                let measured_ago = health.map(|h| (now - h.at).max(0));
                let route = health
                    .map(|h| match h.route {
                        Route::Home => "home",
                        Route::Away => "tailscale",
                    })
                    .or_else(|| ally.address.as_deref().map(|a| if wake::is_tailscale(a) { "tailscale" } else { "home" }));
                AllyView {
                    state: if ally.streaming {
                        "streaming"
                    } else if ally.connected {
                        "connected"
                    } else {
                        "away"
                    },
                    heard_ago: ally.last_seen.map(|at| (now - at).max(0)),
                    address: ally.address.clone(),
                    route,
                    ping_ms: health.map(|h| h.ping_us / 1000),
                    jitter_ms: health.map(|h| h.jitter_us / 1000),
                    mbps: health.and_then(|h| h.link_kbps).map(|kbps| (kbps as f64 / 100.0).round() / 10.0),
                    measured_ago,
                    live: measured_ago.is_some_and(|ago| ago <= LIVE_FOR),
                    pings: pings.get(&ally.id).map(|p| p.values.iter().copied().collect()).unwrap_or_default(),
                    grade: health.map(|h| GradeView {
                        name: h.grade.word(),
                        tone: tone(h.grade),
                        advice: h.advice.first().cloned().unwrap_or_default(),
                    }),
                    version: ally.pulse.as_ref().map(|p| plain(&p.version)).filter(|v| !v.is_empty()),
                    paired: paired.as_ref().map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&ally.name))),
                    remote_admin: self.files.ally_listening(&ally.id).is_some_and(|at| now - at <= CONNECTED_FOR),
                    id: ally.id.clone(),
                    name: ally.name.clone(),
                }
            })
            .collect()
    }

    fn stream_view(&self, now: i64, linked: &[Linked], asked: Option<&Asked>) -> Option<StreamView> {
        let playing = self.files.playing(now)?;
        let asked = asked.filter(|a| a.game_id == playing.game_id);
        let ally = linked.iter().find(|a| a.streaming);
        // The picture the last measurement chose for that Ally.
        let picture = ally.and_then(|a| a.health.as_ref()).map(|h| h.picture.clone());
        let screen = asked.and_then(|a| a.screen);
        let shows = if self.files.virtual_screen().is_some() {
            "A screen of its own, so this PC's monitor stays as it is".to_string()
        } else if let Some(to) = self.files.switched() {
            format!("This PC's screen, switched to {} by {} for the Ally", to.width, to.height)
        } else {
            "This PC's screen, as it is".to_string()
        };
        Some(StreamView {
            game: playing.title,
            ally: ally.map(|a| a.name.clone()),
            minutes: asked.map(|a| (now - a.at).max(0) / 60),
            width: picture.as_ref().map(|p| p.width).or(screen.map(|s| s.width)),
            height: picture.as_ref().map(|p| p.height).or(screen.map(|s| s.height)),
            fps: picture.as_ref().map(|p| p.fps).or(screen.map(|s| s.hz).filter(|hz| *hz > 0)),
            mbps: picture.as_ref().map(|p| p.kbps / 1000),
            level: picture.as_ref().map(|p| p.level.clone()),
            automatic: picture.as_ref().is_some_and(|p| p.automatic),
            screen: shows,
        })
    }

    fn checks(&self, problem: Option<&str>, allies: &[AllyView]) -> Vec<Check> {
        let (addresses, macs) = (self.network)();
        let home: Vec<&String> = addresses.iter().filter(|a| !wake::is_tailscale(a)).collect();
        let tailscale = addresses.iter().any(|a| wake::is_tailscale(a));
        let mut checks = vec![match problem {
            None => Check { key: "sunshine", title: "Sunshine", tone: "ok", detail: "Running, and set up for CarlaOS.".to_string() },
            Some(problem) => Check { key: "sunshine", title: "Sunshine", tone: "warn", detail: problem.to_string() },
        }];
        checks.push(match home.first() {
            Some(address) => Check { key: "network", title: "Home network", tone: "ok", detail: format!("The Ally finds this PC at {address}.") },
            None => Check { key: "network", title: "Home network", tone: "warn", detail: "This PC has no address on a network. Check its cable or Wi-Fi.".to_string() },
        });
        checks.push(if tailscale {
            Check { key: "tailscale", title: "Away from home", tone: "ok", detail: "Tailscale is on, so the Ally can also reach this PC from elsewhere.".to_string() }
        } else {
            Check { key: "tailscale", title: "Away from home", tone: "off", detail: "Tailscale is not on here. Streaming works at home only.".to_string() }
        });
        checks.push(if macs.is_empty() {
            Check { key: "wake", title: "Waking from sleep", tone: "off", detail: "No network adapter the Ally could wake this PC through was found.".to_string() }
        } else {
            Check { key: "wake", title: "Waking from sleep", tone: "ok", detail: "The Ally knows how to wake this PC when it sleeps.".to_string() }
        });
        let driver = self.screens.as_ref().is_some_and(|d| d.present());
        checks.push(match (self.config.virtual_screen, driver, self.config.match_screen) {
            (true, true, _) => Check { key: "screen", title: "Picture for the Ally", tone: "ok", detail: "Each stream gets a screen of its own at the Ally's size.".to_string() },
            (true, false, _) => Check { key: "screen", title: "Picture for the Ally", tone: "warn", detail: "A screen of its own for each stream is switched on, but its driver is not installed.".to_string() },
            (false, _, true) => Check { key: "screen", title: "Picture for the Ally", tone: "ok", detail: "This PC's screen switches to the Ally's size during a stream, and back after.".to_string() },
            (false, _, false) => Check { key: "screen", title: "Picture for the Ally", tone: "off", detail: "This PC's screen is streamed as it is.".to_string() },
        });
        if !allies.is_empty() && allies.iter().all(|a| a.paired.is_some()) {
            let unpaired: Vec<&str> = allies.iter().filter(|a| a.paired == Some(false)).map(|a| a.name.as_str()).collect();
            checks.push(if unpaired.is_empty() {
                Check { key: "moonlight", title: "Moonlight", tone: "ok", detail: "Paired with Sunshine on this PC.".to_string() }
            } else {
                Check { key: "moonlight", title: "Moonlight", tone: "off", detail: format!("{} is not paired yet. That happens by itself at its first stream.", unpaired.join(" and ")) }
            });
        }
        checks
    }

    fn games_view(&self) -> GamesView {
        // Whatever was last found: the dashboard never makes the disks spin.
        let known = locked(&self.games).as_ref().map(|(_, list)| list.clone());
        let found = known.unwrap_or_else(|| self.games(false));
        let extras = self.files.extras().into_iter().filter(|e| e.exe.is_file()).count();
        let count = |source: &str| found.iter().filter(|(s, _)| *s == source).count();
        let from: Vec<(&'static str, usize)> =
            [("Steam", count("steam")), ("Epic", count("epic")), ("GOG", count("gog")), ("Emulated", count("emu")), ("Added by hand", extras)].into_iter().filter(|(_, n)| *n > 0).collect();
        let fetching = (self.installs.downloads)().into_iter().map(|d| Fetching { percent: (d.total > 0).then(|| d.done * 100 / d.total), title: d.title }).collect();
        GamesView { total: found.len() + extras, from, fetching, free_gb: (self.installs.free)().map(|bytes| bytes / 1_073_741_824) }
    }

    fn overview(&self, now: i64) -> Overview {
        let status = self.status(now);
        let asked = locked(&self.asked).clone();
        let streaming = status.playing.is_some();
        let linked = self.files.allies(now, streaming);
        let allies = self.allies(now, &linked);
        let stream = self.stream_view(now, &linked, asked.as_ref());
        let near = allies.iter().find(|a| a.state == "connected");
        let headline = match (&status.problem, &stream, near) {
            (Some(problem), _, _) => Headline { kind: "attention", title: "This PC needs a hand".to_string(), detail: problem.clone() },
            (None, Some(stream), _) => Headline {
                kind: "streaming",
                title: format!("Streaming {}", stream.game),
                detail: match &stream.ally {
                    Some(ally) => format!("{ally} is playing it from this PC."),
                    None => "The Ally is playing it from this PC.".to_string(),
                },
            },
            (None, None, _) if allies.is_empty() => Headline { kind: "link", title: "Ready for an Ally".to_string(), detail: "No Ally is linked to this PC yet. Get a code below and type it on the Ally.".to_string() },
            (None, None, Some(ally)) => Headline { kind: "connected", title: format!("{} is connected", ally.name), detail: "Pick a game on the Ally and it starts here.".to_string() },
            (None, None, None) => Headline { kind: "waiting", title: "Ready and waiting".to_string(), detail: "Everything is fine here. The Ally is asleep, off or somewhere else.".to_string() },
        };
        Overview {
            name: self.config.name.clone(),
            version: VERSION,
            hardware: self.hardware.clone(),
            headline,
            stream,
            checks: self.checks(status.problem.as_deref(), &allies),
            allies,
            vitals: (self.vitals)(),
            games: self.games_view(),
            code: self.files.code(now).map(|code| CodeView { seconds_left: (code.expires - now).max(0), code: code.code }),
        }
    }

    fn newer(&self, fresh: bool) -> Newer {
        let mut newest = locked(&self.newest);
        let answer = match newest.as_ref() {
            Some((at, answer)) if !fresh && at.elapsed() < NEWER_AGE => answer.clone(),
            _ => {
                let answer = (self.newer)();
                *newest = Some((Instant::now(), answer.clone()));
                answer
            }
        };
        match answer {
            Ok(newer) => Newer { newer, problem: None },
            Err(problem) => Newer { newer: None, problem: Some(problem) },
        }
    }

    /// Look for programs that may be games and keep what is found, so that
    /// one of them can be added by its number.
    fn find_games(&self) -> Answer {
        use carla_core::library::discover::lower;
        // One look at a time: it holds one of the few workers that answer
        // the Ally, and it holds no lock the Ally's requests need.
        struct Done<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if self.finding.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return refuse(409, "A look for games is already running. Wait for it to finish.");
        }
        let _done = Done(&self.finding);
        let added: Vec<String> = self.files.extras().iter().map(|e| lower(&e.exe)).collect();
        // Games a store lists are not added by hand: whatever lies in their folders.
        let known = locked(&self.games).as_ref().map(|(_, list)| list.clone());
        let stores = known.unwrap_or_else(|| self.games(false));
        let stored: Vec<String> = stores.iter().filter(|(_, g)| !g.install_path.as_os_str().is_empty()).map(|(_, g)| format!("{}\\", lower(&g.install_path).trim_end_matches('\\'))).collect();
        let list: Vec<Offered> = (self.finder)()
            .into_iter()
            .filter(|c| !added.contains(&lower(&c.exe)) && !stored.iter().any(|dir| lower(&c.exe).starts_with(dir.as_str())))
            // A name that could not be watched safely is dropped; the game is still offered.
            .map(|c| Candidate { watch_hint: c.watch_hint.as_deref().and_then(carla_core::library::launch::valid_watch), title: fit_title(&c.title), ..c })
            .filter(|c| !c.title.is_empty())
            .filter_map(|c| Some(Offered { id: carla_core::library::manual_key(&c.exe.to_string_lossy()), stamp: stamp(&c.exe)?, found: c }))
            .collect();
        let offered: Vec<Offer> = list
            .iter()
            .map(|o| {
                let c = &o.found;
                Offer { id: o.id.clone(), title: c.title.clone(), exe: c.exe.to_string_lossy().into_owned(), folder: c.folder.to_string_lossy().into_owned(), kind: c.kind, note: c.note.clone(), watch: c.watch_hint.clone() }
            })
            .collect();
        *locked(&self.candidates) = Some((Instant::now(), list));
        json(&offered)
    }

    /// Add what the last look found, by its number. Only that: a program
    /// or options named by the page are not accepted.
    fn add_found(&self, bytes: &[u8]) -> Answer {
        let pick: Pick = body(bytes)?;
        let found = locked(&self.candidates);
        let Some((_, list)) = found.as_ref().filter(|(at, _)| at.elapsed() < FOUND_AGE) else {
            return refuse(409, "Look for games first: that list is too old.");
        };
        let Some(offered) = list.iter().find(|o| o.id == pick.id) else {
            return refuse(404, "That game was not in the list.");
        };
        let c = &offered.found;
        // The program must still be the one that was found.
        if stamp(&c.exe).as_ref() != Some(&offered.stamp) {
            return refuse(409, "That program has changed since it was found. Look for games again.");
        }
        let added = self.files.add_extra(&c.title, &c.exe, "", None, c.watch_hint.as_deref()).map_err(|e| (500, e))?;
        json(&serde_json::json!({ "title": added.title }))
    }

    fn forget(&self, bytes: &[u8]) -> Answer {
        let forget: Forget = body(bytes)?;
        if !self.files.forget_device(&forget.id).map_err(|e| (500, e))? {
            return refuse(404, "That device is no longer linked.");
        }
        locked(&self.pings).remove(&forget.id);
        self.files.host(&format!("Device {} was forgotten from the dashboard.", label(&forget.id)));
        ok()
    }

    /// Answer the dashboard. `None` when the request is not for it, which
    /// leaves it to the signed API.
    ///
    /// The caller has already made sure the request came from this machine.
    pub fn local(&self, method: &str, path: &str, host: Option<&str>, token: Option<&str>, bytes: &[u8], now: i64) -> Option<Reply> {
        if path != "/" && !path.starts_with("/local/") {
            return None;
        }
        let answer: Answer = if !own_name(host) {
            refuse(403, "The dashboard only opens on this PC itself.")
        } else if (method, path) == ("GET", "/") {
            Ok((PAGE.replace("__CARLA_TOKEN__", &self.token).into_bytes(), "text/html; charset=utf-8"))
        } else if token != Some(self.token.as_str()) {
            refuse(403, "This page is out of date. Reload it.")
        } else {
            match (method, path) {
                ("GET", "/local/overview") => json(&self.overview(now)),
                ("GET", "/local/log") => json(&LogView { path: self.files.host_log().path().to_string_lossy().into_owned(), lines: self.files.host_log().tail(LOG_LINES) }),
                ("GET", "/local/newer") => json(&self.newer(false)),
                ("POST", "/local/newer") => json(&self.newer(true)),
                ("POST", "/local/code") => self.files.new_code().map_err(|e| (500, e)).and_then(|_| ok()),
                ("POST", "/local/code/hide") => {
                    self.files.clear_code();
                    ok()
                }
                ("POST", "/local/forget") => self.forget(bytes),
                ("POST", "/local/games") => {
                    self.games(true);
                    ok()
                }
                ("POST", "/local/find") => self.find_games(),
                ("POST", "/local/add") => self.add_found(bytes),
                ("POST", "/local/update") => self.update(now),
                ("POST", "/local/end") => self.end(now),
                _ => refuse(404, "The dashboard does not know that request."),
            }
        };
        let (status, kind, body) = match answer {
            Ok((body, kind)) => (200, kind, body),
            Err((status, error)) => (status, "application/json", serde_json::to_vec(&Problem { error }).expect("serialisable")),
        };
        Some(Reply { status, kind, body, sign: None })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::desk;
    use super::*;
    use carla_core::link::client::Error;
    use carla_core::link::health::Health;

    /// The page's own script, run with a tiny fake DOM (scripts/test-dashboard.mjs):
    /// an Add click must not read `event.currentTarget` after the request, when a
    /// browser has already cleared it. Skipped where node is not installed.
    #[test]
    fn the_add_button_of_the_page_works_when_clicked() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/test-dashboard.mjs");
        let page = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dashboard.html");
        let out = match std::process::Command::new("node").arg(script).arg(page).output() {
            Ok(out) => out,
            Err(e) => {
                eprintln!("skipped: node is not available ({e})");
                return;
            }
        };
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{text}");
    }
    use carla_core::link::quality::{self, Measure, Setting};

    fn get(port: u16, path: &str, host: &str, token: Option<&str>) -> (u16, String) {
        ask("GET", port, path, host, token, "")
    }

    fn ask(method: &str, port: u16, path: &str, host: &str, token: Option<&str>, body: &str) -> (u16, String) {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let token = token.map(|t| format!("x-carla-token: {t}\r\n")).unwrap_or_default();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{token}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let status = reply.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        (status, reply.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default())
    }

    /// The token the page was given.
    fn token(port: u16) -> String {
        let (status, page) = get(port, "/", &format!("127.0.0.1:{port}"), None);
        assert_eq!(status, 200);
        assert!(!page.contains("__CARLA_TOKEN__"));
        let at = page.find("const TOKEN = \"").expect("the page holds its token") + 15;
        page[at..at + 32].to_string()
    }

    fn overview(port: u16, token: &str) -> serde_json::Value {
        let (status, body) = get(port, "/local/overview", &format!("localhost:{port}"), Some(token));
        assert_eq!(status, 200, "{body}");
        serde_json::from_str(&body).unwrap()
    }

    /// A measurement as the Ally makes one, for a desktop at home or away.
    fn measured(latency_ms: u32, jitter_ms: u32, kbps: u32, away: bool) -> Health {
        let measure = Measure::plain(latency_ms, jitter_ms, Some(kbps), away);
        let choice = quality::choose(Setting::default(), Some(measure), away);
        let address = if away { "100.84.12.7" } else { "127.0.0.1" };
        Health::of(measure, choice, Some(Mode::new(1920, 1080, 120)), address, None, crate::files::now())
    }

    fn pulse(health: Option<Health>, playing: Option<&str>) -> Pulse {
        Pulse { version: "0.17.0".into(), moonlight: true, paired: Some(true), playing: playing.map(str::to_string), battery: None, charging: None, screen: None, health }
    }

    #[test]
    fn the_dashboard_only_answers_this_pc_under_its_own_name_and_with_its_token() {
        let desk = desk("dash-guard", false);
        let own = format!("127.0.0.1:{}", desk.port);
        let token = token(desk.port);
        // A page elsewhere that points a name of its own at this machine.
        assert_eq!(get(desk.port, "/", "evil.example:47600", None).0, 403);
        assert_eq!(get(desk.port, "/local/overview", "evil.example", Some(&token)).0, 403);
        // No token, or a guessed one: nothing is told and nothing is done.
        assert_eq!(get(desk.port, "/local/overview", &own, None).0, 403);
        assert_eq!(ask("POST", desk.port, "/local/code", &own, Some("0123456789abcdef0123456789abcdef"), "").0, 403);
        assert!(desk.files.code(crate::files::now()).is_none());
        assert_eq!(get(desk.port, "/local/nothing", &own, Some(&token)).0, 404);
        // The signed API is where it was.
        assert_eq!(get(desk.port, "/v1/status", &own, Some(&token)).0, 401);
        assert_eq!(get(desk.port, "/v1/hello", &own, None).0, 200);
    }

    #[test]
    fn the_log_is_shown_on_the_page_only_to_this_pc_under_its_own_name_and_with_its_token() {
        let desk = desk("dash-log", false);
        let own = format!("127.0.0.1:{}", desk.port);
        let token = token(desk.port);
        desk.files.host("a line the page should show");
        // Another site's Host, no token and a guessed one are all refused, and nothing of the log leaks.
        for (host, tok) in [("evil.example", Some(token.as_str())), (own.as_str(), None), (own.as_str(), Some("0123456789abcdef0123456789abcdef"))] {
            let (status, body) = get(desk.port, "/local/log", host, tok);
            assert_eq!(status, 403, "{host} {tok:?}");
            assert!(!body.contains("a line the page should show"), "{body}");
        }
        assert_eq!(ask("POST", desk.port, "/local/log", &own, Some(&token), "").0, 404, "only for GET");
        let (status, body) = get(desk.port, "/local/log", &format!("localhost:{}", desk.port), Some(&token));
        assert_eq!(status, 200, "{body}");
        let log: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(log["lines"].as_array().unwrap().iter().any(|l| l.as_str().is_some_and(|l| l.ends_with("a line the page should show"))), "{body}");
        assert!(log["path"].as_str().unwrap().ends_with("host.log"));
        // The page's own polling is not written into the log it shows.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!desk.files.host_log().text().contains("/local/log -> 200"), "{}", desk.files.host_log().text());
    }

    #[test]
    fn the_newest_hundred_lines_of_the_log_are_what_the_page_gets() {
        let desk = desk("dash-log-tail", false);
        let token = token(desk.port);
        // The server's own first note (Sunshine is ready) is in before the rest.
        while !desk.files.host_log().text().contains("Sunshine is ready.") {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for n in 0..250 {
            desk.files.host(&format!("event {n}"));
        }
        let (_, body) = get(desk.port, "/local/log", &format!("127.0.0.1:{}", desk.port), Some(&token));
        let lines = serde_json::from_str::<serde_json::Value>(&body).unwrap()["lines"].as_array().unwrap().clone();
        assert_eq!(lines.len(), 100);
        assert!(lines[0].as_str().unwrap().ends_with("event 150") && lines[99].as_str().unwrap().ends_with("event 249"));
    }

    #[test]
    fn a_game_is_added_from_the_page_only_by_the_number_of_something_the_scan_found() {
        let desk = desk("dash-add", false);
        let own = format!("127.0.0.1:{}", desk.port);
        let token = token(desk.port);
        let wrong = "0123456789abcdef0123456789abcdef";
        let exe = desk.files.dir().join("Quest.exe");
        std::fs::write(&exe, b"program").unwrap();
        let add = |host: &str, token: Option<&str>, body: &str| ask("POST", desk.port, "/local/add", host, token, body);

        // Before any scan there is nothing to pick from.
        assert_eq!(add(&own, Some(&token), r#"{"id":"0000000000000000"}"#).0, 409);
        // Both routes: another site's Host, no token and a guessed one are refused.
        for path in ["/local/find", "/local/add"] {
            assert_eq!(ask("POST", desk.port, path, "evil.example", Some(&token), r#"{"id":0}"#).0, 403, "{path} under a foreign name");
            assert_eq!(ask("POST", desk.port, path, &own, None, r#"{"id":0}"#).0, 403, "{path} without the token");
            assert_eq!(ask("POST", desk.port, path, &own, Some(wrong), r#"{"id":0}"#).0, 403, "{path} with a guessed token");
            assert_eq!(get(desk.port, path, &own, Some(&token)).0, 404, "{path} is only for POST");
        }
        assert_eq!(ask("POST", desk.port, "/local/find", &own, Some(&token), "").0, 200);
        assert!(desk.files.extras().is_empty(), "nothing is added by refused or by scanning requests");

        let (status, found) = ask("POST", desk.port, "/local/find", &own, Some(&token), "");
        assert_eq!(status, 200);
        let found: serde_json::Value = serde_json::from_str(&found).unwrap();
        let id = carla_core::library::manual_key(&exe.to_string_lossy());
        assert_eq!((found[0]["id"].as_str(), found[0]["title"].as_str(), found[0]["watch"].as_str()), (Some(id.as_str()), Some("Pixel Quest"), Some("Quest-Win64-Shipping.exe")));
        let pick = format!(r#"{{"id":"{id}"}}"#);

        // An id the scan did not give, and anything a page might add of its own.
        assert_eq!(add(&own, Some(&token), r#"{"id":"0000000000000000"}"#).0, 404);
        assert_eq!(add(&own, Some(&token), &exe.to_string_lossy().replace('\\', "/").replace('"', "")).0, 400);
        assert_eq!(add(&own, Some(&token), r#"{"id":7}"#).0, 400);
        assert_eq!(add(&own, Some(&token), r#"{"id":-1}"#).0, 400);
        assert_eq!(add(&own, Some(&token), r#"{"exe":"C:\\Windows\\System32\\calc.exe"}"#).0, 400);
        assert_eq!(add(&own, Some(&token), &format!(r#"{{"id":"{id}","exe":"C:\\Windows\\System32\\calc.exe","args":"x"}}"#)).0, 400);
        assert!(desk.files.extras().is_empty());

        // A program that changed after it was found is not added on the strength of the old look.
        std::fs::write(&exe, b"a different, longer program").unwrap();
        assert_eq!(add(&own, Some(&token), &pick).0, 409);
        assert!(desk.files.extras().is_empty());
        assert_eq!(ask("POST", desk.port, "/local/find", &own, Some(&token), "").0, 200);

        // The id it gave adds exactly what the scan found.
        assert_eq!(add(&own, Some(&token), &pick).0, 200);
        let extras = desk.files.extras();
        assert_eq!((extras.len(), extras[0].exe.clone(), extras[0].args.as_str(), extras[0].watch.as_deref()), (1, exe, "", Some("Quest-Win64-Shipping.exe")));
        // Once added it is no longer offered.
        let (_, again) = ask("POST", desk.port, "/local/find", &own, Some(&token), "");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&again).unwrap(), serde_json::json!([]));
    }

    #[test]
    fn the_dashboard_follows_an_ally_from_linking_to_streaming() {
        let desk = desk("dash-flow", false);
        let own = format!("127.0.0.1:{}", desk.port);
        let token = token(desk.port);
        let first = overview(desk.port, &token);
        assert_eq!(first["headline"]["kind"], "link");
        assert_eq!((first["name"].as_str(), first["code"].is_null(), first["games"]["total"].as_u64()), (Some("HOME PC"), true, Some(3)));
        assert_eq!(first["games"]["from"], serde_json::json!([["Steam", 2], ["GOG", 1]]));
        assert_eq!(first["games"]["fetching"][0]["percent"], 50);

        // A code is asked for on the page, shown there, and links the Ally.
        assert_eq!(ask("POST", desk.port, "/local/code", &own, Some(&token), "").0, 200);
        let code = overview(desk.port, &token)["code"]["code"].as_str().unwrap().to_string();
        let (_, link) = carla_core::link::client::enrol("127.0.0.1", desk.port, &code, "Ally").unwrap();
        let linked = overview(desk.port, &token);
        assert!(linked["code"].is_null(), "a code links one device");
        assert_eq!(linked["allies"][0]["name"], "Ally");
        assert_eq!(linked["allies"][0]["state"], "away", "linked, but it has asked nothing yet");

        // It asks something: connected. It reports its measurement: the numbers are there.
        link.status().unwrap();
        let near = overview(desk.port, &token);
        assert_eq!((near["headline"]["kind"].as_str(), near["headline"]["title"].as_str()), (Some("connected"), Some("Ally is connected")));
        assert!(near["allies"][0]["ping_ms"].is_null());
        let health = measured(4, 1, 310_000, false);
        link.pulse(&pulse(Some(health.clone()), None)).unwrap();
        let ally = &overview(desk.port, &token)["allies"][0];
        assert_eq!((ally["ping_ms"].as_u64(), ally["jitter_ms"].as_u64(), ally["mbps"].as_f64(), ally["live"].as_bool()), (Some(4), Some(1), Some(310.0), Some(true)));
        assert_eq!((ally["grade"]["name"].as_str(), ally["route"].as_str(), ally["version"].as_str()), (Some(health.grade.word()), Some("home"), Some("0.17.0")));
        assert_eq!(ally["pings"], serde_json::json!([4]));
        assert_eq!(ally["paired"], true, "Sunshine knows a Moonlight of that name");

        // A stream: the game, the Ally, its picture; nothing is measured under it, so the last one stays.
        link.prepare_for("steam:10", Some(Mode::new(1920, 1080, 60))).unwrap();
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: crate::files::now() }));
        link.pulse(&pulse(None, Some("First Game"))).unwrap();
        let live = overview(desk.port, &token);
        assert_eq!((live["headline"]["kind"].as_str(), live["headline"]["title"].as_str()), (Some("streaming"), Some("Streaming First Game")));
        let stream = &live["stream"];
        assert_eq!((stream["ally"].as_str(), stream["height"].as_u64(), stream["fps"].as_u64(), stream["mbps"].as_u64()), (Some("Ally"), Some(1080), Some(health.picture.fps as u64), Some(health.picture.kbps as u64 / 1000)));
        assert_eq!(stream["level"].as_str(), Some(health.picture.level.as_str()));
        assert_eq!(live["allies"][0]["state"], "streaming");
        assert_eq!(live["allies"][0]["ping_ms"], 4, "the last measurement is kept");

        // The next measurement is counted in the chart.
        // A measurement is counted by the second it was made, so the next one is a little later.
        let next = Health { at: health.at + 5, ..measured(6, 1, 310_000, false) };
        link.pulse(&pulse(Some(next), Some("First Game"))).unwrap();
        assert_eq!(overview(desk.port, &token)["allies"][0]["pings"], serde_json::json!([4, 6]));

        // The game can be closed from the page, and the Ally forgotten.
        assert_eq!(ask("POST", desk.port, "/local/end", &own, Some(&token), "").0, 200);
        assert!(desk.files.take_end(crate::files::now()));
        let id = live["allies"][0]["id"].as_str().unwrap();
        assert_eq!(ask("POST", desk.port, "/local/forget", &own, Some(&token), &format!("{{\"id\":\"{id}\"}}")).0, 200);
        assert_eq!(link.status().err(), Some(Error::Refused));
        assert_eq!(overview(desk.port, &token)["allies"], serde_json::json!([]));
    }

    #[test]
    fn a_problem_with_sunshine_leads_the_page_and_updates_are_offered() {
        let down = desk("dash-down", true);
        let token = token(down.port);
        let page = overview(down.port, &token);
        assert_eq!((page["headline"]["kind"].as_str(), page["headline"]["detail"].as_str()), (Some("attention"), Some("Sunshine is not running on this desktop.")));
        assert_eq!((page["checks"][0]["key"].as_str(), page["checks"][0]["tone"].as_str()), (Some("sunshine"), Some("warn")));
        let own = format!("127.0.0.1:{}", down.port);
        let (_, newer) = get(down.port, "/local/newer", &own, Some(&token));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&newer).unwrap()["newer"], "9.9.9");
        let (status, updating) = ask("POST", down.port, "/local/update", &own, Some(&token), "");
        assert_eq!((status, serde_json::from_str::<serde_json::Value>(&updating).unwrap()["updating"].as_bool()), (200, Some(true)));
    }

    #[test]
    fn a_long_title_is_cut_to_what_adding_accepts() {
        assert_eq!(fit_title("  Short  "), "Short");
        let long = "\u{30b2}".repeat(40); // 3 bytes each
        let cut = fit_title(&long);
        assert!(cut.len() <= 80 && !cut.is_empty() && long.starts_with(&cut));
        assert_eq!(fit_title(&"a".repeat(200)).len(), 80);
        assert_eq!(fit_title("   "), "");
    }

    #[test]
    fn grades_show_in_three_colours() {
        assert_eq!([tone(Rating::Excellent), tone(Rating::Good), tone(Rating::Playable), tone(Rating::Poor)], ["ok", "ok", "fair", "poor"]);
    }
}
