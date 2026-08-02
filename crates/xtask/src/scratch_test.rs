use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Residue(PathBuf);

impl Drop for Residue {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Plant same-pid residue under `parent` as `{prefix}-{pid}-{n}`, then assert that
/// `allocate` draws exactly the first free index after the planted window and leaves
/// every planted directory untouched.
///
/// `allocate` must draw from the same `next` counter and use the same `{prefix}-{pid}-{n}`
/// naming. Capturing `next` immediately before the helper call proves the helper had to
/// take the `AlreadyExists` branch — otherwise a sibling could consume the whole window
/// and the first draw would already be `last + 1` without skipping.
pub(crate) fn assert_skips_same_pid_residue<T>(
    parent: &Path,
    prefix: &str,
    next: &AtomicU64,
    allocate: impl FnOnce() -> T,
    path_of: impl Fn(&T) -> &Path,
) -> T {
    fs::create_dir_all(parent).unwrap();
    let pid = std::process::id();
    let start = next.load(Ordering::Relaxed);
    let mut blocked = Vec::new();
    let budget = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(4);
    let mut last = start;
    for step in 0..4096u64 {
        last = start + step;
        let path = parent.join(format!("{prefix}-{pid}-{last}"));
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("plant residue: {error}"),
        }
        fs::write(path.join("stale"), b"adopt-me").unwrap();
        blocked.push(Residue(path));
        if last > next.load(Ordering::Relaxed) + budget {
            break;
        }
        assert!(
            step + 1 < 4096,
            "planting hit cap without covering counter+sibling"
        );
    }
    let before = next.load(Ordering::Relaxed);
    assert!(
        before <= last,
        "siblings consumed past the planted window before allocate \
         (before={before}, last={last}); skip would be vacuous"
    );
    let got = allocate();
    let name = path_of(&got).file_name().unwrap().to_string_lossy();
    let drawn: u64 = name.rsplit('-').next().unwrap().parse().unwrap();
    assert_eq!(drawn, last + 1, "must draw last-planted+1; got {name}");
    for planted in &blocked {
        assert_eq!(
            fs::read(planted.0.join("stale")).unwrap(),
            b"adopt-me",
            "must leave same-pid residue untouched"
        );
    }
    got
}
