# Contract Evolution and Deprecation

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the compatibility, migration, and deprecation process discussed during the design interview.

## Compatibility goal

Modules should normally evolve through backward-compatible changes. Consumers depend only on the stable contract and should not require simultaneous edits when the provider's implementation changes.

The system does not assume that compatibility can be preserved forever. Genuine breaking changes will occur. The goal is to turn them into visible, managed migrations rather than codebase-wide atomic patches.

The central rule is:

> A cross-module change is a coordinated migration composed of independently mergeable, single-module changes.

## Expand-migrate-contract

The process discussed was:

1. The provider module introduces the new contract while retaining the old version.
2. The factory identifies consumers of the old version.
3. It creates a migration task for every managed consumer.
4. Every consumer changes in its own pull request.
5. Completion updates a durable migration record.
6. Static dependency analysis and runtime evidence determine whether usage remains.
7. When removal policy is satisfied, the factory creates a final task for the module providing the contract to delete the old version.

The old and new versions coexist during the migration. This is the mechanism that preserves the one-module-per-pull-request invariant.

## Durable migration ownership

The initial description involved callbacks to the agent responsible for the deprecation. That was refined into a durable migration record rather than a dependency on one long-lived model session.

Consumer migrations update the record. Any suitable agent can later resume the deprecation-owner role from that record, the current dependency graph, the current code, and the collected evidence.

The record needs to represent at least the lifecycle discussed:

```text
new contract introduced
-> consumers identified
-> consumer migrations in progress
-> no managed consumers remain
-> removal authorized
-> old contract removed
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

When a provider introduces a new contract version, the binding module receives its own migration task and pull request. Client applications managed by the factory can then receive separate adoption tasks.

This preserves the rule that the provider does not directly edit consumer source or generated SDK copies inside consumer modules.

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

This process makes a codebase-wide change more elaborate than editing several functions in one pull request. It can create more versions, generated artifacts, tasks, and pull requests.

That cost is intentional. The system assumes that agent productivity makes the extra mechanical work affordable. In return, each change has a constrained blast radius, every module remains independently mergeable, and consumers cannot be silently broken by an atomic cross-module patch.

## Matters not yet specified

The discussion did not settle:

- The compatibility rules or schema-diffing mechanism.
- Version numbering and support-window policy.
- The exact durable migration record format.
- How unmanaged clients identify themselves in telemetry.
- When a human is always required for removal.
- How emergency breaking changes bypass or compress the normal migration process.
- How generated SDKs are published and how external repositories register their dependencies.
