//! `carla-companion describe KEY "TEXT"` and `describe KEY --lookup [--apply]`:
//! what the Ally's game page says about a game added by hand.

use carla_core::library::meta::{self, Details, Match};

use crate::files::Files;

pub const USAGE: &str = "Say the game and what it is: carla-companion describe KEY \"TEXT\" (empty text clears it)\n  or: carla-companion describe KEY --lookup [--apply]   (KEY is in carla-companion games)";

/// What `carla-companion describe` was asked.
#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    Set { key: String, text: String },
    Lookup { key: String, apply: bool },
}

pub fn parse(args: &[String]) -> Result<Ask, String> {
    let key = args.get(1).filter(|k| !k.starts_with("--")).ok_or(USAGE)?.clone();
    match &args[2..] {
        [text] if text != "--lookup" && text != "--apply" => Ok(Ask::Set { key, text: text.clone() }),
        [flag] if flag == "--lookup" => Ok(Ask::Lookup { key, apply: false }),
        [a, b] if (a == "--lookup" && b == "--apply") || (a == "--apply" && b == "--lookup") => Ok(Ask::Lookup { key, apply: true }),
        _ => Err(USAGE.to_string()),
    }
}

/// The search result to use for a title: a name that is the title itself
/// (Steam's before Wikipedia's, as they come), otherwise the first one.
pub fn pick<'a>(found: &'a [Match], title: &str) -> Option<&'a Match> {
    let same = |m: &&Match| m.name.trim().eq_ignore_ascii_case(title.trim());
    found.iter().find(same).or_else(|| found.first())
}

/// A name as compared: lower case, letters and digits only, without "the".
fn plain(name: &str) -> String {
    let spaced: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { ' ' }).collect();
    spaced.split_whitespace().filter(|w| *w != "the").collect::<Vec<_>>().join(" ")
}

/// Whether a found name is the game's title, give or take case, punctuation,
/// "the" and spacing.
pub fn same_game(found: &str, title: &str) -> bool {
    let (a, b) = (plain(found), plain(title));
    !a.is_empty() && a == b
}

/// Look a title up with the given search and details functions (the real
/// ones are `meta::search` and `meta::details`), so tests need no network.
/// The flag says whether the result's name is the title; if not, it is a guess.
pub fn lookup_with(
    title: &str,
    search: impl Fn(&str) -> Result<Vec<Match>, String>,
    details: impl Fn(&str) -> Result<Details, String>,
) -> Result<(Details, bool), String> {
    let found = search(title)?;
    let best = pick(&found, title).ok_or_else(|| format!("Neither Steam nor Wikipedia knows a game called {title:?}."))?;
    let mut got = details(&best.id)?;
    got.description = carla_core::library::clean_description(&got.description);
    if got.description.is_empty() {
        return Err(format!("{} was found on {}, but it has no description.", got.name, best.from));
    }
    let sure = found.iter().any(|m| same_game(&m.name, title)) && same_game(&got.name, title);
    Ok((got, sure))
}

pub fn run(files: &Files, args: &[String]) -> Result<i32, String> {
    match parse(args)? {
        Ask::Set { key, text } => {
            let (title, kept) = files.describe_extra(&key, &text)?;
            if kept.is_empty() {
                println!("{title} has no description now.");
            } else {
                println!("{title} is described as: {kept}\nThe Ally picks it up within five minutes, or at once after \"Look for games again\" there.");
            }
            Ok(0)
        }
        Ask::Lookup { key, apply } => {
            let extra = files.extras().into_iter().find(|e| e.key == key).ok_or("No game added by hand has that key (see carla-companion games).")?;
            println!("Asking Steam and Wikipedia about {:?}. Only the game's title is sent.", extra.title);
            let (got, sure) = lookup_with(&extra.title, meta::search, meta::details)
                .map_err(|e| format!("Nothing was looked up: {e}\nNothing was changed. You can write one yourself: carla-companion describe {key} \"TEXT\""))?;
            println!("Found: {}\n{}", got.name, got.description);
            if apply && !sure {
                return Err(format!(
                    "Nothing was changed: {:?} is the closest match, not a game with the same name as {:?}, so it could describe the wrong game.\nIf it is right, write it yourself: carla-companion describe {key} \"TEXT\"",
                    got.name, extra.title
                ));
            }
            if apply {
                files.describe_extra(&key, &got.description)?;
                println!("\nThat is now {}'s description.", extra.title);
            } else {
                println!("\nNothing was changed. To use it: carla-companion describe {key} --lookup --apply");
            }
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|s| s.to_string()).collect()
    }

    fn found(id: &str, name: &str, from: &str) -> Match {
        Match { id: id.into(), name: name.into(), from: from.into() }
    }

    #[test]
    fn describe_reads_text_or_a_lookup() {
        assert_eq!(parse(&words(&["describe", "k1", "A berry."])).unwrap(), Ask::Set { key: "k1".into(), text: "A berry.".into() });
        assert_eq!(parse(&words(&["describe", "k1", ""])).unwrap(), Ask::Set { key: "k1".into(), text: String::new() });
        assert_eq!(parse(&words(&["describe", "k1", "--lookup"])).unwrap(), Ask::Lookup { key: "k1".into(), apply: false });
        assert_eq!(parse(&words(&["describe", "k1", "--lookup", "--apply"])).unwrap(), Ask::Lookup { key: "k1".into(), apply: true });
        assert_eq!(parse(&words(&["describe", "k1", "--apply", "--lookup"])).unwrap(), Ask::Lookup { key: "k1".into(), apply: true });
        for bad in [&["describe"][..], &["describe", "k1"], &["describe", "k1", "--apply"], &["describe", "k1", "a", "b"], &["describe", "--lookup"], &["describe", "k1", "--lookup", "x"]] {
            assert!(parse(&words(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_lookup_prefers_the_exact_name_and_cleans_what_it_finds() {
        let list = vec![found("steam:1", "Celeste Bundle", "Steam"), found("steam:2", "celeste", "Steam"), found("wiki:3", "Celeste", "Wikipedia")];
        assert_eq!(pick(&list, "Celeste").unwrap().id, "steam:2");
        assert_eq!(pick(&list, "Something else").unwrap().id, "steam:1");
        assert!(pick(&[], "Celeste").is_none());

        let ok = |_: &str| Ok(vec![found("steam:2", "Celeste", "Steam")]);
        let asked = std::cell::RefCell::new(Vec::new());
        let (got, sure) = lookup_with("Celeste", ok, |id| {
            asked.borrow_mut().push(id.to_string());
            Ok(Details { name: "Celeste".into(), description: "Help\nMadeline\u{7} climb.  ".into(), header: None })
        })
        .unwrap();
        assert_eq!((got.name.as_str(), got.description.as_str(), sure), ("Celeste", "Help Madeline climb.", true));
        assert_eq!(*asked.borrow(), ["steam:2"]);

        let long = lookup_with("X", |_| Ok(vec![found("wiki:1", "X", "Wikipedia")]), |_| Ok(Details { name: "X".into(), description: "a".repeat(5000), header: None })).unwrap().0;
        assert_eq!(long.description.chars().count(), 1000);
    }

    #[test]
    fn a_lookup_with_no_matching_name_is_only_a_guess() {
        assert!(same_game("The Witness", "witness"));
        assert!(same_game("  Half-Life  2 ", "half life 2"));
        assert!(!same_game("Celeste Bundle", "Celeste"));
        assert!(!same_game("!!!", "???"));
        let guess = lookup_with("Celeste", |_| Ok(vec![found("steam:1", "Celeste Bundle", "Steam")]), |_| Ok(Details { name: "Celeste Bundle".into(), description: "Two games.".into(), header: None })).unwrap();
        assert_eq!((guess.0.name.as_str(), guess.1), ("Celeste Bundle", false));
        // A search hit that matches, whose details carry another name, is not trusted either.
        let odd = lookup_with("Celeste", |_| Ok(vec![found("steam:1", "Celeste", "Steam")]), |_| Ok(Details { name: "Other".into(), description: "x".into(), header: None })).unwrap();
        assert!(!odd.1);
    }

    #[test]
    fn a_lookup_that_fails_says_so() {
        let offline = lookup_with("X", |_| Err("The details could not be fetched. Is the Ally online?".into()), |_| unreachable!());
        assert!(offline.unwrap_err().contains("online"));
        assert!(lookup_with("X", |_| Ok(Vec::new()), |_| unreachable!()).unwrap_err().contains("knows a game"));
        let empty = lookup_with("X", |_| Ok(vec![found("steam:1", "X", "Steam")]), |_| Ok(Details { name: "X".into(), description: " ".into(), header: None }));
        assert!(empty.unwrap_err().contains("no description"));
    }
}
