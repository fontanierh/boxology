# Contract Evolution and Deprecation

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the compatibility, migration, and deprecation process discussed during the design interview.

## Compatibility goal

Modules should normally evolve through backward-compatible changes. Consumers depend only on the generated contract and should not require simultaneous edits when the providing module's implementation changes.

The deterministic language-neutral schema is the compatibility authority. The generator compares the submitted schema with the base revision and reports changes independently of Rust formatting, macro expansion, or binding-specific representation. Documentation-only changes receive their own classification rather than being mixed with semantic evolution. See [Canonical Capability Contract](09-capability-contract.md).

The system does not assume that compatibility can be preserved forever. Genuine breaking changes will occur. The goal is to turn them into visible, managed migrations rather than codebase-wide atomic patches.

The central rule is:

> A cross-module change is a coordinated migration composed of independently mergeable, single-package changes.

## Contract identity, revision, and explicit versions

Three concepts are distinct:

- A **contract identity** is the stable logical identity of a capability, field, or other contract element.
- A **contract revision** or fingerprint identifies one generated state so it can be compared with its base revision.
- An **explicit public version** is a deliberately published surface such as `v1` or `v2`.

Stable identities and comparable revisions are required for dependable analysis. Explicit public versions are optional. A module can continuously evolve one contract surface for its entire lifetime, as many public APIs do, provided it expands, migrates, deprecates, and contracts that surface safely.

The platform therefore does not require one crate, namespace, trait, or route per explicit version. A module may introduce parallel versions when unmanaged consumers, long support windows, or genuinely incompatible semantics make coexistence useful. That is a module and harness policy choice rather than the universal migration mechanism.

## Expand-migrate-contract

The process discussed was:

1. The providing module expands its existing surface with a compatible bridge while retaining the old behavior or shape.
2. The factory identifies consumers affected by the intended contraction or tightening.
3. It creates a migration task for every managed consumer.
4. Every consumer changes in its own pull request.
5. Completion updates a durable migration record.
6. Static dependency analysis, deployment state, and runtime evidence determine whether old usage remains.
7. When configured removal policy is satisfied, the factory creates a final task for the providing module to tighten or remove the deprecated surface.

The bridge and deprecated surface coexist during the migration. They do not need different public version labels. This staged coexistence is what preserves the one-package-per-pull-request invariant.

For example, making an input field required can proceed as:

```text
add the field as optional
-> migrate consumers to populate it
-> verify managed adoption and applicable deployment evidence
-> make the field required
```

Removing a field can proceed as:

```text
mark the field deprecated
-> migrate consumers away from it
-> verify applicable usage has drained
-> remove the field
```

The final step is mechanically incompatible with an old consumer. Compatibility analysis must say so honestly. The harness authorizes it only because the configured migration policy has accepted the evidence; it does not relabel the change as intrinsically compatible.

The contract model supplies several mechanical rules needed by this process:

- Generated consumers ignore unknown fields in provider outputs.
- Generated output enums and errors preserve an unknown variant rather than failing to decode the complete response.
- An older provider rejects an unknown input field or variant with a structured contract error rather than silently discarding caller intent.
- Adding an optional input field therefore uses provider-first deployment before callers send it.
- Tightening validation so previously valid input is rejected is a breaking semantic change.

These rules make some expansions mechanically survivable, but they do not replace semantic change classification. The generator reports the change and the harness applies the configured policy.

## Durable migration ownership

The initial description involved callbacks to the agent responsible for the deprecation. That was refined into a durable migration record rather than a dependency on one long-lived model session.

Consumer migrations update the record. Any suitable agent can later resume the deprecation-owner role from that record, the current dependency graph, the current code, and the collected evidence.

The record needs to represent at least the lifecycle discussed:

```text
compatible bridge introduced
-> consumers identified
-> consumer migrations in progress
-> no managed consumers remain
-> removal authorized
-> deprecated surface removed or tightened
```

The exact storage schema and callback protocol were not designed.

## Finding consumers

Managed Rust modules declare their imported contracts and issue calls through typed runtime capabilities. The factory reads those declarations to identify consumers.

Runtime telemetry provides a second signal. It can confirm actual use, find unexpected traffic, and show whether a deprecated method still receives calls.

Static and dynamic evidence have different roles:

- Static declarations identify managed code that could call the contract.
- Runtime telemetry shows observed use in deployed environments.
- Neither alone proves that an unknown public consumer no longer exists.

## Client-binding modules

A client binding is a thin managed module that imports a module contract and generates a language-native SDK.

For example:

```text
Billing module
-> TypeScript binding module
-> generated npm SDK
-> web application
```

Swift, Kotlin, and other bindings follow the same pattern. The binding remains inside the managed package and dependency system even though its generated output is consumed outside Rust.

When a providing module changes a contract surface used by a binding module, the binding receives its own migration task and pull request. Client applications managed by the factory can then receive separate adoption tasks. If the providing module deliberately introduces a parallel public version, the same staged process applies to adopting it.

This preserves the rule that the providing module does not directly edit consumer source or generated SDK copies inside consumer modules.

## Unknown public consumers

Completely unmanaged clients cannot be enumerated or automatically migrated. Public contracts therefore cannot rely solely on the managed dependency graph.

The deprecation-management agent should assemble evidence before recommending removal. The evidence discussed included:

- Confirmation that all managed consumers migrated.
- Monitoring data, potentially obtained from systems such as Datadog.
- Traffic over an appropriate observation window.
- Remaining caller identities when available.
- The published deprecation deadline or policy.
- Error and operational data.
- Explicit human direction when required.

The harness applies the configured removal policy. The agent collects evidence and reasons about whether the old endpoint is safe to remove. A human can make the authoritative decision to pull the plug, especially for risky or public interfaces.

No universal observation window, support period, or automatic-removal threshold was selected.

## Continuous deprecation quality

The continuous quality system should track deprecations so that old interfaces do not remain indefinitely merely because their initial migration lost attention.

It can surface:

- Deprecations that have existed too long.
- Consumer migrations that are stalled.
- Observed calls to an interface expected to be unused.
- Missing tasks or consumers not represented in the migration plan.

These findings become tasks or human questions through the same factory control plane.

## Complexity tradeoff

This process makes a codebase-wide change more elaborate than editing several functions in one pull request. It can create temporary compatibility shapes, generated artifacts, tasks, and pull requests. Explicit parallel versions add further cost only when the module chooses them.

That cost is intentional. The system assumes that agent productivity makes the extra mechanical work affordable. In return, each change has a constrained blast radius, every module remains independently mergeable, and consumers cannot be silently broken by an atomic cross-module patch.

## Matters not yet specified

The discussion did not settle:

- Compatibility policy for semantic changes that cannot be inferred completely from schema shape.
- Optional public version numbering and support-window policy.
- The exact durable migration record format.
- How unmanaged clients identify themselves in telemetry.
- When a human is always required for removal.
- How emergency breaking changes bypass or compress the normal migration process.
- How generated SDKs are published and how external repositories register their dependencies.
