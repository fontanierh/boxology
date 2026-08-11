use super::*;
#[rustfmt::skip]
use boxology_contract::{BoxId, CallContext, Caller, CancelToken, CapabilityId, ErasedCallError, ErasedCallTarget, ExposureLevel, SlotValue, TraceContext};
#[rustfmt::skip]
use boxology_runtime::{Composition, CompositionBuilder, TransportExposure, test_support::StubTransport};
#[rustfmt::skip]
use std::{future::Future, path::PathBuf, pin::{Pin, pin}, sync::Arc, task::{Context, Poll, Waker}, time::{Instant, SystemTime, UNIX_EPOCH}};

#[rustfmt::skip]
struct Fixture { home: PathBuf, root: PathBuf }
#[rustfmt::skip]
impl Fixture {
    fn new() -> Self {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let home = std::env::temp_dir().join(format!("boxology-tool-runner-{}-{suffix}", std::process::id()));
        let root = home.join("root");
        fs::create_dir_all(&root).unwrap();
        Self { root: fs::canonicalize(root).unwrap(), home }
    }
}
#[rustfmt::skip]
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.home).unwrap(); } }

#[rustfmt::skip]
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("file operation unexpectedly pending"),
    }
}
#[rustfmt::skip]
fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}
#[rustfmt::skip]
fn read(path: &str) -> ExecuteRequest { ExecuteRequest { read: Some(ReadRequest { path: path.into() }), write: None, edit: None, bash: None } }
#[rustfmt::skip]
fn write(path: &str, content: &str) -> ExecuteRequest { ExecuteRequest { read: None, write: Some(WriteRequest { path: path.into(), content: content.into() }), edit: None, bash: None } }
#[rustfmt::skip]
fn edit(path: &str, old_text: &str, new_text: &str) -> ExecuteRequest { ExecuteRequest { read: None, write: None, edit: Some(EditRequest { path: path.into(), old_text: old_text.into(), new_text: new_text.into() }), bash: None } }
#[rustfmt::skip]
fn bash(command: &str, cwd: Option<&str>, timeout_ms: Option<u64>) -> ExecuteRequest { ExecuteRequest { read: None, write: None, edit: None, bash: Some(BashRequest { command: command.into(), cwd: cwd.map(Into::into), timeout_ms }) } }

struct ExposureTarget(Vec<TransportExposure>);
#[rustfmt::skip]
impl ErasedCallTarget for ExposureTarget {
    fn call<'a>(&'a self, capability: &'a CapabilityId, context: CallContext, input: SlotValue)
        -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        self.0.iter().find(|item| item.descriptor().id() == capability)
            .expect("capability exposed").dispatch(context, input)
    }
}
#[rustfmt::skip]
fn assembled(root: PathBuf) -> (Composition, boxology_generated_contract::ToolRunnerHandle) {
    assembled_fault(root, None)
}
#[rustfmt::skip]
fn assembled_fault(root: PathBuf, fault: Option<Fault>) -> (Composition, boxology_generated_contract::ToolRunnerHandle) {
    assembled_control(root, fault, None, Vec::new())
}
#[rustfmt::skip]
fn assembled_control(root: PathBuf, fault: Option<Fault>, edit_pause: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>, environment: Vec<(std::ffi::OsString, std::ffi::OsString)>) -> (Composition, boxology_generated_contract::ToolRunnerHandle) {
    let descriptor = generated::implementation_descriptor();
    let capability = descriptor.contract().capabilities()[0].id().clone();
    let transport = Arc::new(StubTransport::new());
    let mut builder = CompositionBuilder::new();
    builder.add_box(descriptor, move |imports| {
        let mut service = ToolRunnerService::with_environment(root, environment).unwrap();
        service.fault = fault; service.edit_pause = edit_pause;
        generated::factory(service, imports)
    });
    builder.expose(BoxId::new("tool-runner").unwrap(), capability, transport.clone(), ExposureLevel::CodeOnly);
    let composition = builder.start().unwrap();
    let runtime = transport.runtime().unwrap();
    let handle = boxology_generated_contract::ToolRunnerHandle::from_erased(
        Arc::new(ExposureTarget(runtime.exposures().to_vec())));
    (composition, handle)
}
#[rustfmt::skip]
fn call(handle: &boxology_generated_contract::ToolRunnerHandle, ctx: CallContext, request: ExecuteRequest) -> ExecuteOutcome {
    let outcome = ready(handle.execute(ctx, request)).unwrap();
    assert_ne!(outcome.result.is_some(), outcome.failure.is_some());
    outcome
}
#[rustfmt::skip]
fn failure(handle: &boxology_generated_contract::ToolRunnerHandle, request: ExecuteRequest, code: &str, root: &Path) {
    let failure = call(handle, context(), request).failure
        .unwrap_or_else(|| panic!("expected failure {code}"));
    assert_eq!(failure.code, code);
    assert!(!failure.retryable);
    assert!(!failure.side_effect_possible);
    assert!(!failure.message.contains(root.to_str().unwrap()));
}

#[rustfmt::skip]
fn wait_file(path: &Path) {
    let expires = Instant::now() + Duration::from_secs(3);
    while !path.exists() { assert!(Instant::now() < expires, "fixture marker was not created"); std::thread::sleep(Duration::from_millis(5)); }
}
#[rustfmt::skip]
fn fixture_pid(path: &Path) -> rustix::process::Pid {
    wait_file(path); rustix::process::Pid::from_raw(fs::read_to_string(path).unwrap().parse().unwrap()).unwrap()
}
#[rustfmt::skip]
fn assert_gone(pid: rustix::process::Pid) {
    let expires = Instant::now() + Duration::from_secs(3);
    loop { match rustix::process::test_kill_process(pid) {
        Err(error) if error == rustix::io::Errno::SRCH => break,
        _ => { assert!(Instant::now() < expires, "fixture process survived"); std::thread::sleep(Duration::from_millis(5)); }
    }}
}

#[test]
#[rustfmt::skip]
fn generated_fake_runs_through_its_typed_handle() {
    use boxology_generated_contract::test_support::ToolRunnerFake;
    let fake = ToolRunnerFake::new().with_execute(|_, request| async move {
        let request = request.edit.unwrap();
        assert_eq!((request.path.as_str(), request.old_text.as_str(), request.new_text.as_str()),
            ("note.txt", "old", "new"));
        Ok(ExecuteOutcome { result: Some(ExecuteResult { file: Some(file(
            FileOperation::Edit, "note.txt".into(), None, 3, true,
        )), bash: None }), failure: None })
    });
    let result = call(&fake.handle(), context(), edit("note.txt", "old", "new"))
        .result.unwrap().file.unwrap();
    assert!(matches!(result.operation, FileOperation::Edit));
    assert_eq!((result.path.as_str(), result.bytes, result.changed), ("note.txt", 3, true));
}

#[test]
#[rustfmt::skip]
fn generated_fake_carries_bash_request_and_result() {
    use boxology_generated_contract::test_support::ToolRunnerFake;
    let fake = ToolRunnerFake::new().with_execute(|_, request| async move {
        let request = request.bash.unwrap();
        assert_eq!((request.command.as_str(), request.cwd.as_deref(), request.timeout_ms), ("printf ok", Some("nested"), Some(17)));
        Ok(ExecuteOutcome { result: Some(ExecuteResult { file: None, bash: Some(BashResult {
            stdout: "ok".into(), stderr: String::new(), stdout_bytes: 2, stderr_bytes: 0,
            stdout_truncated: false, stderr_truncated: false, exit_code: Some(0), signal: None,
        }) }), failure: None })
    });
    let result = call(&fake.handle(), context(), bash("printf ok", Some("nested"), Some(17)))
        .result.unwrap().bash.unwrap();
    assert_eq!((result.stdout.as_str(), result.exit_code, result.signal), ("ok", Some(0), None));
}

#[test]
#[rustfmt::skip]
fn generated_adapter_writes_reads_and_preserves_replacement_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let (_composition, handle) = assembled(fixture.root.clone());
    let written = call(&handle, context(), write("nested/note.txt", "hello world"))
        .result.unwrap().file.unwrap();
    assert!(written.changed);
    let target = fixture.root.join("nested/note.txt");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let read_back = call(&handle, context(), read("nested/note.txt")).result.unwrap().file.unwrap();
    assert_eq!((read_back.path.as_str(), read_back.content.as_deref(), read_back.bytes),
        ("nested/note.txt", Some("hello world"), 11));
    let maximum = "x".repeat(LIMIT);
    assert_eq!(call(&handle, context(), write("maximum", &maximum)).result.unwrap()
        .file.unwrap().bytes, LIMIT as u64);
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
    let edited = call(&handle, context(), edit("nested/note.txt", "world", "Boxology"))
        .result.unwrap().file.unwrap();
    assert!(matches!(edited.operation, FileOperation::Edit));
    assert_eq!((edited.path.as_str(), edited.bytes, edited.changed),
        ("nested/note.txt", 14, true));
    assert_eq!(fs::read(&target).unwrap(), b"hello Boxology");
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
    let unchanged = call(&handle, context(), write("nested/note.txt", "hello Boxology"))
        .result.unwrap().file.unwrap();
    assert!(!unchanged.changed);
    assert!(fs::read_dir(target.parent().unwrap()).unwrap()
        .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".boxology-")));
}

#[test]
#[rustfmt::skip]
fn boundaries_sizes_and_encoding_are_typed_and_sanitized() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let root_link = fixture.home.join("root-link"); symlink(&fixture.root, &root_link).unwrap();
    assert_eq!(ToolRunnerService::new(root_link).err().unwrap().code, "symlink");
    assert_eq!(ToolRunnerService::new(fixture.root.join("..").join("root"))
        .err().unwrap().code, "outside_root");
    let outside = fixture.home.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("canary"), "safe").unwrap();
    symlink(&outside, fixture.root.join("linked-dir")).unwrap();
    symlink(outside.join("canary"), fixture.root.join("linked-file")).unwrap();
    fs::create_dir(fixture.root.join("directory")).unwrap();
    fs::write(fixture.root.join("plain"), "plain").unwrap();
    fs::write(fixture.root.join("large"), vec![b'x'; LIMIT + 1]).unwrap();
    fs::write(fixture.root.join("binary"), [0xff]).unwrap();
    let (_composition, handle) = assembled(fixture.root.clone());
    for (request, code) in [
        (read(""), "path_invalid"), (read("bad\0path"), "path_invalid"),
        (read("/absolute"), "path_invalid"), (read("./dot"), "outside_root"),
        (read("../outside/canary"), "outside_root"), (read("plain/child"), "not_directory"),
        (read("linked-dir/canary"), "symlink"), (read("linked-file"), "symlink"),
        (read("missing"), "not_found"), (read("directory"), "not_file"),
        (read("large"), "file_too_large"), (read("binary"), "not_utf8"),
        (write("oversize", &"x".repeat(LIMIT + 1)), "file_too_large"),
        (edit("linked-file", "safe", "unsafe"), "symlink"),
        (edit("../outside/canary", "safe", "unsafe"), "outside_root"),
        (edit("binary", "x", "y"), "not_utf8"),
    ] { failure(&handle, request, code, &fixture.root); }
    assert_eq!(fs::read_to_string(outside.join("canary")).unwrap(), "safe");
}

#[test]
#[rustfmt::skip]
fn invalid_shapes_cancellation_and_deadline_never_mutate() {
    use boxology_contract::Deadline;
    let fixture = Fixture::new();
    let (_composition, handle) = assembled(fixture.root.clone());
    for invalid in [ExecuteRequest { read: None, write: None, edit: None, bash: None },
        ExecuteRequest { read: Some(ReadRequest { path: "x".into() }), write: Some(WriteRequest {
            path: "x".into(), content: "x".into() }), edit: None, bash: None },
        ExecuteRequest { read: None, write: Some(WriteRequest { path: "x".into(), content: "x".into() }),
            edit: Some(EditRequest { path: "x".into(), old_text: "x".into(), new_text: "y".into() }), bash: None },
        ExecuteRequest { read: Some(ReadRequest { path: "x".into() }), write: None, edit: None,
            bash: Some(BashRequest { command: "touch escaped".into(), cwd: None, timeout_ms: None }) }] {
        failure(&handle, invalid, "request_invalid", &fixture.root);
    }
    let token = CancelToken::new();
    token.cancel();
    let cancelled = CallContext::new(Caller::Anonymous, None, token, TraceContext::empty(), None);
    let outcome = call(&handle, cancelled, write("cancelled", "secret"));
    assert_eq!(outcome.failure.unwrap().code, "cancelled");
    let expired = CallContext::new(Caller::Anonymous, Some(Deadline::at(Instant::now())),
        CancelToken::new(), TraceContext::empty(), None);
    assert_eq!(call(&handle, expired, write("expired", "secret")).failure.unwrap().code,
        "deadline_exceeded");
    assert!(!fixture.root.join("cancelled").exists());
    assert!(!fixture.root.join("expired").exists());
    assert!(!fixture.root.join("escaped").exists());
}

#[test]
#[rustfmt::skip]
fn edit_rejections_and_exact_size_boundary_are_typed() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("repeated"), "one one").unwrap();
    fs::write(fixture.root.join("maximum"), format!("!{}", "x".repeat(LIMIT - 1))).unwrap();
    fs::write(fixture.root.join("grow-to-limit"), format!("!{}", "x".repeat(LIMIT - 2))).unwrap();
    let (_composition, handle) = assembled(fixture.root.clone());
    for (request, code) in [
        (edit("repeated", "", "x"), "edit_old_empty"),
        (edit("repeated", "missing", "x"), "edit_not_found"),
        (edit("repeated", "one", "x"), "edit_ambiguous"),
        (edit("repeated", "one", "one"), "edit_no_change"),
        (edit("repeated", &"x".repeat(LIMIT + 1), "y"), "edit_text_too_large"),
        (edit("repeated", "one", &"x".repeat(LIMIT + 1)), "edit_text_too_large"),
        (edit("maximum", "!", "!!"), "file_too_large"),
    ] { failure(&handle, request, code, &fixture.root); }
    let accepted = call(&handle, context(), edit("grow-to-limit", "!", "!!"))
        .result.unwrap().file.unwrap();
    assert_eq!(accepted.bytes, LIMIT as u64);
    assert_eq!(fs::metadata(fixture.root.join("grow-to-limit")).unwrap().len(), LIMIT as u64);
    assert_eq!(fs::metadata(fixture.root.join("maximum")).unwrap().len(), LIMIT as u64);
    assert_eq!(fs::read_to_string(fixture.root.join("repeated")).unwrap(), "one one");
}

#[test]
#[rustfmt::skip]
fn edit_reuses_pre_rename_fault_cleanup_and_target_preservation() {
    let fixture = Fixture::new(); let target = fixture.root.join("target");
    fs::write(&target, "original").unwrap();
    for (fault, code, side) in [(Fault::PreRenameCancel, "cancelled", false),
        (Fault::Rename, "local_io", true), (Fault::CleanupSync, "cancelled", true)] {
        let (_composition, handle) = assembled_fault(fixture.root.clone(), Some(fault));
        let failure = call(&handle, context(), edit("target", "original", "changed")).failure.unwrap();
        assert_eq!((failure.code.as_str(), failure.side_effect_possible), (code, side));
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert!(fs::read_dir(&fixture.root).unwrap().all(|entry|
            !entry.unwrap().file_name().to_string_lossy().starts_with(".boxology-")));
    }
}

#[test]
#[rustfmt::skip]
fn same_service_write_waits_for_edit_mutation() {
    let fixture = Fixture::new(); fs::write(fixture.root.join("target"), "old").unwrap();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let (_composition, handle) = assembled_control(fixture.root.clone(), None,
        Some((entered.clone(), release.clone())), Vec::new());
    let handle = Arc::new(handle); let editing = handle.clone();
    let edit_thread = std::thread::spawn(move || call(&editing, context(), edit("target", "old", "new")));
    entered.wait();
    let writing = handle.clone();
    let write_thread = std::thread::spawn(move || call(&writing, context(), write("target", "new+write")));
    release.wait();
    assert!(edit_thread.join().unwrap().result.unwrap().file.unwrap().changed);
    assert!(write_thread.join().unwrap().result.unwrap().file.unwrap().changed);
    assert_eq!(fs::read_to_string(fixture.root.join("target")).unwrap(), "new+write");
}

#[test]
#[rustfmt::skip]
fn mutation_faults_preserve_targets_cleanup_and_side_effect_truth() {
    let fixture = Fixture::new(); let target = fixture.root.join("target");
    fs::write(&target, "original").unwrap();
    for (fault, code, side) in [(Fault::StagedCancel, "cancelled", false), (Fault::CleanupSync, "cancelled", true), (Fault::PreRenameCancel, "cancelled", false), (Fault::Rename, "local_io", true)] {
        let (_composition, handle) = assembled_fault(fixture.root.clone(), Some(fault));
        let failure = call(&handle, context(), write("target", "changed")).failure.unwrap();
        assert_eq!((failure.code.as_str(), failure.side_effect_possible), (code, side));
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert!(fs::read_dir(&fixture.root).unwrap().all(|entry|
            !entry.unwrap().file_name().to_string_lossy().starts_with(".boxology-")));
    }
    let (_composition, handle) = assembled_fault(fixture.root.clone(), Some(Fault::StagedCleanup)); let failure = call(&handle, context(), write("target", "changed")).failure.unwrap();
    assert_eq!((failure.code.as_str(), failure.side_effect_possible), ("cancelled", true));
    assert!(matches!(failure.class, ToolFailureClass::Cancelled));
    let stage = fs::read_dir(&fixture.root).unwrap().find(|entry|
        entry.as_ref().unwrap().file_name().to_string_lossy().starts_with(".boxology-")).unwrap().unwrap().path();
    fs::remove_file(stage).unwrap(); assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    let (_composition, handle) = assembled_fault(fixture.root.clone(), Some(Fault::ParentSync));
    let failure = call(&handle, context(), write("created/leaf", "x")).failure.unwrap();
    assert_eq!((failure.code.as_str(), failure.side_effect_possible), ("local_io", true));
    assert!(fixture.root.join("created").is_dir()); assert!(!fixture.root.join("created/leaf").exists());
}

#[test]
#[rustfmt::skip]
fn bash_runs_in_nested_cwd_with_only_service_environment() {
    let fixture = Fixture::new(); fs::create_dir(fixture.root.join("nested")).unwrap();
    let environment = vec![("EXTRA".into(), "owned".into())];
    let (_composition, handle) = assembled_control(fixture.root.clone(), None, None, environment);
    let command = "printf '%s\n' \"$PWD\" \"$HOME\" \"$PATH\" \"$LANG\" \"$LC_ALL\" \"$TERM\" \"$EXTRA\" \"${CARGO_MANIFEST_DIR-unset}\"";
    let result = call(&handle, context(), bash(command, Some("nested"), None)).result.unwrap().bash.unwrap();
    assert_eq!(result.stdout, format!("{}\n{}\n/usr/bin:/bin:/usr/sbin:/sbin\nC\nC\ndumb\nowned\nunset\n",
        fixture.root.join("nested").display(), fixture.root.display()));
    assert_eq!((result.stderr.as_str(), result.exit_code, result.signal), ("", Some(0), None));
    assert_eq!(result.stdout_bytes, result.stdout.len() as u64);
    assert!(!result.stdout_truncated && !result.stderr_truncated);
}

#[test]
#[rustfmt::skip]
fn bash_validation_is_exact_and_does_not_execute() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(); fs::write(fixture.root.join("file"), "x").unwrap();
    symlink(&fixture.home, fixture.root.join("link")).unwrap();
    let (_composition, handle) = assembled(fixture.root.clone());
    for (request, code) in [
        (bash("", None, None), "command_invalid"), (bash("bad\0command", None, None), "command_invalid"),
        (bash(&"x".repeat(64 * 1024 + 1), None, None), "command_too_large"),
        (bash("touch marker", None, Some(0)), "timeout_invalid"),
        (bash("touch marker", None, Some(300_001)), "timeout_invalid"),
        (bash("touch marker", Some(""), None), "path_invalid"),
        (bash("touch marker", Some("missing"), None), "not_found"),
        (bash("touch marker", Some("file"), None), "not_directory"),
        (bash("touch marker", Some("link"), None), "symlink"),
    ] { failure(&handle, request, code, &fixture.root); }
    assert_eq!(ToolRunnerService::with_environment(fixture.root.clone(), vec![("BAD=KEY".into(), "x".into())]).err().unwrap().code, "environment_invalid");
    assert!(!fixture.root.join("marker").exists());
}

#[test]
#[rustfmt::skip]
fn bash_nonzero_and_bounded_dual_capture_are_normal_results() {
    let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone());
    let exit = call(&handle, context(), bash("printf out; printf err >&2; exit 7", None, None)).result.unwrap().bash.unwrap();
    assert_eq!((exit.stdout.as_str(), exit.stderr.as_str(), exit.exit_code, exit.signal), ("out", "err", Some(7), None));
    let command = "/usr/bin/yes out | /usr/bin/head -c 262145; /usr/bin/yes err | /usr/bin/head -c 262146 >&2";
    let output = call(&handle, context(), bash(command, None, None)).result.unwrap().bash.unwrap();
    assert_eq!((output.stdout_bytes, output.stderr_bytes), (262_145, 262_146));
    assert!(output.stdout_truncated && output.stderr_truncated);
    assert_eq!((output.stdout.len(), output.stderr.len()), (LIMIT, LIMIT));
    assert_eq!(output.stdout, "out\n".repeat(LIMIT / 4));
    assert_eq!(output.stderr, "err\n".repeat(LIMIT / 4));
}

#[test]
#[rustfmt::skip]
fn bash_cleans_background_process_after_success() {
    let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone());
    let result = call(&handle, context(), bash("/bin/sleep 86400 & printf %s $! > pid", None, None)).result.unwrap().bash.unwrap();
    assert_eq!((result.exit_code, result.signal), (Some(0), None));
    assert_gone(fixture_pid(&fixture.root.join("pid")));
}

#[test]
#[rustfmt::skip]
fn bash_cancellation_timeout_and_deadlines_cleanup_owned_groups() {
    use boxology_contract::Deadline;
    let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone());
    let token = CancelToken::new(); let cancel = token.clone(); let cancelling = handle.clone();
    let ctx = CallContext::new(Caller::Anonymous, None, token, TraceContext::empty(), None);
    let thread = std::thread::spawn(move || call(&cancelling, ctx, bash(
        "trap 'printf term > term' TERM; /bin/sleep 86400 & printf %s $! > cancel-pid; printf ready > ready; wait", None, None)));
    wait_file(&fixture.root.join("ready")); cancel.cancel();
    let failure = thread.join().unwrap().failure.unwrap();
    assert_eq!(failure.code, "cancelled"); assert!(failure.side_effect_possible);
    wait_file(&fixture.root.join("term")); assert_gone(fixture_pid(&fixture.root.join("cancel-pid")));

    let timeout = call(&handle, context(), bash(
        "trap '' TERM; /bin/sleep 86400 & printf %s $! > timeout-pid; wait", None, Some(20))).failure.unwrap();
    assert_eq!(timeout.code, "command_timeout"); assert!(timeout.side_effect_possible);
    assert_gone(fixture_pid(&fixture.root.join("timeout-pid")));

    let expired = CallContext::new(Caller::Anonymous, Some(Deadline::at(Instant::now())),
        CancelToken::new(), TraceContext::empty(), None);
    let failure = call(&handle, expired, bash("touch expired", None, None)).failure.unwrap();
    assert_eq!((failure.code.as_str(), failure.side_effect_possible), ("deadline_exceeded", false));
    assert!(!fixture.root.join("expired").exists());

    let earlier = CallContext::new(Caller::Anonymous,
        Some(Deadline::at(Instant::now() + Duration::from_secs(1))), CancelToken::new(), TraceContext::empty(), None);
    let failure = call(&handle, earlier, bash("trap '' TERM; /bin/sleep 86400 & wait", None, Some(3_000))).failure.unwrap();
    assert_eq!((failure.code.as_str(), failure.side_effect_possible), ("deadline_exceeded", true));
}

#[test]
#[rustfmt::skip]
fn poisoned_mutation_lock_is_sanitized() {
    let fixture = Fixture::new(); let service = ToolRunnerService::new(fixture.root.clone()).unwrap();
    let _ = std::panic::catch_unwind(|| { let _guard = service.mutation.lock().unwrap(); panic!("poison"); });
    let failure = ready(service.execute(context(), write("target", "x"))).unwrap().failure.unwrap();
    assert_eq!((failure.code.as_str(), failure.side_effect_possible), ("local_io", false));
    assert!(!failure.message.contains(fixture.root.to_str().unwrap()));
}
