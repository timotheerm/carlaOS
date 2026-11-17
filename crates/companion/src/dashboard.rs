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
//! What the connection is like is measured by the Ally, which is the side
//! that asks and can time the answer, and told to the desktop in
//! `POST /v1/report`: when the Ally looks at its desktops, and every few
//! seconds during a stream.

use std::collections::VecDeque;

use carla_core::link::wake;
use carla_core::link::AllyReport;
use serde::Deserialize;

use super::*;

const PAGE: &str = include_str!("dashboard.html");

/// An Ally that was heard from this recently counts as connected.
const NEAR: i64 = 90;
/// A measurement older than this is shown as history, not as "now".
const FRESH: i64 = 20;
/// How many measurements the chart keeps: about five minutes of a stream.
const HISTORY: usize = 60;
/// What was heard is written down at most this often, to show after a restart.
const SAVE_EVERY: i64 = 60;
/// How long the answer about paired Moonlight devices is reused.
const CLIENTS_AGE: Duration = Duration::from_secs(30);
/// How long the answer about a newer version is reused.
const NEWER_AGE: Duration = Duration::from_secs(6 * 3600);

/// What is known about one linked Ally from its own requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Heard {
    /// When it last asked anything.
    pub at: i64,
    /// The address it asked from.
    pub from: String,
    /// Its last report and when that came.
    #[serde(default)]
    pub report: Option<(i64, AllyReport)>,
    /// The delays of its last reports, oldest first.
    #[serde(default)]
    pub pings: VecDeque<u32>,
    #[serde(skip)]
    saved: i64,
}

/// The stream an Ally last asked this desktop to get ready.
#[derive(Debug, Clone)]
pub(super) struct Asked {
    device: String,
    game_id: String,
    screen: Option<carla_core::display::Mode>,
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
pub struct Grade {
    pub name: &'static str,
    /// "ok", "fair" or "poor".
    pub tone: &'static str,
    pub advice: &'static str,
}

/// Judge a connection the way it feels in a game: the delay, and how steady it is.
pub fn grade(latency_ms: u32, jitter_ms: u32, away: bool) -> Grade {
    let felt = latency_ms + jitter_ms;
    let (name, tone) = match felt {
        0..=10 => ("Excellent", "ok"),
        11..=25 => ("Very good", "ok"),
        26..=50 => ("Good", "ok"),
        51..=90 => ("Playable", "fair"),
        _ => ("Slow", "poor"),
    };
    let advice = match (tone, away, jitter_ms > latency_ms.max(15)) {
        ("ok", _, _) => "",
        (_, true, _) => "The Ally is away from home, so the internet in between sets the pace.",
        (_, false, true) => "The connection is unsteady. Wi-Fi is usually why: move the Ally closer to the router, or use its 5 GHz or 6 GHz network.",
        (_, false, false) => "Plug this PC into the router with a cable if it is on Wi-Fi, and keep the Ally near the router.",
    };
    Grade { name, tone, advice }
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
    grade: Option<Grade>,
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

impl Server {
    /// Note that a linked device asked something just now.
    pub(super) fn heard_from(&self, device: &str, from: &str, now: i64) {
        let mut all = locked(&self.heard);
        let heard = all.entry(device.to_string()).or_insert_with(|| self.files.ally_heard(device).unwrap_or_default());
        heard.at = now;
        heard.from = from.to_string();
        if now - heard.saved >= SAVE_EVERY {
            heard.saved = now;
            self.files.set_ally_heard(device, heard);
        }
    }

    /// An Ally says what the connection is like from its side.
    pub(super) fn report(&self, device: &str, bytes: &[u8], now: i64) -> Answer {
        let mut report: AllyReport = body(bytes)?;
        // Shown on a page: keep what a device may put there short and plain.
        let plain = |text: &str| text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | ' ')).take(24).collect::<String>();
        report.version = plain(&report.version);
        report.level = report.level.as_deref().map(plain);
        let mut all = locked(&self.heard);
        if let Some(heard) = all.get_mut(device) {
            heard.pings.push_back(report.latency_ms);
            while heard.pings.len() > HISTORY {
                heard.pings.pop_front();
            }
            heard.report = Some((now, report));
        }
        json(&link::Reported { playing: self.files.playing(now).is_some() })
    }

    /// Note which Ally asked for which game, for the dashboard.
    pub(super) fn asked_for(&self, device: &str, prepare: &Prepare, now: i64) {
        let mut asked = locked(&self.asked);
        // The same game picked up again is still the same stream.
        if asked.as_ref().is_some_and(|a| a.game_id == prepare.game_id && self.files.playing(now).is_some()) {
            return;
        }
        *asked = Some(Asked { device: device.to_string(), game_id: prepare.game_id.clone(), screen: prepare.screen, at: now });
    }

    fn heard(&self, device: &str) -> Option<Heard> {
        let known = locked(&self.heard).get(device).cloned();
        known.or_else(|| self.files.ally_heard(device))
    }

    /// The names Sunshine has paired a Moonlight under, when it says.
    fn paired_names(&self) -> Option<Vec<String>> {
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

    fn allies(&self, now: i64, streaming: bool, asked: Option<&Asked>) -> Vec<AllyView> {
        let paired = self.paired_names();
        let devices = self.files.devices();
        let single = devices.len() == 1;
        devices
            .into_iter()
            .map(|device| {
                let heard = self.heard(&device.id);
                let heard_ago = heard.as_ref().filter(|h| h.at > 0).map(|h| (now - h.at).max(0));
                let report = heard.as_ref().and_then(|h| h.report.clone());
                let reports_stream = report.as_ref().is_some_and(|(at, r)| r.streaming && now - at <= FRESH);
                let asked_it = asked.is_some_and(|a| a.device == device.id);
                let state = match (streaming && (reports_stream || asked_it || (single && asked.is_none())), heard_ago) {
                    (true, _) => "streaming",
                    (false, Some(ago)) if ago <= NEAR => "connected",
                    _ => "away",
                };
                let address = heard.as_ref().map(|h| h.from.clone()).filter(|a| !a.is_empty());
                let route = address.as_deref().map(|a| if wake::is_tailscale(a) { "tailscale" } else { "home" });
                let measured_ago = report.as_ref().map(|(at, _)| (now - at).max(0));
                let measure = report.as_ref().map(|(_, r)| r);
                AllyView {
                    state,
                    heard_ago,
                    route,
                    ping_ms: measure.map(|r| r.latency_ms),
                    jitter_ms: measure.map(|r| r.jitter_ms),
                    mbps: measure.and_then(|r| r.kbps).map(|kbps| (kbps as f64 / 100.0).round() / 10.0),
                    live: measured_ago.is_some_and(|ago| ago <= FRESH),
                    measured_ago,
                    pings: heard.as_ref().map(|h| h.pings.iter().copied().collect()).unwrap_or_default(),
                    grade: measure.map(|r| grade(r.latency_ms, r.jitter_ms, r.away || route == Some("tailscale"))),
                    version: measure.map(|r| r.version.clone()).filter(|v| !v.is_empty()),
                    paired: paired.as_ref().map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&device.name))),
                    remote_admin: self.files.ally_listening(&device.id).is_some_and(|at| now - at <= NEAR),
                    address,
                    id: device.id,
                    name: device.name,
                }
            })
            .collect()
    }

    fn stream_view(&self, now: i64, allies: &[AllyView], asked: Option<&Asked>) -> Option<StreamView> {
        let playing = self.files.playing(now)?;
        let asked = asked.filter(|a| a.game_id == playing.game_id);
        let ally = allies.iter().find(|a| a.state == "streaming");
        let report = ally.and_then(|a| self.heard(&a.id)).and_then(|h| h.report).map(|(_, r)| r);
        let picture = report.as_ref().and_then(|r| r.picture);
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
            width: picture.map(|p| p.width).or(screen.map(|s| s.width)),
            height: picture.map(|p| p.height).or(screen.map(|s| s.height)),
            fps: picture.map(|p| p.fps).or(screen.map(|s| s.hz).filter(|hz| *hz > 0)),
            mbps: picture.map(|p| p.kbps / 1000),
            level: report.as_ref().and_then(|r| r.level.clone()),
            automatic: report.as_ref().is_some_and(|r| r.automatic),
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
        if !allies.is_empty() {
            let unpaired: Vec<&str> = allies.iter().filter(|a| a.paired == Some(false)).map(|a| a.name.as_str()).collect();
            if allies.iter().all(|a| a.paired.is_some()) {
                checks.push(if unpaired.is_empty() {
                    Check { key: "moonlight", title: "Moonlight", tone: "ok", detail: "Paired with Sunshine on this PC.".to_string() }
                } else {
                    Check { key: "moonlight", title: "Moonlight", tone: "off", detail: format!("{} is not paired yet. That happens by itself at its first stream.", unpaired.join(" and ")) }
                });
            }
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
        let allies = self.allies(now, streaming, asked.as_ref());
        let stream = self.stream_view(now, &allies, asked.as_ref());
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

    fn forget(&self, bytes: &[u8]) -> Answer {
        let forget: Forget = body(bytes)?;
        if !self.files.forget_device(&forget.id).map_err(|e| (500, e))? {
            return refuse(404, "That device is no longer linked.");
        }
        locked(&self.heard).remove(&forget.id);
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
    use carla_core::link::moonlight::Picture;

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

        // It asks something: connected. It reports: the numbers are there.
        link.status().unwrap();
        let near = overview(desk.port, &token);
        assert_eq!((near["headline"]["kind"].as_str(), near["headline"]["title"].as_str()), (Some("connected"), Some("Ally is connected")));
        assert!(near["allies"][0]["ping_ms"].is_null());
        let report = AllyReport { version: "0.9.1<b>".into(), latency_ms: 4, jitter_ms: 1, kbps: Some(310_000), ..AllyReport::default() };
        assert!(!link.report(&report).unwrap().playing);
        let ally = &overview(desk.port, &token)["allies"][0];
        assert_eq!((ally["ping_ms"].as_u64(), ally["jitter_ms"].as_u64(), ally["mbps"].as_f64(), ally["live"].as_bool()), (Some(4), Some(1), Some(310.0), Some(true)));
        assert_eq!((ally["grade"]["name"].as_str(), ally["route"].as_str(), ally["version"].as_str()), (Some("Excellent"), Some("home"), Some("0.9.1b")));
        assert_eq!(ally["paired"], true, "Sunshine knows a Moonlight of that name");

        // A stream: the game, the Ally, its picture, and a chart of the delay.
        link.prepare_for("steam:10", Some(carla_core::display::Mode::new(1920, 1080, 60))).unwrap();
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: crate::files::now() }));
        let picture = Picture { width: 1920, height: 1080, fps: 60, kbps: 25_000, pacing: false };
        for latency_ms in [5, 6, 4] {
            let report = AllyReport { latency_ms, streaming: true, picture: Some(picture), level: Some("sharp".into()), automatic: true, ..report.clone() };
            assert!(link.report(&report).unwrap().playing);
        }
        let live = overview(desk.port, &token);
        assert_eq!((live["headline"]["kind"].as_str(), live["headline"]["title"].as_str()), (Some("streaming"), Some("Streaming First Game")));
        let stream = &live["stream"];
        assert_eq!((stream["ally"].as_str(), stream["height"].as_u64(), stream["fps"].as_u64(), stream["mbps"].as_u64(), stream["level"].as_str()), (Some("Ally"), Some(1080), Some(60), Some(25), Some("sharp")));
        assert_eq!(live["allies"][0]["state"], "streaming");
        assert_eq!(live["allies"][0]["pings"], serde_json::json!([4, 5, 6, 4]));

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
    fn a_connection_is_judged_by_how_it_feels() {
        assert_eq!((grade(3, 1, false).name, grade(3, 1, false).advice), ("Excellent", ""));
        assert_eq!(grade(18, 4, false).name, "Very good");
        assert_eq!(grade(35, 10, true).name, "Good");
        let unsteady = grade(20, 45, false);
        assert_eq!((unsteady.name, unsteady.tone), ("Playable", "fair"));
        assert!(unsteady.advice.contains("unsteady"));
        assert!(grade(70, 5, false).advice.contains("cable"));
        let far = grade(120, 10, true);
        assert_eq!((far.name, far.tone), ("Slow", "poor"));
        assert!(far.advice.contains("away from home"));
    }
}
