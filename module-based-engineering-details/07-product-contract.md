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

The primary v1 operator is an individual developer or very small Rust team starting a greenfield backend, already using GitHub and a skill-compatible coding agent, and willing to operate a factory container on an SSH-reachable machine and interact with it through Slack.

This is the person able to evaluate the foundation milestone, not a claim about the eventual market. Early operators will probably be hobbyists and other experimenters. The long-term ambition is much broader: to become an excellent general way to produce software.

The first supported technical envelope is narrow:

- A greenfield project.
- A repository created by the platform initializer.
- A Rust backend in one Git repository and Cargo workspace.
- Application modules and compositions kept as distinct crates or packages within that repository.
- GitHub as the source-review surface.
- Slack as the only first-class human integration in the foundation milestone.
- One repository managed by a factory installation at a time for the foundation milestone.

A monorepo is not treated as a monolith. Modules retain separate contracts and ownership even when they share a repository and Cargo lockfile. Changes to global derived artifacts require appropriate whole-workspace validation.

## Repository and execution locations

The system distinguishes:

- **Product source repository:** contains the source of the module platform and factory. Through progressive dogfooding, this repository can itself become a managed project repository.
- **Managed project repository:** is initialized by the platform and connected to a factory. It contains application modules, compositions, tests, and repository-local factory configuration. It does not receive a copied implementation of the factory.
- **Deployed factory:** is the external running control plane connected to the managed project repository.
- **Agent sandbox:** is a resumable execution environment created and owned by the deployed factory. It checks out the managed repository and produces an isolated worktree, branch, and pull request.

These are roles rather than necessarily permanent physical boundaries. The product source repository is also eligible to be managed by a deployed factory once progressive dogfooding reaches that stage.

## Installation and onboarding

Installation begins through the developer's existing coding agent.

The project supplies a portable onboarding skill following the shared Agent Skills format. The same core workflow should be usable by compatible hosts such as Codex, Claude Code, Cursor, and other agents that support skills. Host-specific packaging may differ, but the product should not make one coding-agent application part of its architecture.

The onboarding skill provides judgment and guidance. A deterministic, versioned installer owns the actual mutations. The expected flow is:

1. The developer installs or gives the onboarding skill to a compatible coding agent.
2. The agent explains the setup and asks the minimum necessary questions.
3. The agent obtains and runs the project CLI.
4. The CLI creates the Rust workspace, module, composition, and repository configuration.
5. The agent guides deployment or connection of a remote factory.
6. It configures Slack, grants the factory ordinary repository and pull-request access, registers the repository, verifies connectivity, and returns a working Slack entry point to the lead agent.

The module runtime is a normal Rust dependency. The factory is versioned software installed outside the managed project's source, not factory source code copied into every repository. Only project-specific contracts and integration configuration belong in the managed repository.

GitHub is the initial repository and pull-request surface, but a dedicated GitHub App, bot workflow, Issues integration, or other first-class GitHub integration is not part of the foundation milestone. The factory can use ordinary Git and GitHub credentials to push a branch and open a pull request.

## Foundation release bundle

The first release bundle contains:

- A portable onboarding skill.
- A deterministic installer CLI.
- Rust module-runtime packages with Rust and HTTP bindings.
- A factory container containing the control plane, minimal execution engine, Slack integration, and sandbox-provider integration.
- A tested SSH-and-container deployment recipe.

The factory execution engine may be extremely small in this milestone. The bundle validates the factory-owned lifecycle and product boundary, not the novelty of its internal agent loop.

## Initial factory deployment

The first guaranteed factory deployment recipe is:

- A user-controlled machine reachable over SSH.
- A container runtime on that machine.
- The factory installed and run as a container.

This path covers a cloud virtual machine or other remotely reachable hardware without requiring a first-party managed service or provider-specific integration. The same container may run on the developer's current computer for evaluation. That is not the preferred collaborative setup because the factory becomes unavailable whenever the computer is unavailable.

The onboarding agent can later guide users through a first-party hosted service, common compute providers such as AWS or GCP, Kubernetes, or personal hardware. Agent-guided flexibility should remain grounded in deterministic, tested deployment recipes.

## Generated Hello World project

At the end of project initialization, the developer has a small running application that proves the central module abstraction:

- One typed Hello World capability is defined once.
- The capability can be invoked directly through the Rust module interface.
- The same capability can be invoked through an HTTP endpoint.
- The behavior comes from the same module contract rather than two handwritten implementations.

The first proof is database-free. Postgres and Redis remain important provider candidates, but persistence is a later slice and should not obscure validation of the module, binding, and factory loop.

The foundation milestone covers unary request-response only. The first full module-runtime release is expected to add streaming data, streaming events, and real-time interaction as first-class contract shapes.

## Native and foreign-language modules

Everything managed as part of an application should be represented as a module, but not every implementation receives the same guarantees.

- A **native module** is implemented in Rust and receives the full runtime, dependency-analysis, compatibility, and factory guarantees.
- A **foreign-language module** remains a first-class factory package with ownership, tasks, and the one-module pull-request boundary, but its internals receive fewer static and runtime guarantees.
- A **client-binding module** remains in the native managed ecosystem, owns the contract import, and generates a language-native SDK for a foreign module.

A TypeScript application or another foreign-language component can therefore appear anywhere the application composition requires. The platform manages its declared boundary but cannot make the same claims about its internal implementation that it makes for a native Rust module.

## First factory behavior

Slack is the first factory UI. A developer talks to one persistent lead agent. There is no custom dashboard or task interface in the foundation milestone.

The lead agent handles the requested change itself:

1. Receive the request through Slack.
2. Create or resume an isolated remote code sandbox.
3. Work in a dedicated Git worktree and branch.
4. Validate the result according to the repository's current checks.
5. Push the branch and open a GitHub pull request.
6. Return the result and pull-request link through Slack.
7. Stop and wait. A human reviews and merges the pull request.

Autonomous merging is explicitly excluded from v1.

The prescribed acceptance task is to add a new backward-compatible `greet(name)` capability to the generated Hello module. Calling it with `Ada` returns `Hello, Ada!` through both Rust and HTTP. Its pull request must:

- Change no foreign package source.
- Change only the Hello module source and permitted deterministic artifacts.
- Preserve consistent behavior through the Rust and HTTP bindings.
- Produce exactly one pull request.
- Never merge that pull request automatically.

This validates a real module ownership and binding invariant. It does not validate concurrent agent work or the broader safe-parallelism thesis; that requires a later experiment.

## Factory organization is configuration

The eventual factory may include a top-level lead, area leads, workers, reviewers, a merger, and continuous quality agents. Those roles must not be mandatory concepts hard-coded into the execution engine.

The factory control plane provides mechanisms for running agents, isolating work, preserving state, asking humans, and reporting results. Factory configuration determines which roles exist, how work moves between them, and which gates apply. This configuration may remain internal at first while the design is being adjusted.

The first configuration contains only the human-facing lead. GitHub Issues, worker agents, task assignment, review agents, merger coordination, and a dedicated GitHub identity are introduced progressively.

## Factory execution boundary

The onboarding skill runs in the developer's existing coding agent. Agents inside the factory run behind a factory-owned execution interface that preserves the lifecycle and behavioral guarantees in this contract.

The first implementation may wrap an existing runner, call model APIs directly, or use a bare-bones custom loop. The choice must not leak into the managed project contract. Model providers, tool protocols, memory, and the eventual agent architecture remain open design questions.

## Isolation, suspension, and recovery

Every factory agent receives an isolated remote code sandbox. The intended sandbox lifecycle is:

```text
create -> run -> suspend -> resume -> complete -> destroy
                   `-> checkpoint/recover
```

Recovery has two observable levels:

- **Graceful suspension:** the complete sandbox resumes exactly where it stopped.
- **Crash recovery:** all repository bytes and durable records through the last completed execution-engine tool action survive. Only the action that was in flight may be retried. GitHub and Slack side effects are reconciled without duplicate pull requests, comments, or messages.

The following data may not be lost:

- The worktree and uncommitted changes.
- The branch and commits.
- The conversation and request.
- The run status and task history.
- Audit and delivery records.

The exact sandbox provider, checkpoint protocol, and reconciliation algorithm remain to be designed. They must satisfy the observable guarantees above.

## Deployment topology

Modules do not decide how they are deployed. Application compositions own deployment topology and select whether modules are linked into one binary, exposed as separate services, or assembled into other process types.

Kubernetes is an important future target for any module-based application, as well as a possible deployment target for the factory itself. It is deliberately not part of the foundation milestone. Kubernetes support should be supplied by composition and deployment tooling without adding Kubernetes-specific behavior to modules.

## First end-to-end foundation milestone

The foundation milestone is successful when this complete scenario works:

1. A developer starts from a greenfield repository and invokes the onboarding skill through a compatible coding agent.
2. The installer creates the database-free Rust Hello World project.
3. The capability works through both an in-process Rust call and HTTP.
4. The onboarding agent deploys or connects a containerized factory on a remotely reachable machine.
5. Slack is connected, ordinary repository credentials are supplied, and the repository is registered.
6. The developer asks the lead agent in Slack to add the backward-compatible `greet(name)` capability, for which `greet("Ada")` returns `Hello, Ada!`.
7. The lead performs the change in a factory-owned resumable sandbox, worktree, and branch.
8. The resulting change touches no foreign package source, contains only permitted deterministic artifacts outside the Hello module, and behaves consistently through Rust and HTTP.
9. The lead opens exactly one pull request and returns it in Slack.
10. The factory does not merge it; a human makes the merge decision.
11. Graceful suspension resumes the complete sandbox exactly.
12. Crash recovery preserves repository bytes and durable records through the last completed tool action, retries at most the in-flight action, and does not duplicate GitHub or Slack effects.

This scenario proves installation, module definition, two bindings, one module-local evolution, remote factory access, human-agent interaction, isolated implementation, durable execution, and the human merge boundary. It is an end-to-end foundation milestone, not evidence that multi-agent parallelism is already effective.

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
- Streaming, event streaming, or real-time interaction in the foundation milestone; these remain requirements for the first full module-runtime release.

Most are sequencing decisions rather than rejections of the broader direction. Reduced guarantees inside foreign-language implementation code are a deliberate boundary.

Brownfield adoption should be reconsidered only after the greenfield milestones provide evidence worth generalizing. Exit remains reversible: a managed project is an ordinary Cargo workspace in the developer's Git repository, the runtime is a normal Rust dependency, and the factory is external software. Turning off the factory leaves the complete source and Git history under the developer's control; removing the runtime can proceed as an ordinary code migration rather than a data export or repository conversion.

## Open questions after this contract

The product boundary can be fixed while these implementation questions remain open:

- The factory's eventual agent loop, models, providers, tools, and memory beyond the minimal execution engine.
- The sandbox provider interface and exact suspension protocol.
- The factory's durable data model and reconciliation behavior.
- The portable skill's distribution and host-specific installation wrappers.
- Authentication and authorization for Slack, repository credentials, and factory access.
- Additional deployment recipes and a possible managed service.
- Kubernetes generation and operational conventions.
- The configuration language for future roles, handoffs, queues, and gates.

They should be resolved through separate focused design work rather than expanding the foundation milestone.
