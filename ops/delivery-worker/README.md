# Delivery-worker process-group ownership

`supervise.sh` is the opt-in, fail-closed launcher for Boxology delivery-loop
implementation, repair, and validation commands on the project Mac. It is
separate from `ops/process-reaper`: that tool considers old individual review
processes, while this one owns an exclusive POSIX session and process group
created for one named run.

## Interface

```sh
ops/delivery-worker/supervise.sh run \
  --run-id ISSUE-PHASE --phase implement \
  --harness codex --worktree /Users/jim/.codex/worktrees/BOX \
  --cwd /Users/jim/.codex/worktrees/BOX -- command argument

ops/delivery-worker/supervise.sh status --run-id ISSUE-PHASE
ops/delivery-worker/supervise.sh reap --run-id ISSUE-PHASE --dry-run
ops/delivery-worker/supervise.sh reap --run-id ISSUE-PHASE
```

`phase` is exactly `implement`, `repair`, or `validation`. Labels contain only
letters, digits, dot, underscore, and hyphen. The command is not shell-parsed.
The caller supplies an argv vector, and secrets stay in the worker environment.

## Ownership and state

The launcher durably records a `gated` stage, then gates the command until its
guardian has used the macOS system
Perl's `POSIX::setsid` to become both session and process-group leader. The
guardian remains alive after launcher interruption and after the command exits;
it ignores group TERM and holds exact identity/control descriptors. At launch,
the supervisor and guardian open one-way status and release pipes, then unlink
their filesystem names before the command starts. The command closes every
protocol descriptor. The guardian is the sole status writer, the supervisor is
the sole status reader and release writer, and the guardian is the sole release
reader. An exact command status is newline- and EOF-framed. Before sending its
inherited release, the supervisor durably records `release_sent`. A hard crash
before gate release leaves a proven-empty `gated` record; a crash after release
intent leaves an exactly identifiable stopped anchor. The launcher writes all
state beneath
`/Users/jim/.codex/boxology-delivery-worker/runs` (`0700` directory, `0600`
records). The record contains the run, phase, harness, guardian PID/PGID/SID,
UID, start fingerprint, unlinked descriptor identities, canonical worktree/cwd,
reciprocal Git worktree identity, HEAD, lifecycle stage, exact lock-observation
roster, completeness bit, and observed Cargo-lock device/inode/path/mode. It
never records argv, environment, prompts, credentials, or a reopenable protocol.

Only linked Boxology worktrees strictly below `/Users/jim/.codex/worktrees` are
eligible. The main checkout, review and acceptance scratch, Crab paths, CI
scratch, and unrelated worktrees are refused. A clean zero exit proves the
group empty and clears ownership without a signal. An abnormal exit retains
the record for inspection.

## Interruption and recovery

First inspect without changing state or sending a signal:

```sh
ops/delivery-worker/supervise.sh status --run-id ISSUE-PHASE
ops/delivery-worker/supervise.sh reap --run-id ISSUE-PHASE --dry-run
```

If the dry-run proves the recorded group, run `reap` without `--dry-run`.
Immediately before TERM and KILL the tool revalidates worktree identity, the
guardian's birth/cwd/private descriptors, and every live member of the recorded
POSIX SID. The exclusive session prevents an outside process from joining. An
ordinary member found outside the guardian PGID is reported as an escape and
signals nothing. While the guardian is alive, a bounded full-process ancestry
walk also catches direct descendants that call `setsid()` and reports
`session_escape`. Descendants may exit, spawn, or `exec` while the TERM roster
is prepared; a fresh proof is authoritative while the exact guardian pins the
PGID/SID.

If the supervisor disappears without sending its inherited release, pipe EOF
leaves the anchor stopped indefinitely. The reaper never reopens or writes a
control path. TERM gets a bounded grace period, after which the reaper persists
a `kill_intent`, re-proves the anchor and whole SID, and sends group KILL
including the guardian even when it is the sole member. A resumed `kill_intent`
may repeat KILL only while the same guardian birth/descriptors still prove that
SID/PGID; an absent or changed anchor never authorizes a numeric signal. Empty
intent clears, while a surviving session without its anchor retains. Likewise,
`term_prepared` may repeat TERM after full proof, whereas `term_sent` continues
without repeating it. These are at-least-once-safe recovery semantics, not an
exact-once guarantee. An empty `gated` record clears without signaling. A live
`gated` record follows normal proven cleanup because command start was released
but not durably confirmed. A live `release_sent` record is fully re-proved,
durably advances to `kill_intent`, and KILLs the stopped anchor; an empty one
clears.
Never replace a refusal with `kill`, `pkill`, or `killall`; inspect the retained
evidence and fix the proof failure.

Before TERM the reaper first completes any deterministic test barrier, then
observes only the four exact Cargo lock objects opened by one full-SID roster:
`.package-cache`, `.cargo-build-lock`,
`.cargo-artifact-lock`, and `.cargo-lock`. After cleanup it rechecks the same
device/inode/path. The roster is re-proved before TERM; bounded churn that
prevents an exact observation records `lock_complete=0` and can emit only
`owned_lock=unknown`. Stable observations emit one sanitized result:

- `owned_lock=released`
- `owned_lock=released shared_lock=held_by_other`
- `owned_lock=not_observed`
- `owned_lock=unknown`

An unrelated current holder is reported and never signaled. Full paths remain
only in the private record; telemetry contains labels, numeric process IDs,
actions, reasons, and the classification above.

This is a same-user ownership guard, not a security boundary against arbitrary
code already running as that user. Such code can directly signal or debug the
anchor. That interference may force a safe leak or refusal, but stale/recycled
identity is never used to broaden later signaling. A deliberately double-forked
or otherwise reparented same-UID daemon can sever the observable ancestry chain
after leaving the session; containing that behavior requires a real sandbox and
is explicitly outside this primitive.

## Limits and rollback

This is a macOS/Bash 3.2 operator primitive, not a daemon. A schema-1 record
created before exclusive sessions remains inspectable but is never group-signaled
while its group is live; it clears normally once empty. The tool cannot recover a
command that bypassed the wrapper, prove a lock that was never observed, or
decide an ambiguous reused identity. Removing delivery-loop adoption returns
launching to the previous behavior; retained records can be inspected and then
removed only after the operator independently proves their groups are empty.

Run the fixture suite with:

```sh
bash ops/delivery-worker/tests/run-tests.sh
```

The destructive-path stubs never signal live processes. Real signal smoke is
limited to groups created and birth-fingerprinted by that test invocation.
