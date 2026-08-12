use std::{
    fs,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn production_binary_assembles_four_local_boxes_and_exposes_run_turn() {
    let base = std::env::temp_dir().join(format!(
        "boxology-harness-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = base.join("root");
    let state = base.join("state");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&state).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let state = fs::canonicalize(state).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_boxology-agent-harness"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .env("BOXOLOGY_XAI_MODEL", "grok-test")
        .env("BOXOLOGY_XAI_API_KEY", "test-key")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    fs::remove_dir_all(base).unwrap();
}
