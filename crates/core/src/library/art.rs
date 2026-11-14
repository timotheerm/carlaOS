//! Finding pictures for a game, several to choose from.
//!
//! The administrator asks for pictures of one game; CarlaOS asks the places
//! that have them and shows what came back, and the administrator picks.
//! Nothing is chosen for them and nothing is fetched until they ask.
//!
//! Where pictures come from:
//! - **Steam**: the library cover and wide pictures of games sold there.
//! - **Wikipedia**: the box art on a game's page. This is what finds a
//!   console classic that now runs as a native port.
//! - **libretro**: box art of console games, by the game's exact name.
//! - **SteamGridDB**: many community-made covers for nearly every game. It
//!   wants a free key, which the administrator can enter once.
//!
//! The only thing sent anywhere is the name searched for.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::meta::{self, encode, get, get_with, trusted_picture, PICTURES};

const GRID: &str = "https://www.steamgriddb.com/api/v2";
const BOXART: &str = "https://thumbnails.libretro.com";
/// More than this is more than anyone wants to look through.
pub const MOST: usize = 30;

/// Which way up a picture is, which decides where it is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// A cover: the card in the library.
    Tall,
    /// A wide picture: the game's page and "Continue playing".
    Wide,
}

impl Shape {
    fn of(width: u32, height: u32) -> Shape {
        if width > height + height / 5 {
            Shape::Wide
        } else {
            Shape::Tall
        }
    }
}

/// One picture the administrator can choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// "Steam", "Wikipedia", "Box art" or "SteamGridDB".
    pub source: String,
    /// The game it belongs to, as that place names it.
    pub label: String,
    pub shape: Shape,
    /// A small version, to look at.
    pub thumb: String,
    /// The one that is kept.
    pub full: String,
}

impl Candidate {
    /// Both addresses must be on a picture host CarlaOS asks.
    pub fn trusted(&self) -> bool {
        trusted_picture(&self.thumb) && trusted_picture(&self.full)
    }
}

/// What a search found, and what could not be asked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Found {
    pub candidates: Vec<Candidate>,
    /// One sentence for each place that did not answer.
    pub notes: Vec<String>,
}

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The name typed, or the game's title.
    pub term: String,
    /// The console the game is from, when it is from one: "GameCube".
    pub console: String,
    /// The game file's own name without its ending, for an emulated game.
    pub file: Option<String>,
    /// The administrator's SteamGridDB key, when one was entered.
    pub key: Option<String>,
}

fn candidate(source: &str, label: &str, shape: Shape, thumb: String, full: String) -> Candidate {
    Candidate { source: source.to_string(), label: label.to_string(), shape, thumb, full }
}

// ---- Steam ----

/// The pictures Steam keeps for one game. Not every game has every one; one
/// that is missing simply does not load and is left out.
fn steam_pictures(app: &str, name: &str) -> Vec<Candidate> {
    [("library_600x900.jpg", Shape::Tall), ("library_hero.jpg", Shape::Wide), ("header.jpg", Shape::Wide)]
        .into_iter()
        .map(|(file, shape)| {
            let url = format!("{PICTURES}/{app}/{file}");
            candidate("Steam", name, shape, url.clone(), url)
        })
        .collect()
}

fn steam(term: &str) -> Result<Vec<Candidate>, String> {
    let (status, body) = get(&format!("https://store.steampowered.com/api/storesearch/?term={}&cc=us&l=en", encode(term)), 1024 * 1024)?;
    if status != 200 {
        return Err("Steam did not answer.".to_string());
    }
    Ok(meta::parse_search(&body).iter().take(3).flat_map(|m| steam_pictures(m.id.trim_start_matches("steam:"), &m.name)).collect())
}

// ---- Wikipedia ----

fn wikipedia(term: &str) -> Result<Vec<Candidate>, String> {
    Ok(meta::wiki_search(term, 6)?
        .into_iter()
        .filter_map(|page| {
            let (url, width, height) = page.picture?;
            Some(candidate("Wikipedia", &page.title, Shape::of(width, height), url.clone(), url))
        })
        .collect())
}

// ---- libretro box art ----

/// (words in a console's name, the folder its box art is in). The longer
/// names come first, so "Game Boy Advance" is not taken for a Game Boy.
const SYSTEMS: &[(&[&str], &str)] = &[
    (&["nintendo 64", "n64"], "Nintendo - Nintendo 64"),
    (&["gamecube", "game cube"], "Nintendo - GameCube"),
    (&["wii u", "wiiu"], "Nintendo - Wii U"),
    (&["wii"], "Nintendo - Wii"),
    (&["super nintendo", "snes", "super famicom"], "Nintendo - Super Nintendo Entertainment System"),
    (&["game boy advance", "gba"], "Nintendo - Game Boy Advance"),
    (&["game boy color", "gbc"], "Nintendo - Game Boy Color"),
    (&["game boy"], "Nintendo - Game Boy"),
    (&["3ds"], "Nintendo - Nintendo 3DS"),
    (&["nintendo ds", "nds"], "Nintendo - Nintendo DS"),
    (&["nes", "famicom", "nintendo entertainment system"], "Nintendo - Nintendo Entertainment System"),
    (&["playstation 3", "ps3"], "Sony - PlayStation 3"),
    (&["playstation 2", "ps2"], "Sony - PlayStation 2"),
    (&["playstation portable", "psp"], "Sony - PlayStation Portable"),
    (&["playstation", "ps1", "psx"], "Sony - PlayStation"),
    (&["dreamcast"], "Sega - Dreamcast"),
    (&["saturn"], "Sega - Saturn"),
    (&["mega drive", "megadrive", "genesis"], "Sega - Mega Drive - Genesis"),
    (&["master system"], "Sega - Master System - Mark III"),
    (&["xbox"], "Microsoft - Xbox"),
];

/// The box art folders for a console as the administrator named it.
/// "GameCube and Wii" gives both.
pub fn systems(console: &str) -> Vec<&'static str> {
    let mut name = format!(" {} ", console.to_lowercase());
    let mut found = Vec::new();
    for (words, system) in SYSTEMS {
        for word in *words {
            // Whole words only: "nes" is not in "Genesis".
            let spaced = format!(" {word} ");
            if name.contains(&spaced) {
                name = name.replace(&spaced, "  ");
                found.push(*system);
                break;
            }
        }
    }
    found
}

/// "The Legend of Zelda: Ocarina of Time" as the box art collection spells
/// it: "Legend of Zelda, The - Ocarina of Time".
fn catalogued(title: &str) -> String {
    let mut parts: Vec<String> = title.split(": ").map(|p| p.trim().to_string()).collect();
    if let Some(rest) = parts[0].strip_prefix("The ") {
        parts[0] = format!("{rest}, The");
    }
    parts.join(" - ")
}

/// The names a game's box art may be filed under, likeliest first.
fn box_names(term: &str, file: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = file.map(str::to_string).into_iter().collect();
    let base = catalogued(term);
    for region in ["USA", "Europe", "World", "USA, Europe", "Japan"] {
        names.push(format!("{base} ({region})"));
    }
    names.dedup();
    names
}

/// The address of one box art picture. The collection replaces the letters
/// that file names cannot hold with an underscore.
fn box_url(system: &str, name: &str) -> String {
    let safe: String = name.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect();
    format!("{BOXART}/{}/Named_Boxarts/{}.png", encode(system), encode(&safe))
}

/// Box art is found by guessing its name, so every guess is offered and the
/// ones that do not exist are left out when their pictures do not load.
fn box_art(wanted: &Wanted) -> Vec<Candidate> {
    let names = box_names(&wanted.term, wanted.file.as_deref());
    systems(&wanted.console)
        .into_iter()
        .flat_map(|system| {
            names.iter().map(move |name| {
                let url = box_url(system, name);
                candidate("Box art", name, Shape::Tall, url.clone(), url)
            })
        })
        .collect()
}

// ---- SteamGridDB ----

/// The games in a SteamGridDB search answer: (id, name).
pub fn parse_grid_games(body: &[u8]) -> Vec<(u64, String)> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    value["data"].as_array().into_iter().flatten().filter_map(|g| Some((g["id"].as_u64()?, g["name"].as_str()?.to_string()))).collect()
}

/// The pictures in a SteamGridDB answer. Animated ones and ones marked as
/// not for everyone are left out.
pub fn parse_grid_pictures(body: &[u8], label: &str) -> Vec<Candidate> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["nsfw"].as_bool() != Some(true) && p["epilepsy"].as_bool() != Some(true) && p["humor"].as_bool() != Some(true))
        .filter_map(|p| {
            let (url, thumb) = (p["url"].as_str()?, p["thumb"].as_str().or(p["url"].as_str())?);
            let still = [".png", ".jpg", ".jpeg", ".webp"].iter().any(|e| url.to_lowercase().ends_with(e));
            let shape = Shape::of(p["width"].as_u64()? as u32, p["height"].as_u64()? as u32);
            Some(candidate("SteamGridDB", label, shape, thumb.to_string(), url.to_string())).filter(|c| still && c.trusted())
        })
        .collect()
}

fn grid(term: &str, key: &str) -> Result<Vec<Candidate>, String> {
    let ask = |path: &str| -> Result<Vec<u8>, String> {
        match get_with(&format!("{GRID}/{path}"), 2 * 1024 * 1024, Some(key))? {
            (200, body) => Ok(body),
            (401 | 403, _) => Err("SteamGridDB did not accept the key. Enter it again under Game artwork.".to_string()),
            _ => Err("SteamGridDB did not answer.".to_string()),
        }
    };
    let games = parse_grid_games(&ask(&format!("search/autocomplete/{}", encode(term)))?);
    let mut found = Vec::new();
    for (i, (id, name)) in games.iter().take(2).enumerate() {
        let covers = ask(&format!("grids/game/{id}?dimensions=600x900&types=static&nsfw=false&humor=false")).unwrap_or_default();
        found.extend(parse_grid_pictures(&covers, name).into_iter().take(if i == 0 { 10 } else { 4 }));
        if i == 0 {
            let wide = ask(&format!("heroes/game/{id}?types=static&nsfw=false&humor=false")).unwrap_or_default();
            found.extend(parse_grid_pictures(&wide, name).into_iter().take(4));
        }
    }
    Ok(found)
}

/// Is this what a SteamGridDB key looks like? Checked before it is kept or
/// sent, so nothing else typed there by mistake goes anywhere.
pub fn key_shape(key: &str) -> bool {
    (16..=64).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Put what each place found in the order it is shown: covers before wide
/// pictures, and within each the places take turns, so the first screen is
/// not all from one of them.
fn arrange(lists: Vec<Vec<Candidate>>) -> Vec<Candidate> {
    let mut out = Vec::new();
    for shape in [Shape::Tall, Shape::Wide] {
        let mut lists: Vec<_> = lists.iter().map(|list| list.iter().filter(|c| c.shape == shape)).collect();
        loop {
            let before = out.len();
            for list in &mut lists {
                out.extend(list.next().cloned());
            }
            if out.len() == before {
                break;
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|c: &Candidate| c.trusted() && seen.insert(c.full.clone()));
    out
}

/// Ask every place for pictures of a game. Fails only when none of them
/// could be reached.
pub fn find(wanted: &Wanted) -> Result<Found, String> {
    let term = wanted.term.trim();
    if term.is_empty() {
        return Err("Say which game to look for.".to_string());
    }
    let key = wanted.key.as_deref().filter(|k| key_shape(k));
    let (steam, wiki, grid) = std::thread::scope(|scope| {
        let steam = scope.spawn(|| steam(term));
        let wiki = scope.spawn(|| wikipedia(term));
        let grid = scope.spawn(|| key.map(|key| grid(term, key)));
        let gone = || Err("It did not answer.".to_string());
        (steam.join().unwrap_or_else(|_| gone()), wiki.join().unwrap_or_else(|_| gone()), grid.join().unwrap_or(None))
    });
    if steam.is_err() && wiki.is_err() && !matches!(grid, Some(Ok(_))) {
        return Err("No pictures could be fetched. Is the Ally online?".to_string());
    }
    let mut notes = Vec::new();
    if let Some(Err(e)) = &grid {
        notes.push(e.clone());
    }
    // Community covers first when there are any: they are made for this.
    let lists = vec![grid.and_then(Result::ok).unwrap_or_default(), box_art(wanted), wiki.unwrap_or_default(), steam.unwrap_or_default()];
    Ok(Found { candidates: arrange(lists), notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_console_named_by_hand_leads_to_its_box_art() {
        assert_eq!(systems("GameCube and Wii"), ["Nintendo - GameCube", "Nintendo - Wii"]);
        assert_eq!(systems("Nintendo 64"), ["Nintendo - Nintendo 64"]);
        assert_eq!(systems("Wii U"), ["Nintendo - Wii U"]);
        assert_eq!(systems("Game Boy Advance"), ["Nintendo - Game Boy Advance"]);
        assert_eq!(systems("PlayStation 2"), ["Sony - PlayStation 2"]);
        assert_eq!(systems("PlayStation"), ["Sony - PlayStation"]);
        assert_eq!(systems("Nintendo 3DS"), ["Nintendo - Nintendo 3DS"]);
        assert_eq!(systems("Sega Genesis"), ["Sega - Mega Drive - Genesis"], "no NES in Genesis");
        assert_eq!(systems("NES"), ["Nintendo - Nintendo Entertainment System"]);
        assert!(systems("Switch").is_empty() && systems("").is_empty());
    }

    #[test]
    fn box_art_is_looked_for_under_the_names_the_collection_uses() {
        assert_eq!(catalogued("The Legend of Zelda: Ocarina of Time"), "Legend of Zelda, The - Ocarina of Time");
        assert_eq!(catalogued("Super Mario 64"), "Super Mario 64");
        let wanted = Wanted { term: "The Legend of Zelda: Ocarina of Time".into(), console: "Nintendo 64".into(), ..Wanted::default() };
        let guesses = box_art(&wanted);
        assert_eq!(guesses.len(), 5);
        assert_eq!(guesses[0].full, "https://thumbnails.libretro.com/Nintendo%20-%20Nintendo%2064/Named_Boxarts/Legend%20of%20Zelda%2C%20The%20-%20Ocarina%20of%20Time%20%28USA%29.png");
        assert!(guesses.iter().all(Candidate::trusted));
        // An emulated game's own file name is the best guess of all.
        let wanted = Wanted { term: "Tin Knight".into(), console: "GameCube and Wii".into(), file: Some("Tin Knight & Co (Europe) (Rev 1)".into()), ..Wanted::default() };
        let guesses = box_art(&wanted);
        assert_eq!(guesses.len(), 12);
        assert!(guesses[0].full.ends_with("/Nintendo%20-%20GameCube/Named_Boxarts/Tin%20Knight%20_%20Co%20%28Europe%29%20%28Rev%201%29.png"));
        // A game that is from no console has no box art to guess at.
        assert!(box_art(&Wanted { term: "Celeste".into(), ..Wanted::default() }).is_empty());
    }

    #[test]
    fn community_covers_are_read_and_the_unsuitable_left_out() {
        let games = br#"{"success":true,"data":[{"id":4257,"name":"The Legend of Zelda: Ocarina of Time","types":["steam"],"verified":true},{"id":9,"name":"Ocarina of Time 3D"},{"name":"no id"}]}"#;
        assert_eq!(parse_grid_games(games), [(4257, "The Legend of Zelda: Ocarina of Time".to_string()), (9, "Ocarina of Time 3D".to_string())]);
        assert!(parse_grid_games(br#"{"success":false,"errors":["Unauthorized"]}"#).is_empty());
        let pictures = br#"{"success":true,"data":[
            {"id":1,"width":600,"height":900,"nsfw":false,"humor":false,"url":"https://cdn2.steamgriddb.com/grid/a.png","thumb":"https://cdn2.steamgriddb.com/thumb/a.jpg"},
            {"id":2,"width":1920,"height":620,"url":"https://cdn2.steamgriddb.com/hero/b.jpg","thumb":"https://cdn2.steamgriddb.com/hero_thumb/b.jpg"},
            {"id":3,"width":600,"height":900,"nsfw":true,"url":"https://cdn2.steamgriddb.com/grid/c.png","thumb":"https://cdn2.steamgriddb.com/thumb/c.jpg"},
            {"id":4,"width":600,"height":900,"url":"https://cdn2.steamgriddb.com/grid/d.webm","thumb":"https://cdn2.steamgriddb.com/thumb/d.jpg"},
            {"id":5,"width":600,"height":900,"url":"https://elsewhere.example/e.png","thumb":"https://cdn2.steamgriddb.com/thumb/e.jpg"},
            {"id":6,"width":600,"height":900,"humor":true,"url":"https://cdn2.steamgriddb.com/grid/f.png","thumb":"https://cdn2.steamgriddb.com/thumb/f.jpg"}]}"#;
        let found = parse_grid_pictures(pictures, "Ocarina");
        assert_eq!(found.iter().map(|c| (c.full.as_str(), c.shape)).collect::<Vec<_>>(), [("https://cdn2.steamgriddb.com/grid/a.png", Shape::Tall), ("https://cdn2.steamgriddb.com/hero/b.jpg", Shape::Wide)]);
        assert_eq!(found[0].thumb, "https://cdn2.steamgriddb.com/thumb/a.jpg");
        assert!(key_shape("0123456789abcdef0123456789abcdef"));
        assert!(!key_shape("short") && !key_shape("has spaces in it and is long enough") && !key_shape(""));
    }

    #[test]
    fn what_was_found_is_shown_covers_first_with_the_places_taking_turns() {
        let c = |source: &str, n: u32, shape| candidate(source, "x", shape, format!("https://upload.wikimedia.org/{source}{n}.jpg"), format!("https://upload.wikimedia.org/{source}{n}.jpg"));
        let lists = vec![
            vec![c("a", 1, Shape::Tall), c("a", 2, Shape::Wide), c("a", 3, Shape::Tall)],
            vec![],
            vec![c("b", 1, Shape::Wide), c("b", 2, Shape::Tall), c("a", 1, Shape::Tall)],
            vec![candidate("evil", "x", Shape::Tall, "https://evil.example/t.jpg".into(), "https://evil.example/f.jpg".into())],
        ];
        let order: Vec<String> = arrange(lists).iter().map(|c| c.full.rsplit('/').next().unwrap().to_string()).collect();
        assert_eq!(order, ["a1.jpg", "b2.jpg", "a3.jpg", "a2.jpg", "b1.jpg"]);
        assert_eq!(steam_pictures("504230", "Celeste").iter().map(|c| c.shape).collect::<Vec<_>>(), [Shape::Tall, Shape::Wide, Shape::Wide]);
        assert_eq!((Shape::of(600, 900), Shape::of(460, 215), Shape::of(512, 512), Shape::of(256, 176)), (Shape::Tall, Shape::Wide, Shape::Tall, Shape::Wide));
        assert!(find(&Wanted::default()).is_err(), "nothing to look for");
    }
}
