//! Rust identifier rules shared by contract producers and readers.

use unicode_normalization::UnicodeNormalization;

/// NFC-normalizes `value` and returns it when it is an ordinary, non-raw Rust
/// 2024 identifier.
///
/// Validation uses the Rust 2024 lexical identifier profile described by
/// [`is_ordinary_rust_identifier`]. Returning the NFC spelling gives source
/// parsers the same canonical identity that compiler macro tokens carry.
pub fn canonicalize_ordinary_rust_identifier(value: &str) -> Option<String> {
    let canonical = value.nfc().collect::<String>();
    is_normalized_ordinary_rust_identifier(&canonical).then_some(canonical)
}

/// Returns whether `value` is an ordinary, non-raw Rust 2024 identifier.
///
/// This is the Rust 2024 lexical identifier profile: an `XID_Start` character
/// or underscore followed by zero or more `XID_Continue` characters, with a
/// leading underscore requiring at least one continuation character. Strict
/// and reserved keywords, raw spellings, and Rust's two disallowed zero-width
/// characters are excluded. Weak keywords remain valid where Rust treats them
/// as ordinary identifiers. The input is a Rust `&str`, so malformed UTF-8 and
/// non-scalar Unicode values cannot reach this predicate.
pub fn is_ordinary_rust_identifier(value: &str) -> bool {
    canonicalize_ordinary_rust_identifier(value).is_some()
}

fn is_normalized_ordinary_rust_identifier(value: &str) -> bool {
    if value.is_empty() || value.starts_with("r#") || is_strict_or_reserved_keyword(value) {
        return false;
    }

    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if first == '_' {
        if !characters.next().is_some_and(is_allowed_continue) {
            return false;
        }
    } else if !unicode_ident::is_xid_start(first) {
        return false;
    }

    characters.all(is_allowed_continue)
}

fn is_allowed_continue(character: char) -> bool {
    !matches!(character, '\u{200c}' | '\u{200d}') && unicode_ident::is_xid_continue(character)
}

fn is_strict_or_reserved_keyword(value: &str) -> bool {
    matches!(
        value,
        "_" | "as"
            | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_ordinary_rust_identifier, is_ordinary_rust_identifier};

    #[test]
    fn canonicalizer_returns_one_nfc_identity_for_equivalent_spellings() {
        let canonical = Some("é".to_owned());
        assert_eq!(canonicalize_ordinary_rust_identifier("e\u{301}"), canonical);
        assert_eq!(canonicalize_ordinary_rust_identifier("é"), canonical);
        assert!(is_ordinary_rust_identifier("e\u{301}"));
        assert!(is_ordinary_rust_identifier("é"));
    }

    #[test]
    fn invalid_spellings_remain_rejected_after_normalization() {
        for value in ["gen", "r#gen", "\u{fffd}", "a\0b", "a\u{200c}"] {
            assert_eq!(canonicalize_ordinary_rust_identifier(value), None);
        }
    }

    #[test]
    fn identifier_profile_has_exact_ascii_and_unicode_boundaries() {
        let cases = [
            ("a", true),
            ("Z", true),
            ("_name", true),
            ("__", true),
            ("snake_case", true),
            ("a0_b9", true),
            ("bool", true),
            ("u8", true),
            ("genesis", true),
            ("é", true),
            ("Москва", true),
            ("東京", true),
            ("变量", true),
            ("e\u{301}", true),
            ("_é", true),
            ("a\u{301}b", true),
            ("", false),
            ("_", false),
            ("_😀", false),
            ("_\u{200c}", false),
            ("9lives", false),
            ("\u{301}", false),
            ("0变量", false),
            ("😀", false),
            ("a-b", false),
            ("a b", false),
            ("a.b", false),
            ("a\0b", false),
            ("a\u{200c}", false),
            ("a\u{200d}", false),
        ];
        for (value, expected) in cases {
            assert_eq!(
                is_ordinary_rust_identifier(value),
                expected,
                "unexpected identifier classification for {value:?}"
            );
        }
    }

    #[test]
    fn raw_identifier_spellings_are_not_ordinary_identifiers() {
        for (value, expected) in [
            ("r#name", false),
            ("r#async", false),
            ("r#_name", false),
            ("r#", false),
            ("r##name", false),
        ] {
            assert_eq!(is_ordinary_rust_identifier(value), expected, "{value:?}");
        }
    }

    #[test]
    fn every_rust_2024_strict_keyword_is_rejected() {
        let keywords = [
            ("_", false),
            ("as", false),
            ("async", false),
            ("await", false),
            ("break", false),
            ("const", false),
            ("continue", false),
            ("crate", false),
            ("dyn", false),
            ("else", false),
            ("enum", false),
            ("extern", false),
            ("false", false),
            ("fn", false),
            ("for", false),
            ("if", false),
            ("impl", false),
            ("in", false),
            ("let", false),
            ("loop", false),
            ("match", false),
            ("mod", false),
            ("move", false),
            ("mut", false),
            ("pub", false),
            ("ref", false),
            ("return", false),
            ("self", false),
            ("Self", false),
            ("static", false),
            ("struct", false),
            ("super", false),
            ("trait", false),
            ("true", false),
            ("type", false),
            ("unsafe", false),
            ("use", false),
            ("where", false),
            ("while", false),
        ];
        assert_eq!(keywords.len(), 39);
        for (keyword, expected) in keywords {
            assert_eq!(
                is_ordinary_rust_identifier(keyword),
                expected,
                "{keyword:?}"
            );
        }
    }

    #[test]
    fn every_rust_2024_reserved_keyword_is_rejected() {
        let keywords = [
            ("abstract", false),
            ("become", false),
            ("box", false),
            ("do", false),
            ("final", false),
            ("gen", false),
            ("macro", false),
            ("override", false),
            ("priv", false),
            ("try", false),
            ("typeof", false),
            ("unsized", false),
            ("virtual", false),
            ("yield", false),
        ];
        assert_eq!(keywords.len(), 14);
        for (keyword, expected) in keywords {
            assert_eq!(
                is_ordinary_rust_identifier(keyword),
                expected,
                "{keyword:?}"
            );
        }
    }

    #[test]
    fn weak_keywords_follow_their_identifier_context() {
        for (value, expected) in [
            ("macro_rules", true),
            ("raw", true),
            ("safe", true),
            ("union", true),
            ("'static", false),
        ] {
            assert_eq!(is_ordinary_rust_identifier(value), expected, "{value:?}");
        }
    }
}
