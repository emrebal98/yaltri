//! Minimal parser for Valve's text KeyValues format (`.vdf`, `.acf`):
//! quoted keys and values, nested `{ }` blocks, `//` comments.

#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    /// Child by key, case-insensitive (Steam is inconsistent about case).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => {}
                        },
                        c => s.push(c),
                    }
                }
                out.push(Token::Str(s));
            }
            c if c.is_whitespace() => {}
            // Unquoted token (rare): read until whitespace or brace.
            c => {
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Token::Str(s));
            }
        }
    }
    out
}

/// Parses a whole file into a map of its top-level entries.
pub fn parse(text: &str) -> Vdf {
    let tokens = tokenize(text);
    let mut pos = 0;
    Vdf::Map(parse_entries(&tokens, &mut pos))
}

fn parse_entries(tokens: &[Token], pos: &mut usize) -> Vec<(String, Vdf)> {
    let mut items = Vec::new();
    while *pos < tokens.len() {
        let key = match &tokens[*pos] {
            Token::Close => {
                *pos += 1;
                break;
            }
            Token::Open => {
                *pos += 1;
                continue;
            }
            Token::Str(k) => k.clone(),
        };
        *pos += 1;
        match tokens.get(*pos) {
            Some(Token::Str(v)) => {
                items.push((key, Vdf::Str(v.clone())));
                *pos += 1;
            }
            Some(Token::Open) => {
                *pos += 1;
                items.push((key, Vdf::Map(parse_entries(tokens, pos))));
            }
            Some(Token::Close) | None => break,
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks_comments_and_escapes() {
        let v = parse(
            r#"
            // library list
            "libraryfolders"
            {
                "0"
                {
                    "path"    "C:\\Program Files (x86)\\Steam"
                    "apps" { "228980" "123" }
                }
                "1" { "path" "D:\\SteamLibrary" }
            }"#,
        );
        let libs = v.get("libraryfolders").unwrap();
        assert_eq!(libs.entries().len(), 2);
        assert_eq!(libs.get("0").unwrap().str("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(libs.get("1").unwrap().str("PATH"), Some(r"D:\SteamLibrary"));
        assert_eq!(libs.get("0").unwrap().get("apps").unwrap().str("228980"), Some("123"));
    }
}
