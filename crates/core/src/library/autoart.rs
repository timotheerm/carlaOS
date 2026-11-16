//! Giving every game a picture without anyone having to ask.
//!
//! A game with no cover, or no wide picture to put behind it, is looked up by
//! itself: a Steam game by its number, anything else by its name. A picture
//! is only taken when the place that has it calls the game by the same name,
//! so a game never gets another game's face. What the administrator chose by
//! hand always wins, and a game nothing was found for is left alone for a
//! week before it is looked up again.
//!
//! The only thing sent anywhere is the game's name or its Steam number, to
//! the same places the picture chooser asks.

use serde_json::Value;

use super::art::{self, Candidate, Found, Shape, Wanted};
use super::meta::{self, get, trusted_picture, PICTURES};

/// A game that was looked up and had nothing is left alone this long.
pub const TRY_AGAIN_AFTER: i64 = 7 * 24 * 3600;
/// So many addresses are tried for one picture before giving up.
const MOST_TRIES: usize = 6;

/// A game that wants pictures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subject {
    /// The catalogue id: "steam:504230", "manual:…", "emu:…".
    pub id: String,
    pub title: String,
    /// The console it is from, for box art: an emulated game's console, or
    /// the one a port first came out on.
    pub console: String,
    /// An emulated game's file name without its ending.
    pub file: Option<String>,
    pub need_cover: bool,
    pub need_hero: bool,
}

/// Pictures to try, likeliest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub covers: Vec<Candidate>,
    pub heroes: Vec<Candidate>,
}

/// The Steam number in "steam:504230".
pub fn steam_app(id: &str) -> Option<&str> {
    id.strip_prefix("steam:").filter(|n| !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A name as its words, without what differs between two spellings of the
/// same game: capitals, punctuation, "The", an edition, a note in brackets.
fn words(name: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '&' => plain.push_str(" and "),
            '\'' | '\u{2019}' | '\u{ae}' | '\u{2122}' => {}
            c if c.is_alphanumeric() => plain.extend(c.to_lowercase()),
            _ => plain.push(' '),
        }
    }
    let skip = ["the", "a", "an", "edition", "definitive", "remastered", "remaster", "goty", "complete", "deluxe", "hd", "enhanced", "anniversary", "directors", "cut", "ultimate", "gold", "collection"];
    let numeral = |w: &str| match w {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        _ => "",
    };
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ").replace("game of the year", "");
    let all: Vec<String> = plain.split_whitespace().map(|w| if numeral(w).is_empty() { w.to_string() } else { numeral(w).to_string() }).collect();
    let kept: Vec<String> = all.iter().filter(|w| !skip.contains(&w.as_str())).cloned().collect();
    // "The Game of Life" must not come out empty.
    if kept.is_empty() { all } else { kept }
}

/// Do two names mean the same game? "Celeste (video game)" is "Celeste";
/// "Ocarina of Time" is "The Legend of Zelda: Ocarina of Time"; "Portal" is
/// not "Portal 2".
pub fn same_game(a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    // A subtitle alone names the game when it is long enough to be one, and
    // what is left over is not a number, which would be another game.
    if short.len() < 3 {
        return false;
    }
    let rest_is_words = |rest: &[String]| rest.iter().all(|w| !w.chars().any(|c| c.is_ascii_digit()));
    (long.ends_with(short) && rest_is_words(&long[..long.len() - short.len()])) || (long.starts_with(short) && rest_is_words(&long[short.len()..]))
}

fn steam_candidate(app: &str, file: &str, shape: Shape, label: &str) -> Candidate {
    let url = format!("{PICTURES}/{app}/{file}");
    Candidate { source: "Steam".to_string(), label: label.to_string(), shape, thumb: url.clone(), full: url }
}

/// The pictures Steam keeps for a game under its number, the ones made for a
/// library first.
pub fn steam_plan(app: &str, title: &str) -> Plan {
    Plan {
        covers: vec![steam_candidate(app, "library_600x900_2x.jpg", Shape::Tall, title), steam_candidate(app, "library_600x900.jpg", Shape::Tall, title)],
        heroes: vec![steam_candidate(app, "library_hero.jpg", Shape::Wide, title), steam_candidate(app, "header.jpg", Shape::Wide, title)],
    }
}

/// What Steam's shop says a game's pictures are: (the wide one with its
/// name on it, the one behind its shop page). Newer games keep their
/// pictures under addresses that cannot be guessed, and this is where the
/// addresses are written.
pub fn parse_shop_pictures(body: &[u8], app: &str) -> (Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else { return (None, None) };
    let data = &value[app]["data"];
    let picture = |name: &str| data[name].as_str().filter(|u| trusted_picture(u)).map(str::to_string);
    (picture("header_image"), picture("background_raw").or_else(|| picture("background")))
}

fn shop_pictures(app: &str, title: &str) -> Vec<Candidate> {
    let Ok((200, body)) = get(&format!("https://store.steampowered.com/api/appdetails?appids={app}&filters=basic,background&l=en"), 2 * 1024 * 1024) else { return Vec::new() };
    let (header, background) = parse_shop_pictures(&body, app);
    [background, header].into_iter().flatten().map(|url| Candidate { source: "Steam".to_string(), label: title.to_string(), shape: Shape::Wide, thumb: url.clone(), full: url }).collect()
}

/// Out of everything a search found, the pictures that are of this game.
pub fn plan(subject: &Subject, found: &Found) -> Plan {
    let mut plan = Plan::default();
    for c in &found.candidates {
        // Box art is looked for under the game's own name, so it is the
        // game's when it exists at all.
        if !c.trusted() || !(c.source == "Box art" || same_game(&subject.title, &c.label)) {
            continue;
        }
        match c.shape {
            Shape::Tall => plan.covers.push(c.clone()),
            // A wide picture on Wikipedia is a logo or a screenshot, too
            // small to fill the screen.
            Shape::Wide if c.source != "Wikipedia" => plan.heroes.push(c.clone()),
            Shape::Wide => {}
        }
    }
    // Steam's own wide picture without the name on it comes before the one with.
    plan.heroes.sort_by_key(|c| !c.full.ends_with("library_hero.jpg"));
    plan
}

/// Ask the places that have pictures. Fails only when none could be reached.
pub fn search(subject: &Subject, key: Option<&str>) -> Result<Plan, String> {
    if let Some(app) = steam_app(&subject.id) {
        let mut plan = steam_plan(app, &subject.title);
        if let Some(key) = key.filter(|k| art::key_shape(k)) {
            let (covers, heroes) = art::grid_for_steam(app, key, &subject.title);
            plan.covers.extend(covers);
            plan.heroes.extend(heroes);
        }
        // Asked last and only when needed: see `fill`.
        return Ok(plan);
    }
    let wanted = Wanted { term: subject.title.clone(), console: subject.console.clone(), file: subject.file.clone(), key: key.map(str::to_string) };
    Ok(plan(subject, &art::find(&wanted)?))
}

/// The first picture of a list that really loads.
pub fn first_loading(list: &[Candidate], load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<(Candidate, Vec<u8>)> {
    list.iter().take(MOST_TRIES).find_map(|c| load(&c.full).map(|bytes| (c.clone(), bytes)))
}

/// What was found for one game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filled {
    pub cover: Option<Vec<u8>>,
    pub hero: Option<Vec<u8>>,
}

/// Find and fetch the pictures a game is missing.
pub fn fill(subject: &Subject, key: Option<&str>) -> Result<Filled, String> {
    let plan = search(subject, key)?;
    let load = |url: &str| meta::picture(url).ok();
    let mut filled = Filled::default();
    if subject.need_cover {
        filled.cover = first_loading(&plan.covers, &load).map(|(_, bytes)| bytes);
    }
    if subject.need_hero {
        filled.hero = first_loading(&plan.heroes, &load).map(|(_, bytes)| bytes);
    }
    // A Steam game whose pictures are not where they used to be kept.
    if let Some(app) = steam_app(&subject.id) {
        if (subject.need_hero && filled.hero.is_none()) || (subject.need_cover && filled.cover.is_none()) {
            let shop = shop_pictures(app, &subject.title);
            if subject.need_hero && filled.hero.is_none() {
                filled.hero = first_loading(&shop, &load).map(|(_, bytes)| bytes);
            }
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_spellings_of_one_game_are_told_from_two_games() {
        for (a, b) in [
            ("Celeste", "Celeste (video game)"),
            ("The Legend of Zelda: Ocarina of Time", "Legend of Zelda - Ocarina of Time"),
            ("Ocarina of Time", "The Legend of Zelda: Ocarina of Time"),
            ("Sonic 3 & Knuckles", "Sonic 3 and Knuckles"),
            ("Final Fantasy VII", "FINAL FANTASY 7"),
            ("Dark Souls: Remastered", "DARK SOULS\u{2122}"),
            ("Link's Awakening DX", "The Legend of Zelda: Link\u{2019}s Awakening DX"),
            ("The Witcher 3: Wild Hunt", "The Witcher 3: Wild Hunt - Game of the Year Edition"),
        ] {
            assert!(same_game(a, b), "{a} is {b}");
        }
        for (a, b) in [
            ("Portal", "Portal 2"),
            ("Portal 2", "Portal"),
            ("Super Mario 64", "Super Mario 64 DS 2"),
            ("Doom", "Doom Eternal"),
            ("Jak and Daxter", "Daxter"),
            ("Quake", "Quake II"),
            ("", "Anything"),
            ("Tomb Raider", "Rise of the Tomb Raider"),
        ] {
            assert!(!same_game(a, b), "{a} is not {b}");
        }
        assert_eq!((steam_app("steam:504230"), steam_app("steam:"), steam_app("steam:12x"), steam_app("manual:1")), (Some("504230"), None, None, None));
    }

    fn candidate(source: &str, label: &str, shape: Shape, file: &str) -> Candidate {
        let url = format!("https://shared.akamai.steamstatic.com/{file}");
        Candidate { source: source.to_string(), label: label.to_string(), shape, thumb: url.clone(), full: url }
    }

    #[test]
    fn only_pictures_of_the_same_game_are_taken_and_the_first_that_loads_wins() {
        let subject = Subject { id: "manual:1".into(), title: "The Legend of Zelda: Ocarina of Time".into(), console: "Nintendo 64".into(), need_cover: true, need_hero: true, ..Subject::default() };
        let found = Found {
            candidates: vec![
                candidate("SteamGridDB", "The Legend of Zelda: Ocarina of Time", Shape::Tall, "grid.png"),
                candidate("Box art", "Legend of Zelda, The - Ocarina of Time (USA)", Shape::Tall, "box.png"),
                candidate("Wikipedia", "The Legend of Zelda: Majora's Mask", Shape::Tall, "other.png"),
                candidate("Wikipedia", "The Legend of Zelda: Ocarina of Time", Shape::Wide, "logo.png"),
                candidate("Steam", "Ocarina of Time", Shape::Wide, "1/header.jpg"),
                candidate("Steam", "Ocarina of Time", Shape::Wide, "1/library_hero.jpg"),
                Candidate { full: "https://evil.example/x.png".into(), ..candidate("Steam", "Ocarina of Time", Shape::Tall, "x.png") },
            ],
            notes: Vec::new(),
        };
        let plan = plan(&subject, &found);
        let names = |list: &[Candidate]| list.iter().map(|c| c.full.rsplit('/').next().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(&plan.covers), ["grid.png", "box.png"]);
        assert_eq!(names(&plan.heroes), ["library_hero.jpg", "header.jpg"]);
        // The first address has nothing behind it; the second does.
        let load = |url: &str| url.ends_with("box.png").then(|| vec![1u8]);
        assert_eq!(first_loading(&plan.covers, &load).map(|(c, _)| c.source), Some("Box art".to_string()));
        assert_eq!(first_loading(&plan.heroes, &load), None);

        let steam = steam_plan("504230", "Celeste");
        assert!(steam.covers[1].full.ends_with("/504230/library_600x900.jpg") && steam.heroes[0].full.ends_with("/504230/library_hero.jpg"));
        assert!(steam.covers.iter().chain(&steam.heroes).all(Candidate::trusted));
    }

    #[test]
    fn the_shop_says_where_a_newer_games_pictures_are() {
        let body = br#"{"99":{"success":true,"data":{"name":"New Game","header_image":"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/99/abc/header.jpg?t=1","background":"https://store.akamai.steamstatic.com/images/storepagebackground/app/99?t=1","background_raw":"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/99/abc/page_bg_raw.jpg?t=1"}}}"#;
        let (header, background) = parse_shop_pictures(body, "99");
        assert!(header.unwrap().contains("/abc/header.jpg") && background.unwrap().contains("page_bg_raw.jpg"));
        assert_eq!(parse_shop_pictures(br#"{"99":{"success":false}}"#, "99"), (None, None));
        assert_eq!(parse_shop_pictures(br#"{"99":{"data":{"header_image":"https://elsewhere.example/a.jpg"}}}"#, "99"), (None, None));
    }
}
