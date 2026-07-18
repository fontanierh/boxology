# Product Contract and Foundation Milestone

[Back to the white paper](../boxology-whitepaper.md)

This document separates the long-term product direction from the first falsifiable foundation milestone. It resolves the product-boundary questions without pretending that the eventual market, agent implementation, or deployment ecosystem is already known.

## One product with two systems

The product combines:

- A Rust Boxology platform for defining independently evolvable boxes.
- A software-factory direction for changing systems built with that platform, beginning as a harness-neutral skill and growing into richer coordination later.

These are one product, brand, and project. They remain separate applications and packages internally. The Boxology platform reduces the amount of code and context an agent can affect at once. The factory makes the resulting increase in scaffolding, pull requests, and coordinated migrations practical.

The supported product journey combines both systems, but they are not fused technically. The Boxology runtime is delivered as a Rust dependency. V0 factory behavior is guidance used by the developer's existing coding agent, and only promises to describe work in repositories initialized by Boxology; arbitrary-repository migration is out of scope.

## Progressive bootstrap

The system cannot depend on itself before it exists. Development therefore proceeds progressively:

1. Build the initial Boxology foundation through conventional development.
2. Build the first factory without requiring it to modify its own source.
3. Make the factory the first substantial application built with Boxology.
4. Introduce factory-assisted changes gradually.
5. Add task coordination, workers, reviews, merging, and continuous analysis as separate increments.
6. Increase self-hosting as each layer becomes dependable.

The goal is comprehensive dogfooding, not circular bootstrapping. A broken factory must remain repairable without needing that same factory to function.

The transition from the single-lead bootstrap to a box-built factory is governed by a standing forcing commitment recorded in [issue #74](https://github.com/fontanierh/boxology/issues/74) and the [strategy review](10-strategy-review.md): it must arrive uncomfortably early rather than when it feels justified. Its first concrete rung is boxifying Boxology's own tools — the generator, `boxology check`, and the installer become boxes as soon as a working generator exists, so the box tax lands on the platform's own development before it lands on any user. Further, and bootstrap-period friction is classified in advance as mechanical (the factory's future job) or semantic (evidence against the thesis), so relaxations of the discipline are recorded as data instead of rationalized.

## Primary v1 operator and operating envelope

The primary v1 operator is an individual developer or very small Rust team starting a greenfield backend with a skill-compatible coding agent.

This is the person able to evaluate the foundation milestone, not a claim about the eventual market. Early operators will probably be hobbyists and other experimenters. The long-term ambition is much broader: to become an excellent general way to produce software.

The first supported technical envelope is narrow:

- A greenfield project.
- A repository created by the platform initializer.
- A Rust backend in one Git repository and Cargo workspace.
- Application boxes and compositions kept as distinct crates or packages within that repository.
- One repository initialized and worked on by one lead agent at a time for the foundation milestone.

A monorepo is not treated as a monolith. Boxes retain separate contracts and ownership even when they share a repository and Cargo lockfile. An ordinary package change may include only the minimal lockfile resolution reproducible from its own permitted non-derived diff under pinned inputs. A lockfile change that alters a dependency used by another package requires whole-workspace validation and semantic reassessment.

## Repository and execution locations

The system distinguishes:

- **Product source repository:** contains the source of the Boxology platform and factory. Through progressive dogfooding, this repository can itself become a managed project repository.
- **Managed project repository:** is initialized by the platform and contains application boxes, compositions, tests, generated artifacts, and repository-local Boxology configuration. It does not receive a copied agent harness.
- **Lead environment:** is wherever the developer's selected coding-agent harness runs. It may be local, remote, containerized, or managed by another service; it is not a Boxology deployment object in v0.

The foundation has no Boxology factory service, gateway, or sandbox. The product source repository is also eligible to be managed by a lead agent once progressive dogfooding reaches that stage.

## Installation and onboarding

Installation begins through the developer's existing coding agent.

The project supplies a portable Boxology skill following the shared Agent Skills format. The same core guidance should be usable by compatible hosts such as Codex, Claude Code, Cursor, Pi, Hermes, and other agents that support skills. Host-specific packaging may differ, but the product does not make one coding-agent application part of its architecture.

The v0 skill is intentionally small. It explains Boxology's philosophy, box boundaries, contracts, compatibility principles, and way of working, and names the coding agent using it the **lead agent**. A deterministic, versioned installer owns project-generation mutations. The expected flow is:

1. The developer installs or gives the onboarding skill to a compatible coding agent.
2. The agent explains the setup and asks the minimum necessary questions.
3. The agent obtains and runs the project CLI.
4. The CLI creates the Rust workspace, box, composition, and repository configuration.
5. The same coding agent continues as the lead through whatever tools and human interface its chosen harness already provides.

The Boxology runtime is a normal Rust dependency. The skill is installed into or supplied to the chosen agent host; no Boxology harness source is copied into the managed repository. Only project-specific contracts and configuration belong there.

V0 does not prescribe GitHub Issues, a dedicated GitHub App, a bot workflow, a pull-request queue, or a merge protocol. A lead may use ordinary Git and GitHub capabilities when its harness and operator provide them.

## Foundation release bundle

The first release bundle contains:

- A portable onboarding skill.
- A deterministic installer CLI.
- Rust box-runtime packages with Rust and HTTP bindings.

It does not include a Boxology agent harness, communication gateway, factory image, or sandbox runtime.

## Harness and deployment neutrality

V0 runs inside the coding-agent harness selected by the user. Codex with local or remote control, Claude Code, Pi, Hermes with Slack, and other setups are examples rather than Boxology components or conformance targets. Boxology does not build, fork, vendor, publish, or require Hermes and does not select Slack or another communication transport.

The operator may run that harness directly, in a container, on a managed sandbox, or on another target. Docker and Kubernetes remain useful deployment options for chosen harnesses and future Boxology-owned services, but neither belongs to the v0 product contract.

Boxology makes no v0 promise about harness liveness, session persistence, frozen state, stop-and-resume, crash consistency, message catch-up, or recovery. Those properties belong entirely to the selected harness and operator. Stronger factory-owned execution and durability remain later work in [issue #57](https://github.com/fontanierh/boxology/issues/57); possible deployment recipes can be reconsidered when Boxology owns something that needs deployment.

## Generated Hello World project

At the end of project initialization, the developer has a small running application that proves the central box abstraction:

- One implementation method is annotated as a typed Hello World capability.
- Its boundary types are authored beside the implementation and lifted into the generated contract crate.
- A deterministic pre-Cargo step generates its language-neutral schema, contract crate, asynchronous typed handle, implementation-neutral dispatch interface, and implementation-local adapter without a second hand-maintained API.
- The capability can be invoked directly through the Rust box interface.
- The same capability can be invoked through an HTTP endpoint.
- Both paths reach the same annotated implementation method through the generated contract rather than two handwritten interfaces or implementations.

The generated handle uses the invocation envelope in [Canonical Capability Contract](09-capability-contract.md): an explicit call context, a declared domain error, and a distinct invocation-error layer even when the selected binding is in-process.

The first proof is database-free. Postgres and Redis remain important provider candidates, but persistence is a later slice and should not obscure validation of the box, binding, and lead workflow.

The deterministic contract generator is therefore a core foundation deliverable rather than incidental scaffolding. The milestone is not complete until generation, provenance, reproducibility, typed invocation, and compatibility classification work as one path.

The foundation milestone covers unary request-response only. The first full box-runtime release is expected to add streaming data, streaming events, and real-time interaction as first-class contract shapes.

## Native and foreign-language boxes

Everything managed as part of an application should be represented as a box, but not every implementation receives the same guarantees.

- A **native box** is implemented in Rust and receives the full runtime, dependency-analysis, compatibility, and Boxology validation guarantees.
- A **foreign-language box** remains a first-class managed package with ownership and a declared boundary, but its internals receive fewer static and runtime guarantees.
- A **client-binding box** remains in the native managed ecosystem, owns the contract import, and generates a language-native SDK for a foreign box.

A TypeScript application or another foreign-language component can therefore appear anywhere the application composition requires. The platform manages its declared boundary but cannot make the same claims about its internal implementation that it makes for a native Rust box.

## First factory behavior

The developer uses the selected coding agent normally. Once given the Boxology skill, that agent is the lead agent: it understands the project in terms of boxes, their human-owned contracts and data models, compatible evolution, and the rule that communication crosses declared interfaces.

The skill focuses on those principles. It does not yet define GitHub Issues, task pickup, worker communication, review roles, a merger, Slack behavior, pull-request stopping, or autonomous-merging policy. The user and chosen harness remain free to supply their ordinary workflow.

The runtime acceptance task is to add a backward-compatible `greet(name)` capability to the generated Hello box. Calling it with `Ada` returns `Hello, Ada!` through both Rust and HTTP. The resulting repository state must change no foreign package source, include only permitted deterministic artifacts outside the Hello box, and preserve consistent behavior through both bindings. This validates a real box ownership and binding invariant, not multi-agent coordination or the broader safe-parallelism thesis.

## Factory organization is configuration

The eventual factory may include a top-level lead, area leads, workers, reviewers, a merger, and continuous quality agents. Those roles must not be mandatory concepts hard-coded into the execution engine.

As the factory grows, its shared substrate will need mechanisms for running agents, isolating work, preserving state, asking humans, and reporting results. Factory configuration determines which roles exist, how work moves between them, and which gates apply. This configuration may remain internal at first while the design is being adjusted.

The first configuration is only the human-facing lead role supplied by the skill. GitHub Issues, worker agents, task assignment, review agents, merger coordination, dedicated identities, and gateways are introduced progressively.

## Factory execution boundary

The skill runs in the developer's existing coding agent. V0 has no second population of agents inside a Boxology-owned factory and no Boxology execution interface. The selected harness owns models, tools, memory, permissions, user communication, and lifecycle behavior.

## Isolation, suspension, and recovery

Boxology v0 makes no harness guarantee. It does not promise a sandbox, process isolation, session persistence, durable agent state, frozen execution, stop-and-resume, message recovery, crash recovery, or exactly-once external effects. A chosen harness may provide any of those properties independently, but they are neither required nor tested by Boxology.

The repository remains the durable artifact owned by the developer. That fact can help a replacement agent recover project context, but v0 does not define or guarantee a reconstruction procedure.

## Deployment topology

Boxes do not decide how they are deployed. Application compositions own deployment topology and select whether boxes are linked into one binary, exposed as separate services, or assembled into other process types.

Kubernetes is an important future target for any application built with Boxology, as well as a possible deployment target for the factory itself. It is deliberately not part of the foundation milestone. Kubernetes support should be supplied by composition and deployment tooling without adding Kubernetes-specific behavior to boxes.

## First end-to-end foundation milestone

The foundation milestone is successful when this complete scenario works:

1. A developer starts from a greenfield repository and invokes the onboarding skill through a compatible coding agent.
2. The installer creates the database-free Rust Hello World project.
3. The capability works through both an in-process Rust call and HTTP.
4. The skill explains the Boxology model and identifies the coding agent using it as the lead agent, regardless of which compatible harness hosts it.
5. The developer asks that lead to add the backward-compatible `greet(name)` capability, for which `greet("Ada")` returns `Hello, Ada!`.
6. The resulting repository state touches no foreign package source, contains only permitted deterministic artifacts outside the Hello box, and behaves consistently through Rust and HTTP.
7. `boxology check` passes using the same visible validation path available to local development and generated CI.

This scenario proves installation, box definition, two bindings, one box-local evolution, deterministic validation, and harness-neutral lead guidance. It does not test remote execution, persistence, communication delivery, pull-request policy, or multi-agent parallelism.

## Explicit foundation-milestone non-goals

The foundation milestone does not promise:

- Migration of existing codebases.
- Operation on arbitrary or non-initialized repositories.
- Full native guarantees inside foreign-language boxes.
- Multi-repository coordination.
- A Postgres or Redis provider in the generated example.
- Kubernetes deployment.
- A first-party managed hosting service.
- A Boxology-owned coding-agent harness, sandbox, or execution service.
- A Boxology communication gateway or required human interface.
- Hermes, Slack, or any other specific harness and transport.
- A custom factory dashboard or task UI.
- A dedicated GitHub App, bot, or first-class GitHub integration.
- GitHub Issues as a task system.
- Worker, reviewer, area-lead, merger, or continuous-quality roles.
- A prescribed pull-request or autonomous-merging policy.
- A sophisticated or finalized internal factory execution engine.
- A separate factory control-plane service, task ledger, or mandatory event log.
- A mandatory workflow engine or queue.
- Harness-session persistence, frozen state, stop-and-resume, or crash recovery.
- Message catch-up or exactly-once external effects.
- Multi-agent task claims, leases, fencing, or split-brain prevention.
- Streaming, event streaming, or real-time interaction in the foundation milestone; these remain requirements for the first full box-runtime release.

Most are sequencing decisions rather than rejections of the broader direction. Reduced guarantees inside foreign-language implementation code are a deliberate boundary.

The factory-control-plane items above are accepted deferrals, not missing foundation specifications. They do not block implementation or acceptance of the skill-only milestone. Their next design gate is the post-MVP harness and agent-pool work in [issue #57](https://github.com/fontanierh/boxology/issues/57), when concrete coordination requirements exist.

Brownfield adoption should be reconsidered only after the greenfield milestones provide evidence worth generalizing. Exit remains reversible: a managed project is an ordinary Cargo workspace in the developer's Git repository, the runtime is a normal Rust dependency, and the factory is external software. Turning off the factory leaves the complete source and Git history under the developer's control; removing the runtime can proceed as an ordinary code migration rather than a data export or repository conversion.

## Open questions after this contract

The product boundary can be fixed while these implementation questions remain open:

- Whether and when Boxology should own an agent harness, gateway, execution service, or durability layer.
- Sandbox substrates and deployment recipes if Boxology later owns a deployable factory component, tracked in [issue #67](https://github.com/fontanierh/boxology/issues/67).
- Whether an optional lead-authored checkpoint should be standardized and where it should live.
- The portable skill's distribution and host-specific installation wrappers.
- Authentication and authorization for any future Boxology-owned gateway or factory service.
- Additional deployment recipes and a possible managed service.
- Kubernetes generation and operational conventions.
- The configuration language for future roles, handoffs, queues, and gates.
- The post-MVP multi-agent coordination and stronger durability guarantees tracked in [issue #57](https://github.com/fontanierh/boxology/issues/57).

They should be resolved through separate focused design work rather than expanding the foundation milestone.
