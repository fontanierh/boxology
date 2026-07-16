# Software Factory

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the multi-agent coordination, task, merge, and continuous-analysis system discussed during the design interview.

## Purpose

The mature software factory is intended to coordinate development and maintenance of a modular system across many tasks and agents. The foundation deliberately begins with one persistent lead agent rather than assuming that a separate control plane already exists.

The package boundary gives the factory a universal unit of ownership, analysis, review, and merge accountability. Modules are the most common target, but providers, compositions, and platform packages can also own work. Agents can work concurrently because every submitted change has one accountable package and cannot modify another package's non-derived files.

The factory and module platform are delivered as one product while remaining separate applications internally. The factory is intended to become the first substantial application built with the module system, following a progressive bootstrap in which early development does not depend on either system being complete.

An eventual shared factory substrate may supply mechanisms such as agent execution, isolated work, persistence, human interaction, and reporting. Its organization is policy. Leads, workers, reviewers, mergers, handoffs, and gates should be configurable rather than permanently hard-coded into the execution engine.

## Foundation-milestone factory

The governance hierarchy below is a mature direction. The foundation-milestone factory is deliberately smaller:

1. A developer talks to one persistent lead agent through Slack.
2. The lead handles the requested change itself.
3. The lead, its harness, Slack bridge, repository checkout, worktree, and persisted harness state all run in one durable remote sandbox.
4. It opens a GitHub pull request and reports the result in Slack.
5. It stops at the pull request boundary. A human must review and merge.

The foundation milestone has no separate factory service, required task or event ledger, task UI, GitHub Issues workflow, worker pool, area leads, reviewer agent, merger agent, or custom factory dashboard. Launching the factory means ensuring that the lead sandbox and the harness and bridge inside it are running. Additional roles and surfaces can be added progressively without replacing the stable human-facing lead.

The factory owns its agent execution interface and lifecycle guarantees. The first implementation may wrap an existing runner, call model APIs directly, or use a bare-bones custom loop. The foundation milestone specifies observable behavior without requiring a sophisticated original harness.

Its prescribed acceptance task is one backward-compatible, module-local change: add `greet(name)`, returning `Hello, {name}!`, to the generated Hello module; touch no foreign package source; keep Rust and HTTP behavior consistent; open exactly one pull request; and leave merging to a human. This foundation milestone does not yet test concurrent agent work or the safe-parallelism thesis.

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

There can be a coordinator or area lead per package. A sufficiently large module or other package can be divided into explicit logical areas, each with its own lead.

An area lead maintains a broad plan for its area. Worker agents automatically receive the applicable plan when they pick up a ready task. The lead publishes tasks and assigns their relative priority within the area.

Area ownership and subdivisions are explicit configuration rather than something workers infer independently for each task.

### Worker agents

Workers pick up ready tasks and work independently. Once work begins, workers do not communicate with one another to negotiate concurrent changes. Shared context comes from the area plan, task, module contract, and current repository state.

Each worker submits a single-package merge request. Submission moves the work into a waiting state while the merger evaluates it. The exact durable task, claim, messaging, and recovery model for this post-MVP pool remains to be specified.

### Merger

The merger is a continuously operating integration coordinator. It serializes accepted changes against the current main branch.

The ordering proposed was:

- Within an area, tasks are ordered by the priority assigned by the area lead.
- Between areas, the initial tie-break is time of arrival.
- The top-level lead can authoritatively reprioritize areas or work when human guidance or system conditions require it.

Automatic priority aging was discussed as a possible starvation control but was not adopted as a firm rule. The global lead's ability to reprioritize is the agreed control currently captured.

## Optimistic parallelism

Multiple agents may work on the same package or logical area concurrently. The system does not take an exclusive package-wide write lock before work begins.

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
3. **Target package changed:** an intervening merge touched the same package, so the area lead should assess whether semantic rework is needed even if Git can merge it.
4. **Imported contract changed:** a dependency used by the task changed and the consumer assumptions need reassessment.
5. **Shared dependency resolution changed:** a version, source, checksum, or selected dependency used by an affected package changed, so whole-workspace validation and semantic reassessment are required even when the lockfile was mechanically reproduced.
6. **Area plan or human guidance changed:** the task may need revision or may have become obsolete.

Before accepting the merge, the configured merge process also runs the [tracker reconciliation gate](06-quality-and-authority.md#tracker-reconciliation-gate). Linked issues and any other open issues made stale by the change must reflect the decisions that are actually merging. Partially resolved issues remain open with their remaining scope recorded unless every unresolved part is explicitly transferred to named open issues; undecided review suggestions remain proposals.

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

Slack is the only first-class human integration in the foundation milestone. It should provide clear context and strong approval requests rather than relying on implicit authority. Other interfaces may be added later.

GitHub is the initial repository and pull-request surface, but there is no required GitHub App, bot workflow, Issues integration, or other first-class GitHub integration. The lead can use ordinary Git and GitHub credentials to push a branch and open a pull request, but it never merges autonomously. Dedicated identity and task-ledger integrations may be added when worker agents are introduced.

## Remote execution and resumability

The complete foundation factory is one durable lead-agent sandbox. It contains the harness, Slack bridge, managed-repository checkout, dedicated worktree and branch, and persisted harness state. There is no separate foundation control plane that creates or owns that sandbox.

Process restarts, sandbox stop-and-resume, and compute replacement preserve the repository and persisted harness state while durable storage survives. A managed sandbox provider may freeze and resume the whole environment. The same factory image may instead run on any compatible container target, provided that target supplies persistent storage and restart behavior. An ephemeral container does not meet this guarantee.

If the sandbox and its durable storage are both destroyed, a fresh lead reconstructs the project's semantic state from Git, repository instructions and documentation, GitHub issues, branches, pull requests, reviews and comments, Slack history, and any optional checkpoint written by the previous lead. Exact hidden-reasoning continuation, uncommitted-work preservation, and exactly-once GitHub or Slack effects are not promised across this catastrophic boundary. The new lead inspects those external systems before acting, but a rare repeated effect after an ambiguous failure remains possible.

The foundation does not require a central database, event ledger, queue, outbox, deduplication service, or workflow engine. Agents may emit events for observability and the lead may author a checkpoint, but neither is a mandatory source of truth. Selecting stronger post-MVP coordination mechanisms is deferred.

"Remotely hosted" does not require a vendor-operated service. Onboarding may guide users toward a managed durable-sandbox provider or the portable image on common compute providers, personal hardware, and later Kubernetes.

## Continuous quality agent

A perpetual quality agent analyzes the system beyond individual pull requests. It reasons about overall coherence and surfaces work that ordinary feature tasks would not necessarily create.

The responsibilities discussed included:

- Constructing and maintaining evidence for separate Rust-build, live-invocation, asynchronous-event, provider-dependency, and data-flow graphs.
- Supplying that evidence to configured CI and merger policy, which gate newly introduced Rust-build and live-invocation cycles.
- Preserving approved exceptions and grandfathered cycles as durable findings.
- Requiring idempotency, termination, and bounded-amplification evidence for changes completing asynchronous event cycles.
- Surfacing provider-dependency and data-flow cycles for architectural review.
- Prompting planners to produce plans that avoid or remove problematic cycles.
- Combining code analysis with operational evidence where useful.
- Identifying codebase directions that need human evaluation.
- Publishing targeted improvement tasks.

Cycle acceptance is configured CI and merger policy rather than a runtime graph check. The runtime must independently supply invocation safeguards required even for accepted or apparently acyclic graphs; their precise semantics remain open in [issue #6](https://github.com/fontanierh/module-based-engineering/issues/6).

## Relationship to CI and review

The merger relies on package-defined and harness-required CI. Integration tests run against the current merge candidate, not merely against the branch state on which the worker originally finished.

AI reviewers can be part of those required checks. Their configuration belongs to the harness and package quality policy rather than to the runtime.

Passing CI is necessary but may not be sufficient when the target package, imported contract, shared dependency resolution, or authoritative plan changed. Those cases route back through semantic reassessment.

## Matters not yet specified

The discussion did not settle:

- The implementation of the factory's eventual agent system beyond the minimal execution interface, including its model, provider, and tool architecture.
- The first managed sandbox provider and exact container persistence and restart recipes.
- Whether a lead-authored checkpoint should be standardized and where it should live.
- The exact priority model across areas after human overrides.
- How the merger computes affected work beyond the identified signals.
- How an area lead records and versions its broad plan.
- Whether area-lead reassessment is performed by the same model instance or a fresh worker.
- The precise waiting, callback, retry, and cancellation protocols.
- Human interfaces beyond the first Slack integration.
- Which actions the top-level lead can perform without explicit human authorization.

The durable task schema, worker claims, leases and fencing, multi-agent messaging, split-brain prevention, stronger audit or provenance records, and eventual coordination backend are explicitly deferred to [issue #57](https://github.com/fontanierh/module-based-engineering/issues/57). This is an accepted product-sequencing decision, not an incomplete foundation acceptance criterion: those mechanisms should be specified when the factory introduces the agent pool that needs them.
