# Module-Based Engineering for Autonomous Software Factories

AI makes producing code increasingly cheap. The new bottleneck is safely coordinating many simultaneous changes.

Module-based engineering trades additional scaffolding and smaller pull requests for isolation, comprehensibility, and safe parallelism.

[Read the complete design interview and Q&A](module-based-engineering-details/00-design-interview.md).

## [Product](module-based-engineering-details/07-product-contract.md)

The module platform and software factory are one product, brand, and project, even though they remain separate applications and packages internally. The module platform constrains the blast radius of changes; the factory makes the additional coordination and scaffolding economically practical.

Development bootstraps progressively. The module foundation is built conventionally, the factory becomes the first substantial application built with it, and the project increasingly uses its own factory as the system matures.

The first end-to-end foundation milestone targets an individual developer or very small Rust team using a greenfield repository created by the platform. It extracts one annotated Rust implementation method into a generated contract crate callable both as Rust code and through HTTP, connects the repository to one durable lead-agent sandbox, and lets the developer ask that lead through Slack to add a backward-compatible capability in one isolated pull request for human review. The sandbox contains the harness, Slack bridge, repository checkout, worktree, and persisted harness state; there is no separate foundation control plane. This milestone does not yet test multi-agent parallelism.

## [Modules](module-based-engineering-details/01-modules.md)

A module is a self-contained capability that evolves independently behind a stable, machine-extracted contract.

Each module owns:

- Its implementation and state
- Its exported capabilities
- Its declared dependencies
- Its migrations and tests
- Its quality contract

Every pull request has one accountable package and zero foreign source changes. Mechanically reproducible artifacts attributable to that package may accompany it.

One logical module owns a handwritten implementation crate and a mechanically generated contract crate. Annotated implementation methods are the authoring source; consumers compile only against generated contracts. The [Rust build topology](module-based-engineering-details/08-rust-build-topology.md) makes the distinction mechanically enforceable. Modules must communicate through declared interfaces and must not access another module's storage directly. The foundation treats this as an architectural rule checked by CI and review; at L0 it is not a security boundary against malicious same-process code. Compositions can require stronger credential, process, or adversarial isolation profiles.

Rust modules receive the full native guarantee set. Foreign-language modules remain first-class ownership and factory units, but only their managed binding boundary receives the same guarantees.

## [Packages](module-based-engineering-details/02-packages.md)

The ecosystem contains four package kinds:

- Modules implement product capabilities.
- Providers satisfy technical requirements such as relational storage, caching, or pub-sub.
- Compositions assemble modules and providers into deployable applications.
- Platform packages own the runtime, CI, build tooling, repository-wide generators, and enforcement machinery.

A module declares a typed requirement. An environment binds it to a provider:

```text
billing requires relational-store
-> production binds it to Postgres
```

Provider bindings are logically private to their modules, even when infrastructure is physically shared. A composition declares whether that contract is enforced by convention, scoped credentials, process boundaries, or an adversarial sandbox.

## [Runtime](module-based-engineering-details/03-runtime.md)

The runtime provides a standard way to define and invoke typed capabilities.

Rust implementation methods become capabilities when annotated. A deterministic pre-Cargo generator derives the contract crate, typed handles, implementation-neutral dispatch interface, implementation-local adapter, metadata, test bindings, and language-neutral compatibility schema. Developers and agents do not maintain the generated crate manually.

Rust source is the authoring authority while the generated schema is the compatibility authority. Exported types implement a constrained contract-type model, generated handles are asynchronous and distinguish domain errors from invocation failures, and every call carries explicit runtime context. The [canonical capability contract](module-based-engineering-details/09-capability-contract.md) defines the complete boundary.

Capabilities may support request-response, streaming, events, and real-time interaction. Configurable bindings can expose them through Rust, HTTP, RPC, CLI, or other transports.

The runtime remains small and vendor-neutral. Workflow engines such as Temporal may be used inside modules without becoming runtime concepts.

Authentication adapters normalize credentials into realm-scoped principals:

```text
(provider credential)
-> Principal(realm, subject, kind)
```

Modules declare default access policies with endpoint-level overrides. That declaration sets maximum reachability: compositions may narrow or omit an endpoint but never widen it. Authentication realms may provide safe, named development identities so the same authorization behavior can be exercised through local tools.

## [Evolution](module-based-engineering-details/04-evolution.md)

Contract changes normally use an expand-migrate-contract process without requiring public `v1` or `v2` surfaces:

1. Add a compatible bridge, such as an optional replacement field, while retaining the old surface.
2. Identify managed consumers.
3. Create one migration task per consumer module.
4. Track completion through a durable migration record.
5. Confirm remaining usage through dependency analysis and monitoring.
6. Tighten or remove the deprecated surface in a final pull request to the module providing the contract.

The generator always classifies the change. The configured harness decides whether compatibility evidence, deprecation state, and any explicit override permit it to merge. Long-lived parallel versions remain available but are not a runtime requirement.

Client-binding modules generate TypeScript, Swift, Kotlin, or other SDKs while keeping those consumers visible to the factory.

## [Software Factory](module-based-engineering-details/05-software-factory.md)

A top-level lead coordinates the harness and interfaces with humans. Area leads maintain plans and publish prioritized tasks. Worker agents execute tasks independently.

Those roles describe the intended mature organization, not a hard-coded minimum. The first factory contains only the human-facing lead. Roles, handoffs, and gates are factory policy expressed through configuration so workers, reviewers, and a merger can be introduced progressively.

The first factory is one durable sandbox running the lead, its harness, and its Slack bridge. It can use a managed durable-sandbox provider or the project's portable image on a compatible container target with persistent storage and restart behavior. Normal stop-and-resume preserves the repository and persisted harness state while that storage survives. Total sandbox and storage loss falls back to semantic reconstruction from GitHub, Slack, and repository state; exact continuation and exactly-once external effects are not foundation guarantees. Stronger multi-agent coordination is deferred rather than forcing a ledger, queue, or workflow engine into the MVP.

The merger serializes integration. After every merge, it detects Git conflicts, CI failures, changed packages, changed imported contracts, shared dependency-resolution changes, and superseded plans. Affected tasks are reassessed against the new system state before resubmission.

A continuous quality agent analyzes architecture, dependencies, and operational evidence. New Rust-build and live-invocation cycles are blocked by default; asynchronous cycles require idempotency, termination, and bounded-amplification evidence; provider-dependency and data-flow cycles remain analytical findings. Existing accepted cycles do not block unrelated work.

## [Quality and Authority](module-based-engineering-details/06-quality-and-authority.md)

Every package defines a strong automated quality contract appropriate to its kind. The harness runs it, adds required AI reviews, and refuses merges that fail policy.

Before a pull request merges or an issue closes, the tracker is reconciled with the accepted decisions. Linked and otherwise affected issues are updated, fully resolved issues are closed, and partially resolved issues retain their remaining scope unless it is explicitly transferred to named open issues. Reviewer proposals are not mistaken for project decisions.

The runtime does not promise correctness. The harness promises that the configured evidence and approval process were enforced.

Humans retain authoritative control through the top-level lead. Sensitive actions and policy changes use explicit, audited approval requests.
