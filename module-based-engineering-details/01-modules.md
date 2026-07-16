# Modules

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the module model discussed during the design interview. It records the decisions and reasoning from that discussion; it is not yet an implementation specification.

## Core promise

A module is a self-contained capability that can evolve independently behind a stable, versioned contract. The platform exposes that contract consistently through whatever bindings have been configured, such as Rust calls, HTTP, RPC, or CLI.

The intended developer experience is that work inside one module does not require reasoning about the implementations of all other modules. Consumers depend on its declared contract rather than its internals. Compatibility checks and the deprecation workflow protect those consumers as the implementation evolves.

This promise has three parts:

- **Independent evolution:** the module can be edited without editing its consumers in the same change.
- **Contract compatibility:** compatible changes remain safe for existing consumers, while incompatible changes follow an explicit versioned migration.
- **Consistent invocation:** callers use the same capability contract through the bindings selected by the application.

The promise is not that breaking changes never occur. It is that breakage is explicit, versioned, measured, and removed through a managed process.

## Ownership boundary

Every pull request has exactly one semantic owner: one package. It may change that package's owned non-derived files and deterministic artifacts attributable to the change. It may not change hand-authored source, manifests, tests, migrations, configuration, or other non-derived files owned by another package.

The rule is therefore:

> One pull request, one accountable package, zero foreign source changes.

This is a semantic rule rather than a literal one-directory rule. Modules are the most common owner, but provider, application-composition, and platform packages can also own changes. Repository-wide source such as CI, build tooling, and generators belongs to platform packages. Repository-wide derived files can change only when they are declared, reproducible projections attributable to the accountable package.

The ownership model discussed was:

- A module implementation is owned by that module.
- An API contract or schema is owned by the module providing it.
- Shared domain types require an explicit owning contract module rather than ownerless shared code.
- Runtime, CI, and build tooling belong to platform packages.
- Deployment assembly belongs to an application composition package.
- Generated indexes and lockfiles are derived artifacts rather than foreign package source, but shared lockfile impact can require whole-workspace validation and reassessment.
- Generated clients are published artifacts; a consumer adopts a new version in its own change.

Derived artifacts must remain mechanically reproducible from the base revision plus only the accountable package's complete permitted non-derived diff under pinned generators. They cannot hide hand-written semantic changes or weaken protected ownership and quality policy. The common manifest, ownership algorithm, and lockfile rules are defined in [Packages, Providers, and Compositions](02-packages.md#common-ownership-manifest).

## Why accept more pull requests

An ordinary codebase may change several areas in one pull request because doing so minimizes coordination and overhead for human developers. This system deliberately makes the opposite trade.

A change spanning four modules becomes a coordinated sequence of four independently mergeable pull requests. This produces more scaffolding, code generation, and integration work. The bet is that agents make those mechanical costs cheap enough that the system can optimize for limited blast radius and strong ownership instead of minimizing the number of pull requests.

The package boundary is therefore the universal unit of agent work and merge accountability. Module packages remain the primary unit for product-capability work.

## State ownership

A module must not inspect or mutate another module's store. When several modules use the same infrastructure provider, every provider binding is logically private to its owning module.

The strength of that rule is determined by the application composition's declared [provider-isolation profile](02-packages.md#provider-isolation). The foundation profile relies on mutually trusted code, review, provider scoping, and conformance evidence. It is intended to prevent or detect ordinary boundary mistakes but is not a security boundary against malicious same-process code.

The provider supplies the binding-level controls for its claimed profile. The composition, deployment substrate, and runtime supply process, operating-system, network, resource, sandbox, and egress controls where required. A Postgres provider might use separate servers, databases, schemas, table-level isolation, or binding-specific credentials. The consuming module receives its own logical store and is required to use only that binding.

Cross-module information moves only through normal module interfaces. The patterns discussed were:

- A typed request-response call for current information.
- An event subscription that maintains a local projection.
- An explicit snapshot passed into an operation.

The owning module remains the source of truth for live data. A consuming module can own a deliberately historical snapshot, such as the billing address recorded on an issued invoice.

## Dependencies

Dependencies must be declared statically and exercised through typed runtime capabilities.

The common ownership manifest's declared contract dependencies are the authoritative semantic dependency record. For example, billing declares that it requires a version of the customer contact contract. Every Rust dependency giving one module access to a contract owned by another must correspond to such a declared import. Workspace CI must map Cargo edges to package ownership and reject both undeclared inter-module edges and any edge to a crate classified as a foreign implementation, even when a contract import exists. The exact crate classification and topology remain tracked in [issue #39](https://github.com/fontanierh/module-based-engineering/issues/39).

The runtime or generated Rust interface supplies typed capability handles only for declared imports. This makes ordinary invocation and impact analysis depend on the declared graph; it does not claim that handles are unforgeable under convention-level, same-process isolation.

Using raw networking, filesystem access, build scripts, dynamically constructed topic names, or similar mechanisms to bypass a module or provider boundary is a quality violation. The foundation can detect some bypasses mechanically and others through review, but convention-level isolation does not technically prevent them.

The factory uses declared imports for dependable dependency analysis. Runtime observations add evidence about which operations are actually being used. Static declarations and dynamic telemetry complement rather than replace one another.

Managed clients outside Rust participate through client-binding modules, which generate language-native SDKs while keeping the contract dependency visible to the factory. Completely unmanaged public consumers cannot receive the same migration guarantee.

## Native and foreign-language modules

Everything managed as part of an application should be represented as a module. Rust is the native implementation path and receives the full runtime, dependency-analysis, compatibility, and factory guarantees.

A foreign-language component, such as a TypeScript application, can also be represented as a first-class module package. It retains explicit ownership, factory tasks, and the one-package pull-request boundary. A client-binding module remains in the native managed ecosystem, owns the contract import, and generates the language-native SDK the foreign module consumes.

The platform can enforce and evolve that declared binding boundary, but it cannot provide the same static and runtime guarantees inside foreign-language implementation code. Polyglot code is therefore allowed anywhere the application composition requires it, with an explicitly reduced guarantee level rather than an implicit claim of equivalence with native Rust modules.

## Dependency cycles

The discussion did not make dependency cycles a runtime error. Backward-compatible contracts and single-package pull requests remove much of the change-management difficulty normally associated with a cycle.

Cycles can still signal engineering problems, including recursive request paths, availability coupling, unclear ownership, or difficult isolated testing. The agreed direction was to treat these as quality concerns:

- Planners receive the dependency graph and should avoid introducing cycles.
- Mechanical analysis can identify new declared cycles.
- The continuous quality agent can combine static analysis with operational traces and create decoupling tasks.
- Existing cycles need not stop unrelated work.
- The runtime remains neutral rather than embedding a universal cycle policy.

## Matters not yet specified

The discussion did not settle:

- The concrete serialization and implementation tooling for the common package manifest contract.
- The precise granularity at which a large module should be divided.
- The exact type system or interface-definition language.
- The Rust contract/implementation crate topology, dispatch model, and concrete representation of coexisting contract versions.
- Whether any categories of new dependency cycle should be mechanically blocked rather than only surfaced.
