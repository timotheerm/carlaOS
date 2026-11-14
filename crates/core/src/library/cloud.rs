//! Cloud gaming: games that run in a data centre and are played through a
//! web page.
//!
//! GeForce NOW, Xbox Cloud Gaming, Boosteroid and Luna all play in a browser,
//! with the controller, full screen. So a cloud game in CarlaOS is a link to
//! the game's page on its service, and playing it opens that page in a
//! browser window of its own: full screen, without the browser's bars, with
//! a profile kept only for this, so the administrator signs in once and the
//! player never sees a sign-in page.
//!
//! CarlaOS buys nothing, signs in to nothing and sends nothing to the
//! services itself; it only opens their pages.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The place name of cloud games: they are played "in the cloud", not on
/// this Ally or a desktop.
pub const CLOUD: &str = "cloud";

/// A cloud gaming service CarlaOS knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    /// The page that opens when the service itself is chosen.
    pub home: &'static str,
    /// The one host its pages are on.
    #[serde(skip)]
    host: &'static str,
    /// What a page's path starts with, after an optional language part
    /// such as `/en-US`.
    #[serde(skip)]
    path: &'static str,
}

pub const SERVICES: &[Service] = &[
    Service { id: "gfn", name: "GeForce NOW", home: "https://play.geforcenow.com/mall/", host: "play.geforcenow.com", path: "/" },
    Service { id: "xcloud", name: "Xbox Cloud Gaming", home: "https://www.xbox.com/play", host: "www.xbox.com", path: "/play" },
    Service { id: "boosteroid", name: "Boosteroid", home: "https://cloud.boosteroid.com/", host: "cloud.boosteroid.com", path: "/" },
    Service { id: "luna", name: "Amazon Luna", home: "https://luna.amazon.com/", host: "luna.amazon.com", path: "/" },
];

pub fn service(id: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|s| s.id == id)
}

/// "/en-US/play/games/x" without its language part: "/play/games/x".
fn without_language(path: &str) -> &str {
    let mut parts = path.splitn(3, '/');
    let (_, first) = (parts.next(), parts.next().unwrap_or_default());
    let looks_like_language = (first.len() == 2 || (first.len() == 5 && first.as_bytes()[2] == b'-')) && first.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-');
    if looks_like_language {
        &path[first.len() + 1..]
    } else {
        path
    }
}

/// A link the administrator gave, checked: a page of a service CarlaOS
/// knows, and nothing that could be read as anything but an address. Gives
/// the service and the link as it will be opened.
pub fn checked(link: &str) -> Result<(&'static Service, String), String> {
    let link = link.trim();
    let wrong = || "That is not a link to a game on GeForce NOW, Xbox Cloud Gaming, Boosteroid or Luna. Copy the game's link from its page there.".to_string();
    if link.len() > 2048 || link.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`' | '^' | '{' | '}' | '|')) {
        return Err(wrong());
    }
    let rest = link.strip_prefix("https://").ok_or_else(wrong)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = (&rest[..end], &rest[end..]);
    // No user name, no port, nothing but the service's own host.
    let service = SERVICES.iter().find(|s| s.host.eq_ignore_ascii_case(host)).ok_or_else(wrong)?;
    let path = if path.is_empty() || path.starts_with(['?', '#']) { "/" } else { path };
    if !without_language(path).starts_with(service.path) {
        return Err(wrong());
    }
    Ok((service, format!("https://{}{path}", service.host)))
}

/// A name to start from, read from the link where the service puts one:
/// "https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN"
/// suggests "Forza Horizon 5".
pub fn suggest_title(link: &str) -> String {
    let Ok((service, link)) = checked(link) else { return String::new() };
    let path = link.trim_start_matches("https://").split_once('/').map_or("", |(_, p)| p);
    let path = format!("/{}", path.split(['?', '#']).next().unwrap_or_default());
    let parts: Vec<&str> = without_language(&path).split('/').filter(|p| !p.is_empty()).collect();
    let slug = match service.id {
        "xcloud" => parts.windows(2).find(|w| matches!(w[0], "games" | "launch")).map(|w| w[1]),
        _ => None,
    };
    slug.map(|s| {
        s.split('-')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
    .unwrap_or_default()
}

/// The browsers a cloud game can be played in, in the order they are tried.
/// Microsoft Edge comes with Windows, so there is always one.
fn browsers() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        let Some(base) = std::env::var_os(var).map(PathBuf::from) else { continue };
        found.push(base.join("Microsoft").join("Edge").join("Application").join("msedge.exe"));
        found.push(base.join("Google").join("Chrome").join("Application").join("chrome.exe"));
    }
    // Elsewhere, for trying CarlaOS out.
    found.extend(["/usr/bin/microsoft-edge", "/usr/bin/google-chrome", "/usr/bin/chromium"].map(PathBuf::from));
    found
}

pub fn find_browser() -> Option<PathBuf> {
    // Edge before Chrome whatever the folder: the loop above alternates.
    let all = browsers();
    let edge = all.iter().find(|p| p.to_string_lossy().contains("msedge") && p.is_file());
    edge.or_else(|| all.iter().find(|p| p.is_file())).cloned()
}

/// The options that open a page as a full screen window of its own, in a
/// profile kept only for cloud gaming.
pub fn browser_args(profile: &Path, link: &str) -> Vec<String> {
    vec![
        format!("--user-data-dir={}", profile.display()),
        format!("--app={link}"),
        "--start-fullscreen".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        // The services start their sound before anyone has pressed anything.
        "--autoplay-policy=no-user-gesture-required".to_string(),
        // No "restore pages?" bubble after a game was ended from the quick menu.
        "--disable-session-crashed-bubble".to_string(),
        "--hide-crash-restore-bubble".to_string(),
        "--disable-features=Translate,msEdgeSidebarV2,msUndersideButton,msImplicitSignin".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_to_a_known_service_are_taken_and_anything_else_is_refused() {
        let (s, link) = checked(" https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN ").unwrap();
        assert_eq!((s.id, link.as_str()), ("xcloud", "https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN"));
        assert_eq!(checked("https://www.xbox.com/play/launch/halo-infinite/9PP5G1F0C2B6").unwrap().0.id, "xcloud");
        assert_eq!(checked("https://play.geforcenow.com/mall/#/deeplink?game-id=6a1b0c8e-0000-4d4b-9b1e-0123456789ab").unwrap().0.id, "gfn");
        assert_eq!(checked("https://cloud.boosteroid.com/application/123").unwrap().0.id, "boosteroid");
        assert_eq!(checked("https://LUNA.amazon.com").unwrap().1, "https://luna.amazon.com/");
        for wrong in [
            "http://www.xbox.com/play",
            "https://www.xbox.com/en-US/games/store/x",
            "https://www.xbox.com.evil.example/play",
            "https://evil.example/?https://play.geforcenow.com/",
            "https://user@play.geforcenow.com/mall/",
            "https://play.geforcenow.com:8443/mall/",
            "https://play.geforcenow.com/mall/\" --renderer-cmd-prefix=calc",
            "https://play.geforcenow.com/mall/ --flag",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "",
        ] {
            assert!(checked(wrong).is_err(), "{wrong:?}");
        }
        assert!(checked(&format!("https://play.geforcenow.com/{}", "a".repeat(3000))).is_err());
    }

    #[test]
    fn a_name_is_read_from_the_link_where_there_is_one() {
        assert_eq!(suggest_title("https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN"), "Forza Horizon 5");
        assert_eq!(suggest_title("https://www.xbox.com/play/launch/hollow-knight/9MW9469V91LM?x=1"), "Hollow Knight");
        assert_eq!(suggest_title("https://play.geforcenow.com/mall/#/deeplink?game-id=x"), "");
        assert_eq!(suggest_title("https://evil.example/play/games/x/y"), "");
        assert_eq!((without_language("/en-US/play"), without_language("/play/x"), without_language("/de/play")), ("/play", "/play/x", "/play"));
        assert_eq!(service("gfn").unwrap().name, "GeForce NOW");
        assert!(service("nope").is_none());
        assert!(SERVICES.iter().all(|s| checked(s.home).map(|(found, _)| found.id) == Ok(s.id)), "every home page is a link of its own service");
    }

    #[test]
    fn the_browser_opens_one_page_full_screen_in_its_own_profile() {
        let args = browser_args(Path::new("/data/cloud-browser"), "https://www.xbox.com/play");
        assert_eq!(args[0], "--user-data-dir=/data/cloud-browser");
        assert_eq!(args[1], "--app=https://www.xbox.com/play");
        assert!(args.contains(&"--start-fullscreen".to_string()));
        // Each option is one argument: nothing in a link can become another.
        assert!(args.iter().all(|a| a.starts_with("--")));
    }
}
