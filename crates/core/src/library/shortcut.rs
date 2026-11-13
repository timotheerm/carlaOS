//! Reading a Windows shortcut (`.lnk`).
//!
//! Windows cannot start a shortcut the way it starts a program: only the
//! desktop knows how to follow one. So when the administrator points CarlaOS
//! at a shortcut, it is read here and the program behind it is what goes into
//! the library, with the options and the folder the shortcut starts it in.
//!
//! The file layout is Microsoft's published "Shell Link" format. Only the
//! parts that say what to start are read.

use std::path::{Path, PathBuf};

/// What a shortcut starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub target: PathBuf,
    /// The options it passes, as one line.
    pub args: String,
    /// "Start in", when the shortcut names one.
    pub dir: Option<PathBuf>,
}

const HEADER: usize = 0x4C;
const HAS_ID_LIST: u32 = 0x01;
const HAS_LINK_INFO: u32 = 0x02;
const HAS_NAME: u32 = 0x04;
const HAS_RELATIVE_PATH: u32 = 0x08;
const HAS_WORKING_DIR: u32 = 0x10;
const HAS_ARGUMENTS: u32 = 0x20;
const HAS_ICON: u32 = 0x40;
const IS_UNICODE: u32 = 0x80;
/// The block that carries a target written with `%variables%`.
const ENVIRONMENT_BLOCK: u32 = 0xA000_0001;

/// Is this path a shortcut, going by its ending?
pub fn is_shortcut(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let part = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(part)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }


    /// A counted string from the string section.
    fn string(&mut self, unicode: bool) -> Option<String> {
        let count = usize::from(self.u16()?);
        if unicode {
            Some(wide(self.take(count * 2)?))
        } else {
            Some(narrow(self.take(count)?))
        }
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Two-byte letters, up to the first zero.
fn wide(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    String::from_utf16_lossy(&units)
}

/// One-byte letters, up to the first zero. Read as Latin-1: right for plain
/// names, and the two-byte spelling is preferred wherever a shortcut has both.
fn narrow(bytes: &[u8]) -> String {
    bytes.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect()
}

fn text_at(block: &[u8], offset: u32, unicode: bool) -> Option<String> {
    let rest = block.get(offset as usize..)?;
    Some(if unicode { wide(rest) } else { narrow(rest) })
}

/// The target as the "link info" section spells it: a folder on a disk plus
/// the rest of the path.
fn link_info(block: &[u8]) -> Option<String> {
    let header = u32_at(block, 4)?;
    let flags = u32_at(block, 8)?;
    // Only a path on a local disk; a path on another machine is not something
    // CarlaOS starts games from.
    if flags & 0x1 == 0 {
        return None;
    }
    let (base, suffix) = if header >= 0x24 {
        (text_at(block, u32_at(block, 28)?, true), text_at(block, u32_at(block, 32)?, true))
    } else {
        (None, None)
    };
    // Each half may be empty; the two-byte spelling is used when there is one.
    let filled = |text: Option<String>| text.filter(|t| !t.is_empty());
    let base = filled(base).or_else(|| text_at(block, u32_at(block, 16)?, false)).unwrap_or_default();
    let suffix = filled(suffix).or_else(|| text_at(block, u32_at(block, 24)?, false)).unwrap_or_default();
    Some(base + &suffix).filter(|path| !path.is_empty())
}

/// `%ProgramFiles%\Game\game.exe` with the variables filled in. A `%` that
/// does not name a variable is left as it is.
fn expand(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let value = after.find('%').and_then(|end| std::env::var(&after[..end]).ok().filter(|_| end > 0).map(|v| (v, end)));
        match value {
            Some((value, end)) => {
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out + rest
}

/// Read a shortcut's bytes. `beside` is the folder the shortcut is in, for a
/// shortcut that only says where its target is from there.
pub fn parse(bytes: &[u8], beside: &Path) -> Result<Shortcut, String> {
    let wrong = || "That file is not a shortcut Windows made.".to_string();
    if bytes.len() < HEADER || u32_at(bytes, 0) != Some(HEADER as u32) {
        return Err(wrong());
    }
    let flags = u32_at(bytes, 20).ok_or_else(wrong)?;
    let unicode = flags & IS_UNICODE != 0;
    let mut reader = Reader { bytes, at: HEADER };
    if flags & HAS_ID_LIST != 0 {
        let len = usize::from(reader.u16().ok_or_else(wrong)?);
        reader.take(len).ok_or_else(wrong)?;
    }
    let mut target = None;
    if flags & HAS_LINK_INFO != 0 {
        let len = u32_at(bytes, reader.at).ok_or_else(wrong)? as usize;
        let block = reader.take(len.max(4)).ok_or_else(wrong)?;
        target = link_info(block);
    }
    let mut relative = None;
    let mut dir = None;
    let mut args = String::new();
    for flag in [HAS_NAME, HAS_RELATIVE_PATH, HAS_WORKING_DIR, HAS_ARGUMENTS, HAS_ICON] {
        if flags & flag == 0 {
            continue;
        }
        let text = reader.string(unicode).ok_or_else(wrong)?;
        match flag {
            HAS_RELATIVE_PATH => relative = Some(text),
            HAS_WORKING_DIR => dir = Some(text),
            HAS_ARGUMENTS => args = text,
            _ => {}
        }
    }
    // Extra blocks: the one with a target written with %variables%.
    let mut with_variables = None;
    while let Some(len) = u32_at(bytes, reader.at).map(|l| l as usize) {
        if len < 8 {
            break;
        }
        let Some(block) = reader.take(len) else { break };
        if u32_at(block, 4) == Some(ENVIRONMENT_BLOCK) {
            with_variables = block.get(8 + 260..).map(wide).filter(|s| !s.is_empty()).or_else(|| block.get(8..8 + 260).map(narrow).filter(|s| !s.is_empty()));
        }
    }
    let target = target
        .or_else(|| with_variables.map(|t| expand(&t)))
        .map(PathBuf::from)
        .or_else(|| relative.map(|r| beside.join(r.replace('\\', std::path::MAIN_SEPARATOR_STR))))
        .ok_or("That shortcut does not say which program it starts. Choose the game's program itself.")?;
    Ok(Shortcut { target, args: args.trim().to_string(), dir: dir.map(|d| PathBuf::from(expand(&d))).filter(|d| !d.as_os_str().is_empty()) })
}

/// Read the shortcut at `path`.
pub fn read(path: &Path) -> Result<Shortcut, String> {
    // A shortcut is a few kilobytes. Anything large is something else.
    let size = std::fs::metadata(path).map_err(|_| "That shortcut could not be found.".to_string())?.len();
    if size > 1024 * 1024 {
        return Err("That file is not a shortcut Windows made.".to_string());
    }
    let bytes = std::fs::read(path).map_err(|_| "That shortcut could not be read.".to_string())?;
    parse(&bytes, path.parent().unwrap_or(Path::new("")))
}

/// A shortcut's bytes, for tests here and elsewhere.
#[cfg(test)]
pub(crate) fn sample(target: &str, args: &str, dir: &str, unicode_info: bool) -> Vec<u8> {
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    let counted = |s: &str| [(s.encode_utf16().count() as u16).to_le_bytes().to_vec(), utf16(s)].concat();
    let mut flags = HAS_ID_LIST | HAS_LINK_INFO | HAS_NAME | IS_UNICODE;
    if !dir.is_empty() {
        flags |= HAS_WORKING_DIR;
    }
    if !args.is_empty() {
        flags |= HAS_ARGUMENTS;
    }
    let mut out = vec![0u8; HEADER];
    out[0..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
    out[20..24].copy_from_slice(&flags.to_le_bytes());
    // An id list of six bytes that says nothing.
    out.extend_from_slice(&[6, 0, 4, 0, 1, 2, 0, 0]);
    // Link info: the folder and the file name kept apart, as Windows does.
    let (base, suffix) = target.split_at(target.rfind('\\').map_or(0, |i| i + 1));
    let header: u32 = if unicode_info { 0x24 } else { 0x1C };
    let mut info = vec![0u8; header as usize];
    let mut tail = Vec::new();
    let put = |tail: &mut Vec<u8>, field: usize, bytes: Vec<u8>, info: &mut Vec<u8>| {
        let at = header + tail.len() as u32;
        info[field..field + 4].copy_from_slice(&at.to_le_bytes());
        tail.extend(bytes);
    };
    // The one-byte spelling loses the letters it cannot hold.
    let ansi = |s: &str| s.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).chain([0]).collect::<Vec<u8>>();
    put(&mut tail, 16, ansi(base), &mut info);
    put(&mut tail, 24, ansi(suffix), &mut info);
    if unicode_info {
        put(&mut tail, 28, [utf16(base), vec![0, 0]].concat(), &mut info);
        put(&mut tail, 32, [utf16(suffix), vec![0, 0]].concat(), &mut info);
    }
    info[4..8].copy_from_slice(&header.to_le_bytes());
    info[8..12].copy_from_slice(&1u32.to_le_bytes());
    let total = (info.len() + tail.len()) as u32;
    info[0..4].copy_from_slice(&total.to_le_bytes());
    out.extend(info);
    out.extend(tail);
    out.extend(counted("A game"));
    if !dir.is_empty() {
        out.extend(counted(dir));
    }
    if !args.is_empty() {
        out.extend(counted(args));
    }
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_says_what_it_starts_and_how() {
        let bytes = sample(r"C:\Games\Hookshot\hookshot.exe", "--fullscreen --lang en", r"C:\Games\Hookshot", false);
        let read = parse(&bytes, Path::new("C:/Users/Carla/Desktop")).unwrap();
        assert_eq!(read.target, PathBuf::from(r"C:\Games\Hookshot\hookshot.exe"));
        assert_eq!(read.args, "--fullscreen --lang en");
        assert_eq!(read.dir, Some(PathBuf::from(r"C:\Games\Hookshot")));

        // Letters the one-byte spelling cannot hold come from the two-byte one.
        let bytes = sample(r"D:\Jeux\Timothée\zèbre.exe", "", "", true);
        let read = parse(&bytes, Path::new("")).unwrap();
        assert_eq!(read.target, PathBuf::from(r"D:\Jeux\Timothée\zèbre.exe"));
        assert_eq!((read.args.as_str(), read.dir), ("", None));
    }

    #[test]
    fn what_is_not_a_shortcut_is_refused_without_reading_past_its_end() {
        assert!(parse(b"MZ\x90\x00 a program", Path::new("")).is_err());
        assert!(parse(&[], Path::new("")).is_err());
        let whole = sample(r"C:\Games\A\a.exe", "-x", r"C:\Games\A", true);
        // Cut anywhere, it is either refused or read; it never panics.
        for cut in 0..whole.len() {
            let _ = parse(&whole[..cut], Path::new(""));
        }
        // A length that points far past the end.
        let mut lying = whole.clone();
        lying[HEADER..HEADER + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(parse(&lying, Path::new("")).is_err());
        assert!(is_shortcut(Path::new("C:/x/Game.LNK")) && !is_shortcut(Path::new("C:/x/game.exe")));
    }

    #[test]
    fn a_shortcut_that_only_knows_the_way_from_its_own_folder_is_followed() {
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        let mut bytes = vec![0u8; HEADER];
        bytes[0..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&(HAS_RELATIVE_PATH | IS_UNICODE).to_le_bytes());
        let relative = "bin\\game.exe";
        bytes.extend((relative.len() as u16).to_le_bytes());
        bytes.extend(utf16(relative));
        let read = parse(&bytes, Path::new("/games/tin")).unwrap();
        assert_eq!(read.target, Path::new("/games/tin").join("bin").join("game.exe"));

        // Nothing at all to go by.
        let mut empty = vec![0u8; HEADER];
        empty[0..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        assert!(parse(&empty, Path::new("")).is_err());
    }

    #[test]
    fn variables_in_a_path_are_filled_in() {
        std::env::set_var("CARLA_TEST_GAMES", "D:\\Games");
        assert_eq!(expand("%CARLA_TEST_GAMES%\\Tin\\tin.exe"), "D:\\Games\\Tin\\tin.exe");
        assert_eq!(expand("%CARLA_NOT_SET%\\x.exe"), "%CARLA_NOT_SET%\\x.exe");
        assert_eq!(expand("C:\\100% Orange\\o.exe"), "C:\\100% Orange\\o.exe");
    }
}
