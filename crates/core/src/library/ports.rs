//! Recognising a program the administrator adds.
//!
//! A native port of a console game has a program name that says nothing to a
//! shop search: `soh.exe` is Ocarina of Time. A short list of well-known
//! ports is kept here, so such a game gets its real name, the console it
//! first came out on and the right pictures without the administrator having
//! to know any of that. Nothing is downloaded and nothing is started here:
//! it is only a list of names.
//!
//! Also here: following a shortcut to the program behind it, and a first
//! guess at a name for anything else.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::shortcut;
use super::Kind;

/// A native port CarlaOS knows by its program's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    /// The game it is a version of.
    pub title: &'static str,
    /// What the port calls itself.
    pub port: &'static str,
    /// The console the game first came out on.
    pub console: &'static str,
}

const fn port(title: &'static str, port: &'static str, console: &'static str) -> Port {
    Port { title, port, console }
}

/// (program or folder name with only letters and digits, lower case; the port).
/// A name has to match whole, unless it ends in `*`: then it may be the start
/// of the name, so "sm64ex-coop" is found under "sm64ex*".
const KNOWN: &[(&str, Port)] = &[
    ("soh", port("The Legend of Zelda: Ocarina of Time", "Ship of Harkinian", "Nintendo 64")),
    ("shipofharkinian*", port("The Legend of Zelda: Ocarina of Time", "Ship of Harkinian", "Nintendo 64")),
    ("hookshot*", port("The Legend of Zelda: Ocarina of Time", "Hookshot", "Nintendo 64")),
    ("2ship*", port("The Legend of Zelda: Majora's Mask", "2 Ship 2 Harkinian", "Nintendo 64")),
    ("2s2h", port("The Legend of Zelda: Majora's Mask", "2 Ship 2 Harkinian", "Nintendo 64")),
    ("zelda64recomp*", port("The Legend of Zelda: Majora's Mask", "Zelda 64: Recompiled", "Nintendo 64")),
    ("sm64ex*", port("Super Mario 64", "sm64ex", "Nintendo 64")),
    ("sm64coopdx*", port("Super Mario 64", "sm64coopdx", "Nintendo 64")),
    ("sm64us*", port("Super Mario 64", "Super Mario 64 PC port", "Nintendo 64")),
    ("sm64eu*", port("Super Mario 64", "Super Mario 64 PC port", "Nintendo 64")),
    ("sm64jp*", port("Super Mario 64", "Super Mario 64 PC port", "Nintendo 64")),
    ("sm64pc*", port("Super Mario 64", "Super Mario 64 PC port", "Nintendo 64")),
    ("sm64plus*", port("Super Mario 64", "Super Mario 64 Plus", "Nintendo 64")),
    ("sm64rt*", port("Super Mario 64", "sm64rt", "Nintendo 64")),
    ("render96*", port("Super Mario 64", "Render96", "Nintendo 64")),
    ("ghostship", port("Super Mario 64", "Ghostship", "Nintendo 64")),
    ("starship", port("Star Fox 64", "Starship", "Nintendo 64")),
    ("starfox64recomp*", port("Star Fox 64", "Star Fox 64: Recompiled", "Nintendo 64")),
    ("spaghettikart*", port("Mario Kart 64", "SpaghettiKart", "Nintendo 64")),
    ("spaghettify", port("Mario Kart 64", "SpaghettiKart", "Nintendo 64")),
    ("mariokart64recomp*", port("Mario Kart 64", "Mario Kart 64: Recompiled", "Nintendo 64")),
    ("banjorecomp*", port("Banjo-Kazooie", "Banjo: Recompiled", "Nintendo 64")),
    ("perfectdark", port("Perfect Dark", "Perfect Dark port", "Nintendo 64")),
    ("pdx8664", port("Perfect Dark", "Perfect Dark port", "Nintendo 64")),
    ("goemon64recomp*", port("Mystical Ninja Starring Goemon", "Goemon 64: Recompiled", "Nintendo 64")),
    ("drmario64recomp*", port("Dr. Mario 64", "Dr. Mario 64: Recompiled", "Nintendo 64")),
    ("zelda3", port("The Legend of Zelda: A Link to the Past", "Zelda 3", "Super Nintendo")),
    ("smw", port("Super Mario World", "smw", "Super Nintendo")),
    ("sonic3air*", port("Sonic 3 & Knuckles", "Sonic 3 A.I.R.", "Mega Drive")),
    ("linksawakeningdxhd*", port("The Legend of Zelda: Link's Awakening DX", "Link's Awakening DX HD", "Game Boy Color")),
    ("opengoal*", port("Jak and Daxter: The Precursor Legacy", "OpenGOAL", "PlayStation 2")),
    ("re3", port("Grand Theft Auto III", "re3", "PlayStation 2")),
    ("revc", port("Grand Theft Auto: Vice City", "reVC", "PlayStation 2")),
    ("unleashedrecomp*", port("Sonic Unleashed", "Unleashed Recompiled", "Xbox 360")),
    ("marathonrecomp*", port("Sonic the Hedgehog (2006)", "Marathon Recompiled", "Xbox 360")),
    ("tr1x", port("Tomb Raider", "TR1X", "PlayStation")),
    ("tomb1main", port("Tomb Raider", "Tomb1Main", "PlayStation")),
    ("tr2x", port("Tomb Raider II", "TR2X", "PlayStation")),
    ("openlara", port("Tomb Raider", "OpenLara", "PlayStation")),
    ("wipeoutrewrite", port("Wipeout", "wipEout Rewrite", "PlayStation")),
    ("cannonball", port("OutRun", "Cannonball", "Arcade")),
    ("openmw", port("The Elder Scrolls III: Morrowind", "OpenMW", "PC")),
    ("daggerfallunity", port("The Elder Scrolls II: Daggerfall", "Daggerfall Unity", "PC")),
    ("openrct2", port("RollerCoaster Tycoon 2", "OpenRCT2", "PC")),
    ("openloco", port("Chris Sawyer's Locomotion", "OpenLoco", "PC")),
    ("corsixth", port("Theme Hospital", "CorsixTH", "PC")),
    ("devilutionx", port("Diablo", "DevilutionX", "PC")),
    ("gzdoom", port("Doom", "GZDoom", "PC")),
    ("dsdadoom", port("Doom", "DSDA-Doom", "PC")),
    ("crispydoom", port("Doom", "Crispy Doom", "PC")),
    ("chocolatedoom", port("Doom", "Chocolate Doom", "PC")),
    ("eduke32", port("Duke Nukem 3D", "EDuke32", "PC")),
    ("ecwolf", port("Wolfenstein 3D", "ECWolf", "PC")),
    ("quakespasm*", port("Quake", "QuakeSpasm", "PC")),
    ("ironwail", port("Quake", "Ironwail", "PC")),
    ("vkquake", port("Quake", "vkQuake", "PC")),
    ("yquake2", port("Quake II", "Yamagi Quake II", "PC")),
    ("ioquake3*", port("Quake III Arena", "ioquake3", "PC")),
    ("dhewm3", port("Doom 3", "dhewm3", "PC")),
    ("rbdoom3bfg", port("Doom 3: BFG Edition", "RBDOOM-3-BFG", "PC")),
    ("openxcom*", port("X-COM: UFO Defense", "OpenXcom", "PC")),
    ("theforceengine", port("Star Wars: Dark Forces", "The Force Engine", "PC")),
    ("openjk*", port("Star Wars Jedi Knight: Jedi Academy", "OpenJK", "PC")),
    ("fheroes2", port("Heroes of Might and Magic II", "fheroes2", "PC")),
    ("vcmiclient", port("Heroes of Might and Magic III", "VCMI", "PC")),
    ("vcmilauncher", port("Heroes of Might and Magic III", "VCMI", "PC")),
    ("julius", port("Caesar III", "Julius", "PC")),
    ("augustus", port("Caesar III", "Augustus", "PC")),
    ("opentyrian*", port("Tyrian", "OpenTyrian", "PC")),
    ("fallout2ce", port("Fallout 2", "Fallout 2 Community Edition", "PC")),
    ("falloutce", port("Fallout", "Fallout Community Edition", "PC")),
    ("arxlibertatis", port("Arx Fatalis", "Arx Libertatis", "PC")),
    ("exult", port("Ultima VII: The Black Gate", "Exult", "PC")),
    ("sdlpop", port("Prince of Persia", "SDLPoP", "PC")),
    ("isle", port("LEGO Island", "isle-portable", "PC")),
    ("xash3d", port("Half-Life", "Xash3D", "PC")),
    ("opengothic", port("Gothic II", "OpenGothic", "PC")),
    ("openmohaa", port("Medal of Honor: Allied Assault", "OpenMoHAA", "PC")),
    ("nxengine*", port("Cave Story", "NXEngine", "PC")),
];

fn squash(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The last two names of a path, read the same way on any machine: the
/// program without its ending, and the folder it is in.
fn names(path: &Path) -> (String, String) {
    let text = path.to_string_lossy();
    let mut parts = text.rsplit(['/', '\\']).filter(|p| !p.is_empty());
    let file = parts.next().unwrap_or_default();
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    (stem.to_string(), parts.next().unwrap_or_default().to_string())
}

/// The port this program is, going by its name or its folder's.
pub fn recognise(exe: &Path) -> Option<Port> {
    let (stem, folder) = names(exe);
    [squash(&stem), squash(&folder)].iter().find_map(|name| {
        KNOWN.iter().find(|(key, _)| key.strip_suffix('*').map_or(name == key, |start| name.starts_with(start))).map(|(_, port)| *port)
    })
}

/// Folder and program names that say how a game was built, not what it is.
const GENERIC: &[&str] = &[
    "game", "launcher", "start", "play", "bin", "bin32", "bin64", "win32", "win64", "x64", "x86", "x8664", "binaries", "app", "main", "release", "build", "windows", "retail", "shipping", "dist", "client", "run",
    "engine", "system", "exe", "program", "files", "data",
];

fn generic(name: &str) -> bool {
    GENERIC.contains(&squash(name).as_str())
}

/// "TinKnight-Win64-Shipping" is "Tin Knight"; "paper_lanterns_v1.2.0_x64"
/// is "paper lanterns".
fn readable(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for token in name.replace(['_', '-'], " ").split_whitespace() {
        let plain = token.to_lowercase();
        let digits_after = |start: &str| plain.strip_prefix(start).is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        let version = digits_after("v") || (digits_after("") && plain.contains('.'));
        // What comes after the name: how it was built and which version it is.
        if !words.is_empty() && (version || ["win64", "win32", "x64", "x86", "shipping", "windows", "win", "64bit", "32bit", "release", "final", "portable", "setup"].contains(&plain.as_str())) {
            break;
        }
        for word in token.split('.').filter(|w| !w.is_empty()) {
        // "TinKnight" is two words; "OpenGOAL" and "sm64" stay as they are.
        let mut split = String::new();
        let letters: Vec<char> = word.chars().collect();
        for (i, c) in letters.iter().enumerate() {
            let lower_before = i > 0 && letters[i - 1].is_lowercase();
            let lower_after = letters.get(i + 1).is_some_and(|n| n.is_lowercase());
            if c.is_uppercase() && lower_before && lower_after {
                split.push(' ');
            }
            split.push(*c);
        }
        words.extend(split.split_whitespace().map(str::to_string));
        }
    }
    words.join(" ")
}

/// "C:\Games\Paper Lanterns\lanterns.exe" suggests "lanterns";
/// "C:\Games\Paper Lanterns\Binaries\Win64\game.exe" suggests "Paper Lanterns".
pub fn suggest_title(exe: &Path) -> String {
    let text = exe.to_string_lossy();
    let mut parts = text.rsplit(['/', '\\']).filter(|p| !p.is_empty());
    let file = parts.next().unwrap_or_default();
    let file = file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string();
    // The nearest folder that is named after something.
    let folder = parts.take(4).find(|p| !generic(p) && !p.ends_with(':')).unwrap_or_default().to_string();
    let read = readable(&file);
    let tidy = folder.replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    // The same name spelled twice: the one written as words reads better.
    if !tidy.is_empty() && squash(&read) == squash(&tidy) {
        return if read.split(' ').count() > tidy.split(' ').count() { read } else { tidy };
    }
    // The folder usually carries the nicer spelling of the same name.
    let folder_is_better = generic(&read) || generic(&file) || file.len() < 3 || squash(&file) == squash(&folder);
    if folder_is_better && !tidy.is_empty() {
        tidy
    } else {
        read
    }
}

/// What CarlaOS makes of something the administrator pointed at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspected {
    /// The program to start: the one behind a shortcut, when it was one.
    pub exe: PathBuf,
    /// The options a shortcut passes.
    pub args: String,
    pub title: String,
    pub kind: Kind,
    /// The console the game first came out on, for a port CarlaOS knows.
    pub origin: String,
    /// What the port calls itself, when CarlaOS knows it.
    pub port: String,
    /// It was a shortcut and has been followed.
    pub shortcut: bool,
}

/// Look at what the administrator chose. A shortcut is followed to its
/// program; a link to a web address or a store page is refused, because
/// CarlaOS could never tell when such a game has closed.
pub fn inspect(path: &Path) -> Result<Inspected, String> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("url")) {
        return Err("That is a link to an address, not a program. Choose the game's program, or a shortcut to it.".to_string());
    }
    let (exe, args, named) = if shortcut::is_shortcut(path) {
        let read = shortcut::read(path)?;
        if shortcut::is_shortcut(&read.target) {
            return Err("That shortcut leads to another shortcut. Choose the game's program itself.".to_string());
        }
        if !read.target.is_file() {
            return Err(format!("That shortcut leads to {}, which is not there any more.", read.target.display()));
        }
        // A shortcut is usually named after the game, which the program rarely is.
        (read.target, read.args, Some(names(path).0))
    } else {
        (path.to_path_buf(), String::new(), None)
    };
    // What the program calls itself, which is often the only place its real
    // name is written.
    let product = super::pe::product_name(&exe);
    let known = recognise(&exe)
        .or_else(|| named.as_deref().and_then(|n| recognise(Path::new(&format!("{n}.lnk")))))
        .or_else(|| product.as_deref().and_then(|n| recognise(Path::new(&format!("{n}.exe")))));
    Ok(Inspected {
        title: known.map(|p| p.title.to_string()).or(named.clone()).or(product).unwrap_or_else(|| suggest_title(&exe)),
        kind: if known.is_some() { Kind::Port } else { Kind::Game },
        origin: known.map(|p| p.console.to_string()).unwrap_or_default(),
        port: known.map(|p| p.port.to_string()).unwrap_or_default(),
        shortcut: named.is_some(),
        exe,
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_ports_are_recognised_by_program_or_folder() {
        let oot = recognise(Path::new(r"C:\Games\Ship\soh.exe")).unwrap();
        assert_eq!((oot.title, oot.console), ("The Legend of Zelda: Ocarina of Time", "Nintendo 64"));
        assert_eq!(recognise(Path::new(r"D:\Ports\Hookshot\game.exe")).unwrap().port, "Hookshot");
        assert_eq!(recognise(Path::new("/games/2ship2harkinian/2ship.exe")).unwrap().title, "The Legend of Zelda: Majora's Mask");
        assert_eq!(recognise(Path::new(r"C:\sm64ex-coop\sm64.us.f3dex2e.exe")).unwrap().title, "Super Mario 64");
        assert_eq!(recognise(Path::new(r"C:\Games\Sonic 3 AIR\Sonic3AIR.exe")).unwrap().console, "Mega Drive");
        // A short name has to match whole: these are other programs.
        assert_eq!(recognise(Path::new(r"C:\Games\Soho Nights\sohonights.exe")), None);
        assert_eq!(recognise(Path::new(r"C:\Games\Smwich\smwich.exe")), None);
        assert_eq!(recognise(Path::new(r"C:\Games\Starship Rangers\StarshipRangers.exe")), None);
        assert_eq!(recognise(Path::new(r"C:\Games\Paper Lanterns\lanterns.exe")), None);
    }

    #[test]
    fn a_name_is_suggested_from_the_program_or_its_folder() {
        assert_eq!(suggest_title(Path::new(r"C:\Games\Paper Lanterns\lanterns.exe")), "lanterns");
        assert_eq!(suggest_title(Path::new(r"C:\Games\Paper Lanterns\game.exe")), "Paper Lanterns");
        assert_eq!(suggest_title(Path::new(r"C:\Games\Tin_Knight\TinKnight.exe")), "Tin Knight");
        assert_eq!(suggest_title(Path::new(r"C:\go.exe")), "go");
        assert_eq!(suggest_title(Path::new("/games/moss-and-ember/Launcher")), "moss and ember");
        // How a game was built is not its name.
        assert_eq!(suggest_title(Path::new(r"D:\Games\Paper Lanterns\Binaries\Win64\game.exe")), "Paper Lanterns");
        assert_eq!(suggest_title(Path::new(r"D:\Stuff\TinKnight-Win64-Shipping.exe")), "Tin Knight");
        assert_eq!(suggest_title(Path::new(r"D:\Stuff\paper_lanterns_v1.2.0_x64.exe")), "paper lanterns");
        assert_eq!(suggest_title(Path::new(r"D:\Stuff\Lowpoly Rally 2.exe")), "Lowpoly Rally 2");
        assert_eq!(suggest_title(Path::new(r"D:\Stuff\OpenGOAL.exe")), "OpenGOAL");
    }

    #[test]
    fn more_ports_and_rebuilt_classics_are_known() {
        let seen = |path: &str| recognise(Path::new(path)).map(|p| (p.title, p.console));
        assert_eq!(seen(r"C:\Ports\sm64\sm64.us.f3dex2e.exe"), Some(("Super Mario 64", "Nintendo 64")));
        assert_eq!(seen(r"C:\Ports\Perfect Dark\pd.x86_64.exe"), Some(("Perfect Dark", "Nintendo 64")));
        assert_eq!(seen(r"C:\Ports\UnleashedRecomp\UnleashedRecomp.exe"), Some(("Sonic Unleashed", "Xbox 360")));
        assert_eq!(seen(r"C:\Games\OpenMW 0.49\openmw.exe"), Some(("The Elder Scrolls III: Morrowind", "PC")));
        assert_eq!(seen(r"C:\Games\re3\re3.exe"), Some(("Grand Theft Auto III", "PlayStation 2")));
        assert_eq!(seen(r"C:\Games\Fallout\fallout-ce.exe"), Some(("Fallout", "PC")));
        assert_eq!(seen(r"C:\Games\Isles of Mist\islesofmist.exe"), None);
    }

    #[test]
    fn a_program_that_names_itself_is_believed() {
        let dir = std::env::temp_dir().join(format!("carla-named-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let exe = dir.join("bin").join("game.exe");
        std::fs::write(&exe, crate::library::pe::sample(&[("ProductName", "Paper Lanterns")])).unwrap();
        let seen = inspect(&exe).unwrap();
        assert_eq!((seen.title.as_str(), seen.kind), ("Paper Lanterns", Kind::Game));
        // A port that only its own details give away.
        let exe = dir.join("bin").join("play.exe");
        std::fs::write(&exe, crate::library::pe::sample(&[("ProductName", "Ship of Harkinian")])).unwrap();
        let seen = inspect(&exe).unwrap();
        assert_eq!((seen.title.as_str(), seen.kind, seen.origin.as_str()), ("The Legend of Zelda: Ocarina of Time", Kind::Port, "Nintendo 64"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shortcut_is_followed_to_its_program() {
        let dir = std::env::temp_dir().join(format!("carla-inspect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = dir.join("Hookshot");
        std::fs::create_dir_all(&game).unwrap();
        let exe = game.join("hookshot.exe");
        std::fs::write(&exe, b"program").unwrap();
        let link = dir.join("Zelda.lnk");
        std::fs::write(&link, shortcut::sample(&exe.to_string_lossy(), "--fullscreen", "", true)).unwrap();

        let seen = inspect(&link).unwrap();
        assert_eq!((seen.exe.as_path(), seen.args.as_str(), seen.shortcut), (exe.as_path(), "--fullscreen", true));
        assert_eq!((seen.title.as_str(), seen.kind, seen.origin.as_str(), seen.port.as_str()), ("The Legend of Zelda: Ocarina of Time", Kind::Port, "Nintendo 64", "Hookshot"));

        // A game CarlaOS does not know takes the shortcut's name.
        let other = dir.join("lanterns.exe");
        std::fs::write(&other, b"program").unwrap();
        let link = dir.join("Paper Lanterns.lnk");
        std::fs::write(&link, shortcut::sample(&other.to_string_lossy(), "", "", true)).unwrap();
        let seen = inspect(&link).unwrap();
        assert_eq!((seen.title.as_str(), seen.kind, seen.origin.as_str()), ("Paper Lanterns", Kind::Game, ""));

        // The program itself, chosen directly.
        let seen = inspect(&exe).unwrap();
        assert_eq!((seen.exe.as_path(), seen.shortcut, seen.kind), (exe.as_path(), false, Kind::Port));

        // What cannot be started is said plainly.
        std::fs::remove_file(&other).unwrap();
        assert!(inspect(&link).unwrap_err().contains("not there any more"));
        assert!(inspect(&dir.join("Store page.url")).unwrap_err().contains("address"));
        std::fs::write(dir.join("not.lnk"), b"hello").unwrap();
        assert!(inspect(&dir.join("not.lnk")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
