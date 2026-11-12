//! Talking to Sunshine on this machine through its own web API.
//!
//! Sunshine serves that API over HTTPS with a certificate it made itself, so
//! the certificate cannot be checked against anything. That is acceptable only
//! because the companion refuses to talk to Sunshine anywhere but on this
//! machine: the connection never leaves it.

use std::time::{Duration, Instant};

use carla_core::link::SUNSHINE_APP;
use serde_json::{json, Value};
use ureq::tls::TlsConfig;

/// What the server needs from the streaming host. Tests use a stand-in.
pub trait Streamer: Send + Sync {
    /// Is the streaming host running and does it accept us?
    fn check(&self) -> Result<(), String>;
    /// Make sure streams go through the companion's launcher.
    fn ensure_app(&self, command: &str) -> Result<(), String>;
    /// Complete the pairing Moonlight started from `from` with this PIN.
    fn pair(&self, pin: &str, name: &str, from: &str) -> Result<(), String>;
}

pub struct Sunshine {
    base: String,
    user: String,
    password: String,
}

const NOT_RUNNING: &str = "Sunshine is not running on this desktop.";

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16 | u32::from(*chunk.get(1).unwrap_or(&0)) << 8 | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Only this machine, by name or loopback address.
fn local(url: &str) -> bool {
    let rest = url.strip_prefix("https://").unwrap_or("");
    let host = rest.split('/').next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, port)| if port.bytes().all(|b| b.is_ascii_digit()) { h } else { host });
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// The app entry Sunshine should hold. The launcher stays alive for as long as
/// the game runs, so Sunshine ends the stream when the game ends.
fn app(command: &str, index: i64) -> Value {
    json!({
        "name": SUNSHINE_APP,
        "output": "",
        "cmd": command,
        "index": index,
        "exclude-global-prep-cmd": false,
        "elevated": false,
        "auto-detach": false,
        "wait-all": false,
        "exit-timeout": 5,
        "prep-cmd": [],
        "detached": [],
        "image-path": "",
    })
}

/// Where our app is in Sunshine's list and whether it needs writing.
fn place(apps: &Value, command: &str) -> (i64, bool) {
    let list = apps.get("apps").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    match list.iter().position(|a| a.get("name").and_then(Value::as_str) == Some(SUNSHINE_APP)) {
        Some(i) => (i as i64, list[i].get("cmd").and_then(Value::as_str) != Some(command)),
        None => (-1, true),
    }
}

/// Which waiting pairing request is the Ally's.
fn pick(pairings: &[Value], from: &str) -> Option<String> {
    let id = |p: &Value| p.get("id").and_then(Value::as_str).map(str::to_string);
    let same = |p: &&Value| p.get("address").and_then(Value::as_str).is_some_and(|a| a == from || a.ends_with(&format!(":{from}")));
    match pairings.iter().find(same) {
        Some(p) => id(p),
        // Sunshine may report the address in a form we did not expect. A single
        // request is still unambiguous; with several we refuse to guess.
        None if pairings.len() == 1 => id(&pairings[0]),
        None => None,
    }
}

impl Sunshine {
    pub fn new(url: &str, user: &str, password: &str) -> Result<Self, String> {
        if !local(url) {
            return Err("The companion only talks to Sunshine on this machine (https://localhost:47990).".to_string());
        }
        Ok(Self { base: url.trim_end_matches('/').to_string(), user: user.to_string(), password: password.to_string() })
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, String), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .tls_config(TlsConfig::builder().disable_verification(true).build())
            .build()
            .into();
        let auth = format!("Basic {}", base64(format!("{}:{}", self.user, self.password).as_bytes()));
        let url = format!("{}{path}", self.base);
        let reply = match (method, body) {
            ("POST", body) => agent
                .post(&url)
                .header("authorization", &auth)
                .header("content-type", "application/json")
                .send(body.map(Value::to_string).unwrap_or_else(|| "{}".to_string()).as_bytes()),
            _ => agent.get(&url).header("authorization", &auth).call(),
        };
        let mut reply = reply.map_err(|_| NOT_RUNNING.to_string())?;
        let status = reply.status().as_u16();
        let text = reply.body_mut().read_to_string().unwrap_or_default();
        if status == 401 {
            return Err("Sunshine refused the user name or password the companion was given. Run the companion's set-up again.".to_string());
        }
        Ok((status, text))
    }

    fn json(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value, String> {
        let (status, text) = self.call(method, path, body, timeout)?;
        match serde_json::from_str(&text) {
            Ok(value) if status == 200 => Ok(value),
            _ => Err(format!("Sunshine answered {path} in a way CarlaOS does not understand (status {status}). It may need updating.")),
        }
    }
}

const SHORT: Duration = Duration::from_secs(5);
/// Sunshine holds the PIN request open while Moonlight finishes.
const PAIRING: Duration = Duration::from_secs(60);

impl Streamer for Sunshine {
    fn check(&self) -> Result<(), String> {
        self.json("GET", "/api/apps", None, SHORT).map(|_| ())
    }

    fn ensure_app(&self, command: &str) -> Result<(), String> {
        let apps = self.json("GET", "/api/apps", None, SHORT)?;
        let (index, write) = place(&apps, command);
        if write {
            let saved = self.json("POST", "/api/apps", Some(&app(command, index)), SHORT)?;
            if saved.get("status").and_then(Value::as_bool) == Some(false) {
                return Err("Sunshine did not save the CarlaOS application.".to_string());
            }
        }
        Ok(())
    }

    fn pair(&self, pin: &str, name: &str, from: &str) -> Result<(), String> {
        let refused = || "Sunshine did not accept the pairing. Try linking again.".to_string();
        let accepted = |reply: Value| if reply.get("status").and_then(Value::as_bool) == Some(true) { Ok(()) } else { Err(refused()) };
        let end = Instant::now() + Duration::from_secs(25);
        // Newer Sunshine lists the requests that are waiting and wants to be
        // told which one the PIN is for. Older Sunshine takes the PIN alone.
        let (status, text) = self.call("GET", "/api/pin", None, SHORT)?;
        let listed = (status == 200).then(|| serde_json::from_str::<Value>(&text).ok()).flatten().filter(|v| v.get("pairings").is_some());
        if listed.is_none() {
            // Moonlight may not have asked yet; older Sunshine then says no.
            loop {
                if accepted(self.json("POST", "/api/pin", Some(&json!({ "pin": pin, "name": name })), PAIRING)?).is_ok() {
                    return Ok(());
                }
                if Instant::now() >= end {
                    return Err(refused());
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        let mut current = listed;
        loop {
            let pairings = current.as_ref().and_then(|v| v.get("pairings")).and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(id) = pick(&pairings, from) {
                return accepted(self.json("POST", "/api/pin", Some(&json!({ "pairing_id": id, "pin": pin, "name": name })), PAIRING)?);
            }
            if Instant::now() >= end {
                return Err(if pairings.is_empty() {
                    "Moonlight on the Ally never asked this desktop to pair. Check both are on the same network.".to_string()
                } else {
                    "Several devices are asking to pair at once. Try again when only the Ally is.".to_string()
                });
            }
            std::thread::sleep(Duration::from_secs(1));
            current = Some(self.json("GET", "/api/pin", None, SHORT)?);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"admin:pa55 word"), "YWRtaW46cGE1NSB3b3Jk");
    }

    #[test]
    fn only_this_machine_counts_as_local() {
        assert!(local("https://localhost:47990"));
        assert!(local("https://127.0.0.1:47990/"));
        assert!(local("https://[::1]:47990"));
        assert!(!local("http://localhost:47990"), "never without TLS");
        assert!(!local("https://192.168.1.20:47990"));
        assert!(!local("https://localhost.evil.example:47990"));
        assert!(!local("https://evil.example/localhost"));
    }

    #[test]
    fn the_app_is_added_once_and_repaired_when_it_moved() {
        let command = r#""C:\CarlaOS\carla-companion.exe" launch"#;
        assert_eq!(place(&json!({ "apps": [] }), command), (-1, true));
        assert_eq!(place(&json!({}), command), (-1, true));
        let apps = json!({ "apps": [{ "name": "Desktop" }, { "name": "CarlaOS", "cmd": command }] });
        assert_eq!(place(&apps, command), (1, false));
        assert_eq!(place(&apps, "elsewhere launch"), (1, true));
        assert_eq!(app(command, 1)["index"], 1);
    }

    #[test]
    fn the_pin_goes_to_the_allys_request_only() {
        let a = json!({ "id": "aa", "name": "Ally", "address": "192.168.1.50" });
        let b = json!({ "id": "bb", "name": "Phone", "address": "192.168.1.60" });
        let mapped = json!({ "id": "cc", "name": "Ally", "address": "::ffff:192.168.1.50" });
        assert_eq!(pick(&[a.clone(), b.clone()], "192.168.1.50").as_deref(), Some("aa"));
        assert_eq!(pick(&[b.clone(), mapped], "192.168.1.50").as_deref(), Some("cc"));
        assert_eq!(pick(&[b.clone()], "192.168.1.50").as_deref(), Some("bb"), "one request is unambiguous");
        assert_eq!(pick(&[b.clone(), b], "192.168.1.50"), None, "never guess between strangers");
        assert_eq!(pick(&[], "192.168.1.50"), None);
        let _ = a;
    }
}
