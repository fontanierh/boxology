# Module-Based Engineering Design Interview

[Back to the white paper](../module-based-engineering-whitepaper.md)

This is a comprehensive chronological record of the substantive Q&A that produced the initial white paper, followed by later decision interviews where identified. It is lightly edited and condensed for readability rather than presented as a verbatim chat transcript. It preserves the ideas, uncertainty, clarifications, and decisions discussed at each stage.

## Opening concept

The initial proposal combined two ideas.

The first was a Rust application architecture built from self-contained modules. A module would define typed capabilities or endpoints and could be used through different forms, including a Rust library, RPC, HTTP, or CLI. Modules would remain isolated enough that agents could understand and modify them independently.

The second was a persistent software factory. A coordinator would prioritize and decompose work; agents would pick up ready tasks; review and CI would be enforced; a merger would serialize changes; workers would wait for merge feedback; humans could answer questions or approve decisions; and a perpetual agent would analyze the coherence of the codebase and publish further work.

The first synthesis separated this into a module plane and a factory plane. The common thesis was that module architecture could become a concurrency-control mechanism for autonomous software development.

## Question 1: What promise should every module make?

**Question:** What fundamental promise should every module make, regardless of whether it is accessed as Rust code, a CLI, or RPC?

**Answer:** The module should define a service that can be edited in a backward-compatible way so it does not break other modules and the developer does not have to think about those other implementations. At the same time, it should provide consistent ways to call into the service based on configuration.

The answer was acknowledged as difficult to phrase precisely.

**Captured formulation:**

> A module is a self-contained capability that can evolve independently behind a stable, versioned contract. The platform preserves compatibility for its consumers and exposes the contract consistently through configured adapters or bindings such as Rust, CLI, and RPC.

The three promises identified were independent evolution, enforced contract compatibility, and consistent configurable invocation.

## Question 2: What happens when compatibility cannot be preserved?

**Question:** What should happen when a module genuinely needs a change that cannot remain backward-compatible?

**Answer:** Breaking changes should ideally be rare, but they will happen. Versioning allows an older way to be deprecated gradually. The harness can identify affected modules, create one adaptation task for each, and use callbacks to report when consumers no longer use the old contract. A final task can then remove the deprecated API. Continuous quality analysis should also detect deprecations that have remained too long.

The system should treat internal contracts with discipline similar to public APIs. This creates more scaffolding and more pull requests than a codebase-wide atomic edit, but agent productivity should make that cost acceptable.

A strong invariant was introduced in module-specific terms: a pull request always changes one module, and a merge request may never change several modules at once. Question 3 records the later generalization from module to accountable package.

**Clarification:** Callbacks should update a durable migration record rather than depend on one long-lived agent session. Any suitable worker can resume responsibility from that record.

**Captured formulation:**

> A cross-module change is never one patch. It is a coordinated migration composed of independently mergeable, single-package changes.

The lifecycle became: add the new version, identify consumers, migrate each consumer separately, verify zero remaining use, and remove the old version in a final change to the module providing the contract.

## Question 3: How are shared repository artifacts handled?

**Question:** Under the then-current one-module-per-pull-request rule, how should repository-wide artifacts such as workspace configuration, dependency lockfiles, shared schemas, generated clients, CI configuration, and the runtime be classified and changed?

The initial response was a request for a recommendation.

**Proposed answer:** Define the invariant as one semantic owner per pull request rather than one directory. A change can modify its target package's non-derived files and deterministic artifacts derived from them, but never another package's non-derived files.

The proposed ownership model was:

- Contracts belong to their providing modules.
- Generated clients are published artifacts adopted separately by consumers.
- Shared domain types require an owning contract module.
- Runtime and CI belong to platform packages.
- Deployment configuration belongs to an application composition package.
- Lockfiles and indexes are derived artifacts.

The shared lockfile was identified as an awkward but acceptable initial derived artifact. Independent build roots could be considered later if it causes real cross-package breakage.

**Answer:** This was accepted as a good way to express the rule.

**Captured formulation:**

> One pull request has one accountable package and zero foreign source changes. It may also contain mechanically reproducible artifacts attributable to that package.

**Later refinement:** Review generalized the original module phrasing into four accountable package kinds: module, provider, application composition, and platform. Every kind can own source, a factory area, a pull request, and a quality contract. The merger resolves ownership from the base revision, requires exactly one non-derived owner, and regenerates declared derived outputs byte-for-byte from that package's complete permitted non-derived diff under pinned inputs. A shared `Cargo.lock` is reproducible but not semantically harmless: resolution changes outside the minimal closure are rejected, while changes affecting dependencies used by foreign packages trigger whole-workspace validation and reassessment. The concrete manifest syntax remains open; these semantics do not.

## Question 4: Where do exposure and authentication policy live?

**Question:** Where should exposure and authentication policy live when a module might contain code-only, internal-service, and user-facing operations?

**Answer:** The runtime should have a standard way to define an endpoint or capability. The endpoint API should declare what authentication is expected. A module can define a default, while individual endpoints can override it.

A code-only module can default every method to code-only access. A user-facing service can default to requiring authentication and explicitly mark an endpoint public when needed.

The system also needs flexible authentication providers for web applications.

**Clarification:** Exposure, identity, and permission were separated. The working categories were code/internal/external exposure, normalized caller identity, and a capability or scope. External reachability does not imply anonymous access.

**Later refinement:** The effective endpoint declaration sets maximum reachability. Compositions may omit or narrow an endpoint but never widen it; raising the maximum belongs to the module as a contract change. Code-only endpoints receive no platform-generated network route. Internal endpoints are restricted to a declared trust zone but still require authentication and authorization. These guarantees govern platform-generated routing and do not sandbox mutually trusted code in the foundation's shared-process isolation profile.

The initial `User(user_id)` concept was considered too narrow.

## Identity refinement within question 4

**Concern:** A user identity needs at least a provider or realm and a user identifier. There may be several web applications, including customer-facing and internal applications. The same external OAuth identity could map to different users in different applications.

**Refined model:** Separate the application principal from authentication evidence.

```text
Principal {
  realm
  subject
  kind
}

Authentication {
  provider
  external_subject
}
```

An authentication provider maps its external identity into a principal owned by an application realm.

Realms should support generic, flexible adapters. Examples discussed included Auth0, WorkOS, Google authentication, and custom systems. A module should not depend on those vendors directly; it should see the normalized principal.

**Captured formulation:**

> Authentication providers are runtime adapters. Modules depend only on normalized, realm-scoped principals and declared permissions.

## Question 5: How are identities across realms related?

**Question:** Where should linking identities across different realms live, and how much should the runtime know about those relationships?

The first phrasing was unclear, so a concrete example was used: one person logs into a customer application through Google and an internal application through WorkOS. Should the runtime know they are the same person?

The useful tests discussed were whether matching email addresses should link accounts, whether access in one realm should imply access in another, how compromise would propagate, and whether audit logs should show one global person.

**Answer:** Realm identities should remain separate. If an employee has accounts in both applications and the relationship is useful, an employee module can own a store that links those principals and returns an employee profile. Other applications can use that module when they need the relationship.

**Captured formulation:**

> The runtime treats realm-scoped principals as independent. Domain modules may explicitly associate multiple principals with a shared entity.

## Question 6: How is authentication tested from development tools?

**Question:** After a caller is authenticated, what should the runtime decide before invoking a module and what should remain module logic?

**Answer and redirection:** The immediate concern was that a production authentication adapter, such as a cookie-based web system, would not naturally work through a CLI. The desired system should allow a secondary development-only authentication mechanism that maps to the same principal and permissions. That would make it easy to test through CLI a module exposed through HTTP in production.

**Clarification:** The production and development authentication adapters should feed the same normalized principal into the same runtime policy and module logic.

```text
production credential -> production adapter --+
                                             +-> same principal -> same policy -> module
development credential -> development adapter-+
```

Development authentication is an alternative credential source, not alternative authorization. It does not test the production adapter's credential verification, so that adapter still requires integration tests.

The division of responsibility discussed was:

- The authentication realm establishes who the caller is.
- The runtime carries the principal and enforces the declared coarse policy.
- The module decides what the caller may do with module-owned resources.

## Question 7: Who owns development identities?

**Question:** How should a CLI obtain a development identity safely, and where should named development identities be maintained?

The initial response requested a recommendation.

**Proposed answer:** Use named development identities rather than arbitrary production impersonation:

```text
alice_customer
support_agent
company_admin
```

A local development issuer creates a short-lived credential bound to the development runtime. Production runtimes do not include or trust that issuer. Privileged identities must be explicitly declared, and development-authenticated calls are identifiable in audit information.

The ownership split proposed was:

- The runtime owns the principal format and development credential mechanism.
- The authentication realm owns named development identities and their claims.
- Feature modules reference identities and create their own module-specific test data.

For example, billing can authenticate as `alice_customer` and seed invoices for that principal without defining the credential itself.

**Answer:** This separation felt correct.

**Captured formulation:**

> Authentication realms own version-controlled development identities. The runtime mints safe development credentials, while feature modules reference those identities in their own test scenarios.

## Question 8: How should CLI generation work?

**Question:** When a module endpoint is defined, what CLI should be generated automatically and where should the author need to customize it?

**Answer:** Ideally there should be no customization. The API for defining capabilities or endpoints should collect all information required to turn the endpoint into a CLI.

**Clarification:** A generic structured invocation and an ergonomic schema-derived CLI can both come from the same contract. The metadata needed includes names, documentation, types, defaults, validation, structured output and errors, authentication policy, streaming or progress behavior, files or secrets, and side-effect or confirmation information.

The principle captured at this stage was that custom CLI implementation code should be unnecessary.

## Question 9: Who is the CLI for?

**Question:** Who is the generated CLI intended for, and what stability promise should it make?

**Answer:** CLI is simply one transport or binding. Most systems will primarily use RPC or HTTP. CLI can be useful for development testing, but calling the real API is also possible. Some modules may genuinely be internal command-line tools built using the same module system.

The system should be able to support many transports. RPC, HTTP, server-sent events, and other mechanisms were mentioned. Protobuf was discussed as an encoding or schema rather than itself a transport.

**Captured formulation:**

> CLI is one optional runtime binding, not a required public interface or a special module type.

The runtime can offer a generic development CLI, while an application may deliberately package a CLI as a supported product.

## Question 10: Which interaction patterns are required?

**Question:** Beyond ordinary request-response, which interaction patterns should the first version support?

**Answer:** Request-response is required, but streaming should also be fully supported, including streams of data and events. Real-time systems should be supported as well.

Some modules may be backed by durable workflow engines. An agent-harness module, for example, could run an agent loop in a persistent workflow, receive messages, produce work, and write events for consumers.

**Initial clarification:** Transport adapters and execution adapters were separated, and a generic persistent-instance lifecycle was proposed.

**Correction:** The runtime should not be opinionated about Temporal or any workflow engine. Temporal may simply be an implementation choice inside module code. If shared workflow infrastructure later benefits from consistent tooling, it can become a provider. The runtime should not adopt vendor-specific lifecycle concepts.

**Captured formulation:**

> Request-response, streaming data, streaming events, and real-time interaction are runtime-level contract concerns. A particular durable workflow engine is not.

## Question 11: What is the generic adapter or provider model?

**Question:** Where should transport, authentication, and execution adapters be configured?

**Answer and redirection:** It was not clear that workflow engines belong in the runtime at all. The broader idea was a generic system that creates consistency around shared technologies.

Postgres was the main example. A module could require a store, and a Postgres integration could automatically establish a migrations folder, ORM configuration, local setup, and consistent runtime configuration while preserving module isolation. Redis was another likely integration. Temporal might later provide shared namespace or worker conventions without becoming a runtime primitive.

**Clarification:** The word adapter was split into requirement, provider, and binding:

- A requirement states what a module needs.
- A provider implements that type of requirement.
- A binding is the provider instance assigned to the module in an environment.

The provider's responsibilities can span scaffolding, configuration, runtime injection, migration tooling, CI, provisioning, and health conventions. This makes the provider system broader than the runtime itself.

**Answer:** Postgres and Redis were identified as interesting providers to ship.

Semantic requirements were preferred over vendor requirements, such as relational storage, cache, key-value storage, and pub-sub. A module can request vendor-specific behavior explicitly when necessary.

### Is a provider a module?

**Question:** Is a provider itself a module?

**Answer:** The recommendation was no, but both are versioned packages. A module represents product capability; a provider describes how the platform satisfies technical infrastructure. A provider may contain libraries, tooling, deployment logic, and supporting services. A supporting service that exposes product capability can itself be a module.

The package kinds initially identified were module packages, provider packages, and application composition packages. Later ownership review promoted the already-mentioned platform package into a fourth first-class kind for the runtime, CI, build tooling, repository-wide generators, and enforcement machinery.

### What is an application composition package?

**Question:** What does application composition package mean?

**Answer:** It is the deployable assembly of modules and providers. It selects modules and versions, endpoint exposure, transports, provider bindings, authentication realms, application configuration, integration tests, and the final build or deployment.

It should contain almost no business logic. A monolith may have one composition containing many modules; independently deployed services may each have a small composition.

## Question 12: What storage isolation does a provider guarantee?

**Question:** When several modules share a Postgres provider, what should one module be allowed to know or do with another module's data?

**Answer:** Nothing. The physical implementation belongs to the provider. It can use separate servers, separate databases, separate schemas, or even isolated tables in one database.

The provider's role is to isolate its instances. A consumer assumes it can access only its own state. If that is false, the provider is defective. The runtime does not protect the system from badly written provider code; creating a provider carries responsibility for satisfying its contract.

**Captured formulation:**

> Every provider binding is private to one module. Physical topology is provider-specific, but the provider must prevent one binding from accessing another.

**Later refinement:** Privacy is an architectural contract whose enforcement strength is declared by the composition. L0 relies on mutually trusted code and convention; L1 uses least-privilege binding credentials; L2 adds process, operating-system, network, and resource isolation; L3 adds sandboxing, controlled egress, and unforgeable scoped capabilities for untrusted code. Conformance tests provide evidence for a claimed profile but cannot prove universal non-access. This supersedes any reading of the original formulation as an unconditional security guarantee.

## Question 13: How does data cross module boundaries?

**Question:** How should a module obtain and retain information owned by another module without violating storage isolation?

The first response requested a concrete explanation.

The example used was a customer module that owns names and email addresses and a billing module that owns invoices. Billing cannot join the customer database, so it could call the customer interface, consume update events into a local projection, or receive an explicit snapshot when creating an invoice.

The standard microservice approaches were explained as synchronous API calls, event-driven local projections, snapshots, and workflow orchestration. The tradeoffs are availability coupling, eventual consistency, and deliberate historical staleness.

**Answer:** Data exchange should go through the normal module interface. With backward-compatible contracts, clear deprecation, and single-package pull requests, this is sufficient.

**Captured formulation:**

> Modules exchange data only through normal versioned interfaces, including request-response calls, event streams, and explicit snapshots; never through another module's store.

## Question 14: How are dependencies represented?

**Question:** When billing uses `Customer.get_contact`, how should that dependency be represented so the runtime and factory can identify consumers, enforce compatibility, and coordinate deprecations?

**Answer:** Within Rust, calls should always use a function exposed by the runtime. That creates a trackable dependency. An external-facing module is marked as such, although a completely foreign JavaScript client cannot receive the same static guarantees automatically.

Language adapters or bindings could bring TypeScript, iOS, and other clients back into the managed dependency system.

**Clarification:** Runtime observation alone is insufficient for a dependable dependency graph, so a module should declare imported contracts statically and receive typed runtime capability handles. Runtime telemetry then records actual use.

A client binding was defined as a thin module inside the Rust-managed ecosystem. It imports the contract, generates a language-native SDK, and makes that SDK available to the client application. This keeps generation and migration visible to the factory.

**Captured formulation:**

> Dependencies are declared statically, invoked through typed runtime capabilities, and observed dynamically. Managed external clients participate through generated client-binding modules.

**Later refinement:** The common ownership manifest is authoritative. Workspace CI must reject Rust dependency edges between modules that lack a corresponding declared contract import and every edge to a crate classified as a foreign implementation. The crate-classification topology remains separate design work. Runtime or generated code supplies handles only for declared imports. Raw networking, filesystem access, build scripts, or dynamic topics used to bypass those boundaries are quality violations; the foundation detects them where possible but does not technically prevent them under convention-level isolation.

## Question 15: When can an unknown public consumer be disconnected?

**Question:** When external consumers cannot be enumerated or automatically migrated, what evidence should allow a deprecated public endpoint to be removed?

**Answer:** The harness and the agent managing the deprecation should assess whether removal is safe. The agent may have access to Datadog or other monitoring. A human may also authoritatively state that the endpoint can be removed.

**Clarification:** The agent should assemble an evidence packet containing managed-consumer status, runtime traffic, remaining caller identities if known, deprecation policy or deadline, monitoring and errors, and any required human approval. The harness applies the configured policy; the agent gathers evidence and recommends the action.

No universal policy window or automatic-removal threshold was selected.

## Question 16: Are dependency cycles a problem?

**Question:** Suppose billing calls customer and customer also calls billing. How should the architecture prevent or resolve the cycle?

**Response:** With backward-compatible changes and single-package pull requests, a graph-level cycle did not appear to be an inherent problem.

**Clarification:** The remaining concerns are operational rather than merge-related: recursive synchronous calls, timeouts and retries, availability coupling, startup ordering, isolated testing, and unclear ownership. A graph-level cycle can exist without causing a recursive request path.

**Answer:** The harness, especially the perpetual quality agent, should aggressively guide planners away from cycles and create tasks to mitigate them. This is an engineering and programming problem, not a responsibility of the runtime.

**Captured formulation:**

> Cycles are a quality and planning concern, not a runtime primitive. The factory analyzes and mitigates them while the runtime remains neutral.

## Question 17: How do concurrent tasks in one module work?

**Question:** How should the factory handle several ready tasks that all want to change the same module at the same time?

**Answer:** There should be a coordinator per module, or per hard-coded logical area for a large module. The coordinator maintains a broad area plan that every agent sees and publishes ready tasks with priority.

Agents work independently without communicating once they start. They submit merge requests. The merger processes them in priority order within an area. Between areas, arrival time is the tie-breaker.

If a change is no longer mergeable after another task merges, it is returned to its worker. Other affected agents are notified, inspect what merged, reason about changed requirements or substrate, revise their work, and resubmit.

**Clarification:** This is optimistic parallel development with serialized integration rather than an exclusive module lock. Tasks should record the plan and base they began from. Rework belongs to the durable task and can be resumed by a compatible agent rather than requiring the same model session.

Automatic priority aging was mentioned as a possible starvation control but was not adopted as a decision.

## Question 18: How is affected work detected?

Before answering, the governance model was expanded.

**Governance addition:** A top-level lead agent should be the human interface and the big boss of the harness. Humans can push authoritative information to it. It can ask humans for decisions, reshuffle areas, and reprioritize one area above another. It needs a proper interface with strong approval requests.

**Question:** After merging one task, how should the merger determine which in-progress tasks require reassessment beyond file overlap?

**Answer:** Git conflicts are the obvious first signal. CI and integration tests run on the next merge candidate; failures return the task to its agent. If the same module changed after an agent produced its work, the merger can coordinate with the area lead to determine whether semantic rework is needed.

**Clarification:** The layered signals captured were Git conflict, CI failure, target-module change, imported-contract change, and changed area plan or human guidance. Passing Git and CI is necessary but does not eliminate semantic reassessment after relevant intervening changes.

## Question 19: What evidence must a merge request provide?

**Question:** What minimum evidence should every single-package pull request provide before the merger can accept it?

**Answer:** Engineers and users are responsible for defining excellent CI for their modules so that behavior is checked automatically. CI can and should include AI reviewers. Strong guarantees should be baked into the harness, while the runtime makes no quality promise.

**Captured formulation:**

> Each module declares its quality contract. The harness refuses merge until declared and mandatory checks pass. The system guarantees enforcement of the process, not complete correctness.

Provider packages can have their own conformance tests, including isolation guarantees.

## Question 20: How is the quality contract protected?

**Question:** How should changes to a module's own CI and quality rules be governed so an implementation agent cannot weaken the checks simply to pass its pull request?

**Answer:** One possible default is to force human approval whenever a change touches CI or check definitions. GitHub checks can provide the enforcement. Some teams may want a far more permissive mode and let agents act autonomously. The platform should not impose one universal policy, but should ship good defaults and make the safe path easy.

**Captured formulation:**

> CI and policy files are protected control-plane artifacts. By default, changing them requires human approval. Teams may select a more autonomous policy, but weakening protection must be explicit and auditable.

## Follow-up: Product boundary and foundation milestone

Issue #1 asked whether the proposal was a philosophy, Rust framework, runtime, tooling ecosystem, hosted factory, or some combination, and requested a falsifiable first slice. A follow-up interview established the following decisions.

### Product shape and bootstrap

The module platform and factory are one product, brand, and project, although they remain separate applications and packages. The factory is intended to be the first substantial application built with the module system. Self-hosting is progressive: early work does not depend on either unfinished system, and the project increasingly uses its own factory only as the required layers become dependable.

The product source repository contains the module platform and factory source and is intended to become managed by the factory through progressive dogfooding. A user's managed project repository is a separate concept: it is a greenfield Rust monorepo and Cargo workspace with application modules, compositions, tests, and repository-local factory configuration, but no copied factory implementation. Migrating existing codebases is a non-goal. The first factory release supports only repositories created by the project initializer, not arbitrary repositories.

The primary v1 operator is an individual developer or very small Rust team starting a greenfield backend, already using GitHub and a skill-compatible coding agent, and willing to operate a factory container on an SSH-reachable machine and interact through Slack. This is an operational evaluator rather than a permanent market definition. Early users will likely be hobbyists and other experimenters; the ambition is to grow into a broadly applicable way to produce software.

### Agent-assisted installation

Installation should begin by adding a portable skill to the developer's existing coding agent. The skill follows the shared Agent Skills format so the core workflow can be used by compatible hosts such as Codex, Claude Code, Cursor, and others.

The coding agent guides setup, but a deterministic, versioned CLI owns repository mutations. The module runtime arrives as a Rust dependency. The factory remains external versioned software rather than source vendored into each application. The onboarding agent creates the project, helps obtain a remote factory, configures Slack and ordinary repository access, registers the repository, verifies the connection, and returns the lead-agent entry point.

The first supported factory deployment is a container on a user-controlled machine reachable over SSH. This can be a cloud VM or other remotely reachable computer. Running the same container locally is useful for evaluation but weaker for team collaboration. Managed hosting, provider-specific setup, personal hardware, and Kubernetes may become additional guided recipes later.

### First generated application

The initializer produces a database-free Hello World Rust project. One typed capability is defined once and invoked both as an in-process Rust interface and through HTTP. Postgres remains an important future provider but is not required for the first proof.

The foundation milestone exercises unary request-response only. Streaming data, streaming events, and real-time interaction remain requirements for the first full module-runtime release.

Everything managed in an application is represented as a module. Native Rust modules receive the full platform guarantees. Foreign-language modules, including TypeScript applications, remain first-class ownership and factory units but receive the full guarantee only at their managed client-binding boundary, not throughout their internal implementation.

Deployment topology belongs to the application composition rather than the module. Kubernetes is an important future target for module-based applications and potentially for the factory, but it is not a foundation-milestone requirement.

### First factory

Slack is the first factory UI. The developer talks to one persistent lead agent, which handles the requested change itself. The mature hierarchy of area leads, workers, reviewers, a merger, and continuous quality agents is introduced progressively rather than required at launch.

Those roles are factory configuration, not hard-coded execution-engine concepts. The factory control plane supplies execution, isolation, persistence, human interaction, and reporting mechanisms; configuration determines the organization and gates, even if that configuration is initially internal.

The lead works in a remote isolated code sandbox with dedicated Git worktree and branch isolation. It pushes its result, opens a GitHub pull request, reports it in Slack, and stops. A human must merge every v1 pull request.

The prescribed acceptance task adds a backward-compatible `greet(name)` capability to the generated Hello module, with `greet("Ada")` returning `Hello, Ada!` through Rust and HTTP. The resulting change may touch only that module and permitted deterministic artifacts, must open exactly one pull request, and must never merge automatically. This is an end-to-end foundation milestone rather than a validation of safe parallelism.

Graceful suspension preserves and resumes the complete factory-owned sandbox exactly. After a crash, repository bytes and durable records survive through the last completed execution-engine tool action; only the in-flight action may retry; and GitHub and Slack effects are reconciled without duplication.

The factory owns the execution interface, but the initial implementation can wrap an existing runner, call model APIs directly, or use a bare-bones custom loop. The choice must not leak into the managed project contract.

Slack is the only first-class integration in the foundation milestone. GitHub is the repository and pull-request surface, accessed with ordinary credentials rather than a dedicated GitHub App, bot, or Issues integration. Those integrations may be introduced when workers and a task ledger are added.

The complete decision is maintained in [Product Contract and Foundation Milestone](07-product-contract.md).

## Resulting documentation set

The interview produced:

- A short white paper stating the central thesis and system shape.
- Detailed documents for modules, packages and providers, runtime behavior, contract evolution, the software factory, and quality and authority.
- A product contract separating the long-term direction from the first end-to-end foundation milestone.
- This Q&A record, which preserves the reasoning and unresolved decisions behind those documents.
