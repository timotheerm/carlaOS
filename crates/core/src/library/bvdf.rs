//! Steam's binary key-value files (`appcache/stats/*.bin`).
//!
//! Each entry is a type byte, a zero-terminated name, then a value: a nested
//! map that runs until an end byte, a zero-terminated string, or a number.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Map(BTreeMap<String, Node>),
    Text(String),
    Number(i64),
}

impl Node {
    /// A child by name. Steam is not consistent about the case of names.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(map) => map.get(key).or_else(|| map.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Node::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<i64> {
        match self {
            Node::Number(n) => Some(*n),
            Node::Text(text) => text.parse().ok(),
            Node::Map(_) => None,
        }
    }

    pub fn children(&self) -> impl Iterator<Item = (&String, &Node)> {
        let map = match self {
            Node::Map(map) => Some(map),
            _ => None,
        };
        map.into_iter().flatten()
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Newer files keep every name once, in a table, and refer to it by number.
    names: Option<&'a [String]>,
}

/// Files nest a handful of levels; anything deeper is not one of them.
const DEEPEST: usize = 32;

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }

    fn string(&mut self) -> Option<String> {
        let rest = self.bytes.get(self.at..)?;
        let len = rest.iter().position(|b| *b == 0)?;
        let text = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.at += len + 1;
        Some(text)
    }

    /// Entries until the end byte, or the end of the file at the top level.
    fn map(&mut self, depth: usize) -> Option<Node> {
        if depth > DEEPEST {
            return None;
        }
        let mut map = BTreeMap::new();
        loop {
            let kind = match self.byte() {
                Some(0x08 | 0x0B) => break,
                None if depth == 0 => break,
                None => return None,
                Some(kind) => kind,
            };
            let name = match self.names {
                Some(table) => table.get(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize)?.clone(),
                None => self.string()?,
            };
            let value = match kind {
                0x00 => self.map(depth + 1)?,
                0x01 => Node::Text(self.string()?),
                0x02 | 0x04 | 0x06 => Node::Number(i64::from(i32::from_le_bytes(self.take(4)?.try_into().ok()?))),
                0x03 => Node::Number(f32::from_le_bytes(self.take(4)?.try_into().ok()?) as i64),
                0x07 | 0x0A => Node::Number(i64::from_le_bytes(self.take(8)?.try_into().ok()?)),
                _ => return None,
            };
            map.insert(name, value);
        }
        Some(Node::Map(map))
    }
}

pub fn parse(bytes: &[u8]) -> Option<Node> {
    Reader { bytes, at: 0, names: None }.map(0)
}

/// One map from the start of `bytes`, up to and including its end byte, and
/// how many bytes that was. For files that hold many maps one after another.
/// With `names`, each name is a number into that table instead of text.
pub fn parse_prefix(bytes: &[u8], names: Option<&[String]>) -> Option<(Node, usize)> {
    let mut reader = Reader { bytes, at: 0, names };
    let node = reader.map(0)?;
    Some((node, reader.at))
}

/// Builds files in the same format, for tests here and in the scanners.
#[cfg(test)]
pub(crate) mod write {
    pub fn map(name: &str, children: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend(name.as_bytes());
        out.push(0);
        children.iter().for_each(|c| out.extend(c));
        out.push(0x08);
        out
    }
    pub fn text(name: &str, value: &str) -> Vec<u8> {
        let mut out = vec![0x01];
        out.extend(name.as_bytes());
        out.push(0);
        out.extend(value.as_bytes());
        out.push(0);
        out
    }
    pub fn int(name: &str, value: i32) -> Vec<u8> {
        let mut out = vec![0x02];
        out.extend(name.as_bytes());
        out.push(0);
        out.extend(value.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_maps_strings_and_numbers_are_read() {
        let file = write::map("440", &[write::text("gamename", "Example"), write::map("stats", &[write::int("version", 7)])]);
        let root = parse(&file).unwrap();
        let game = root.get("440").unwrap();
        assert_eq!(game.get("GameName").and_then(Node::text), Some("Example"));
        assert_eq!(game.get("stats").and_then(|s| s.get("version")).and_then(Node::number), Some(7));
        assert_eq!(game.children().count(), 2);
    }

    #[test]
    fn damaged_files_are_refused_not_trusted() {
        let file = write::map("a", &[write::text("b", "c")]);
        assert!(parse(&file[..file.len() - 3]).is_none(), "cut off in the middle");
        assert!(parse(&[0x02, b'x', 0, 1, 2]).is_none(), "number cut short");
        assert!(parse(&[0x63, b'x', 0]).is_none(), "unknown type");
        let mut deep = Vec::new();
        for _ in 0..100 {
            deep.extend([0x00, b'x', 0]);
        }
        assert!(parse(&deep).is_none(), "absurd nesting");
        assert_eq!(parse(&[]), Some(Node::Map(BTreeMap::new())));
    }
}
