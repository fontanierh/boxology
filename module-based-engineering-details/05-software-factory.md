# Software Factory

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the multi-agent coordination, task, merge, and continuous-analysis system discussed during the design interview.

## Purpose

The software factory is a persistent control plane for developing and maintaining a modular system. It manages many tasks and agents across time rather than running one isolated coding request from beginning to end.

The module boundary gives the factory a natural unit of ownership, analysis, review, and merge accountability. Agents can work concurrently because every submitted change has one target module and cannot directly modify another module's source.

The factory and module platform are delivered as one product while remaining separate applications internally. The factory is intended to become the first substantial application built with the module system, following a progressive bootstrap in which early development does not depend on either system being complete.

The factory runtime supplies mechanisms such as agent execution, isolated work, persistence, human interaction, and reporting. Its organization is policy. Leads, workers, reviewers, mergers, handoffs, and gates should be configurable rather than permanently hard-coded into the runtime.

## First factory slice

The governance hierarchy below is a mature direction. The first viable factory is deliberately smaller:

1. A developer talks to one persistent lead agent through Slack.
2. The lead handles the requested change itself.
3. It works in a remote, isolated code sandbox with a dedicated worktree and branch.
4. It opens a GitHub pull request and reports the result in Slack.
5. It stops at the pull request boundary. A human must review and merge.

The first slice has no required task UI, GitHub Issues workflow, worker pool, area leads, reviewer agent, merger agent, or custom factory dashboard. These roles and surfaces can be added progressively without replacing the stable human-facing lead.

The factory's agent loop is expected to be a purpose-built harness rather than an invocation of Codex CLI or Claude Code. Its internal model, provider, and tooling design has not been selected. The first slice specifies its behavior, not that implementation.

## Governance hierarchy

The hierarchy discussed was:

```text
Humans <-> top-level lead
           |-- area leads
           |-- merger
           `-- continuous quality agent
                    |
                worker agents
```

### Top-level lead

The top-level lead is the primary interface between humans and the harness. It receives authoritative human guidance and can ask humans for information, direction, review, or approval.

Its control-plane responsibilities include:

- Reorganizing areas.
- Reprioritizing work across areas.
- Publishing authoritative guidance.
- Asking humans about issues surfaced by analysis or implementation.
- Cancelling or superseding work.
- Routing strong approval requests.

The phrase "keys to the whole harness" referred to control over planning and coordination. Sensitive actions and policy changes still need explicit approval according to the configured harness policy.

### Area leads

There can be a coordinator or area lead per module. A sufficiently large module can be divided into hard-coded logical areas, each with its own lead.

An area lead maintains a broad plan for its area. Worker agents automatically receive the applicable plan when they pick up a ready task. The lead publishes tasks and assigns their relative priority within the area.

Area ownership and subdivisions are explicit configuration rather than something workers infer independently for each task.

### Worker agents

Workers pick up ready tasks and work independently. Once work begins, workers do not communicate with one another to negotiate concurrent changes. Shared context comes from the area plan, task, module contract, and current repository state.

Each worker submits a single-module merge request. Submission moves the work into a waiting state while the merger evaluates it. The durable task, sandbox, branch, evidence, and feedback remain the source of truth for resumption and rework.

### Merger

The merger is a continuously operating integration coordinator. It serializes accepted changes against the current main branch.

The ordering proposed was:

- Within an area, tasks are ordered by the priority assigned by the area lead.
- Between areas, the initial tie-break is time of arrival.
- The top-level lead can authoritatively reprioritize areas or work when human guidance or system conditions require it.

Automatic priority aging was discussed as a possible starvation control but was not adopted as a firm rule. The global lead's ability to reprioritize is the agreed control currently captured.

## Optimistic parallelism

Multiple agents may work on the same module or logical area concurrently. The system does not take an exclusive module-wide write lock before work begins.

Instead, it uses optimistic parallel development with serialized integration:

```text
area plan
-> prioritized ready tasks
-> independent parallel work
-> ordered merge requests
-> one accepted merge at a time
```

This permits more concurrency but accepts that some branches will become stale. The response to staleness is reassessment, not blind rebasing.

## Merge and reassessment loop

When a merge request reaches the front of the relevant queue, the merger checks whether it is still valid against the latest accepted state.

The signals discussed were:

1. **Git conflict:** the change cannot be applied mechanically and is returned for rework.
2. **CI or integration failure:** required checks fail on the current merge candidate and the task is returned.
3. **Target module changed:** an intervening merge touched the same module, so the area lead should assess whether semantic rework is needed even if Git can merge it.
4. **Imported contract changed:** a dependency used by the task changed and the consumer assumptions need reassessment.
5. **Area plan or human guidance changed:** the task may need revision or may have become obsolete.

After a merge, in-progress work falls into one of the states discussed:

```text
unaffected -> continue
affected   -> reassess against new main
conflicted -> revise, validate, and resubmit
obsolete   -> close
```

The worker performing rework receives the newly merged change and reasons about how the substrate or requirements changed. The merger does not treat a clean textual rebase as proof of semantic compatibility.

## Human control

Humans can push authoritative information into the harness at any time through the top-level lead. This is the mechanism for resolving ambiguous product questions, changing priorities, approving sensitive actions, or correcting the factory's understanding of the codebase.

The lead can also initiate questions when area analysis, quality findings, implementation, or deprecation evidence requires judgment that the harness should not invent.

Slack is the first human interface to the lead. It should provide clear context and strong approval requests rather than relying on implicit authority. Other interfaces may be added later.

GitHub is the first review and integration surface. The initial factory opens a pull request but never merges it autonomously. GitHub Issues may later become the task ledger when worker agents are introduced. A factory GitHub App or bot identity can act on behalf of the system while comments and factory records attribute work to a logical agent and run; the exact identity design remains open.

## Remote execution and resumability

Factory agents run in remote code sandboxes. Every sandbox is isolated and supports a lifecycle comparable to create, suspend, resume, checkpoint, and destroy. Worktree and branch isolation are required from the first version, even while only one lead agent performs work.

Stopping the factory must not discard agent state. The target behavior is to freeze and resume the complete sandbox exactly where it stopped. Recovery may return to a recent durable checkpoint and repeat a small, bounded amount of work, but losing the worktree, branch, conversation, task history, or audit record is unacceptable.

The factory itself is distributed as a container and can be stopped and resumed. Its first supported deployment recipe is a user-controlled machine reachable over SSH and capable of running containers. That machine may be a cloud VM or another remotely reachable computer. Running the container on the developer's current computer is useful for evaluation, although it is a weaker team setup because availability depends on that computer.

"Remotely hosted" does not require a vendor-operated service. A future onboarding flow may guide users toward a managed offering, common compute providers, Kubernetes, or their own hardware.

## Continuous quality agent

A perpetual quality agent analyzes the system beyond individual pull requests. It reasons about overall coherence and surfaces work that ordinary feature tasks would not necessarily create.

The responsibilities discussed included:

- Static analysis of module dependencies.
- Detecting or discouraging cycles.
- Prompting planners to produce plans that avoid cycles.
- Combining code analysis with operational evidence where useful.
- Identifying codebase directions that need human evaluation.
- Publishing targeted improvement tasks.

Cycle management was explicitly placed here and in factory planning rather than in the application runtime. The runtime remains capable of executing declared module relationships; the factory tries to prevent or mitigate problematic designs.

## Relationship to CI and review

The merger relies on module-defined and harness-required CI. Integration tests run against the current merge candidate, not merely against the branch state on which the worker originally finished.

AI reviewers can be part of those required checks. Their configuration belongs to the harness and module quality policy rather than to the runtime.

Passing CI is necessary but may not be sufficient when the target module, imported contract, or authoritative plan changed. Those cases route back through semantic reassessment.

## Matters not yet specified

The discussion did not settle:

- The implementation of the factory's own agent harness, including its model, provider, and tool architecture.
- The exact sandbox provider contract, checkpoint frequency, and bounded rollback behavior.
- Managed hosting and provider-specific deployment recipes beyond the first SSH-and-container path.
- The exact durable task schema and storage system.
- How workers claim tasks and how leases expire.
- The exact priority model across areas after human overrides.
- How the merger computes affected work beyond the identified signals.
- How an area lead records and versions its broad plan.
- Whether area-lead reassessment is performed by the same model instance or a fresh worker.
- The precise waiting, callback, retry, and cancellation protocols.
- Human interfaces beyond the first Slack integration.
- Which actions the top-level lead can perform without explicit human authorization.
