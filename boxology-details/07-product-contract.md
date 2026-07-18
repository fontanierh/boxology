# Product Contract and Foundation Milestone

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document separates the long-term product direction from the first falsifiable foundation milestone. It resolves the product-boundary questions without pretending that the eventual market, agent implementation, or deployment ecosystem is already known.

## One product with two systems

The product combines:

- A Rust module platform for defining independently evolvable capabilities.
- A remotely hosted software factory for changing systems built with that platform.

These are one product, brand, and project. They remain separate applications and packages internally. The module platform reduces the amount of code and context an agent can affect at once. The factory makes the resulting increase in scaffolding, pull requests, and coordinated migrations practical.

The supported product journey combines both systems, but they are not fused technically. The module runtime is delivered as a Rust dependency and can execute without a running factory. The first factory release, however, only promises to operate on repositories initialized by the module platform; arbitrary repositories are out of scope.

## Progressive bootstrap

The system cannot depend on itself before it exists. Development therefore proceeds progressively:

1. Build the initial module foundation through conventional development.
2. Build the first factory without requiring it to modify its own source.
3. Make the factory the first substantial application built with the module system.
4. Introduce factory-assisted changes gradually.
5. Add task coordination, workers, reviews, merging, and continuous analysis as separate increments.
6. Increase self-hosting as each layer becomes dependable.

The goal is comprehensive dogfooding, not circular bootstrapping. A broken factory must remain repairable without needing that same factory to function.

## Primary v1 operator and operating envelope

The primary v1 operator is an individual developer or very small Rust team starting a greenfield backend, already using GitHub and a skill-compatible coding agent, and willing to provision or connect a durable lead-agent sandbox and interact with it through Slack.

This is the person able to evaluate the foundation milestone, not a claim about the eventual market. Early operators will probably be hobbyists and other experimenters. The long-term ambition is much broader: to become an excellent general way to produce software.

The first supported technical envelope is narrow:

- A greenfield project.
- A repository created by the platform initializer.
- A Rust backend in one Git repository and Cargo workspace.
- Application modules and compositions kept as distinct crates or packages within that repository.
- GitHub as the source-review surface.
- Slack as the only first-class human integration in the foundation milestone.
- One repository managed by a factory installation at a time for the foundation milestone.

A monorepo is not treated as a monolith. Modules retain separate contracts and ownership even when they share a repository and Cargo lockfile. An ordinary package change may include only the minimal lockfile resolution reproducible from its own permitted non-derived diff under pinned inputs. A lockfile change that alters a dependency used by another package requires whole-workspace validation and semantic reassessment.

## Repository and execution locations

The system distinguishes:

- **Product source repository:** contains the source of the module platform and factory. Through progressive dogfooding, this repository can itself become a managed project repository.
- **Managed project repository:** is initialized by the platform and connected to a factory. It contains application modules, compositions, tests, and repository-local factory configuration. It does not receive a copied implementation of the factory.
- **Lead-agent sandbox:** is the complete deployed factory in the foundation milestone. It contains the agent harness, Slack bridge, managed-repository checkout, worktree, branch, and persisted harness state.

The foundation has no separate factory service outside that sandbox. The product source repository is also eligible to be managed by a lead sandbox once progressive dogfooding reaches that stage.

## Installation and onboarding

Installation begins through the developer's existing coding agent.

The project supplies a portable onboarding skill following the shared Agent Skills format. The same core workflow should be usable by compatible hosts such as Codex, Claude Code, Cursor, and other agents that support skills. Host-specific packaging may differ, but the product should not make one coding-agent application part of its architecture.

The onboarding skill provides judgment and guidance. A deterministic, versioned installer owns the actual mutations. The expected flow is:

1. The developer installs or gives the onboarding skill to a compatible coding agent.
2. The agent explains the setup and asks the minimum necessary questions.
3. The agent obtains and runs the project CLI.
4. The CLI creates the Rust workspace, module, composition, and repository configuration.
5. The agent provisions or connects a durable lead sandbox and installs the factory sandbox image or equivalent package into it.
6. It configures the sandbox's Slack bridge, grants the lead ordinary repository and pull-request access, verifies connectivity, starts the harness, and returns the working Slack channel.

The module runtime is a normal Rust dependency. The factory is versioned software installed inside the lead sandbox, not factory source code copied into every managed repository. Only project-specific contracts and integration configuration belong in the managed repository.

GitHub is the initial repository and pull-request surface, but a dedicated GitHub App, bot workflow, Issues integration, or other first-class GitHub integration is not part of the foundation milestone. The factory can use ordinary Git and GitHub credentials to push a branch and open a pull request.

## Foundation release bundle

The first release bundle contains:

- A portable onboarding skill.
- A deterministic installer CLI.
- Rust module-runtime packages with Rust and HTTP bindings.
- A portable factory sandbox image containing an agent harness and Slack bridge.
- Bootstrap support for a managed durable-sandbox provider or a compatible container target with crash-consistent durable storage and restart behavior.

The agent harness may be an existing runner or an extremely small wrapper in this milestone. The bundle validates the sandbox lifecycle and product boundary, not the novelty of its internal agent loop.

## Initial factory deployment

The MVP has one deployed object: the durable lead sandbox. It can be supplied by a managed sandbox provider or run from the project's portable container image on a compatible target.

A self-hosted target must provide crash-consistent durable storage for the repository and harness state plus restart behavior for the bridge and harness. An ephemeral container can execute the image but does not satisfy the recovery guarantee. The exact managed provider, cloud, container host, and storage mechanism remain implementation choices.

This keeps the deployment portable across hosted sandbox systems, cloud virtual machines, personal hardware, and later Kubernetes targets without introducing a second control-plane service.

## Generated Hello World project

At the end of project initialization, the developer has a small running application that proves the central module abstraction:

- One implementation method is annotated as a typed Hello World capability.
- Its boundary types are authored beside the implementation and lifted into the generated contract crate.
- A deterministic pre-Cargo step generates its language-neutral schema, contract crate, asynchronous typed handle, implementation-neutral dispatch interface, and implementation-local adapter without a second hand-maintained API.
- The capability can be invoked directly through the Rust module interface.
- The same capability can be invoked through an HTTP endpoint.
- Both paths reach the same annotated implementation method through the generated contract rather than two handwritten interfaces or implementations.

The generated handle uses the invocation envelope in [Canonical Capability Contract](09-capability-contract.md): an explicit call context, a declared domain error, and a distinct invocation-error layer even when the selected binding is in-process.

The first proof is database-free. Postgres and Redis remain important provider candidates, but persistence is a later slice and should not obscure validation of the module, binding, and factory loop.

The deterministic contract generator is therefore a core foundation deliverable rather than incidental scaffolding. The milestone is not complete until generation, provenance, reproducibility, typed invocation, and compatibility classification work as one path.

The foundation milestone covers unary request-response only. The first full module-runtime release is expected to add streaming data, streaming events, and real-time interaction as first-class contract shapes.

## Native and foreign-language modules

Everything managed as part of an application should be represented as a module, but not every implementation receives the same guarantees.

- A **native module** is implemented in Rust and receives the full runtime, dependency-analysis, compatibility, and factory guarantees.
- A **foreign-language module** remains a first-class factory package with ownership, tasks, and the one-package pull-request boundary, but its internals receive fewer static and runtime guarantees.
- A **client-binding module** remains in the native managed ecosystem, owns the contract import, and generates a language-native SDK for a foreign module.

A TypeScript application or another foreign-language component can therefore appear anywhere the application composition requires. The platform manages its declared boundary but cannot make the same claims about its internal implementation that it makes for a native Rust module.

## First factory behavior

Slack is the first factory UI. A developer talks to one persistent lead agent. There is no custom dashboard or task interface in the foundation milestone.

The Slack bridge must not depend only on live event delivery. When it starts or resumes, it catches up on requests still available to it in the configured channel history that arrived while it was unavailable. This recovery is necessarily bounded by the history the Slack workspace retains and permits the bridge to read.

The lead agent handles the requested change itself:

1. Receive the request through Slack.
2. Continue inside its durable remote sandbox and managed-repository checkout.
3. Create or resume a dedicated Git worktree and branch.
4. Validate the result according to the repository's current checks.
5. Push the branch and open a GitHub pull request.
6. Return the result and pull-request link through Slack.
7. Stop and wait. A human reviews and merges the pull request.

Autonomous merging is explicitly excluded from v1.

Before retrying an external effect whose outcome is uncertain, the lead inspects the current state of the system that owns that effect. This is a standing recovery rule, not an exactly-once guarantee: an ambiguous failure may still produce a rare repeated effect.

The prescribed acceptance task is to add a new backward-compatible `greet(name)` capability to the generated Hello module. Calling it with `Ada` returns `Hello, Ada!` through both Rust and HTTP. Its pull request must:

- Change no foreign package source.
- Change only the Hello module source and permitted deterministic artifacts.
- Preserve consistent behavior through the Rust and HTTP bindings.
- Produce exactly one pull request.
- Never merge that pull request automatically.

This validates a real module ownership and binding invariant. It does not validate concurrent agent work or the broader safe-parallelism thesis; that requires a later experiment.

## Factory organization is configuration

The eventual factory may include a top-level lead, area leads, workers, reviewers, a merger, and continuous quality agents. Those roles must not be mandatory concepts hard-coded into the execution engine.

As the factory grows, its shared substrate will need mechanisms for running agents, isolating work, preserving state, asking humans, and reporting results. Factory configuration determines which roles exist, how work moves between them, and which gates apply. This configuration may remain internal at first while the design is being adjusted.

The first configuration contains only the human-facing lead. GitHub Issues, worker agents, task assignment, review agents, merger coordination, and a dedicated GitHub identity are introduced progressively.

## Factory execution boundary

The onboarding skill runs in the developer's existing coding agent. Agents inside the factory run behind a factory-owned execution interface that preserves the lifecycle and behavioral guarantees in this contract.

The first implementation may wrap an existing runner, call model APIs directly, or use a bare-bones custom loop. The choice must not leak into the managed project contract. Model providers, tool protocols, memory, and the eventual agent architecture remain open design questions.

## Isolation, suspension, and recovery

The complete foundation factory runs in one durable lead-agent sandbox:

```text
durable lead sandbox
|-- agent harness
|-- Slack bridge
|-- repository checkout and worktree
`-- persisted harness state
```

Its foundation recovery boundary is:

- **Normal recovery:** process restarts, unclean harness termination, sandbox stop-and-resume, and replacement of lost compute while its durable storage survives recover the repository and persisted harness state from a valid, internally consistent recovery point. Work after that point may need to be repeated. A managed sandbox may freeze and resume its full state; a container target must provide equivalent durable storage, crash-consistent persistence, and restart behavior. The exact persistence mechanism is an implementation choice.
- **Catastrophic sandbox loss:** simultaneous loss of the sandbox and its durable storage is outside the foundation persistence guarantee. A fresh lead can reconstruct the project's semantic state from the repository and Git history, project instructions and documentation, GitHub issues, branches, pull requests, reviews and comments, Slack history that remains retained and accessible, and any optional lead-authored checkpoint.

Catastrophic reconstruction does not promise exact continuation of hidden reasoning, preservation of uncommitted work, or recovery of an action that existed only inside the destroyed sandbox. It is semantic recovery: before repeating an external action, the lead inspects current GitHub and Slack state, just as a human would notice that a pull request or reply already exists. Rare duplicate or repeated effects after ambiguous failures remain possible; the foundation does not promise exactly-once delivery or require a central outbox or deduplication ledger.

The foundation does not require a factory database, event ledger, queue, or workflow engine. Agents may emit events when useful for observability, and the lead may write a checkpoint to the repository or a future factory-owned store, but neither is a required source of truth in this milestone. The backend used for stronger future coordination remains open.

## Deployment topology

Modules do not decide how they are deployed. Application compositions own deployment topology and select whether modules are linked into one binary, exposed as separate services, or assembled into other process types.

Kubernetes is an important future target for any module-based application, as well as a possible deployment target for the factory itself. It is deliberately not part of the foundation milestone. Kubernetes support should be supplied by composition and deployment tooling without adding Kubernetes-specific behavior to modules.

## First end-to-end foundation milestone

The foundation milestone is successful when this complete scenario works:

1. A developer starts from a greenfield repository and invokes the onboarding skill through a compatible coding agent.
2. The installer creates the database-free Rust Hello World project.
3. The capability works through both an in-process Rust call and HTTP.
4. The onboarding agent provisions or connects one durable lead sandbox through a managed provider or a compatible persistent container target.
5. The factory image or equivalent package is installed there; Slack is connected; ordinary repository credentials are supplied; and the harness is started.
6. The developer asks the lead agent in Slack to add the backward-compatible `greet(name)` capability, for which `greet("Ada")` returns `Hello, Ada!`.
7. The lead performs the change inside that sandbox in a dedicated worktree and branch.
8. The resulting change touches no foreign package source, contains only permitted deterministic artifacts outside the Hello module, and behaves consistently through Rust and HTTP.
9. The lead opens exactly one pull request and returns it in Slack.
10. The factory does not merge it; a human makes the merge decision.
11. Stopping and resuming the sandbox, or replacing its compute while durable storage survives, preserves its repository and persisted harness state.
12. Terminating the harness uncleanly during work and restarting it recovers a valid repository and harness state from a consistent recovery point; the lead inspects current Slack and GitHub state before retrying any uncertain external effect.
13. A request sent to the configured Slack channel while the bridge is stopped is discovered after restart when it remains available in channel history.

This scenario proves installation, module definition, two bindings, one module-local evolution, remote factory access, human-agent interaction, isolated implementation, clean and unclean storage-backed recovery, Slack catch-up, and the human merge boundary. It is an end-to-end foundation milestone, not evidence that multi-agent parallelism, exactly-once effects, or catastrophic exact recovery is already effective.

## Explicit foundation-milestone non-goals

The foundation milestone does not promise:

- Migration of existing codebases.
- Operation on arbitrary or non-initialized repositories.
- Full native guarantees inside foreign-language modules.
- Multi-repository coordination.
- A Postgres or Redis provider in the generated example.
- Kubernetes deployment.
- A first-party managed hosting service.
- A custom factory dashboard or task UI.
- A dedicated GitHub App, bot, or first-class GitHub integration.
- GitHub Issues as a task system.
- Worker, reviewer, area-lead, merger, or continuous-quality roles.
- Autonomous merging.
- A sophisticated or finalized internal factory execution engine.
- A separate factory control-plane service, task ledger, or mandatory event log.
- A mandatory workflow engine or queue.
- Exact continuation after the sandbox and its durable storage are both destroyed.
- Exactly-once GitHub or Slack effects.
- Multi-agent task claims, leases, fencing, or split-brain prevention.
- Streaming, event streaming, or real-time interaction in the foundation milestone; these remain requirements for the first full module-runtime release.

Most are sequencing decisions rather than rejections of the broader direction. Reduced guarantees inside foreign-language implementation code are a deliberate boundary.

The factory-control-plane items above are accepted deferrals, not missing foundation specifications. They do not block implementation or acceptance of the one-sandbox milestone. Their next design gate is the post-MVP agent-pool work in [issue #57](https://github.com/fontanierh/module-based-engineering/issues/57), when concrete coordination requirements exist.

Brownfield adoption should be reconsidered only after the greenfield milestones provide evidence worth generalizing. Exit remains reversible: a managed project is an ordinary Cargo workspace in the developer's Git repository, the runtime is a normal Rust dependency, and the factory is external software. Turning off the factory leaves the complete source and Git history under the developer's control; removing the runtime can proceed as an ordinary code migration rather than a data export or repository conversion.

## Open questions after this contract

The product boundary can be fixed while these implementation questions remain open:

- The factory's eventual agent loop, models, providers, tools, and memory beyond the minimal execution engine.
- The first managed sandbox provider and the exact container storage, restart, and suspension recipes.
- Whether an optional lead-authored checkpoint should be standardized and where it should live.
- The portable skill's distribution and host-specific installation wrappers.
- Authentication and authorization for Slack, repository credentials, and factory access.
- Additional deployment recipes and a possible managed service.
- Kubernetes generation and operational conventions.
- The configuration language for future roles, handoffs, queues, and gates.
- The post-MVP multi-agent coordination and stronger durability guarantees tracked in [issue #57](https://github.com/fontanierh/module-based-engineering/issues/57).

They should be resolved through separate focused design work rather than expanding the foundation milestone.
