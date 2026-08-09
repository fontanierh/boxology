# Owned delivery-worker process groups

All external implementation and repair workers, participating native-worker
build/test shells, and operator validation shells must run through:

```sh
ops/delivery-worker/supervise.sh run \
  --run-id TASK-PHASE --phase implement --harness HARNESS \
  --worktree /absolute/linked/worktree --cwd /absolute/linked/worktree \
  -- executable argument
```

Use a unique stable run ID per active command. Select `repair` or `validation`
when applicable. Pass the worker as argv; never put API keys in argv or prompts.
The wrapper is inert until invoked and accepts only reciprocal linked Boxology
worktrees under the configured worktree root.

On normal zero exit the wrapper proves its group empty and clears the record.
On interruption or abnormal exit, keep the record. Recovery order is exact:

```sh
ops/delivery-worker/supervise.sh status --run-id TASK-PHASE
ops/delivery-worker/supervise.sh reap --run-id TASK-PHASE --dry-run
ops/delivery-worker/supervise.sh reap --run-id TASK-PHASE
```

The dry-run sends zero signals and does not advance state. A real reap verifies
the recorded guardian/group, TERM-signals that group, waits, and KILL-signals
only surviving recorded fingerprints. It also emits a sanitized Cargo-lock
release classification. Refusal means ownership is not proven: preserve the
record for operator inspection and do not use ad hoc `kill`, `pkill`,
`killall`, process-name matching, or a negative PGID signal outside the helper.

See [`ops/delivery-worker/README.md`](../../../../ops/delivery-worker/README.md)
for the state schema, safety boundary, telemetry, recovery, tests, and limits.
