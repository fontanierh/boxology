# Boxology: Independent Software Boxes

AI makes producing code increasingly cheap. The new bottleneck is safely coordinating many simultaneous changes.

Boxology treats a software system as independent boxes connected only through explicit, typed interfaces. Humans own the box boundaries, contracts, types, data models, and composition. Agents may implement and replace the code hidden inside each box.

Box-and-arrow diagrams are often dismissed as architectural slideware because nothing enforces their boundaries. Boxology reclaims the idea by making the boxes and arrows executable and mechanically checked.

This approach trades additional scaffolding and smaller pull requests for isolation, comprehensibility, and safe parallelism.

The justification that ages best is oversight. Model capability grows, but human attention does not scale, and humans remain accountable for what merges. Box-scoped pull requests with machine-classified contract diffs are a trust interface: they keep every unit of agent work reviewable by the people responsible for it. The [strategy review](boxology-details/10-strategy-review.md) records this positioning, the thesis risks, and the self-hosting ladder.

[Read the complete design interview and Q&A](boxology-details/00-design-interview.md).

## [Product](boxology-details/07-product-contract.md)

Boxology is the platform: the box model, runtime, contract generator, validation tooling, installer, and a harness-neutral skill. It works with any coding-agent harness. The autonomous software factory is not part of Boxology; it is the project's committed flagship application, built with Boxology, and the vehicle for the safe-parallelism thesis. The boundary is crisp: Boxology defines what a safe change is; a factory — any factory — decides who makes changes and when they merge. The platform constrains the blast radius of changes; a factory makes the additional coordination and scaffolding economically practical.

Development bootstraps progressively. The Boxology foundation is built conventionally, the factory becomes the first substantial application built with it, and the project increasingly uses its own factory as the system matures.

The first end-to-end foundation milestone targets an individual developer or very small Rust team using a greenfield repository created by the platform. It extracts one annotated Rust implementation method into a generated contract crate callable both as Rust code and through HTTP. A small, portable skill explains the Boxology principles to the developer's existing coding agent and names it the lead agent. Boxology does not yet supply that agent's harness, gateway, sandbox, persistence, or GitHub workflow. This milestone tests the box model and harness-neutral guidance, not multi-agent parallelism.

## [Boxes](boxology-details/01-boxes.md)

A box is a self-contained capability that evolves independently behind a stable, machine-extracted contract.

Each box owns:

- Its implementation and state
- Its exported capabilities
- Its declared dependencies
- Its migrations and tests
- Its quality contract

Every pull request has one accountable package and zero foreign source changes. Mechanically reproducible artifacts attributable to that package may accompany it.

One logical box owns a handwritten implementation crate and a mechanically generated contract crate. Annotated implementation methods are the authoring source; consumers compile only against generated contracts. The [Rust build topology](boxology-details/08-rust-build-topology.md) makes the distinction mechanically enforceable. Boxes must communicate through declared interfaces and must not access another box's storage directly. The foundation treats this as an architectural rule checked by CI and review; at L0 it is not a security boundary against malicious same-process code. Compositions can require stronger credential, process, or adversarial isolation profiles.

Rust boxes receive the full native guarantee set. Foreign-language boxes remain first-class ownership and factory units, but only their managed binding boundary receives the same guarantees.

## [Packages](boxology-details/02-packages.md)

The ecosystem contains four package kinds:

- Boxes implement product capabilities.
- Providers satisfy technical requirements such as relational storage, caching, or pub-sub.
- Compositions assemble boxes and providers into deployable applications.
- Platform packages own the runtime, CI, build tooling, repository-wide generators, and enforcement machinery.

A box declares a typed requirement. An environment binds it to a provider:

```text
billing requires relational-store
-> production binds it to Postgres
```

Provider bindings are logically private to their boxes, even when infrastructure is physically shared. A composition declares whether that contract is enforced by convention, scoped credentials, process boundaries, or an adversarial sandbox.

## [Runtime](boxology-details/03-runtime.md)

The runtime provides a standard way to define and invoke typed capabilities.

Rust implementation methods become capabilities when annotated. A deterministic pre-Cargo generator derives the contract crate, typed handles, implementation-neutral dispatch interface, implementation-local adapter, metadata, test bindings, and language-neutral compatibility schema. Developers and agents do not maintain the generated crate manually.

Rust source is the authoring authority while the generated schema is the compatibility authority. Exported types implement a constrained contract-type model, generated handles are asynchronous and distinguish domain errors from invocation failures, and every call carries explicit runtime context. The [canonical capability contract](boxology-details/09-capability-contract.md) defines the complete boundary.

Capabilities may support request-response, streaming, events, and real-time interaction. Configurable bindings can expose them through Rust, HTTP, RPC, CLI, or other transports.

The runtime remains small and vendor-neutral. Workflow engines such as Temporal may be used inside boxes without becoming runtime concepts.

Authentication adapters normalize credentials into realm-scoped principals:

```text
(provider credential)
-> Principal(realm, subject, kind)
```

Boxes declare default access policies with endpoint-level overrides. That declaration sets maximum reachability: compositions may narrow or omit an endpoint but never widen it. Authentication realms may provide safe, named development identities so the same authorization behavior can be exercised through local tools.

## [Evolution](boxology-details/04-evolution.md)

Contract changes normally use an expand-migrate-contract process without requiring public `v1` or `v2` surfaces:

1. Add a compatible bridge, such as an optional replacement field, while retaining the old surface.
2. Identify managed consumers.
3. Create one migration task per consumer box.
4. Track completion through a durable migration record.
5. Confirm remaining usage through dependency analysis and monitoring.
6. Tighten or remove the deprecated surface in a final pull request to the box providing the contract.

The generator always classifies the change. The configured harness decides whether compatibility evidence, deprecation state, and any explicit override permit it to merge. Long-lived parallel versions remain available but are not a runtime requirement.

Client-binding boxes generate TypeScript, Swift, Kotlin, or other SDKs while keeping those consumers visible to the factory.

## [Software Factory](boxology-details/05-software-factory.md) — the flagship application

A top-level lead coordinates the harness and interfaces with humans. Area leads maintain plans and publish prioritized tasks. Worker agents execute tasks independently.

Those roles describe the intended mature organization, not a hard-coded minimum. V0 is only a harness-neutral Boxology skill: the coding agent using it is the human-facing lead. The skill explains the philosophy, boundaries, contracts, compatibility principles, and way of working. It does not yet prescribe GitHub Issues, workers, reviews, merging, or another coordination workflow.

Codex, Claude Code, Pi, Hermes, and other compatible harnesses can host that lead. Their communication surfaces, permissions, persistence, and recovery behavior remain their own. Boxology supplies no v0 harness, Slack integration, sandbox, factory image, liveness monitor, message catch-up, or durability guarantee. Those capabilities can be introduced progressively when Boxology owns the execution substrate that can provide them.

The merger serializes integration. After every merge, it detects Git conflicts, CI failures, changed packages, changed imported contracts, shared dependency-resolution changes, and superseded plans. Affected tasks are reassessed against the new system state before resubmission.

A continuous quality agent analyzes architecture, dependencies, and operational evidence. New Rust-build and live-invocation cycles are blocked by default; asynchronous cycles require idempotency, termination, and bounded-amplification evidence; provider-dependency and data-flow cycles remain analytical findings. Existing accepted cycles do not block unrelated work.

## [Quality and Authority](boxology-details/06-quality-and-authority.md)

Every package defines a strong automated quality contract appropriate to its kind. The harness runs it, adds required AI reviews, and refuses merges that fail policy.

Before a pull request merges or an issue closes, the tracker is reconciled with the accepted decisions. Linked and otherwise affected issues are updated, fully resolved issues are closed, and partially resolved issues retain their remaining scope unless it is explicitly transferred to named open issues. Reviewer proposals are not mistaken for project decisions.

The runtime does not promise correctness. `boxology check` produces the configured evidence; enforcement belongs to the user's current workflow until a later Boxology-owned harness exists.

Humans retain authoritative control through the top-level lead. In v0, authority is ordinary agent conversation governed by the selected harness and editable project instructions; formal roles and audited approval protocols remain later work.
