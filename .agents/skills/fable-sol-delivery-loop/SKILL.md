---
name: fable-sol-delivery-loop
description: Orchestrate implementation work through an independent Fable xhigh task specification, a Sol High coding subagent, an independent Fable High review, Sol feedback repair, validation, tracker reconciliation, and merge. Use when Codex must ship one or more repository tasks with strong phase gates, isolated worktrees, explicit reviewer independence, controlled parallelism, and evidence-backed completion.
---

# Fable–Sol Delivery Loop

Ship each task through separate planning, implementation, review, repair, and operator gates. Keep product decisions with the authoritative repository and user; use agents to execute and challenge them.

## Establish authority and order

1. Read every applicable `AGENTS.md` completely.
2. Sync read-only external state and inspect the current base, task, linked specs, reviews, checks, and related tracker items.
3. Derive the task dependency order. Parallelize only independent tasks and never exceed the user's concurrency cap.
4. Create one clean branch and worktree per task from the correct committed base. Never let two coding agents edit the same worktree.
5. Keep the primary agent responsible for scope, decisions, GitHub writes, merge order, and final verification.

## Run the task loop

### Fable availability fallback

At every phase assigned to Fable, try Fable first, even if an earlier phase hit a weekly, session, or usage limit. If that attempt explicitly reports such a limit, use a fresh Sol xhigh subagent for that phase instead. Preserve reviewer independence: a Sol fallback reviewer must not be the implementation worker. Do not assume that a prior limit still applies, and do not treat an ordinary timeout or tool failure as proof that Fable usage is exhausted.

### 1. Ask Fable to specify

Run Fable at xhigh effort with full repository and network access. Keep this pass advisory-only.

```text
claude --model fable --effort xhigh --dangerously-skip-permissions --print
```

Require Fable to:

- inspect the current task, base, accepted specs, relevant docs, sibling tasks, and tracker state;
- preserve accepted decisions and identify genuine unresolved decisions rather than inventing them;
- define exact scope boundaries, files or interfaces, acceptance evidence, and task/PR splits;
- honor repository budgets and sequencing constraints;
- end with a GO or NO-GO verdict;
- make no edits or external writes.

Persist or explicitly resend the returned directive. Do not assume tool output is inherited by a subagent.

### 2. Ask Sol High to implement

When running inside Codex with native subagents available, use a native Codex subagent for the Sol role. Count it against the concurrency cap; do not launch a nested Codex CLI merely to obtain Sol. Otherwise, use the available `gpt-5.6-sol` Codex worker at high reasoning effort.

Give one Sol High coding subagent the complete Fable directive and the exact worktree path. Require it to:

- read repository instructions itself;
- edit only that worktree using the required editing mechanism;
- stay inside task scope and preserve unrelated changes;
- run proportionate positive and negative acceptance checks;
- report the exact diff or review-budget size and any divergence;
- avoid commits, pushes, issues, PRs, merges, and other external writes.

If the task cannot fit a repository limit honestly, split at a coherent seam. Never delete required tests or diagnostics merely to fit.

### 3. Perform the pre-review operator audit

Inspect every changed and untracked file, the complete diff, status, and budget. Re-run the main acceptance command when useful. Do not substitute this audit for independent review.

### 4. Ask Fable to review

Run a fresh Fable pass at high effort with full access, again advisory-only.

```text
claude --model fable --effort high --dangerously-skip-permissions --print
```

Require exact actionable findings with severity, file and line, failure scenario, and minimal fix. Ask it to verify scope, tests, error paths, budget, and the task's acceptance contract. It must not edit.

### 5. Apply review feedback through Sol

Evaluate findings against the user decisions and repository authority. Send accepted findings back to the implementation agent, with exact expected behavior. Do not silently adopt reviewer proposals that introduce undecided product policy.

Have Sol apply the fixes and rerun relevant checks. Then independently inspect the fix and rerun the final gate. Request another Fable review when changes are material or the previous verdict was not mergeable.

### 6. Ship through the operator gate

Before committing or merging:

1. Stage exactly the intended files when checks depend on tracked-file state.
2. Run the repository's canonical validation, diff checks, line-budget check, and required negative cases.
3. Confirm the branch is based on current upstream state.
4. Commit, push, and open the PR only when authorized.
5. Record scope, acceptance evidence, budget, review outcome, and any explicit deferral in the PR.
6. Wait for required GitHub checks and reviews.
7. Reconcile the PR against its linked issue, current base, review threads, and every other open issue whose premise changed. Update affected tracker items before merge or closure.
8. Merge only when all gates are clean. Verify the merge and issue state, then refresh the next task from the new base.

## Parallel execution rules

- Parallelize Fable planning or implementation only when dependency evidence says the tasks are independent.
- Preserve one worktree and one accountable implementation agent per task.
- Serialize merges in dependency order even when implementation ran concurrently.
- After each merge, notify or rebase affected work and revalidate it against the new base.
- Reserve enough concurrency for the primary agent to coordinate; treat the user's cap as absolute.

## Completion standard

Treat completion as unproven until current evidence covers every task, artifact, acceptance command, review, tracker update, PR, and merge named by the objective. A plausible implementation, an agent report, or a green narrow test is not sufficient evidence for a broad completion claim.
