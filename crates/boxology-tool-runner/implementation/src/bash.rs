use super::*;
use rustix::process::{Pid, Signal, kill_process_group};
use std::io::Read;
use std::os::unix::io::AsFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, atomic::AtomicBool};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const SHELL: &str = "/bin/bash";
const POLL: Duration = Duration::from_millis(5);
const GRACE: Duration = Duration::from_millis(100);

#[rustfmt::skip]
struct Capture { retained: Vec<u8>, bytes: u64, truncated: bool }
#[rustfmt::skip]
struct Drain { join: JoinHandle<std::io::Result<Capture>>, stop: Arc<AtomicBool> }
#[rustfmt::skip]
impl Drain {
    fn finish(self, eof: bool) -> Option<Capture> {
        if eof { let expires = Instant::now() + GRACE * 5; while !self.join.is_finished() && Instant::now() < expires { thread::sleep(POLL); } }
        self.stop.store(true, Ordering::Relaxed); self.join.join().ok().and_then(Result::ok)
    }
}

impl ToolRunnerService {
    pub(super) fn bash(
        &self,
        context: &boxology::CallContext,
        request: BashRequest,
    ) -> Result<BashResult, ToolFailure> {
        if request.command.is_empty() || request.command.contains('\0') {
            return Err(fail(ToolFailureClass::Input, "command_invalid", false));
        }
        if request.command.len() > 64 * 1024 {
            return Err(fail(ToolFailureClass::Resource, "command_too_large", false));
        }
        let timeout = request.timeout_ms.unwrap_or(60_000);
        if !(1..=300_000).contains(&timeout) {
            return Err(fail(ToolFailureClass::Input, "timeout_invalid", false));
        }
        check(context, false)?;
        self.cwd(request.cwd.as_deref())?;
        let mut anchor = self.anchor()?;
        if let Err(failure) = check(context, false) {
            return Err(before_spawn(failure, &mut anchor));
        }
        let cwd = match self.cwd(request.cwd.as_deref()) {
            Ok(cwd) => cwd,
            Err(failure) => return Err(before_spawn(failure, &mut anchor)),
        };
        let group = Pid::from_child(&anchor);
        if !anchor_reserved(&anchor) {
            reap_known(&mut anchor);
            return Err(fail(ToolFailureClass::Local, "cleanup_failed", false));
        }
        let mut command = Command::new(SHELL);
        command
            .args(["--noprofile", "--norc", "-c", &request.command])
            .current_dir(cwd)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(group.as_raw_pid());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let cleaned = reap_known(&mut anchor);
                return Err(fail(
                    ToolFailureClass::Local,
                    if cleaned {
                        "spawn_failed"
                    } else {
                        "cleanup_failed"
                    },
                    !cleaned,
                ));
            }
        };
        #[cfg(test)]
        if matches!(
            self.fault,
            Some(Fault::BashDrainFirst | Fault::BashDrainSecond)
        ) {
            record_fault_pids(&self.root, group, &child);
        }
        let Some(stdout_pipe) = child.stdout.take() else {
            let cleaned = cleanup_group(group, &mut child, &mut anchor, false, false);
            return Err(post_spawn_failure(cleaned));
        };
        let stdout = match drain(stdout_pipe, cfg!(test) && self.drain_fault(true)) {
            Ok(stdout) => stdout,
            Err(_) => {
                let cleaned = cleanup_group(group, &mut child, &mut anchor, false, false);
                return Err(post_spawn_failure(cleaned));
            }
        };
        let Some(stderr_pipe) = child.stderr.take() else {
            let cleaned = cleanup_group(group, &mut child, &mut anchor, false, false);
            let _ = stdout.finish(false);
            return Err(post_spawn_failure(cleaned));
        };
        let stderr = match drain(stderr_pipe, cfg!(test) && self.drain_fault(false)) {
            Ok(stderr) => stderr,
            Err(_) => {
                let cleaned = cleanup_group(group, &mut child, &mut anchor, false, false);
                let _ = stdout.finish(false);
                return Err(post_spawn_failure(cleaned));
            }
        };
        let expires = Instant::now() + Duration::from_millis(timeout);
        let (status, interruption) = loop {
            let now = Instant::now();
            let interruption = if context.cancellation().is_cancelled() {
                Some((ToolFailureClass::Cancelled, "cancelled"))
            } else if context
                .deadline()
                .is_some_and(|value| value.remaining_at(now) == Duration::ZERO)
            {
                Some((ToolFailureClass::Deadline, "deadline_exceeded"))
            } else if now >= expires {
                Some((ToolFailureClass::Deadline, "command_timeout"))
            } else {
                None
            };
            if let Some(interruption) = interruption {
                break (None, Some(interruption));
            }
            match child.try_wait() {
                Ok(Some(status)) => break (Some(status), None),
                Ok(None) => {}
                Err(_) => break (None, Some((ToolFailureClass::Local, "local_io"))),
            }
            #[cfg(test)]
            if self.fault == Some(Fault::BashAnchorDeath)
                && self.root.join("anchor-death-ready").exists()
                && !self.root.join("bash-fault-anchor-pid").exists()
                && anchor.kill().is_ok()
            {
                record_fault_pids(&self.root, group, &child);
            }
            if !anchor_reserved(&anchor) {
                let _ = cleanup_group(group, &mut child, &mut anchor, false, false);
                let _ = stdout.finish(false);
                let _ = stderr.finish(false);
                return Err(fail(ToolFailureClass::Local, "cleanup_failed", true));
            }
            thread::sleep(POLL);
        };
        #[cfg(test)]
        if matches!(self.fault, Some(Fault::BashTerm | Fault::BashKill)) {
            record_fault_pids(&self.root, group, &child);
        }
        let cleaned = cleanup_group(
            group,
            &mut child,
            &mut anchor,
            cfg!(test) && self.signal_fault(Signal::TERM),
            cfg!(test) && self.signal_fault(Signal::KILL),
        );
        let eof = cleaned && status.is_some() && interruption.is_none();
        let stdout = stdout.finish(eof);
        let stderr = stderr.finish(eof);
        if !cleaned {
            return Err(fail(ToolFailureClass::Local, "cleanup_failed", true));
        }
        let stdout = stdout.ok_or_else(|| fail(ToolFailureClass::Local, "local_io", true))?;
        let stderr = stderr.ok_or_else(|| fail(ToolFailureClass::Local, "local_io", true))?;
        if let Some((class, code)) = interruption {
            return Err(fail(class, code, true));
        }
        let Some(status) = status else {
            return Err(fail(ToolFailureClass::Local, "local_io", true));
        };
        let (exit_code, signal) = (status.code(), status.signal());
        if exit_code.is_some() == signal.is_some() {
            return Err(fail(ToolFailureClass::Local, "local_io", true));
        }
        Ok(BashResult {
            stdout: String::from_utf8_lossy(&stdout.retained).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.retained).into_owned(),
            stdout_bytes: stdout.bytes,
            stderr_bytes: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
            exit_code,
            signal,
        })
    }

    #[rustfmt::skip]
    fn drain_fault(&self, first: bool) -> bool {
        #[cfg(test)] { self.fault == Some(if first { Fault::BashDrainFirst } else { Fault::BashDrainSecond }) }
        #[cfg(not(test))] { let _ = first; false }
    }

    #[rustfmt::skip]
    fn signal_fault(&self, signal: Signal) -> bool {
        #[cfg(test)] { matches!((self.fault, signal), (Some(Fault::BashTerm), Signal::TERM) | (Some(Fault::BashKill), Signal::KILL)) }
        #[cfg(not(test))] { let _ = signal; false }
    }

    fn cwd(&self, raw: Option<&str>) -> Result<PathBuf, ToolFailure> {
        regular_dir(&self.root, false)?;
        let target = match raw {
            Some(raw) => self.resolve(raw)?.0,
            None => return Ok(self.root.clone()),
        };
        let relative = target
            .strip_prefix(&self.root)
            .map_err(|_| fail(ToolFailureClass::Boundary, "outside_root", false))?;
        let mut current = self.root.clone();
        for component in relative.components() {
            current.push(component);
            regular_dir(&current, false)?;
        }
        Ok(target)
    }

    fn anchor(&self) -> Result<Child, ToolFailure> {
        let mut command = Command::new(SHELL);
        command
            .args([
                "--noprofile",
                "--norc",
                "-c",
                "trap '' TERM; printf R; exec /bin/sleep 86400",
            ])
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        let mut anchor = command
            .spawn()
            .map_err(|_| fail(ToolFailureClass::Local, "spawn_failed", false))?;
        let mut readiness = [0];
        let ready = anchor
            .stdout
            .take()
            .is_some_and(|mut pipe| pipe.read_exact(&mut readiness).is_ok())
            && readiness == *b"R"
            && fresh_anchor(&anchor);
        if ready {
            Ok(anchor)
        } else {
            let cleaned = reap_known(&mut anchor);
            Err(fail(
                ToolFailureClass::Local,
                if cleaned {
                    "spawn_failed"
                } else {
                    "cleanup_failed"
                },
                !cleaned,
            ))
        }
    }
}

fn before_spawn(mut failure: ToolFailure, anchor: &mut Child) -> ToolFailure {
    if !reap_known(anchor) {
        failure = fail(ToolFailureClass::Local, "cleanup_failed", true);
    }
    failure
}

#[rustfmt::skip]
fn post_spawn_failure(cleaned: bool) -> ToolFailure { fail(ToolFailureClass::Local, if cleaned { "local_io" } else { "cleanup_failed" }, true) }

fn reap_known(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(Some(_)) => true,
        Ok(None) if child.kill().is_ok() => child.wait().is_ok(),
        Ok(None) => matches!(child.try_wait(), Ok(Some(_))),
        Err(_) => false,
    }
}

#[rustfmt::skip]
fn cleanup_group(group: Pid, child: &mut Child, anchor: &mut Child, fail_term: bool, fail_kill: bool) -> bool {
    let mut cleaned = true;
    if anchor_reserved(anchor) {
        if fail_term || kill_process_group(group, Signal::TERM).is_err() {
            cleaned = false;
        }
        thread::sleep(GRACE);
        if anchor_reserved(anchor) {
            let killed = !fail_kill && kill_process_group(group, Signal::KILL).is_ok();
            if !killed {
                cleaned = false;
                if anchor_reserved(anchor) { let _ = kill_process_group(group, Signal::KILL); }
            }
        } else {
            cleaned = false;
        }
    } else {
        cleaned = false;
    }
    if !reap_known(child) { cleaned = false; }
    if !reap_known(anchor) { cleaned = false; }
    cleaned
}

#[rustfmt::skip]
fn fresh_anchor(anchor: &Child) -> bool { let pid = Pid::from_child(anchor); anchor_reserved(anchor) && rustix::process::getpgid(Some(pid)) == Ok(pid) }
#[rustfmt::skip]
fn anchor_reserved(anchor: &Child) -> bool { rustix::process::test_kill_process(Pid::from_child(anchor)).is_ok() }

#[rustfmt::skip]
fn drain(mut pipe: impl Read + AsFd + Send + 'static, fail_start: bool) -> std::io::Result<Drain> {
    if fail_start {
        return Err(std::io::Error::other("injected reader start failure"));
    }
    rustix::io::ioctl_fionbio(&pipe, true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let join = thread::Builder::new()
        .name("boxology-bash-drain".into())
        .spawn(move || {
            let mut retained = Vec::with_capacity(LIMIT);
            let mut bytes = 0_u64;
            let mut buffer = [0_u8; 8192];
            let mut stop_at = None; let mut eof = false;
            loop {
                if stop_at.is_none() && stopped.load(Ordering::Relaxed) { stop_at = Some(Instant::now() + GRACE); }
                if stop_at.is_some_and(|expires| Instant::now() >= expires) { break; }
                match pipe.read(&mut buffer) {
                    Ok(0) => { eof = true; break; },
                    Ok(count) => {
                        bytes = bytes.saturating_add(count as u64);
                        let keep = (LIMIT - retained.len()).min(count);
                        retained.extend_from_slice(&buffer[..keep]);
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        if stop_at.is_some() { break; }
                        thread::sleep(POLL);
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(Capture { truncated: bytes > retained.len() as u64 || !eof, retained, bytes })
        })?;
    Ok(Drain { join, stop })
}

#[cfg(test)]
#[rustfmt::skip]
fn record_fault_pids(root: &Path, group: Pid, child: &Child) { let _ = fs::write(root.join("bash-fault-anchor-pid"), group.as_raw_pid().to_string()); let _ = fs::write(root.join("bash-fault-user-pid"), child.id().to_string()); }
