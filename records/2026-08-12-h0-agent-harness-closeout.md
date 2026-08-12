# 2026-08-12 — H0 agent harness closeout

## Outcome

The minimum Pi-like H0 harness is complete on merged `main` commit
`349d518aa83b8446e44f03b5d7147c7e976258dc`. A production JSONL binary composes the
model-completion, tool-runner, session-store, and agent-loop boxes locally and exposes only
the generated agent-loop handle. It supports correlated `run_turn` and `compact`, bounded
records and output, process-lifetime request IDs, deadlines, active cancellation, idle
shutdown, deterministic persisted replay, and checked-in static repository context.

The final live proof ran Grok 4.5 in a detached clean worktree with one allowed `write` tool.
Request `h0-live-6` created only `h0_live_dogfood.rs`, made exactly one correlated write, and
returned success with usage `(input=5520, output=321, total=6459)`. The session contains the
exact ordered events `user`, `tool_call`, `tool_result`, and `assistant`.

## Delivered chain

- PRs #640 and #641 delivered the JSONL foundation and complete harness lifecycle.
- PR #645 compile-time embedded root `AGENTS.md` and the Boxology skill and removed
  caller-controlled system prompts.
- PR #646 moved blocking xAI transport work off the current-thread Tokio runtime.
- PR #647 accepted inclusive provider totals containing unexposed reasoning tokens while
  retaining overflow and undercount rejection.
- PR #648 aligned xAI tool policy with H0: required singleton first call, no parallel calls,
  and no second tool call after a result.
- PRs #649 and #650 normalized xAI's empty and nonempty tool-call preambles at the provider
  boundary without weakening final-text validation.

## Exact live and deterministic evidence

The successful supervised run `h0-live-dogfood-576-sixth` used a fresh session, a 150-second
deadline, `max_output_tokens = 1024`, and credentials supplied only through the child
environment. The key was not placed in arguments, logs, or repository files. Before and after
the run, tracked and staged diffs were empty; the only untracked path was the requested file.

The generated file passed its three model-authored Rust 2024 unit tests. A separately compiled
verifier passed `Rust 2026 -> rust-2026`, `a---b -> a-b`, `é -> ""`, and `A9_z -> a9-z`.

In a second pristine worktree at the same commit, cold generation reported byte-unchanged for
`model-completion`, `tool-runner`, `session-store`, and `agent-loop`. Supervised run
`h0-live-closeout-check-576` executed the complete base-aware `boxology check`; its result is
recorded in the merge evidence for this closeout.

## Dogfood-discovered friction

Five earlier attempts failed closed before an effective tool write and directly produced the
interoperability repairs above: a Tokio/blocking-client panic, inclusive reasoning-token
accounting, xAI's default automatic/parallel tool policy, empty tool-call content, and a
nonempty tool-call preamble. Each failed session and candidate worktree was inspected before
repair; none was described as a successful dogfood run.

## Explicit limitations

H0 is deliberately one sequential tool call per turn. It is not a sandbox and does not add
JSON-RPC 2.0, batching, notifications, networking/daemon mode, streaming, UI, dynamic tool
plugins, provider marketplaces, automatic compaction summaries, or recursive agents. Bash
remains an unsandboxed tool with only a root-confined initial working directory. Those features
remain deferred until an accepted consumer requires them.

With this record merged, issues #576 and #74 have no remaining H0 product or evidence work and
may close. The broader post-V0 epic #572 remains open for the installed check cutover and later
self-hosting milestones tracked by #575.
