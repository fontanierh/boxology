# Delivery-worker process-group ownership

`supervise.sh` is the opt-in, fail-closed launcher for Boxology delivery-loop
implementation, repair, and validation commands on the project Mac. It is
separate from `ops/process-reaper`: that tool considers old individual review
processes, while this one owns a process group created for one named run.

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

The launcher gates the command until it has created a dedicated process-group
guardian and atomically written a private record beneath
`/Users/jim/.codex/boxology-delivery-worker/runs` (`0700` directory, `0600`
records). The record contains the run, phase, harness, guardian PID/PGID,
session, UID, start fingerprint, canonical worktree/cwd, reciprocal Git
worktree identity, HEAD, lifecycle stage, exact member fingerprints, and any
observed Cargo-lock device/inode/path/mode. It never records argv, environment,
prompts, or credentials.

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
Immediately before TERM and KILL the tool revalidates worktree identity,
guardian/member birth fingerprints, session, ancestry, cwd, and group
membership. TERM gets a bounded grace period; KILL applies only to surviving
pre-TERM fingerprints. `term_prepared` and `term_sent` records are resumable.
Ambiguity, stale identity, tool failure, or a changed member retains state and
signals nothing further. Never replace a refusal with `kill`, `pkill`, or
`killall`; inspect the retained evidence and fix the proof failure.

Before TERM the reaper observes only the four exact Cargo lock objects opened
by verified owned members: `.package-cache`, `.cargo-build-lock`,
`.cargo-artifact-lock`, and `.cargo-lock`. After cleanup it rechecks the same
device/inode/path and emits one sanitized result:

- `owned_lock=released`
- `owned_lock=released shared_lock=held_by_other`
- `owned_lock=not_observed`
- `owned_lock=unknown`

An unrelated current holder is reported and never signaled. Full paths remain
only in the private record; telemetry contains labels, numeric process IDs,
actions, reasons, and the classification above.

## Limits and rollback

This is a macOS/Bash 3.2 operator primitive, not a daemon. It cannot recover a
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
