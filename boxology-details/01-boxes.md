# Boxes

[Back to the white paper](../boxology-whitepaper.md)

This document records the long-term box model discussed during the design interview. The delivered V0 behavior is defined by [V0 Streams](11-v0-streams.md) and its linked specifications; broader statements here describe the target model unless those delivered authorities say otherwise.

**Box** is Boxology's product term for the independently owned capability unit. It is not a Rust language module and does not imply a public Rust type named `Box`; native boxes may use any internal Rust organization behind their generated contracts.

## Core promise

A box is a self-contained capability that can evolve independently behind a stable, machine-extracted contract. The platform exposes that contract consistently through whatever bindings have been configured, such as Rust calls, HTTP, RPC, or CLI.

The intended developer experience is that work inside one box does not require reasoning about the implementations of all other boxes. Consumers depend on its declared contract rather than its internals. Compatibility checks and the deprecation workflow protect those consumers as the implementation evolves.

This promise has three parts:

- **Independent evolution:** the box can be edited without editing its consumers in the same change.
- **Contract compatibility:** compatible changes remain safe for existing consumers, while incompatible changes follow an explicit, evidenced migration.
- **Consistent invocation:** callers use the same capability contract through the bindings selected by the application.

The promise is not that breaking changes never occur. It is that breakage is detected, explicit, measured, and removed through a managed process. Public `v1` or `v2` surfaces can be useful, but the runtime does not require them.

## Ownership boundary

Every pull request has exactly one semantic owner: one package. It may change that package's owned non-derived files and deterministic artifacts attributable to the change. It may not change hand-authored source, manifests, tests, migrations, configuration, or other non-derived files owned by another package.

The rule is therefore:

> One pull request, one accountable package, zero foreign source changes.

This is a semantic rule rather than a literal one-directory rule. Boxes are the most common owner, but provider, application-composition, and platform packages can also own changes. Repository-wide source such as CI, build tooling, and generators belongs to platform packages. Repository-wide derived files can change only when they are declared, reproducible projections attributable to the accountable package.

The ownership model discussed was:

- A box's handwritten implementation and controlled contract block are owned by that box.
- Its generated contract crate, schema, handles, and dispatch glue are reproducible artifacts attributable to that box.
- Shared domain types require an explicit owning contract box rather than ownerless shared code.
- Runtime, CI, and build tooling belong to platform packages.
- Deployment assembly belongs to an application composition package.
- Generated indexes and lockfiles are derived artifacts rather than foreign package source, but shared lockfile impact can require whole-workspace validation and reassessment.
- Generated clients are published artifacts; a consumer adopts a new version in its own change.

In V0, generated artifacts are regenerated from the candidate's permitted source inputs and checked byte-for-byte, while ownership findings are reported relative to base-revision manifests and schemas. Reconstructing a candidate from immutable base policy and tooling is the future factory-merger protocol, not a shipped V0 guarantee. Derived artifacts cannot hide hand-written semantic changes or weaken protected ownership and quality policy. The common manifest, ownership algorithm, and lockfile rules are defined in [Packages, Providers, and Compositions](02-packages.md#common-ownership-manifest).

## Why accept more pull requests

An ordinary codebase may change several areas in one pull request because doing so minimizes coordination and overhead for human developers. This system deliberately makes the opposite trade.

A change spanning four boxes becomes a coordinated sequence of four independently mergeable pull requests. This produces more scaffolding, code generation, and integration work. The bet is that agents make those mechanical costs cheap enough that the system can optimize for limited blast radius and strong ownership instead of minimizing the number of pull requests.

The package boundary is therefore the universal unit of agent work and merge accountability. Box packages remain the primary unit for product-capability work.

## State ownership

A box must not inspect or mutate another box's store. When several boxes use the same infrastructure provider, every provider binding is logically private to its owning box.

The strength of that rule is determined by the application composition's declared [provider-isolation profile](02-packages.md#provider-isolation). The foundation profile relies on mutually trusted code, review, provider scoping, and conformance evidence. It is intended to prevent or detect ordinary boundary mistakes but is not a security boundary against malicious same-process code.

The provider supplies the binding-level controls for its claimed profile. The composition, deployment substrate, and runtime supply process, operating-system, network, resource, sandbox, and egress controls where required. A Postgres provider might use separate servers, databases, schemas, table-level isolation, or binding-specific credentials. The consuming box receives its own logical store and is required to use only that binding.

Cross-box information moves only through normal box interfaces. The patterns discussed were:

- A typed request-response call for current information.
- An event subscription that maintains a local projection.
- An explicit snapshot passed into an operation.

The owning box remains the source of truth for live data. A consuming box can own a deliberately historical snapshot, such as the billing address recorded on an issued invoice.

## Dependencies

Dependencies must be declared statically and exercised through typed runtime capabilities.

The common ownership manifest's declared contract dependencies are the authoritative semantic dependency record. For example, billing declares that it requires the customer contact capability and any explicit contract surface it selects. Every Rust dependency giving one box access to a contract owned by another must correspond to such a declared import.

One logical box owns a handwritten implementation crate and a mechanically generated contract crate. Workspace CI maps complete Cargo edges to logical ownership and role. It rejects undeclared inter-box edges and every box-owned dependency on a foreign implementation, even when a contract import exists. Application compositions are the authorized assembly roots that may depend on selected implementation crates. The full edge policy and generation model are defined in [Rust Build Topology](08-rust-build-topology.md).

The generated contract crate supplies the canonical typed handle. The runtime and composition create and inject those handles only for declared imports, optionally through a generated consumer `Imports` structure. This makes ordinary invocation and impact analysis depend on the declared graph; it does not claim that handles are unforgeable under convention-level, same-process isolation.

Using raw networking, filesystem access, build scripts, dynamically constructed topic names, or similar mechanisms to bypass a box or provider boundary is a quality violation. The foundation can detect some bypasses mechanically and others through review, but convention-level isolation does not technically prevent them.

The factory uses declared imports for dependable dependency analysis. Runtime observations add evidence about which operations are actually being used. Static declarations and dynamic telemetry complement rather than replace one another.

Managed clients outside Rust participate through client-binding boxes, which generate language-native SDKs while keeping the contract dependency visible to the factory. Completely unmanaged public consumers cannot receive the same migration guarantee.

## Native and foreign-language boxes

Everything managed as part of an application should be represented as a box. Rust is the native implementation path and receives the full runtime, dependency-analysis, compatibility, and factory guarantees.

A foreign-language component, such as a TypeScript application, can also be represented as a first-class box package. It retains explicit ownership, factory tasks, and the one-package pull-request boundary. A client-binding box remains in the native managed ecosystem, owns the contract import, and generates the language-native SDK the foreign box consumes.

The platform can enforce and evolve that declared binding boundary, but it cannot provide the same static and runtime guarantees inside foreign-language implementation code. Polyglot code is therefore allowed anywhere the application composition requires it, with an explicitly reduced guarantee level rather than an implicit claim of equivalence with native Rust boxes.

## Dependency cycles

"Cycle" is not one property. V0 mechanically enforces only the Rust/Cargo graph described below. Live invocation, asynchronous event, provider-dependency, and data-flow graph gates remain future factory design. The long-term platform analyzes these separate graphs:

- The **Rust build graph** contains Cargo package and crate dependencies.
- The **live invocation graph** contains declared calls in which the caller waits for or maintains a live dependency on the callee, including unary request-response, call-scoped streams, and applicable real-time sessions.
- The **asynchronous event graph** contains detached publications and subscriptions that do not keep the publisher waiting on a live consumer.
- The **provider dependency graph** contains declared dependencies among provider packages together with box requirements and provider bindings.
- The **data-flow graph** records declared movement and ownership of information.

Each graph has a different policy:

- **Rust build graph:** cycles are forbidden. Contract crates never depend on implementation crates; box implementations may depend on declared foreign contract crates but never on foreign implementations. Application compositions link the selected implementations. CI must reject build-, development-, target-, or generated-dependency tricks that bypass these rules. The concrete topology is defined in [Rust Build Topology](08-rust-build-topology.md).
- **Live invocation graph:** a merge candidate that adds an edge completing a new cycle is blocked by default. A configured architectural approver may authorize an exception, but the durable approval must record the rationale, affected operations, and required runtime safeguards.
- **Asynchronous event graph:** cycles are allowed. A change completing one must record its idempotency, termination, and bounded-amplification argument so an event cannot circulate or multiply indefinitely without an explicit design.
- **Provider-dependency and data-flow graphs:** cycles are observed and surfaced for architectural review but are not merge-blocking merely because they exist.

When this policy is introduced, existing accepted cycles are snapshotted and tracked as quality findings. They do not block unrelated work; the gates apply to newly introduced cycle edges.

Graph policy is not runtime safety. Deadlines, cancellation and budget propagation, retry ceilings, and recursion protection are required independently because an acyclic declared graph can still recurse dynamically. Their precise invocation semantics remain part of the runtime execution-model work in [issue #6](https://github.com/fontanierh/boxology/issues/6).

## Matters not yet specified

The discussion did not settle:

- The precise granularity at which a large box should be divided.
- Post-v0 extensions to the fail-closed contract grammar.
- The internal erased-dispatch implementation behind composition-selected typed handles.
