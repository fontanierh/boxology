# Software Factory

[Back to the white paper](../boxology-whitepaper.md)

This document expands the multi-agent coordination, task, merge, and continuous-analysis system discussed during the design interview.

**Scope note:** the factory described here is not part of the Boxology platform. It is the design direction for the Boxology-based factory *application* — the platform's committed flagship consumer. Boxology defines what a safe change is; the factory decides who makes changes and when they merge. The separation decision is recorded in the [design interview](00-design-interview.md#follow-up-the-platform-and-the-factory-are-separate-products).

## Purpose

The mature software factory is intended to coordinate development and maintenance of a modular system across many tasks and agents. The foundation begins as a portable skill used by one coding agent rather than assuming that a separate control plane, persistent session, or Boxology-owned harness already exists. The agent using that skill is the **lead agent**.

The package boundary gives the factory a universal unit of ownership, analysis, review, and merge accountability. Boxes are the most common target, but providers, compositions, and platform packages can also own work. Agents can work concurrently because every submitted change has one accountable package and cannot modify another package's non-derived files.

The factory is a separate application built with the Boxology platform rather than a component of it. It is intended to become the first substantial application built with Boxology, following a progressive bootstrap in which early development does not depend on either system being complete.

An eventual shared factory substrate may supply mechanisms such as agent execution, isolated work, persistence, human interaction, and reporting. Its organization is policy. Leads, workers, reviewers, mergers, handoffs, and gates should be configurable rather than permanently hard-coded into the execution engine.

## Foundation-milestone factory

The governance hierarchy below is a mature direction. The v0 factory is deliberately only the Boxology skill:

1. A developer gives the skill to a compatible coding-agent harness.
2. The skill explains Boxology's philosophy, box boundaries, contracts, compatibility rules, and way of working.
3. The coding agent adopts the lead-agent role and works through whatever tools and human interface its harness provides.

V0 has no Boxology-owned harness, gateway, sandbox, factory service, task or event ledger, GitHub Issues workflow, worker pool, area leads, reviewer, merger, dashboard, or required communication transport. Codex, Claude Code, Pi, Hermes, and other compatible harnesses are equally valid hosts. Hermes with Slack is one possible operator setup, not part of Boxology.

The skill does not promise session persistence, frozen execution, stop-and-resume, message catch-up, or recovery. Those properties belong to the chosen harness until a later Boxology-owned execution layer exists. V0 also does not prescribe a GitHub task or pull-request workflow; richer factory behavior is added iteratively after the box model itself is usable.

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

The top-level lead is the primary interface between humans and the harness. In v0 this simply names the coding agent using the Boxology skill. It receives authoritative human guidance through its harness and can ask humans for information, direction, review, or approval.

Its control-plane responsibilities include:

- Reorganizing areas.
- Reprioritizing work across areas.
- Publishing authoritative guidance.
- Asking humans about issues surfaced by analysis or implementation.
- Cancelling or superseding work.
- Routing strong approval requests.

The phrase "keys to the whole harness" referred to the mature lead's control over planning and coordination. V0 adds no authority layer of its own; the selected harness, its system prompt, project instructions, supplied credentials, and the agent's judgment determine what it can do.

### Area leads

There can be a coordinator or area lead per package. A sufficiently large box or other package can be divided into explicit logical areas, each with its own lead.

An area lead maintains a broad plan for its area. Worker agents automatically receive the applicable plan when they pick up a ready task. The lead publishes tasks and assigns their relative priority within the area.

Area ownership and subdivisions are explicit configuration rather than something workers infer independently for each task.

### Worker agents

Workers pick up ready tasks and work independently. Once work begins, workers do not communicate with one another to negotiate concurrent changes. Shared context comes from the area plan, task, box contract, and current repository state.

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

Humans can push authoritative information into the selected harness through the lead. This is the mechanism for resolving ambiguous product questions, changing priorities, approving sensitive actions, or correcting the lead's understanding of the codebase.

The lead can also initiate questions when analysis or implementation requires judgment that it should not invent. Boxology does not select the interface: it can be a local agent UI, remote control, Slack, WhatsApp, or anything else the harness or gateway supports.

V0 contains no Boxology communication gateway, message-recovery protocol, or GitHub integration. The chosen harness and operator own access control, delivery, persistence, repository credentials, and external-effect behavior.

## Foundation authority model

The foundation deliberately treats the lead as an agent operating with the authority available through its chosen harness, not as a process constrained by a separate Boxology authorization service.

- Boxology defines no channel, user allowlist, identity model, or role assignment. Any instruction the harness presents as an authorized user message is authoritative.
- Conflicting or ambiguous guidance has no deterministic newest-message, quorum, or priority algorithm. The lead follows its system prompt and project instructions, uses agent judgment, and asks humans when useful.
- The lead may use every filesystem, process, network, Git, GitHub, communication, or other capability made available by the operator.
- Project instructions and system prompts are editable guidance, not a platform-enforced capability boundary.
- Approvals are ordinary conversation interpreted by the lead. V0 has no formal approval object, nonce, expiry, replay protection, separate approval store, capability matrix, immutable policy layer, or agent-controlled break-glass protocol.

The simple v0 skill focuses on Boxology principles rather than prescribing GitHub Issues, pull-request queues, or a human-merge protocol. A user may instruct the lead to follow any of those workflows through ordinary harness and repository configuration.

Formal roles, enforced authority policy, structured approvals, revocation, and break-glass behavior are deferred to [issue #66](https://github.com/fontanierh/boxology/issues/66). The later multi-agent coordination substrate remains tracked in [issue #57](https://github.com/fontanierh/boxology/issues/57).

## Execution and resumability

Boxology v0 does not own the lead's execution environment. The user can run the skill in a local coding agent, remote agent, container, managed sandbox, or another harness-supported setup. Boxology makes no promise about session persistence, uncommitted work, message delivery, stop-and-resume, crash consistency, external-effect deduplication, or reconstruction after loss.

Persistent workspaces and repository-backed context may be useful operator choices, but they are not v0 conformance requirements. Stronger factory-owned execution and durability guarantees are deferred until Boxology introduces the harness or coordination substrate that can implement them.

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

Cycle acceptance is configured CI and merger policy rather than a runtime graph check. The runtime must independently supply invocation safeguards required even for accepted or apparently acyclic graphs; their precise semantics remain open in [issue #6](https://github.com/fontanierh/boxology/issues/6).

## Relationship to CI and review

The merger relies on package-defined and harness-required CI. Integration tests run against the current merge candidate, not merely against the branch state on which the worker originally finished.

AI reviewers can be part of those required checks. Their configuration belongs to the harness and package quality policy rather than to the runtime.

Passing CI is necessary but may not be sufficient when the target package, imported contract, shared dependency resolution, or authoritative plan changed. Those cases route back through semantic reassessment.

## Matters not yet specified

The discussion did not settle:

- The implementation of a future Boxology-owned harness or factory execution layer, including its model, provider, tool, gateway, and durability architecture.
- Whether a lead-authored checkpoint should be standardized and where it should live.
- The exact priority model across areas after human overrides.
- How the merger computes affected work beyond the identified signals.
- How an area lead records and versions its broad plan.
- Whether area-lead reassessment is performed by the same model instance or a fresh worker.
- The precise waiting, callback, retry, and cancellation protocols.
- Whether Boxology should eventually ship or select any human communication gateway.
- The formal role and approval architecture beyond the permissive foundation model, tracked in [issue #66](https://github.com/fontanierh/boxology/issues/66).

The durable task schema, worker claims, leases and fencing, multi-agent messaging, split-brain prevention, stronger audit or provenance records, and eventual coordination backend are explicitly deferred to [issue #57](https://github.com/fontanierh/boxology/issues/57). A GitHub-native design direction for the status-broadcast and messaging portion is proposed in [Factory Comms](12-factory-comms.md). This is an accepted product-sequencing decision, not an incomplete foundation acceptance criterion: those mechanisms should be specified when the factory introduces the agent pool that needs them.
