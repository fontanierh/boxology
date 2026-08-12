use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct ChildGuard(Option<std::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn production_binary_assembles_four_local_boxes_exposes_both_and_stops_idle() {
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
    let child = Command::new(env!("CARGO_BIN_EXE_boxology-agent-harness"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .env("BOXOLOGY_XAI_MODEL", "grok-test")
        .env("BOXOLOGY_XAI_API_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(Some(child));
    let child_ref = child.0.as_mut().unwrap();
    let mut stdin = child_ref.stdin.take().unwrap();
    let stdout = child_ref.stdout.take().unwrap();
    writeln!(stdin,r#"{{"schema":"boxology.agent-harness@1","id":"ready","method":"compact","params":{{"session_id":"s","checkpoint_id":"c","summary":"summary"}}}}"#).unwrap();
    stdin.flush().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut ready = String::new();
        let result = BufReader::new(stdout).read_line(&mut ready).map(|_| ready);
        let _ = ready_tx.send(result);
    });
    let ready = match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => result.unwrap(),
        Err(error) => {
            if let Some(process) = child.0.as_mut() {
                let _ = process.kill();
                let _ = process.wait();
            }
            child.0.take();
            reader.join().unwrap();
            panic!("readiness response timed out: {error}")
        }
    };
    reader.join().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ready).unwrap()["id"],
        "ready"
    );
    let pid = child.0.as_mut().unwrap().id();
    assert_ne!(pid, std::process::id());
    assert!(child.0.as_mut().unwrap().try_wait().unwrap().is_none());
    assert!(
        Command::new("/bin/kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let stopped = loop {
        if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "child did not stop after SIGINT");
        std::thread::sleep(Duration::from_millis(10))
    };
    child.0.take();
    assert!(stopped.success());
    fs::remove_dir_all(base).unwrap();
}
