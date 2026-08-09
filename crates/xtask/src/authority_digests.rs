use std::{fs, path::Path};

const SOURCE_PATH: &str = "crates/boxology-http-conformance/src/lib.rs";
const AUTHORITIES: [(&str, &str, &str); 2] = [
    (
        "runtime",
        "boxology-details/03-runtime.md",
        "pub const RUNTIME_AUTHORITY_DIGEST: u64 = ",
    ),
    (
        "spec",
        "specs/s3-http-binding.md",
        "pub const SPEC_AUTHORITY_DIGEST: u64 = ",
    ),
];

pub fn check(root: &Path) -> bool {
    match validate(root) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("authority-digests: {error}");
            false
        }
    }
}

fn validate(root: &Path) -> Result<(), String> {
    let source = fs::read_to_string(root.join(SOURCE_PATH))
        .map_err(|error| format!("cannot read {SOURCE_PATH}: {error}"))?;
    for (name, authority_path, prefix) in AUTHORITIES {
        let expected = expected_digest(&source, prefix)?;
        let bytes = fs::read(root.join(authority_path))
            .map_err(|error| format!("cannot read {authority_path}: {error}"))?;
        let actual = fnv1a64(&bytes);
        if actual != expected {
            return Err(format!(
                "{name} authority digest drift: reviewed changes to {authority_path} must update {SOURCE_PATH} (expected {expected:#018x}, actual {actual:#018x})"
            ));
        }
    }
    Ok(())
}

fn expected_digest(source: &str, prefix: &str) -> Result<u64, String> {
    let value = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .and_then(|value| value.strip_suffix(';'))
        .and_then(|value| value.strip_prefix("0x"))
        .ok_or_else(|| format!("missing canonical digest declaration in {SOURCE_PATH}"))?;
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "invalid canonical digest declaration in {SOURCE_PATH}"
        ));
    }
    u64::from_str_radix(value, 16)
        .map_err(|_| format!("invalid canonical digest declaration in {SOURCE_PATH}"))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_and_declaration_parser_are_pinned() {
        assert_eq!(fnv1a64(b"hello"), 0xa430d84680aabd0b);
        assert_eq!(
            expected_digest(
                "pub const SPEC_AUTHORITY_DIGEST: u64 = 0xa430d84680aabd0b;",
                AUTHORITIES[1].2
            ),
            Ok(0xa430d84680aabd0b)
        );
        assert!(
            expected_digest("const SPEC_AUTHORITY_DIGEST: u64 = 0x1;", AUTHORITIES[1].2).is_err()
        );
    }
}
