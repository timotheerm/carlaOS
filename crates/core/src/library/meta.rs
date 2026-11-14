//! Looking up a game's public details: a description and a cover.
//!
//! Used when the administrator adds a game by hand and asks for it. The only
//! thing sent anywhere is the name typed into the search: to Steam's public
//! store and to Wikipedia, neither of which needs an account. Steam knows
//! what is sold on Steam; Wikipedia knows nearly every game there has been,
//! which is what finds a console classic that now runs as a native port.
//! Nothing is ever downloaded but text and pictures.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const STORE: &str = "https://store.steampowered.com/api";
pub(crate) const PICTURES: &str = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps";
pub(crate) const WIKI: &str = "https://en.wikipedia.org/w/api.php";
const WAIT: Duration = Duration::from_secs(8);
pub(crate) const LARGEST_PICTURE: u64 = 8 * 1024 * 1024;
/// Pictures this wide are asked of Wikipedia: enough for a cover.
pub(crate) const WIKI_WIDTH: u32 = 640;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Match {
    /// Where it was found and its id there: "steam:504230", "wiki:22936".
    pub id: String,
    pub name: String,
    /// "Steam" or "Wikipedia", to tell two of the same name apart.
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Details {
    pub name: String,
    pub description: String,
    /// A picture of the game: Steam's wide one, or the one on its Wikipedia page.
    pub header: Option<String>,
}

/// One Wikipedia page as a search or a lookup returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub id: u64,
    pub title: String,
    pub extract: String,
    /// Its picture: address, width, height.
    pub picture: Option<(String, u32, u32)>,
}

fn offline<E>(_: E) -> String {
    "The details could not be fetched. Is the Ally online?".to_string()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(WAIT)).http_status_as_error(false).build().into()
}

pub(crate) fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Fetch an address. `key` is sent as a bearer token, for the one service
/// that wants one.
pub(crate) fn get_with(url: &str, limit: u64, key: Option<&str>) -> Result<(u16, Vec<u8>), String> {
    // Wikipedia asks every program to say what it is.
    let mut request = agent().get(url).header("User-Agent", format!("CarlaOS/{} (game library)", crate::VERSION));
    if let Some(key) = key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut reply = request.call().map_err(offline)?;
    let status = reply.status().as_u16();
    let bytes = reply.body_mut().with_config().limit(limit).read_to_vec().map_err(offline)?;
    Ok((status, bytes))
}

pub(crate) fn get(url: &str, limit: u64) -> Result<(u16, Vec<u8>), String> {
    get_with(url, limit, None)
}

pub fn parse_search(body: &[u8]) -> Vec<Match> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    let items = value.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str).is_none_or(|t| t == "app"))
        .filter_map(|item| Some(Match { id: format!("steam:{}", item.get("id")?.as_u64()?), name: item.get("name")?.as_str()?.to_string(), from: "Steam".to_string() }))
        .take(6)
        .collect()
}

/// The pages in a Wikipedia answer, best match first. Pages that only list
/// other pages are left out.
pub fn parse_wiki(body: &[u8]) -> Vec<Page> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    let Some(pages) = value.pointer("/query/pages").and_then(Value::as_object) else { return Vec::new() };
    let mut found: Vec<(u64, Page)> = pages
        .values()
        .filter(|page| page.pointer("/pageprops/disambiguation").is_none())
        .filter_map(|page| {
            let picture = page.get("thumbnail").and_then(|t| {
                let size = |name| t.get(name).and_then(Value::as_u64).map(|n| n as u32);
                Some((t.get("source")?.as_str()?.to_string(), size("width")?, size("height")?))
            });
            Some((
                page.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX),
                Page {
                    id: page.get("pageid")?.as_u64()?,
                    title: page.get("title")?.as_str()?.to_string(),
                    extract: brief(page.get("extract").and_then(Value::as_str).unwrap_or_default()),
                    picture: picture.filter(|(url, ..)| trusted_picture(url)),
                },
            ))
        })
        .collect();
    found.sort_by_key(|(index, _)| *index);
    found.into_iter().map(|(_, page)| page).collect()
}

/// The first sentences of a page, short enough for a game's page here.
fn brief(text: &str) -> String {
    const MOST: usize = 420;
    let text = text.split('\n').next().unwrap_or_default().trim();
    if text.chars().count() <= MOST {
        return text.to_string();
    }
    let cut: String = text.chars().take(MOST).collect();
    match cut.rfind(". ") {
        Some(end) if end > MOST / 3 => cut[..=end].to_string(),
        _ => format!("{}…", cut.trim_end()),
    }
}

/// What every Wikipedia question here asks for: the opening text and the
/// page's picture, including pictures that are only there under fair use,
/// which is what box art is.
pub(crate) fn wiki_query(which: &str) -> String {
    format!("{WIKI}?action=query&format=json&redirects=1&{which}&prop=extracts%7Cpageimages%7Cpageprops&ppprop=disambiguation&exintro=1&explaintext=1&exsentences=4&exlimit=max&piprop=thumbnail&pithumbsize={WIKI_WIDTH}&pilimit=max&pilicense=any")
}

/// Wikipedia pages about games whose text matches.
pub(crate) fn wiki_search(term: &str, most: usize) -> Result<Vec<Page>, String> {
    let which = format!("generator=search&gsrnamespace=0&gsrlimit={most}&gsrsearch={}", encode(&format!("{term} video game")));
    let (status, body) = get(&wiki_query(&which), 2 * 1024 * 1024)?;
    if status != 200 {
        return Err(offline(()));
    }
    Ok(parse_wiki(&body))
}

pub fn parse_details(body: &[u8], id: &str) -> Option<Details> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let entry = value.get(id)?;
    if entry.get("success").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let data = entry.get("data")?;
    Some(Details {
        name: data.get("name")?.as_str()?.to_string(),
        description: plain(data.get("short_description").and_then(Value::as_str).unwrap_or_default()),
        // Only pictures from the store's own picture hosts are fetched.
        header: data.get("header_image").and_then(Value::as_str).filter(|u| trusted_picture(u)).map(str::to_string),
    })
}

/// Pictures are only ever fetched from the picture hosts of the places
/// CarlaOS asks: Steam, Wikipedia, SteamGridDB and the libretro box art.
pub fn trusted_picture(url: &str) -> bool {
    let host = url.strip_prefix("https://").and_then(|rest| rest.split(['/', '?', '#']).next()).unwrap_or_default();
    host.ends_with(".steamstatic.com")
        || host.ends_with(".steampowered.com")
        || host == "upload.wikimedia.org"
        || host.ends_with(".steamgriddb.com")
        || host == "thumbnails.libretro.com"
}

/// Store text carries a few HTML entities and the odd tag.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").trim().to_string()
}

/// Games whose name matches: what Steam sells first, then what Wikipedia
/// knows, so a game that never came out on Steam is found too.
pub fn search(term: &str) -> Result<Vec<Match>, String> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let (steam, wiki) = std::thread::scope(|scope| {
        let steam = scope.spawn(|| {
            let (status, body) = get(&format!("{STORE}/storesearch/?term={}&cc=us&l=en", encode(term)), 1024 * 1024)?;
            if status != 200 {
                return Err(offline(()));
            }
            Ok(parse_search(&body))
        });
        let wiki = scope.spawn(|| wiki_search(term, 5));
        (steam.join().unwrap_or_else(|_| Err(offline(()))), wiki.join().unwrap_or_else(|_| Err(offline(()))))
    });
    if let (Err(e), Err(_)) = (&steam, &wiki) {
        return Err(e.clone());
    }
    let mut found: Vec<Match> = steam.unwrap_or_default().into_iter().take(4).collect();
    found.extend(wiki.unwrap_or_default().into_iter().filter(|p| !p.extract.is_empty()).take(4).map(|p| Match { id: format!("wiki:{}", p.id), name: p.title, from: "Wikipedia".to_string() }));
    Ok(found)
}

/// The number in "steam:504230" or "wiki:22936".
fn number<'a>(id: &'a str, from: &str) -> Option<&'a str> {
    id.strip_prefix(from)?.strip_prefix(':').filter(|n| !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn details(id: &str) -> Result<Details, String> {
    if let Some(page) = number(id, "wiki") {
        let (status, body) = get(&wiki_query(&format!("pageids={page}")), 2 * 1024 * 1024)?;
        let found = parse_wiki(&body).into_iter().next().filter(|_| status == 200).ok_or("Wikipedia has no page for that game.")?;
        return Ok(Details { name: found.title, description: found.extract, header: found.picture.map(|(url, ..)| url) });
    }
    let app = number(id, "steam").ok_or("That is not a game from the search.")?;
    let (status, body) = get(&format!("{STORE}/appdetails?appids={app}&filters=basic&l=en"), 2 * 1024 * 1024)?;
    parse_details(&body, app).filter(|_| status == 200).ok_or_else(|| "The store has no details for that game.".to_string())
}

/// JPEG, PNG or WebP, going by how the bytes begin. Gives the file ending.
pub fn picture_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Fetch one picture from a trusted picture host.
pub fn picture(url: &str) -> Result<Vec<u8>, String> {
    if !trusted_picture(url) {
        return Err("That picture is not from a place CarlaOS fetches pictures from.".to_string());
    }
    match get(url, LARGEST_PICTURE)? {
        (200, bytes) if picture_kind(&bytes).is_some() => Ok(bytes),
        _ => Err("That picture could not be fetched.".to_string()),
    }
}

/// The game's cover as picture bytes: Steam's tall library cover when it has
/// one, otherwise the picture that came with the details.
pub fn cover(id: &str, header: Option<&str>) -> Result<Vec<u8>, String> {
    let tall = number(id, "steam").map(|app| format!("{PICTURES}/{app}/library_600x900.jpg"));
    for url in [tall.as_deref(), header].into_iter().flatten() {
        if let Ok(bytes) = picture(url) {
            return Ok(bytes);
        }
    }
    Err("No cover was found for that game.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results_are_read_from_the_store_answer() {
        let body = br#"{"total":2,"items":[
            {"type":"app","name":"Celeste","id":504230,"tiny_image":"https://x/y.jpg","metascore":"88"},
            {"type":"bundle","name":"Celeste Bundle","id":1},
            {"name":"No type","id":7}]}"#;
        assert_eq!(
            parse_search(body),
            [Match { id: "steam:504230".into(), name: "Celeste".into(), from: "Steam".into() }, Match { id: "steam:7".into(), name: "No type".into(), from: "Steam".into() }]
        );
        assert!(parse_search(b"<html>busy</html>").is_empty());
        assert!(parse_search(br#"{"total":0,"items":[]}"#).is_empty());
    }

    #[test]
    fn details_are_cleaned_and_pictures_only_come_from_the_store() {
        let body = br#"{"504230":{"success":true,"data":{"type":"game","name":"Celeste",
            "short_description":"Help <b>Madeline</b> &amp; friends &quot;climb&quot;. ",
            "header_image":"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/504230/header.jpg?t=1"}}}"#;
        let d = parse_details(body, "504230").unwrap();
        assert_eq!(d.name, "Celeste");
        assert_eq!(d.description, "Help Madeline & friends \"climb\".");
        assert!(d.header.unwrap().ends_with("header.jpg?t=1"));
        assert!(parse_details(body, "1").is_none());
        assert!(parse_details(br#"{"5":{"success":false}}"#, "5").is_none());
        let elsewhere = br#"{"5":{"success":true,"data":{"name":"X","header_image":"https://evil.example/steamstatic.com/x.jpg"}}}"#;
        assert_eq!(parse_details(elsewhere, "5").unwrap().header, None);
    }

    #[test]
    fn search_terms_are_safe_in_an_address_and_ids_are_digits() {
        assert_eq!(encode("Moss & Ember: 2"), "Moss%20%26%20Ember%3A%202");
        assert_eq!(picture_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(picture_kind(b"RIFF\x10\x00\x00\x00WEBPVP8 "), Some("webp"));
        assert_eq!(picture_kind(b"<html>"), None);
        assert_eq!((number("steam:504230", "steam"), number("wiki:22936", "wiki")), (Some("504230"), Some("22936")));
        assert_eq!((number("wiki:22936", "steam"), number("steam:5/../x", "steam"), number("steam:", "steam")), (None, None, None));
    }

    #[test]
    fn a_console_classic_is_found_on_wikipedia() {
        let body = br#"{"batchcomplete":"","query":{"pages":{
            "22936":{"pageid":22936,"ns":0,"title":"The Legend of Zelda: Ocarina of Time","index":1,
                "extract":"The Legend of Zelda: Ocarina of Time is a 1998 action-adventure game by Nintendo for the Nintendo 64. It was released in Japan and North America in November 1998.\nSecond paragraph.",
                "thumbnail":{"source":"https://upload.wikimedia.org/wikipedia/en/5/57/The_Legend_of_Zelda_Ocarina_of_Time.jpg","width":256,"height":176}},
            "5":{"pageid":5,"ns":0,"title":"Ocarina (disambiguation)","index":2,"pageprops":{"disambiguation":""},"extract":"Ocarina may refer to:"},
            "9":{"pageid":9,"ns":0,"title":"Ocarina of Time 3D","index":3,"extract":"A remake.",
                "thumbnail":{"source":"https://evil.example/upload.wikimedia.org/x.jpg","width":10,"height":10}},
            "7":{"pageid":7,"ns":0,"title":"Master Quest","index":0,"extract":""}}}}"#;
        let pages = parse_wiki(body);
        assert_eq!(pages.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Master Quest", "The Legend of Zelda: Ocarina of Time", "Ocarina of Time 3D"]);
        let oot = &pages[1];
        assert_eq!(oot.id, 22936);
        assert!(oot.extract.starts_with("The Legend of Zelda") && oot.extract.ends_with("November 1998."), "only the opening paragraph");
        assert_eq!(oot.picture.as_ref().map(|(_, w, h)| (*w, *h)), Some((256, 176)));
        assert_eq!(pages[2].picture, None, "a picture from anywhere else is not kept");
        assert!(parse_wiki(b"<html>busy</html>").is_empty());
        assert!(parse_wiki(br#"{"batchcomplete":""}"#).is_empty());

        // A long opening is cut at the end of a sentence.
        let long = format!("{} The end is far.", "A sentence of some length about a game. ".repeat(15));
        let cut = brief(&long);
        assert!(cut.chars().count() <= 420 && cut.ends_with("game."), "{cut}");
        assert!(wiki_query("pageids=1").contains("pilicense=any"));
    }

    #[test]
    fn pictures_only_come_from_the_places_carlaos_asks() {
        for good in [
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1/header.jpg",
            "https://upload.wikimedia.org/wikipedia/en/5/57/Box.jpg",
            "https://cdn2.steamgriddb.com/grid/abc.png",
            "https://thumbnails.libretro.com/Nintendo%20-%20Nintendo%2064/Named_Boxarts/X.png",
        ] {
            assert!(trusted_picture(good), "{good}");
        }
        for bad in ["http://upload.wikimedia.org/x.jpg", "https://upload.wikimedia.org.evil.example/x.jpg", "https://evil.example/?https://cdn2.steamgriddb.com/x.png", "https://steamgriddb.com.evil.example/x.png", "file:///C:/x.png", ""] {
            assert!(!trusted_picture(bad), "{bad}");
        }
        assert!(picture("https://evil.example/x.png").is_err());
    }
}
