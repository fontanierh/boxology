# Product Contract and First Viable Slice

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document separates the long-term product direction from the first falsifiable vertical slice. It resolves the product-boundary questions without pretending that the eventual audience, agent harness, or deployment ecosystem is already known.

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

## Audience and operating envelope

The initial audience is intentionally not expressed as a mature customer profile. Early users will probably be hobbyists and other experimenters. The long-term ambition is much broader: to become an excellent general way to produce software.

The first supported technical envelope is narrow:

- A greenfield project.
- A repository created by the platform initializer.
- A Rust backend in one Git repository and Cargo workspace.
- Module packages, providers, compositions, runtime tooling, and factory-related packages kept as distinct crates or packages within that repository.
- GitHub as the source-review surface.
- Slack as the first human-facing factory interface.
- One repository managed by a factory installation at a time for the initial slice.

A monorepo is not treated as a monolith. Modules retain separate contracts and ownership even when they share a repository and Cargo lockfile. Changes to global derived artifacts require appropriate whole-workspace validation.

## Installation and onboarding

Installation begins through the developer's existing coding agent.

The project supplies a portable onboarding skill following the shared Agent Skills format. The same core workflow should be usable by compatible hosts such as Codex, Claude Code, Cursor, and other agents that support skills. Host-specific packaging may differ, but the product should not make one coding-agent application part of its architecture.

The onboarding skill provides judgment and guidance. A deterministic, versioned installer owns the actual mutations. The expected flow is:

1. The developer installs or gives the onboarding skill to a compatible coding agent.
2. The agent explains the setup and asks the minimum necessary questions.
3. The agent obtains and runs the project CLI.
4. The CLI creates the Rust workspace, module, composition, and repository configuration.
5. The agent guides deployment or connection of a remote factory.
6. It configures GitHub and Slack, registers the repository, verifies connectivity, and returns a working Slack entry point to the lead agent.

The module runtime is a normal Rust dependency. The factory is versioned software installed outside the target application's source, not factory source code copied into every repository. Only project-specific contracts and configuration belong in the target repository.

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

## First factory behavior

Slack is the first factory UI. A developer talks to one persistent lead agent. There is no custom dashboard or task interface in the first slice.

The lead agent handles the requested change itself:

1. Receive the request through Slack.
2. Create or resume an isolated remote code sandbox.
3. Work in a dedicated Git worktree and branch.
4. Validate the result according to the repository's current checks.
5. Push the branch and open a GitHub pull request.
6. Return the result and pull-request link through Slack.
7. Stop and wait. A human reviews and merges the pull request.

Autonomous merging is explicitly excluded from v1.

## Factory organization is configuration

The eventual factory may include a top-level lead, area leads, workers, reviewers, a merger, and continuous quality agents. Those roles must not be mandatory concepts hard-coded into the execution engine.

The runtime provides mechanisms for running agents, isolating work, preserving state, asking humans, and reporting results. Factory configuration determines which roles exist, how work moves between them, and which gates apply. This configuration may remain internal at first while the design is being adjusted.

The first configuration contains only the human-facing lead. GitHub Issues, worker agents, task assignment, review agents, and merger coordination are introduced progressively. If GitHub Issues becomes the task ledger, a factory-owned GitHub identity can perform actions while structured metadata identifies the logical agent and run responsible for each item.

## Agent harness boundary

The onboarding skill runs in the developer's existing coding agent. Agents inside the factory are different: they are expected to run through a purpose-built harness rather than through Codex CLI or Claude Code.

The internal harness remains an explicit open design question. The first product contract does not choose its model providers, agent loop, tool protocol, memory representation, or prompting architecture. It requires only the externally observable lead-agent behavior described above.

## Isolation, suspension, and recovery

Every factory agent receives an isolated remote code sandbox. The intended sandbox lifecycle is:

```text
create -> run -> suspend -> resume -> complete -> destroy
                   `-> checkpoint/recover
```

The target guarantee is that the sandbox and agent state can be frozen and resumed exactly as they were. Stopping the factory must not discard active work. If exact continuation fails, recovery may roll back to a recent durable checkpoint and repeat a small, bounded number of steps.

The following data may not be lost:

- The worktree and uncommitted changes.
- The branch and commits.
- The conversation and request.
- The run status and task history.
- Audit and delivery records.

The exact sandbox provider, checkpoint protocol, rollback bound, and reconciliation algorithm remain to be designed.

## Deployment topology

Modules do not decide how they are deployed. Application compositions own deployment topology and select whether modules are linked into one binary, exposed as separate services, or assembled into other process types.

Kubernetes is an important future target for any module-based application, as well as a possible deployment target for the factory itself. It is deliberately not part of the first slice. Kubernetes support should be supplied by composition and deployment tooling without adding Kubernetes-specific behavior to modules.

## First end-to-end proof

The first slice is successful when this complete scenario works:

1. A developer starts from a greenfield repository and invokes the onboarding skill through a compatible coding agent.
2. The installer creates the database-free Rust Hello World project.
3. The capability works through both an in-process Rust call and HTTP.
4. The onboarding agent deploys or connects a containerized factory on a remotely reachable machine.
5. GitHub and Slack are connected and the repository is registered.
6. The developer asks the lead agent in Slack to change the generated application.
7. The lead performs the change in a resumable isolated sandbox, worktree, and branch.
8. The lead opens a pull request and returns it in Slack.
9. The factory does not merge it; a human makes the merge decision.
10. Suspending and resuming the factory during work does not lose the run or its code state, subject only to the bounded checkpoint fallback.

This scenario proves installation, module definition, two bindings, remote factory access, human-agent interaction, isolated implementation, durable execution, and the human merge boundary.

## Explicit first-slice non-goals

The first slice does not promise:

- Migration of existing codebases.
- Operation on arbitrary or non-initialized repositories.
- Non-Rust server-side modules or general polyglot support.
- Multi-repository coordination.
- A Postgres or Redis provider in the generated example.
- Kubernetes deployment.
- A first-party managed hosting service.
- A custom factory dashboard or task UI.
- GitHub Issues as a task system.
- Worker, reviewer, area-lead, merger, or continuous-quality roles.
- Autonomous merging.
- A finalized internal factory agent harness.

These are sequencing decisions, not rejections of the broader direction.

## Open questions after this contract

The product boundary can be fixed while these implementation questions remain open:

- The factory's internal agent loop, models, providers, tools, and memory.
- The sandbox provider interface and exact suspension protocol.
- The factory's durable data model and reconciliation behavior.
- The portable skill's distribution and host-specific installation wrappers.
- Authentication and authorization for Slack, GitHub, and factory access.
- Additional deployment recipes and a possible managed service.
- Kubernetes generation and operational conventions.
- The configuration language for future roles, handoffs, queues, and gates.

They should be resolved through separate focused design work rather than expanding the first viable slice.
