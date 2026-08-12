use std::{fs, path::PathBuf};

const PACKAGE: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const CONTRACT: &str = include_str!("../src/contract.rs");
const PACKAGE_HASH: u64 = 8_561_730_811_314_096_507;
const LIB_HASH: u64 = 2_709_613_950_373_674_942;
const CONTRACT_HASH: u64 = 4_186_782_839_223_558_405;

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn governed_classifier_surface_is_exact() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            files.push(entry.file_name().into_string().unwrap());
        }
    }
    files.sort();
    assert_eq!(files, ["Cargo.toml"]);
    assert_eq!(hash(PACKAGE.as_bytes()), PACKAGE_HASH);
    assert_eq!(hash(LIB.as_bytes()), LIB_HASH);
    assert_eq!(hash(CONTRACT.as_bytes()), CONTRACT_HASH);
    assert_eq!(LIB.matches("mod contract;").count(), 1);
    assert_eq!(LIB.matches("pub use contract::*;").count(), 1);
    assert_eq!(CONTRACT.matches("boxology::contract! {").count(), 1);
    assert_eq!(LIB.matches("#[boxology::implementation]").count(), 1);
    assert_eq!(
        LIB.matches("include!(\"../../generated/adapter/adapter.rs\")")
            .count(),
        1
    );
}
