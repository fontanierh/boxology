use super::*;
use rustix::process::{Pid, Signal, kill_process_group};
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const SHELL: &str = "/bin/bash";
const POLL: Duration = Duration::from_millis(5);
const GRACE: Duration = Duration::from_millis(100);

struct Capture {
    retained: Vec<u8>,
    bytes: u64,
    truncated: bool,
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
        if !alive(&mut anchor) {
            reap_known(&mut anchor);
            return Err(fail(ToolFailureClass::Local, "cleanup_failed", false));
        }
        let group = Pid::from_child(&anchor);
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
        let stdout = drain(child.stdout.take().expect("piped stdout"));
        let stderr = drain(child.stderr.take().expect("piped stderr"));
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
            if !alive(&mut anchor) {
                reap_known(&mut child);
                reap_known(&mut anchor);
                return Err(fail(ToolFailureClass::Local, "cleanup_failed", true));
            }
            thread::sleep(POLL);
        };
        if !cleanup_group(group, &mut child, &mut anchor) {
            return Err(fail(ToolFailureClass::Local, "cleanup_failed", true));
        }
        let stdout = stdout
            .join()
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| fail(ToolFailureClass::Local, "local_io", true))?;
        let stderr = stderr
            .join()
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| fail(ToolFailureClass::Local, "local_io", true))?;
        if let Some((class, code)) = interruption {
            return Err(fail(class, code, true));
        }
        let status = status.expect("completed without interruption");
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

    fn cwd(&self, raw: Option<&str>) -> Result<PathBuf, ToolFailure> {
        regular_dir(&self.root, false)?;
        let target = match raw {
            Some(raw) => self.resolve(raw)?.0,
            None => return Ok(self.root.clone()),
        };
        let relative = target.strip_prefix(&self.root).expect("resolved path");
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
            .expect("piped readiness")
            .read_exact(&mut readiness)
            .is_ok()
            && readiness == *b"R"
            && alive(&mut anchor);
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

fn alive(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

fn reap_known(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(Some(_)) => true,
        Ok(None) if child.kill().is_ok() => child.wait().is_ok(),
        Ok(None) => matches!(child.try_wait(), Ok(Some(_))),
        Err(_) => false,
    }
}

fn cleanup_group(group: Pid, child: &mut Child, anchor: &mut Child) -> bool {
    if !alive(anchor) || kill_process_group(group, Signal::TERM).is_err() {
        return false;
    }
    thread::sleep(GRACE);
    if !alive(anchor) || kill_process_group(group, Signal::KILL).is_err() {
        return false;
    }
    let child_reaped = child.wait().is_ok();
    let anchor_reaped = anchor.wait().is_ok();
    child_reaped && anchor_reaped
}

fn drain(mut pipe: impl Read + Send + 'static) -> JoinHandle<std::io::Result<Capture>> {
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(LIMIT);
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 8192];
        loop {
            let count = pipe.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes = bytes.saturating_add(count as u64);
            let keep = (LIMIT - retained.len()).min(count);
            retained.extend_from_slice(&buffer[..keep]);
        }
        Ok(Capture {
            truncated: bytes > retained.len() as u64,
            retained,
            bytes,
        })
    })
}
