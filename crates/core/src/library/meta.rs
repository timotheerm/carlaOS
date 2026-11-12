//! Looking up a game's public details: a description and a cover.
//!
//! Used when the administrator adds a game by hand and asks for it. The only
//! thing sent anywhere is the name typed into the search, to Steam's public
//! store, which needs no account. Nothing is ever downloaded but text and
//! pictures.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const STORE: &str = "https://store.steampowered.com/api";
const PICTURES: &str = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps";
const WAIT: Duration = Duration::from_secs(8);
const LARGEST_PICTURE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Match {
    /// Steam's id for the game, digits only.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Details {
    pub name: String,
    pub description: String,
    /// Wide picture, always present on the store.
    pub header: Option<String>,
}

fn offline<E>(_: E) -> String {
    "The details could not be fetched. Is the Ally online?".to_string()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(WAIT)).http_status_as_error(false).build().into()
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn get(url: &str, limit: u64) -> Result<(u16, Vec<u8>), String> {
    let mut reply = agent().get(url).call().map_err(offline)?;
    let status = reply.status().as_u16();
    let bytes = reply.body_mut().with_config().limit(limit).read_to_vec().map_err(offline)?;
    Ok((status, bytes))
}

pub fn parse_search(body: &[u8]) -> Vec<Match> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    let items = value.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str).is_none_or(|t| t == "app"))
        .filter_map(|item| Some(Match { id: item.get("id")?.as_u64()?.to_string(), name: item.get("name")?.as_str()?.to_string() }))
        .take(6)
        .collect()
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

fn trusted_picture(url: &str) -> bool {
    let host = url.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or_default();
    host.ends_with(".steamstatic.com") || host.ends_with(".steampowered.com")
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

/// Games on the store whose name matches.
pub fn search(term: &str) -> Result<Vec<Match>, String> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let (status, body) = get(&format!("{STORE}/storesearch/?term={}&cc=us&l=en", encode(term)), 1024 * 1024)?;
    if status != 200 {
        return Err(offline(()));
    }
    Ok(parse_search(&body))
}

fn digits(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err("That is not a game from the search.".to_string())
    }
}

pub fn details(id: &str) -> Result<Details, String> {
    digits(id)?;
    let (status, body) = get(&format!("{STORE}/appdetails?appids={id}&filters=basic&l=en"), 2 * 1024 * 1024)?;
    parse_details(&body, id).filter(|_| status == 200).ok_or_else(|| "The store has no details for that game.".to_string())
}

fn is_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// The game's cover as picture bytes: the tall library cover when the store
/// has one, otherwise the wide header.
pub fn cover(id: &str, header: Option<&str>) -> Result<Vec<u8>, String> {
    digits(id)?;
    let tall = format!("{PICTURES}/{id}/library_600x900.jpg");
    for url in [Some(tall.as_str()), header.filter(|u| trusted_picture(u))].into_iter().flatten() {
        if let Ok((200, bytes)) = get(url, LARGEST_PICTURE) {
            if is_picture(&bytes) {
                return Ok(bytes);
            }
        }
    }
    Err("The store has no cover for that game.".to_string())
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
            [Match { id: "504230".into(), name: "Celeste".into() }, Match { id: "7".into(), name: "No type".into() }]
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
        assert!(digits("504230").is_ok());
        assert!(digits("50/../x").is_err());
        assert!(digits("").is_err());
        assert!(is_picture(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_picture(b"<html>"));
    }
}
