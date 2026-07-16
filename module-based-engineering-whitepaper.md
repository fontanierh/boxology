# Module-Based Engineering for Autonomous Software Factories

AI makes producing code increasingly cheap. The new bottleneck is safely coordinating many simultaneous changes.

Module-based engineering trades additional scaffolding and smaller pull requests for isolation, comprehensibility, and safe parallelism.

[Read the complete design interview and Q&A](module-based-engineering-details/00-design-interview.md).

## [Product](module-based-engineering-details/07-product-contract.md)

The module platform and software factory are one product, brand, and project, even though they remain separate applications and packages internally. The module platform constrains the blast radius of changes; the factory makes the additional coordination and scaffolding economically practical.

Development bootstraps progressively. The module foundation is built conventionally, the factory becomes the first substantial application built with it, and the project increasingly uses its own factory as the system matures.

The first end-to-end foundation milestone targets an individual developer or very small Rust team using a greenfield repository created by the platform. It generates one capability callable both as Rust code and through HTTP, connects the repository to a remotely hosted factory, and lets the developer ask a lead agent through Slack to add a backward-compatible capability in one isolated pull request for human review. It does not yet test multi-agent parallelism.

## [Modules](module-based-engineering-details/01-modules.md)

A module is a self-contained capability that evolves independently behind a stable, versioned contract.

Each module owns:

- Its implementation and state
- Its exported capabilities
- Its declared dependencies
- Its migrations and tests
- Its quality contract

Every pull request has one accountable package and zero foreign source changes. Mechanically reproducible artifacts attributable to that package may accompany it.

Modules must communicate through declared, versioned interfaces and must not access another module's storage directly. The foundation treats this as an architectural rule checked by CI and review; at L0 it is not a security boundary against malicious same-process code. Compositions can require stronger credential, process, or adversarial isolation profiles.

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

Capabilities may support request-response, streaming, events, and real-time interaction. Configurable bindings can expose them through Rust, HTTP, RPC, CLI, or other transports.

The runtime remains small and vendor-neutral. Workflow engines such as Temporal may be used inside modules without becoming runtime concepts.

Authentication adapters normalize credentials into realm-scoped principals:

```text
(provider credential)
-> Principal(realm, subject, kind)
```

Modules declare default access policies with endpoint-level overrides. That declaration sets maximum reachability: compositions may narrow or omit an endpoint but never widen it. Authentication realms may provide safe, named development identities so the same authorization behavior can be exercised through local tools.

## [Evolution](module-based-engineering-details/04-evolution.md)

Breaking changes use an expand-migrate-contract process:

1. Add the new version.
2. Identify managed consumers.
3. Create one migration task per consumer module.
4. Track completion through a durable migration record.
5. Confirm remaining usage through dependency analysis and monitoring.
6. Remove the deprecated version in a final pull request to the module providing the contract.

Client-binding modules generate TypeScript, Swift, Kotlin, or other SDKs while keeping those consumers visible to the factory.

## [Software Factory](module-based-engineering-details/05-software-factory.md)

A top-level lead coordinates the harness and interfaces with humans. Area leads maintain plans and publish prioritized tasks. Worker agents execute tasks independently.

Those roles describe the intended mature organization, not a hard-coded minimum. The first factory contains only the human-facing lead. Roles, handoffs, and gates are factory policy expressed through configuration so workers, reviewers, and a merger can be introduced progressively.

The merger serializes integration. After every merge, it detects Git conflicts, CI failures, changed packages, changed imported contracts, shared dependency-resolution changes, and superseded plans. Affected tasks are reassessed against the new system state before resubmission.

A continuous quality agent analyzes architecture, dependencies, and operational evidence. It surfaces coherence problems such as cycles and publishes targeted improvement tasks.

## [Quality and Authority](module-based-engineering-details/06-quality-and-authority.md)

Every package defines a strong automated quality contract appropriate to its kind. The harness runs it, adds required AI reviews, and refuses merges that fail policy.

The runtime does not promise correctness. The harness promises that the configured evidence and approval process were enforced.

Humans retain authoritative control through the top-level lead. Sensitive actions and policy changes use explicit, audited approval requests.
