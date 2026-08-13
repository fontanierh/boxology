use std::{error::Error, fmt};

use crate::{OpaqueNumber, OpaqueTree};

/// Default maximum nesting depth accepted by [`super::decode`].
pub const DEFAULT_DEPTH_LIMIT: usize = 128;

/// Resource limits applied before and while parsing JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum accepted input length in bytes.
    pub max_bytes: usize,
    /// Maximum nested JSON container depth.
    pub max_depth: usize,
}

impl Limits {
    /// Constructs explicit byte and nesting limits.
    pub const fn new(max_bytes: usize, max_depth: usize) -> Self {
        Self {
            max_bytes,
            max_depth,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A payload-free JSON syntax or resource-limit failure.
pub enum SyntaxError {
    /// Input exceeded the configured byte limit.
    PayloadTooLarge { limit: usize },
    /// Input was not valid UTF-8.
    InvalidUtf8,
    /// JSON containers exceeded the configured nesting depth.
    DepthLimitExceeded { limit: usize },
    /// Input was not exactly one valid JSON value.
    MalformedJson,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { limit } => write!(formatter, "payload too large ({limit})"),
            Self::InvalidUtf8 => formatter.write_str("payload is not valid UTF-8"),
            Self::DepthLimitExceeded { limit } => {
                write!(formatter, "depth limit exceeded ({limit})")
            }
            Self::MalformedJson => formatter.write_str("malformed JSON"),
        }
    }
}

impl Error for SyntaxError {}

/// Parses one lossless JSON syntax tree while preserving object order and duplicates.
pub fn parse(input: &[u8], limits: Limits) -> Result<OpaqueTree, SyntaxError> {
    if input.len() > limits.max_bytes {
        return Err(SyntaxError::PayloadTooLarge {
            limit: limits.max_bytes,
        });
    }
    let text = std::str::from_utf8(input).map_err(|_| SyntaxError::InvalidUtf8)?;
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
        max_depth: limits.max_depth,
    };
    parser.whitespace();
    let tree = parser.value(0)?;
    parser.whitespace();
    (parser.at == parser.bytes.len())
        .then_some(tree)
        .ok_or(SyntaxError::MalformedJson)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    max_depth: usize,
}

impl Parser<'_> {
    fn value(&mut self, parent_depth: usize) -> Result<OpaqueTree, SyntaxError> {
        match self.peek() {
            Some(b'n') => self.keyword(b"null", OpaqueTree::Null),
            Some(b't') => self.keyword(b"true", OpaqueTree::Bool(true)),
            Some(b'f') => self.keyword(b"false", OpaqueTree::Bool(false)),
            Some(b'"') => self.string().map(OpaqueTree::String),
            Some(b'[') => {
                let depth = self.enter(parent_depth)?;
                self.list(depth)
            }
            Some(b'{') => {
                let depth = self.enter(parent_depth)?;
                self.object(depth)
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(SyntaxError::MalformedJson),
        }
    }

    fn enter(&self, parent: usize) -> Result<usize, SyntaxError> {
        parent
            .checked_add(1)
            .filter(|depth| *depth <= self.max_depth)
            .ok_or(SyntaxError::DepthLimitExceeded {
                limit: self.max_depth,
            })
    }

    fn list(&mut self, depth: usize) -> Result<OpaqueTree, SyntaxError> {
        self.at += 1;
        self.whitespace();
        let mut values = Vec::new();
        if self.take(b']') {
            return Ok(OpaqueTree::List(values));
        }
        loop {
            values.push(self.value(depth)?);
            self.whitespace();
            if self.take(b']') {
                return Ok(OpaqueTree::List(values));
            }
            if !self.take(b',') {
                return Err(SyntaxError::MalformedJson);
            }
            self.whitespace();
        }
    }

    fn object(&mut self, depth: usize) -> Result<OpaqueTree, SyntaxError> {
        self.at += 1;
        self.whitespace();
        let mut entries = Vec::new();
        if self.take(b'}') {
            return Ok(OpaqueTree::Object(entries));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(SyntaxError::MalformedJson);
            }
            let key = self.string()?;
            self.whitespace();
            if !self.take(b':') {
                return Err(SyntaxError::MalformedJson);
            }
            self.whitespace();
            entries.push((key, self.value(depth)?));
            self.whitespace();
            if self.take(b'}') {
                return Ok(OpaqueTree::Object(entries));
            }
            if !self.take(b',') {
                return Err(SyntaxError::MalformedJson);
            }
            self.whitespace();
        }
    }

    fn string(&mut self) -> Result<String, SyntaxError> {
        let start = self.at;
        self.at += 1;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.at += 1;
                    let token = std::str::from_utf8(&self.bytes[start..self.at])
                        .map_err(|_| SyntaxError::InvalidUtf8)?;
                    return serde_json::from_str(token).map_err(|_| SyntaxError::MalformedJson);
                }
                b'\\' => self.at = self.at.saturating_add(2).min(self.bytes.len()),
                _ => self.at += 1,
            }
        }
        Err(SyntaxError::MalformedJson)
    }

    fn number(&mut self) -> Result<OpaqueTree, SyntaxError> {
        let start = self.at;
        if self.take(b'-') && self.peek().is_none() {
            return Err(SyntaxError::MalformedJson);
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.at += 1;
                self.digits();
            }
            _ => return Err(SyntaxError::MalformedJson),
        }
        if self.take(b'.') && !self.digits() {
            return Err(SyntaxError::MalformedJson);
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !self.digits() {
                return Err(SyntaxError::MalformedJson);
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| SyntaxError::InvalidUtf8)?;
        OpaqueNumber::new(token)
            .map(OpaqueTree::Number)
            .map_err(|_| SyntaxError::MalformedJson)
    }

    fn keyword(&mut self, word: &[u8], value: OpaqueTree) -> Result<OpaqueTree, SyntaxError> {
        if self.bytes[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(SyntaxError::MalformedJson)
        }
    }

    fn digits(&mut self) -> bool {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at != start
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        let present = self.peek() == Some(byte);
        self.at += usize::from(present);
        present
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_bytes: usize, max_depth: usize) -> Limits {
        Limits::new(max_bytes, max_depth)
    }

    fn parse_ok(input: &str) -> OpaqueTree {
        parse(input.as_bytes(), limits(input.len(), DEFAULT_DEPTH_LIMIT)).unwrap()
    }

    fn parse_error(input: &[u8], limits: Limits) -> SyntaxError {
        parse(input, limits).unwrap_err()
    }

    #[test]
    fn parses_every_shape_with_json_whitespace() {
        let tree = parse_ok(" \t\r\n{\"n\":null,\"items\":[true,false,\"s\",-2.5e+3],\"o\":{}}\n");
        assert_eq!(
            tree,
            OpaqueTree::Object(vec![
                ("n".into(), OpaqueTree::Null),
                (
                    "items".into(),
                    OpaqueTree::List(vec![
                        OpaqueTree::Bool(true),
                        OpaqueTree::Bool(false),
                        OpaqueTree::String("s".into()),
                        OpaqueTree::Number(OpaqueNumber::new("-2.5e+3").unwrap()),
                    ]),
                ),
                ("o".into(), OpaqueTree::Object(vec![])),
            ])
        );
    }

    #[test]
    fn preserves_number_tokens_exactly() {
        for token in ["-0", "1E+09", "12.3400", "123456789012345678901234567890"] {
            let OpaqueTree::Number(number) = parse_ok(token) else {
                panic!("not a number")
            };
            assert_eq!(number.as_str(), token);
        }
    }

    #[test]
    fn preserves_object_order_and_decoded_duplicate_keys() {
        let OpaqueTree::Object(entries) = parse_ok(r#"{"b":1,"a":2,"\u0061":3,"a":4}"#) else {
            panic!("not an object")
        };
        assert_eq!(
            entries
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            ["b", "a", "a", "a"]
        );
    }

    #[test]
    fn decodes_strings_and_surrogate_pairs() {
        for (input, expected) in [
            (r#""plain""#, "plain"),
            (r#""\"\\\/\b\f\n\r\t""#, "\"\\/\u{8}\u{c}\n\r\t"),
            (r#""\u0061\uD834\uDD1E""#, "a𝄞"),
        ] {
            assert_eq!(parse_ok(input), OpaqueTree::String(expected.into()));
        }
    }

    #[test]
    fn rejects_malformed_json_and_non_utf8() {
        #[rustfmt::skip]
        let malformed: &[&[u8]] = &[
            b"", b" \n", b"\xef\xbb\xbfnull",
            br#""\x""#, br#""\uD800""#, br#""\uDC00""#, b"\"\x01\"",
            b"[", b"[1 2]", b"[1,]", b"{", br#"{"a" 1}"#, br#"{"a":}"#, br#"{"a":1,}"#,
            b"01", b"+1", b"1.", b"1e", b"1e+", b"NaN", b"null true",
        ];
        for input in malformed {
            assert_eq!(
                parse_error(input, limits(usize::MAX, DEFAULT_DEPTH_LIMIT)),
                SyntaxError::MalformedJson,
                "{input:?}"
            );
        }
        assert_eq!(
            parse_error(&[b'"', 0xff, b'"'], limits(3, 128)),
            SyntaxError::InvalidUtf8
        );
    }

    #[test]
    fn applies_byte_limit_before_utf8_and_grammar() {
        assert_eq!(parse(b"null", limits(4, 128)), Ok(OpaqueTree::Null));
        assert_eq!(
            parse_error(b"null", limits(3, 128)),
            SyntaxError::PayloadTooLarge { limit: 3 }
        );
        assert_eq!(
            parse_error(&[0xff], limits(0, 128)),
            SyntaxError::PayloadTooLarge { limit: 0 }
        );
        assert_eq!(
            parse_error(&[0xff], limits(1, 128)),
            SyntaxError::InvalidUtf8
        );
    }

    #[test]
    fn enforces_container_depth_without_counting_string_brackets() {
        let at_limit = format!("{}null{}", "[".repeat(128), "]".repeat(128));
        assert!(parse(at_limit.as_bytes(), limits(at_limit.len(), 128)).is_ok());
        let over_limit = format!("{}null{}", "[".repeat(129), "]".repeat(129));
        assert_eq!(
            parse_error(over_limit.as_bytes(), limits(over_limit.len(), 128)),
            SyntaxError::DepthLimitExceeded { limit: 128 }
        );
        assert_eq!(
            parse(br#""[[{{""#, limits(6, 0)),
            Ok(OpaqueTree::String("[[{{".into()))
        );
        assert_eq!(
            parse_error(b"[]", limits(2, 0)),
            SyntaxError::DepthLimitExceeded { limit: 0 }
        );
    }

    #[test]
    fn errors_never_retain_or_format_payload_content() {
        const SENTINEL: &str = "DO_NOT_LEAK_1e999";
        let input = format!(r#"{{"{SENTINEL}":"{SENTINEL}",}}"#);
        let error = parse(input.as_bytes(), limits(input.len(), 128)).unwrap_err();
        let diagnostic = format!("{error} {error:?}");
        assert!(!diagnostic.contains(SENTINEL));
        assert!(error.source().is_none());
    }
}
