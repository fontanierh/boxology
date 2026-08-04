---
name: repository-delivery-loop
description: Orchestrate repository implementation through independently configured specification, implementation, review, and repair workers, followed by validation, tracker reconciliation, and merge. Use when a coordinator must ship one or more repository tasks with strong phase gates, isolated worktrees, explicit reviewer independence, controlled parallelism, and evidence-backed completion.
---

# Repository Delivery Loop

Ship each task through separate specification, implementation, review, repair, and operator gates. Keep product decisions with the authoritative repository and user; use configured workers to execute and challenge them.

## Establish authority and order

1. Read every applicable `AGENTS.md` completely.
2. Sync read-only external state and inspect the current base, task, linked specs, reviews, checks, and related tracker items.
3. Derive the task dependency order. Parallelize only independent tasks and never exceed the user's concurrency cap.
4. Create one clean branch and worktree per task from the correct committed base. Never let two coding workers edit the same worktree.
5. Keep the primary agent responsible for scope, decisions, GitHub writes, merge order, and final verification.

## Load the worker configuration

Before starting a task, read `models.toml` from the directory containing this skill. Treat it as repository policy, not as executable input.

Require `schema = 1` and configured `spec`, `implement`, and `review` roles. A role must declare a non-empty ordered `candidates` list or inherit one other role with `use`. Reject unknown fallback conditions, missing fields, inheritance cycles, and roles that declare both `candidates` and `use`.

Each candidate names a `harness`, `model`, and `effort`. Treat all three as exact requirements. Do not execute commands, flags, or other arbitrary values from the configuration.

### Select a candidate

The first candidate is the default. Select a later candidate only when the user explicitly requests its harness or names that exact configured candidate. An explicit selection is not fallback: do not mutate, reorder, or persist changes to `models.toml`.

Selection may be task-wide or limited to named roles. Repair inherits the implementation selection when `repair` uses `implement`. If an explicitly selected candidate is unavailable, unauthenticated, or cannot honor its exact model and effort, stop and report the failure; never silently fall back.

Automatic fallback is allowed only when the role explicitly configures `fallback_on` and the observed condition matches it.

### Choose the execution surface

Determine which harness hosts the primary agent before launching a worker.

- If a candidate names the active harness, launch it through that harness's native sub-agent mechanism.
- If a candidate names another harness, launch it through that harness's installed CLI.
- Never launch the active harness through its CLI merely to obtain another worker.
- Never represent a worker from one harness as a worker from another.
- Count native and CLI-launched workers equally against the concurrency cap.

For example, a Claude primary launches a Claude candidate as a native Claude sub-agent and a Kimi or Pi candidate through that harness's CLI. A primary hosted elsewhere does the inverse and launches the Claude candidate through the Claude CLI.

Use the configured model and effort. If the active harness's native mechanism cannot honor them, do not switch to that same harness's CLI. Treat the candidate as unavailable and follow the selection and fallback rules above. Launch an external worker through the CLI's supported model and effort selection; if its CLI is absent or cannot honor the requested values, treat that candidate as unavailable.

External CLI workers do not inherit the primary agent's conversation, repository instructions, worktree, or prior worker output. Give them the complete directive and exact worktree path explicitly. Use any process-management mechanism required by applicable repository instructions.

A configured model may also be served by an unrelated harness. Never take that shortcut: route each candidate through the harness its configuration names, so the worker, its account, and its evidence stay attributable.

Pass configuration to every external CLI as direct process environment and argv values, never by shell-constructing configuration values. Launch each with the exact assigned worktree as its working directory and capture both stdout and stderr. Worker API keys live in `/Users/jim/.config/boxology-delivery-loop/credentials.env` (mode `600`, outside the repository); load them into the process environment and keep them out of argv, prompts, and logs.

#### `harness = "kimi"`

Read [`references/kimi-code.md`](references/kimi-code.md) first.

- environment: `KIMI_CODE_NO_AUTO_UPDATE=1`, `KIMI_DISABLE_CRON=1`, and `KIMI_MODEL_THINKING_EFFORT=<configured effort>`;
- argv: `/Users/jim/.kimi-code/bin/kimi`, `-m`, `kimi-code/k3`, `-p`, `<complete worker directive>`.

Do not add `--auto` or `--yolo` to `-p`: prompt mode is already unattended and the current CLI rejects that combination. In every Kimi directive, forbid subagents, schedules, background work, `--add-dir`, and leaving the assigned worktree.

#### `harness = "pi"`

Read [`references/pi-cli.md`](references/pi-cli.md) first.

- environment: `XAI_API_KEY=<key>` and `PI_TELEMETRY=0`;
- argv: `/opt/homebrew/bin/pi`, `-p`, `--no-session`, `--approve`, `--model`, `<configured model>`, `--thinking`, `<configured effort>`, `<complete worker directive>`.

Pi receives reasoning effort separately through `--thinking`, so pass the configured value exactly. `--no-session` makes every worker an independent ephemeral process, and the assigned worktree is fixed by the process working directory rather than an argv workspace flag. Never pass the API key through `--api-key`. In every Pi directive, forbid `--continue`, `--resume`, `--session`, `--fork`, background work, nested agents, and leaving the assigned worktree.

#### `harness = "claude"`

A Claude primary launches this candidate as a native Claude sub-agent, which inherits the session's model and effort. Confirm the session actually runs the configured model and effort before relying on it; if it does not, treat the candidate as unavailable rather than reaching for the CLI. A non-Claude primary launches it as a CLI process: `claude`, `-p`, `--model`, `<configured model>`, `--effort`, `<configured effort>`, with the worktree as the working directory.

#### Every advisory role

Advisory specification and review directives must forbid edits and require a worktree-status audit for unexpected mutation. Launch review in a fresh process and session.

### Apply fallback policy

Without an explicit selection, resolve every phase independently from the first candidate. A fallback used during one phase does not change candidate ordering for a later phase. Do not apply this fallback policy to an explicitly selected candidate.

Fallback is allowed only when the observed condition appears in the role's `fallback_on` list:

- `model_unavailable`: the harness explicitly reports that the requested model or effort is unknown, unsupported, or unavailable, or the required external CLI is not installed;
- `usage_exhausted`: the harness explicitly reports a weekly, session, account, or other usage limit.

An ordinary timeout, transport failure, tool failure, worker error, rejected implementation, review finding, or `NO-GO` verdict is not a fallback condition. Stop and report it unless repository policy defines another recovery path.

If no candidate remains, stop before the phase and report every attempted candidate and the evidence that allowed or prevented fallback. Record the selected harness, model, effort, execution surface (`native` or `cli`), and any fallback reason in the task's delivery evidence.

## Run the task loop

### 1. Ask the specification worker to specify

Resolve the `spec` role. Run the selected worker with full repository and network access, but keep this pass advisory-only.

Require the specification worker to:

- inspect the current task, base, accepted specs, relevant docs, sibling tasks, and tracker state;
- preserve accepted decisions and identify genuine unresolved decisions rather than inventing them;
- define exact scope boundaries, files or interfaces, acceptance evidence, and task/PR splits;
- honor repository budgets and sequencing constraints;
- end with a `GO` or `NO-GO` verdict;
- make no edits or external writes.

Persist or explicitly resend the returned directive. Do not assume one worker's output is inherited by another.

### 2. Ask the implementation worker to implement

Resolve the `implement` role. Give one coding worker the complete specification directive and the exact worktree path.

Require the implementation worker to:

- read repository instructions itself;
- edit only that worktree using the required editing mechanism;
- stay inside task scope and preserve unrelated changes;
- run proportionate positive and negative acceptance checks;
- report the exact diff or review-budget size and any divergence;
- avoid commits, pushes, issues, PRs, merges, and other external writes.

If the task cannot fit a repository limit honestly, split it at a coherent seam. Never delete required tests or diagnostics merely to fit.

### 3. Perform the pre-review operator audit

Inspect every changed and untracked file, the complete diff, status, and budget. Re-run the main acceptance command when useful. Do not substitute this audit for independent review.

### 4. Ask the review worker to review

Resolve the `review` role independently and launch a fresh worker or session. Never reuse the implementation worker, including when review selects the same harness or model.

Keep the review advisory-only. Require exact actionable findings with severity, file and line, failure scenario, and minimal fix. Ask the reviewer to verify scope, tests, error paths, budget, and the task's acceptance contract. It must not edit.

### 5. Apply review feedback through the repair worker

Evaluate findings against user decisions and repository authority. Do not silently adopt reviewer proposals that introduce undecided product policy.

If `repair` inherits `implement`, return accepted findings to the original implementation worker when it remains available. Otherwise resolve the repair role and give the replacement worker the complete original directive, current diff, review findings, and expected behavior.

Have the repair worker apply accepted fixes and rerun relevant checks. Then independently inspect the fix and rerun the final gate. Request another fresh review when changes are material or the previous verdict was not mergeable.

### 6. Ship through the operator gate

Before committing or merging:

1. Stage exactly the intended files when checks depend on tracked-file state.
2. Run the repository's canonical validation, diff checks, line-budget check, and required negative cases.
3. Confirm the branch is based on current upstream state.
4. Commit, push, and open the PR only when authorized.
5. Record scope, acceptance evidence, budget, selected workers, fallback outcomes, review outcome, and any explicit deferral in the PR.
6. Wait for required GitHub checks and reviews.
7. Reconcile the PR against its linked issue, current base, review threads, and every other open issue whose premise changed. Update affected tracker items before merge or closure.
8. Merge only when all gates are clean. Verify the merge and issue state, then refresh the next task from the new base.

## Parallel execution rules

- Parallelize specification or implementation only when dependency evidence says the tasks are independent.
- Preserve one worktree and one accountable implementation worker per task.
- Serialize merges in dependency order even when implementation ran concurrently.
- After each merge, notify or rebase affected work and revalidate it against the new base.
- Reserve enough concurrency for the primary agent to coordinate; treat the user's cap as absolute.

## Completion standard

Treat completion as unproven until current evidence covers every task, artifact, acceptance command, review, tracker update, PR, and merge named by the objective. A plausible implementation, a worker report, or a green narrow test is not sufficient evidence for a broad completion claim.
