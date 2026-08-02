use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
pub const MAX_TREE_ENTRIES: usize = 4096;
pub const MAX_TREE_BYTES: u64 = 256 * 1024 * 1024;
const HEADER: &str = "boxology-determinism-manifest schema=1";
#[derive(Debug, Eq, PartialEq)]
pub struct ManifestRecord {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Eq, PartialEq)]
pub struct Manifest(Vec<ManifestRecord>);
impl Manifest {
    pub fn records(&self) -> &[ManifestRecord] {
        &self.0
    }
    pub fn serialize(&self) -> Vec<u8> {
        let mut text = format!("{HEADER}\n");
        for record in &self.0 {
            text.push_str(&format!(
                "{}\t{}\t{}\n",
                record.path, record.size, record.sha256
            ));
        }
        text.into_bytes()
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "manifest is not UTF-8")?;
        if !text.ends_with('\n') || text.contains('\r') || text.contains('\0') {
            return Err("manifest must be LF text ending in LF".into());
        }
        let mut lines = text.split_terminator('\n');
        let header = lines.next().ok_or("manifest has no header")?;
        if header != HEADER {
            return Err(
                if header.starts_with("boxology-determinism-manifest schema=") {
                    "unknown manifest schema".into()
                } else {
                    "malformed manifest header".into()
                },
            );
        }
        let mut records = Vec::new();
        let mut previous: Option<&str> = None;
        let mut folded = BTreeSet::new();
        for line in lines {
            if records.len() == MAX_TREE_ENTRIES {
                return Err("manifest exceeds MAX_TREE_ENTRIES".into());
            }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 3 {
                return Err("malformed manifest record".into());
            }
            let (path, size, sha256) = (fields[0], fields[1], fields[2]);
            validate_manifest_path(path)?;
            if previous.is_some_and(|value| value.as_bytes() >= path.as_bytes()) {
                return Err("manifest paths are duplicate or unsorted".into());
            }
            if !folded.insert(path.to_ascii_lowercase()) {
                return Err("manifest paths collide under ASCII case-folding".into());
            }
            if !(size == "0"
                || (!size.starts_with('0') && size.bytes().all(|byte| byte.is_ascii_digit())))
            {
                return Err("invalid manifest size".into());
            }
            let size = size.parse::<u64>().map_err(|_| "invalid manifest size")?;
            if sha256.len() != 64
                || !sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err("invalid lowercase SHA-256".into());
            }
            records.push(ManifestRecord {
                path: path.into(),
                size,
                sha256: sha256.into(),
            });
            previous = Some(path);
        }
        if records.is_empty() {
            return Err("manifest contains no records".into());
        }
        Ok(Self(records))
    }
}
pub(crate) fn diff_records<'a>(
    left: &'a Manifest,
    right: &'a Manifest,
) -> Vec<(Option<&'a ManifestRecord>, Option<&'a ManifestRecord>)> {
    let mut left = left.records().iter().peekable();
    let mut right = right.records().iter().peekable();
    let mut differences = Vec::new();
    loop {
        let pair = match (left.peek(), right.peek()) {
            (Some(a), Some(b)) if a.path == b.path => (left.next(), right.next()),
            (Some(a), Some(b)) if a.path.as_bytes() < b.path.as_bytes() => (left.next(), None),
            (Some(_), Some(_)) => (None, right.next()),
            (Some(_), None) => (left.next(), None),
            (None, Some(_)) => (None, right.next()),
            (None, None) => break,
        };
        if pair.0 != pair.1 {
            differences.push(pair);
        }
    }
    differences
}
pub(crate) fn manifest_side(record: Option<&ManifestRecord>) -> String {
    record.map_or_else(
        || "absent".into(),
        |record| format!("{}:{}", record.size, &record.sha256[..16]),
    )
}
fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\t' | b'\n' | b'\r' | 0))
}
fn validate_manifest_path(path: &str) -> Result<(), String> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() < 2 || parts.iter().any(|part| !valid_component(part)) {
        Err("invalid manifest path".into())
    } else {
        Ok(())
    }
}
/// Hash a subject output tree without following links.
///
/// Only paths, file sizes, and bytes are semantic; file permissions are deliberately excluded.
pub fn scan_tree(subject: &str, root: &Path) -> Result<Manifest, String> {
    scan_tree_with_limits(subject, root, MAX_TREE_ENTRIES, MAX_TREE_BYTES)
}
/// Scan the globally bounded `trees/<subject>/...` artifact layout.
pub fn scan_subject_trees(root: &Path) -> Result<Manifest, String> {
    let mut manifest = scan_tree("registry", root)?;
    for record in &mut manifest.0 {
        record.path = record
            .path
            .strip_prefix("registry/")
            .ok_or("invalid registered-subject path")?
            .into();
    }
    Manifest::parse(&manifest.serialize())
}
fn scan_tree_with_limits(
    subject: &str,
    root: &Path,
    max_entries: usize,
    max_bytes: u64,
) -> Result<Manifest, String> {
    if !valid_component(subject) {
        return Err("invalid subject name".into());
    }
    let metadata = fs::symlink_metadata(root).map_err(|error| format!("tree root: {error}"))?;
    if !metadata.file_type().is_dir() {
        return Err("tree root is not a directory".into());
    }
    let mut state = ScanState {
        subject,
        max_entries,
        max_bytes,
        entries: 0,
        bytes: 0,
        folded: BTreeSet::new(),
        records: Vec::new(),
    };
    walk(root, &mut Vec::new(), &mut state)?;
    state
        .records
        .sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(Manifest(state.records))
}
struct ScanState<'a> {
    subject: &'a str,
    max_entries: usize,
    max_bytes: u64,
    entries: usize,
    bytes: u64,
    folded: BTreeSet<String>,
    records: Vec<ManifestRecord>,
}
fn walk(
    directory: &Path,
    relative: &mut Vec<String>,
    state: &mut ScanState<'_>,
) -> Result<(), String> {
    let remaining = state.max_entries.saturating_sub(state.entries);
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(|error| format!("read tree: {error}"))?
        .take(remaining.saturating_add(1))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("read tree: {error}"))?;
    if entries.is_empty() {
        return Err(format!("empty directory: /{}", relative.join("/")));
    }
    entries.sort_by(|a, b| {
        a.file_name()
            .as_encoded_bytes()
            .cmp(b.file_name().as_encoded_bytes())
    });
    for entry in entries {
        state.entries += 1;
        if state.entries > state.max_entries {
            return Err(format!(
                "tree exceeds MAX_TREE_ENTRIES ({})",
                state.max_entries
            ));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "tree path is not UTF-8")?;
        if !valid_component(&name) {
            return Err("tree path contains an invalid component".into());
        }
        relative.push(name);
        let logical = format!("{}/{}", state.subject, relative.join("/"));
        if !state.folded.insert(logical.to_ascii_lowercase()) {
            return Err(format!("ASCII case-fold collision: {logical}"));
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("inspect tree entry: {error}"))?;
        if kind.is_dir() {
            walk(&entry.path(), relative, state)?;
        } else if kind.is_file() {
            let (size, sha256) = hash_file(&entry.path(), state.max_bytes - state.bytes)?;
            state.bytes += size;
            state.records.push(ManifestRecord {
                path: logical,
                size,
                sha256,
            });
        } else {
            return Err(format!("unsupported tree entry: {logical}"));
        }
        relative.pop();
    }
    Ok(())
}
pub(crate) fn hash_file(path: &Path, remaining: u64) -> Result<(u64, String), String> {
    let mut file = File::open(path).map_err(|error| format!("open output file: {error}"))?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read output file: {error}"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or("tree byte count overflow")?;
        if size > remaining {
            return Err("tree exceeds MAX_TREE_BYTES".into());
        }
        digest.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", digest.finalize())))
}
#[derive(Debug, Eq, PartialEq)]
pub struct ByteDiff {
    pub first_offset: usize,
    pub left_window: Vec<u8>,
    pub right_window: Vec<u8>,
    pub common_prefix_len: usize,
    pub left_len: usize,
    pub right_len: usize,
}
pub fn byte_diff(left: &[u8], right: &[u8]) -> Option<ByteDiff> {
    let offset = left
        .iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or(left.len().min(right.len()));
    (left != right).then(|| ByteDiff {
        first_offset: offset,
        left_window: left[offset..left.len().min(offset + 16)].to_vec(),
        right_window: right[offset..right.len().min(offset + 16)].to_vec(),
        common_prefix_len: offset,
        left_len: left.len(),
        right_len: right.len(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Un-namespaced {pid}-{n} paths are shared by every Temp::new caller; gate
    // creation so the residue test can plant without racing sibling tests.
    static TEMP_GATE: Mutex<()> = Mutex::new(());
    fn sibling_budget() -> u64 {
        std::thread::available_parallelism()
            .map(|n| n.get() as u64)
            .unwrap_or(4)
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let _gate = TEMP_GATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let parent =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/determinism-tests");
            fs::create_dir_all(&parent).unwrap();
            let path = loop {
                let candidate = parent.join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create temp: {error}"),
                }
            };
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn temp_skips_same_pid_residue() {
        let pid = std::process::id();
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/determinism-tests");
        fs::create_dir_all(&parent).unwrap();
        // Hold the gate only while planting; drop it before Temp::new so the
        // test exercises the real call site without re-entrant deadlock.
        let blocked = {
            let _gate = TEMP_GATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let start = NEXT.load(Ordering::Relaxed);
            let mut blocked = Vec::new();
            // Size from the live counter, plus a sibling budget for Temp::new
            // calls that can sneak between dropping the gate and our allocate.
            let budget = sibling_budget();
            loop {
                let n = start + blocked.len() as u64;
                let path = parent.join(format!("{pid}-{n}"));
                let _ = fs::create_dir(&path);
                fs::write(path.join("stale"), b"adopt-me").unwrap();
                blocked.push(Temp(path));
                if n > NEXT.load(Ordering::Relaxed) + budget {
                    break;
                }
            }
            blocked
        };
        let before = NEXT.load(Ordering::Relaxed);
        let got = Temp::new();
        let after = NEXT.load(Ordering::Relaxed);
        assert!(
            after > before + 1,
            "Temp::new must iterate past residue (NEXT {before} -> {after}), not adopt on first try"
        );
        assert!(
            !blocked.iter().any(|temp| temp.0 == got.0),
            "Temp::new must skip same-pid residue, not adopt it; got {}",
            got.0.display()
        );
        for temp in &blocked {
            assert_eq!(
                fs::read(temp.0.join("stale")).unwrap(),
                b"adopt-me",
                "Temp::new must leave same-pid residue untouched"
            );
        }
    }
    fn line(path: &str, size: &str, sha: &str) -> Vec<u8> {
        format!("{HEADER}\n{path}\t{size}\t{sha}\n").into_bytes()
    }
    fn zero() -> &'static str {
        "0000000000000000000000000000000000000000000000000000000000000000"
    }
    #[test]
    fn tree_roundtrip_is_stable_byte_sorted_and_hashes_known_bytes() {
        let temp = Temp::new();
        // `a.txt` and the directory `a/` conflict on a byte prefix: the walk emits
        // `a/z` first (`a` < `a.txt` as a name) while bytewise `a.txt` (0x2e) sorts
        // before `a/z` (0x2f). Only the top-level sort repairs that order.
        fs::create_dir(temp.0.join("a")).unwrap();
        fs::write(temp.0.join("a/z"), b"").unwrap();
        fs::write(temp.0.join("a.txt"), b"").unwrap();
        fs::write(temp.0.join("é"), b"").unwrap();
        fs::write(temp.0.join("z"), b"abc").unwrap();
        let manifest = scan_tree("clean", &temp.0).unwrap();
        let paths: Vec<_> = manifest.records().iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["clean/a.txt", "clean/a/z", "clean/z", "clean/é"]);
        assert_eq!(scan_tree("clean", &temp.0).unwrap(), manifest);
        assert_eq!(Manifest::parse(&manifest.serialize()).unwrap(), manifest);
        assert_eq!(
            manifest.records()[2].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn parser_rejects_bad_schema_records_order_names_sizes_and_hashes() {
        // A future schema and a corrupt header are separate diagnoses; `is_err`
        // alone cannot tell which of the two arms produced the error.
        assert_eq!(
            Manifest::parse(b"boxology-determinism-manifest schema=2\n"),
            Err("unknown manifest schema".into())
        );
        assert_eq!(
            Manifest::parse(b"bad\n"),
            Err("malformed manifest header".into())
        );
        assert!(Manifest::parse(format!("{HEADER}\n").as_bytes()).is_err());
        assert!(Manifest::parse(format!("{HEADER}\ns/a\t0\n").as_bytes()).is_err());
        let a = line("s/a", "0", zero());
        let duplicate = [a.as_slice(), &a[HEADER.len() + 1..]].concat();
        assert!(Manifest::parse(&duplicate).is_err());
        let unsorted = format!("{HEADER}\ns/b\t0\t{}\ns/a\t0\t{}\n", zero(), zero());
        assert!(Manifest::parse(unsorted.as_bytes()).is_err());
        let collision = format!("{HEADER}\ns/A\t0\t{}\ns/a\t0\t{}\n", zero(), zero());
        assert!(Manifest::parse(collision.as_bytes()).is_err());
        for bad in ["s/.", "s/..", "s/a\rb", "s/a\0b"] {
            assert!(Manifest::parse(&line(bad, "0", zero())).is_err());
        }
        for bad in ["", "00", "-1", "+1"] {
            assert!(Manifest::parse(&line("s/a", bad, zero())).is_err());
        }
        for bad in ["A".repeat(64), "g".repeat(64), "0".into()] {
            assert!(Manifest::parse(&line("s/a", "0", &bad)).is_err());
        }
        let mut crowded = format!("{HEADER}\n");
        for n in 0..=MAX_TREE_ENTRIES {
            crowded.push_str(&format!("s/{n:04}\t0\t{}\n", zero()));
        }
        assert!(
            Manifest::parse(crowded.as_bytes())
                .unwrap_err()
                .contains("MAX_TREE_ENTRIES")
        );
    }
    #[test]
    fn tree_rejects_empty_invalid_links_special_files_and_small_caps() {
        let empty = Temp::new();
        assert!(
            scan_tree("s", &empty.0)
                .unwrap_err()
                .contains("empty directory")
        );
        let names = Temp::new();
        fs::write(names.0.join("bad\tname"), b"x").unwrap();
        assert!(scan_tree("s", &names.0).is_err());
        let caps = Temp::new();
        fs::create_dir(caps.0.join("a")).unwrap();
        fs::write(caps.0.join("a/file"), b"").unwrap();
        fs::write(caps.0.join("z"), b"abc").unwrap();
        assert!(
            scan_tree_with_limits("s", &caps.0, 99, 2)
                .unwrap_err()
                .contains("MAX_TREE_BYTES")
        );
        assert!(
            scan_tree_with_limits("s", &caps.0, 2, 99)
                .unwrap_err()
                .contains("MAX_TREE_ENTRIES")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = Temp::new();
            fs::write(link.0.join("file"), b"x").unwrap();
            symlink(link.0.join("file"), link.0.join("link")).unwrap();
            assert!(scan_tree("s", &link.0).unwrap_err().contains("unsupported"));
            let fifo = Temp::new();
            assert!(
                Command::new("mkfifo")
                    .arg(fifo.0.join("pipe"))
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(scan_tree("s", &fifo.0).unwrap_err().contains("unsupported"));
        }
    }
    #[test]
    fn byte_diff_reports_content_and_length_only_changes() {
        assert_eq!(byte_diff(b"same", b"same"), None);
        let content = byte_diff(b"abcde", b"abXde").unwrap();
        assert_eq!((content.first_offset, content.common_prefix_len), (2, 2));
        assert_eq!(
            (content.left_window, content.right_window),
            (b"cde".to_vec(), b"Xde".to_vec())
        );
        let length = byte_diff(b"abc", b"abcde").unwrap();
        assert_eq!(
            (length.first_offset, length.left_len, length.right_len),
            (3, 3, 5)
        );
        assert!(length.left_window.is_empty());
        assert_eq!(length.right_window, b"de");
    }
}
