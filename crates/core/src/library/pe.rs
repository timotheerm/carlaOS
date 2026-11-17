//! What a Windows program says about itself.
//!
//! Most games carry their real name inside their program, in the details
//! Windows shows under Properties: "Product name". `game.exe` in a folder
//! called `bin` says nothing; its product name says "Paper Lanterns". Only
//! that one line is read, straight from the file, and nothing is run.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// More than this is not the part of a program that holds its details.
const LARGEST: u64 = 48 * 1024 * 1024;

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The part of the program that holds its icon and details, as bytes.
fn resources(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut head = vec![0u8; 4096];
    let got = file.read(&mut head).ok()?;
    head.truncate(got);
    if head.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(&head, 0x3c)? as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(&head, pe + 6)? as usize;
    let table = pe + 24 + u16_at(&head, pe + 20)? as usize;
    for i in 0..sections.min(96) {
        let at = table + i * 40;
        if head.get(at..at + 8)? != b".rsrc\0\0\0" {
            continue;
        }
        let (size, offset) = (u64::from(u32_at(&head, at + 16)?), u64::from(u32_at(&head, at + 20)?));
        if size == 0 || size > LARGEST {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = Vec::new();
        file.take(size).read_to_end(&mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

/// The value kept under one name in a program's details.
fn value(resources: &[u8], name: &str) -> Option<String> {
    // Names and values are kept two bytes a letter, each ended by a zero.
    let key: Vec<u8> = name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let mut from = 0;
    while let Some(found) = resources.get(from..)?.windows(key.len()).position(|w| w == key) {
        let at = from + found;
        from = at + 2;
        // Before the name: the whole entry's size, how long the value is, and
        // that it is text.
        if at < 6 || at % 2 != 0 {
            continue;
        }
        let (length, kind) = (u16_at(resources, at - 4)? as usize, u16_at(resources, at - 2)?);
        if length == 0 || length > 512 || kind != 1 {
            continue;
        }
        // The value starts on the next four-byte boundary after the name.
        let mut start = at + key.len();
        while start % 4 != 0 {
            start += 1;
        }
        let units: Vec<u16> = (0..length).map_while(|i| u16_at(resources, start + i * 2)).take_while(|u| *u != 0).collect();
        let text = String::from_utf16_lossy(&units).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Names that say what a program was made with, not what it is.
const NOT_A_NAME: &[&str] = &[
    "unity", "unityplayer", "unreal engine", "unrealengine", "bootstrappackagedgame", "electron", "godot engine", "godot", "nw.js", "nwjs", "love", "java platform se",
    "openjdk platform", "python", "game", "launcher", "application", "todo: <product name>", "product name", "microsoft windows operating system", "setup", "installer",
];

fn tidy(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !matches!(c, '\u{ae}' | '\u{2122}' | '\u{a9}')).collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = name.to_lowercase();
    if name.len() < 2 || name.len() > 80 || NOT_A_NAME.contains(&plain.as_str()) || plain.contains("crash") || plain.contains("redistributable") {
        return None;
    }
    Some(name)
}

/// The name a program gives itself, when it gives a useful one.
pub fn product_name(path: &Path) -> Option<String> {
    let resources = resources(path)?;
    ["ProductName", "FileDescription"].iter().find_map(|name| value(&resources, name).and_then(|v| tidy(&v)))
}

/// A tiny program file with the given details, to try this on.
#[cfg(test)]
pub(crate) fn sample(details: &[(&str, &str)]) -> Vec<u8> {
    let mut rsrc = vec![0u8; 16];
    for (name, text) in details {
        while rsrc.len() % 4 != 0 {
            rsrc.push(0);
        }
        let name: Vec<u8> = name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let text16: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let start = rsrc.len();
        rsrc.extend([0, 0]);
        rsrc.extend((text16.len() as u16).to_le_bytes());
        rsrc.extend(1u16.to_le_bytes());
        rsrc.extend(&name);
        while rsrc.len() % 4 != 0 {
            rsrc.push(0);
        }
        rsrc.extend(text16.iter().flat_map(|u| u.to_le_bytes()));
        let length = (rsrc.len() - start) as u16;
        rsrc[start..start + 2].copy_from_slice(&length.to_le_bytes());
    }
    let mut file = vec![0u8; 0x200];
    file[..2].copy_from_slice(b"MZ");
    file[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    file[0x80..0x84].copy_from_slice(b"PE\0\0");
    file[0x86..0x88].copy_from_slice(&2u16.to_le_bytes()); // two sections
    file[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes()); // optional header size
    let table = 0x80 + 24 + 0xf0;
    file[table..table + 8].copy_from_slice(b".text\0\0\0");
    let second = table + 40;
    file[second..second + 8].copy_from_slice(b".rsrc\0\0\0");
    file[second + 16..second + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
    file[second + 20..second + 24].copy_from_slice(&0x200u32.to_le_bytes());
    file.extend(rsrc);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_asked_for_the_name_it_gives_itself() {
        let dir = std::env::temp_dir().join(format!("carla-pe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, details: &[(&str, &str)]| {
            let path = dir.join(name);
            std::fs::write(&path, sample(details)).unwrap();
            path
        };
        assert_eq!(product_name(&write("game.exe", &[("CompanyName", "Small Studio"), ("ProductName", "Paper Lanterns\u{2122}"), ("FileDescription", "x")])).as_deref(), Some("Paper Lanterns"));
        // What it was made with is not its name; the description may be.
        assert_eq!(product_name(&write("a.exe", &[("ProductName", "Unreal Engine"), ("FileDescription", "Tin Knight")])).as_deref(), Some("Tin Knight"));
        assert_eq!(product_name(&write("b.exe", &[("ProductName", "BootstrapPackagedGame"), ("FileDescription", "UnityPlayer")])), None);
        assert_eq!(product_name(&write("c.exe", &[("CompanyName", "Nobody")])), None);
        // Not a program at all, or one cut short.
        std::fs::write(dir.join("d.exe"), b"program").unwrap();
        assert_eq!(product_name(&dir.join("d.exe")), None);
        std::fs::write(dir.join("e.exe"), &sample(&[("ProductName", "Whole")])[..0x210]).unwrap();
        assert_eq!(product_name(&dir.join("e.exe")), None);
        assert_eq!(product_name(&dir.join("missing.exe")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
