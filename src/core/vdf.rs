use std::fmt;

const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    String(String),
    Object(Node),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    pub pairs: Vec<(String, Value)>,
}

impl Node {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.pairs
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

pub(crate) fn decode(bytes: &[u8]) -> Result<Node, ParseError> {
    if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
        return Err(ParseError("VDF is empty or exceeds the 8 MiB limit".into()));
    }
    if bytes[0] == 0 {
        return decode_binary(bytes);
    }

    let text = decode_text_encoding(bytes)?;
    parse_text(&text)
}

fn decode_text_encoding(bytes: &[u8]) -> Result<String, ParseError> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(bytes[3..].to_vec())
            .map_err(|_| ParseError("VDF text is not valid UTF-8".into()));
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return Err(ParseError("truncated UTF-16 VDF text".into()));
        }
        let le = bytes[0] == 0xff;
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if le {
                    u16::from_le_bytes(*pair)
                } else {
                    u16::from_be_bytes(*pair)
                }
            })
            .collect::<Vec<_>>();
        return String::from_utf16(&units)
            .map_err(|_| ParseError("invalid UTF-16 VDF text".into()));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| ParseError("VDF text is not valid UTF-8".into()))
}

fn decode_binary(bytes: &[u8]) -> Result<Node, ParseError> {
    let mut parser = BinaryParser {
        bytes,
        offset: 0,
        nodes: 0,
    };
    let root_type = parser.byte()?;
    if root_type != 0 {
        return Err(ParseError("binary VDF root must be an object".into()));
    }
    let root_name = parser.cstring()?;
    let root_object = parser.object(1)?;
    if parser.byte()? != 8 || parser.offset != bytes.len() {
        return Err(ParseError(
            "binary VDF has an invalid final root marker".into(),
        ));
    }
    let mut root = Node::default();
    root.pairs.push((root_name, Value::Object(root_object)));
    Ok(root)
}

struct BinaryParser<'a> {
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
}

impl BinaryParser<'_> {
    fn byte(&mut self) -> Result<u8, ParseError> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| ParseError("truncated binary VDF".into()))?;
        self.offset += 1;
        Ok(value)
    }

    fn take(&mut self, len: usize) -> Result<&[u8], ParseError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| ParseError("binary VDF length overflow".into()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ParseError("truncated binary VDF value".into()))?;
        self.offset = end;
        Ok(value)
    }

    fn cstring(&mut self) -> Result<String, ParseError> {
        let rest = &self.bytes[self.offset..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| ParseError("unterminated binary VDF string".into()))?;
        let value = std::str::from_utf8(&rest[..end])
            .map_err(|_| ParseError("invalid UTF-8 in binary VDF".into()))?
            .to_owned();
        self.offset += end + 1;
        Ok(value)
    }

    fn object(&mut self, depth: usize) -> Result<Node, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError("binary VDF nesting limit exceeded".into()));
        }
        let mut node = Node::default();
        loop {
            let kind = self.byte()?;
            if kind == 8 || kind == 11 {
                return Ok(node);
            }
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(ParseError("binary VDF node limit exceeded".into()));
            }
            let key = self.cstring()?;
            let value = match kind {
                0 => Value::Object(self.object(depth + 1)?),
                1 => Value::String(self.cstring()?),
                2 => {
                    Value::String(i32::from_le_bytes(self.take(4)?.try_into().unwrap()).to_string())
                }
                3 => {
                    let bits = u32::from_le_bytes(self.take(4)?.try_into().unwrap());
                    let f = f32::from_bits(bits);
                    if !f.is_finite() {
                        return Err(ParseError("non-finite float in binary VDF".into()));
                    }
                    Value::String(f.to_string())
                }
                4 | 6 => {
                    Value::String(u32::from_le_bytes(self.take(4)?.try_into().unwrap()).to_string())
                }
                7 => {
                    Value::String(u64::from_le_bytes(self.take(8)?.try_into().unwrap()).to_string())
                }
                10 => {
                    Value::String(i64::from_le_bytes(self.take(8)?.try_into().unwrap()).to_string())
                }
                5 => {
                    return Err(ParseError(
                        "binary VDF wide strings are not supported".into(),
                    ));
                }
                _ => return Err(ParseError(format!("unsupported binary VDF type {kind}"))),
            };
            node.pairs.push((key, value));
        }
    }
}

fn parse_text(text: &str) -> Result<Node, ParseError> {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        index: 0,
        nodes: 0,
    };
    let root = lexer.object(0, false)?;
    if lexer.next()?.is_some() {
        return Err(ParseError("trailing tokens after VDF root".into()));
    }
    Ok(root)
}

struct Lexer {
    chars: Vec<char>,
    index: usize,
    nodes: usize,
}

impl Lexer {
    fn skip_space_comments(&mut self) {
        loop {
            while self.index < self.chars.len()
                && (self.chars[self.index].is_whitespace() || self.chars[self.index] == ';')
            {
                self.index += 1;
            }
            if self.index + 1 < self.chars.len()
                && self.chars[self.index] == '/'
                && self.chars[self.index + 1] == '/'
            {
                while self.index < self.chars.len() && self.chars[self.index] != '\n' {
                    self.index += 1;
                }
                continue;
            }
            break;
        }
    }

    fn next(&mut self) -> Result<Option<String>, ParseError> {
        self.skip_space_comments();
        let Some(&current) = self.chars.get(self.index) else {
            return Ok(None);
        };
        if current == '{' || current == '}' {
            self.index += 1;
            return Ok(Some(current.to_string()));
        }
        if current == '"' {
            self.index += 1;
            let mut result = String::new();
            while let Some(&c) = self.chars.get(self.index) {
                self.index += 1;
                match c {
                    '"' => return Ok(Some(result)),
                    '\\' => {
                        let escaped = *self
                            .chars
                            .get(self.index)
                            .ok_or_else(|| ParseError("unterminated VDF escape".into()))?;
                        self.index += 1;
                        result.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            c => c,
                        });
                    }
                    c => result.push(c),
                }
            }
            return Err(ParseError("unterminated quoted VDF string".into()));
        }
        let start = self.index;
        while self.index < self.chars.len() {
            let c = self.chars[self.index];
            if c.is_whitespace() || c == '{' || c == '}' {
                break;
            }
            self.index += 1;
        }
        if start == self.index {
            return Err(ParseError("unexpected VDF token".into()));
        }
        Ok(Some(self.chars[start..self.index].iter().collect()))
    }

    fn object(&mut self, depth: usize, nested: bool) -> Result<Node, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError("VDF nesting limit exceeded".into()));
        }
        let mut node = Node::default();
        loop {
            let Some(key) = self.next()? else {
                return if nested {
                    Err(ParseError("missing closing VDF brace".into()))
                } else {
                    Ok(node)
                };
            };
            if key == "}" {
                return if nested {
                    Ok(node)
                } else {
                    Err(ParseError("unexpected closing VDF brace".into()))
                };
            }
            if key == "{" {
                return Err(ParseError("expected VDF key before opening brace".into()));
            }
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(ParseError("VDF node limit exceeded".into()));
            }
            let value = self
                .next()?
                .ok_or_else(|| ParseError(format!("VDF key {key:?} has no value")))?;
            let value = if value == "{" {
                Value::Object(self.object(depth + 1, true)?)
            } else if value == "}" {
                return Err(ParseError(format!("VDF key {key:?} has no value")));
            } else {
                Value::String(value)
            };
            node.pairs.push((key, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_text_and_case_insensitive_fields() {
        let doc = decode(
            br#""Root" { "Owner" "alpha" "Items" { "730" { "SteamID" "76561198000000000" } } }"#,
        )
        .unwrap();
        let Value::Object(root) = doc.get("root").unwrap() else {
            panic!("root should be object")
        };
        assert_eq!(root.get("owner"), Some(&Value::String("alpha".into())));
        let Value::Object(items) = root.get("ITEMS").unwrap() else {
            panic!("items should be object")
        };
        assert!(items.get("730").is_some());
    }

    #[test]
    fn parses_binary_kv1_and_unsigned_steam_id() {
        let mut b = Vec::new();
        fn z(b: &mut Vec<u8>, s: &str) {
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        b.push(0);
        z(&mut b, "coplay");
        b.push(0);
        z(&mut b, "0");
        b.push(7);
        z(&mut b, "SteamID");
        b.extend_from_slice(&76561198000000000u64.to_le_bytes());
        b.push(7);
        z(&mut b, "gameid");
        b.extend_from_slice(&730u64.to_le_bytes());
        b.push(2);
        z(&mut b, "Playtime");
        b.extend_from_slice(&1793094741i32.to_le_bytes());
        b.extend_from_slice(&[8, 8, 8]);
        let doc = decode(&b).unwrap();
        let Value::Object(root) = doc.get("coplay").unwrap() else {
            panic!("root should be object")
        };
        let Value::Object(entry) = root.get("0").unwrap() else {
            panic!("entry should be object")
        };
        assert_eq!(
            entry.get("SteamID"),
            Some(&Value::String("76561198000000000".into()))
        );
        assert_eq!(entry.get("gameid"), Some(&Value::String("730".into())));
        assert_eq!(
            entry.get("Playtime"),
            Some(&Value::String("1793094741".into()))
        );
    }

    #[test]
    fn rejects_truncated_binary_and_unclosed_text() {
        assert!(decode(&[0, b'x', 0, 0, b'0', 0, 7, b'i', 0, 1, 2]).is_err());
        assert!(decode(br#""root" { "x" "v""#).is_err());
        assert!(decode(&vec![0; MAX_FILE_BYTES + 1]).is_err());
    }
}
