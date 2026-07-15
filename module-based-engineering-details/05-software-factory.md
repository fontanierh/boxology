# Software Factory

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the multi-agent coordination, task, merge, and continuous-analysis system discussed during the design interview.

## Purpose

The software factory is a persistent control plane for developing and maintaining a modular system. It manages many tasks and agents across time rather than running one isolated coding request from beginning to end.

The module boundary gives the factory a natural unit of ownership, analysis, review, and merge accountability. Agents can work concurrently because every submitted change has one target module and cannot directly modify another module's source.

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

Each worker submits a single-module merge request. Submission moves the work into a waiting state while the merger evaluates it. The durable task and its artifacts remain the source of truth; the system need not preserve the exact model session forever. A compatible worker can resume rework from the stored task, branch, evidence, and feedback.

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

The exact user interface for these questions and approvals was not designed. It should provide clear context and strong approval requests rather than relying on unstructured side-channel instructions.

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

- The exact durable task schema and storage system.
- How workers claim tasks and how leases expire.
- The exact priority model across areas after human overrides.
- How the merger computes affected work beyond the identified signals.
- How an area lead records and versions its broad plan.
- Whether area-lead reassessment is performed by the same model instance or a fresh worker.
- The precise waiting, callback, retry, and cancellation protocols.
- The interface through which humans inspect, steer, approve, and reorganize the harness.
- Which actions the top-level lead can perform without explicit human authorization.

