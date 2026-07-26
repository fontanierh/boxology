use crate::{D2_SOURCE, Diagnostic, RelativePath, Span, has_drive_prefix, is_forbidden_byte};

const CONTROL: &str = "glob patterns must not contain backslashes or control characters";
const DIALECT: &str = "the v1 glob dialect supports only * and ** wildcards";
const SEGMENT: &str = "glob patterns must not contain empty or . segments";
const UPWARD: &str = "glob patterns must not contain .. segments";
const WHOLE: &str = "** must stand alone as a complete segment";

/// A validated pattern of the frozen v1 manifest glob dialect.
///
/// Patterns are anchored at the manifest's own directory, so `*.rs` matches only at that level
/// and `**/*.rs` is required to recurse. `*` matches zero or more characters within one segment
/// and never crosses `/`. `**` stands alone as a whole segment and matches zero or more segments,
/// except as the final segment, where it matches one or more, so `a/**` never matches the file
/// `a`. Every other character, including `{`, `}`, `]`, a non-leading `!`, and spaces, is an
/// untrimmed literal — except a backslash or an ASCII control byte, which the whole C0 range and
/// DEL are, and which BXW0017 rejects so a validated pattern is safe to echo into a terminal
/// report. Matching is bytewise and case-sensitive over file paths only, so a pattern equal to a
/// directory path matches nothing by itself.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GlobPattern(String);
impl GlobPattern {
    /// Validates one pattern of the dialect, locating any rejection at `span` within `path`.
    // One located diagnostic is 128 bytes and is returned by value rather than boxed.
    #[allow(clippy::result_large_err)]
    pub fn parse(pattern: &str, path: &RelativePath, span: Span) -> Result<Self, Diagnostic> {
        let reject = |code, rule| diagnose(path, span, code, rule);
        let bytes = pattern.as_bytes();
        if pattern.is_empty() {
            return Err(reject("BXW0013", "glob patterns must be non-empty"));
        }
        if pattern.starts_with('/') || has_drive_prefix(bytes) {
            return Err(reject("BXW0014", "glob patterns must be relative"));
        }
        if bytes.iter().any(|byte| is_forbidden_byte(*byte)) {
            return Err(reject("BXW0017", CONTROL));
        }
        if pattern.starts_with('!') || pattern.contains(['?', '[']) {
            return Err(reject("BXW0018", DIALECT));
        }
        for segment in pattern.split('/') {
            if segment.is_empty() || segment == "." {
                return Err(reject("BXW0015", SEGMENT));
            }
            if segment == ".." {
                return Err(reject("BXW0016", UPWARD));
            }
            if segment != "**" && segment.contains("**") {
                return Err(reject("BXW0019", WHOLE));
            }
        }
        Ok(Self(String::from(pattern)))
    }
    /// Returns the exact, unnormalized pattern spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Reports whether this pattern matches `path`, which must name a file.
    pub fn matches(&self, path: &RelativePath) -> bool {
        let pattern: Vec<&str> = self.0.split('/').collect();
        let segments: Vec<&str> = path.as_str().split('/').collect();
        // A final `**` matches one or more segments: reserve the last one for it, then match the
        // remaining prefix against the whole pattern under zero-or-more semantics.
        let segments: &[&str] = match pattern.last() {
            Some(&"**") => segments.split_last().map_or(&[], |(_, rest)| rest),
            _ => &segments,
        };
        wildcard(&pattern, segments, is_any, segment_matches)
    }
}
fn is_any(segment: &&str) -> bool {
    *segment == "**"
}
fn segment_matches(pattern: &&str, text: &&str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    wildcard(pattern, text, |byte| *byte == b'*', u8::eq)
}
fn diagnose(path: &RelativePath, span: Span, code: &'static str, rule: &'static str) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span,
        code,
        offending: String::from("glob pattern"),
        rule,
        rule_source: D2_SOURCE,
    }
}
/// Matches `input` against `pattern` iteratively, where a star item matches zero or more items.
/// The only backtracking point is the most recent star, retried one item further on each
/// mismatch, so this terminates without recursion for any input.
fn wildcard<P, I>(
    pattern: &[P],
    input: &[I],
    is_star: impl Fn(&P) -> bool,
    matches: impl Fn(&P, &I) -> bool,
) -> bool {
    let (mut next, mut taken) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while let Some(value) = input.get(taken) {
        if pattern.get(next).is_some_and(&is_star) {
            star = Some((next, taken));
            next += 1;
        } else if pattern.get(next).is_some_and(|item| matches(item, value)) {
            next += 1;
            taken += 1;
        } else if let Some((star_next, star_taken)) = star {
            (next, taken) = (star_next + 1, star_taken + 1);
            star = Some((star_next, star_taken + 1));
        } else {
            return false;
        }
    }
    pattern.iter().skip(next).all(is_star)
}
