//! Reader for Valve's text key-value format, as used by `libraryfolders.vdf`
//! and `appmanifest_*.acf`. Only what those files need: quoted strings, nested
//! blocks and `//` comments.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Block(Vec<(String, Value)>),
}

impl Value {
    /// Child by key. Steam is not consistent about case, so neither is this.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Block(pairs) => pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Value::Text(_) => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(s) => Some(s),
            Value::Block(_) => None,
        }
    }

    pub fn children(&self) -> &[(String, Value)] {
        match self {
            Value::Block(pairs) => pairs,
            Value::Text(_) => &[],
        }
    }
}

enum Token {
    Text(String),
    Open,
    Close,
}

fn tokens(input: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                out.push(Token::Text(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                // Unquoted word.
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Token::Text(s));
            }
        }
    }
    out
}

fn block(tokens: &mut std::vec::IntoIter<Token>, top: bool) -> Option<Value> {
    let mut pairs = Vec::new();
    loop {
        let key = match tokens.next() {
            Some(Token::Text(k)) => k,
            Some(Token::Close) if !top => break,
            None if top => break,
            _ => return None,
        };
        let value = match tokens.next()? {
            Token::Text(v) => Value::Text(v),
            Token::Open => block(tokens, false)?,
            Token::Close => return None,
        };
        pairs.push((key, value));
    }
    Some(Value::Block(pairs))
}

/// Parse a whole file. `None` when it is not well formed.
pub fn parse(input: &str) -> Option<Value> {
    block(&mut tokens(input).into_iter(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_blocks_escapes_and_comments() {
        let v = parse(
            r#"
            // a comment
            "libraryfolders"
            {
                "0"
                {
                    "path"		"C:\\Program Files (x86)\\Steam"
                    "apps" { "620" "123" }
                }
            }"#,
        )
        .unwrap();
        let first = v.get("LibraryFolders").unwrap().get("0").unwrap();
        assert_eq!(first.text("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(first.get("apps").unwrap().children().len(), 1);
    }

    #[test]
    fn rejects_unbalanced_input() {
        assert!(parse(r#""a" { "b" "c""#).is_none());
        assert!(parse(r#""a" }"#).is_none());
    }
}
