# Packages, Providers, and Compositions

[Back to the white paper](../boxology-whitepaper.md)

This document expands the package kinds and infrastructure model discussed during the design interview.

## Package kinds

The ecosystem contains four distinct package kinds. Every kind can own source, a pull request, a factory area, and a quality contract.

| Package kind | Responsibility | Baseline change contract |
| --- | --- | --- |
| **Box** | Implements a product or domain capability, including managed client-binding boxes. | Contract compatibility, box tests, and expand-migrate-contract for breaking interfaces. |
| **Provider** | Satisfies a technical requirement using a particular technology or strategy. | Provider conformance, binding isolation, and migration or provisioning validation where applicable. |
| **Application composition** | Assembles boxes and providers into a deployable application. | Assembly and integration validation; almost no business logic. |
| **Platform** | Owns the runtime, CI, build tooling, repository-wide generators, and enforcement machinery. | Whole-workspace validation and stricter approval by default, subject to configured harness policy, because changes can have global blast radius. |

They share packaging, versioning, testing, ownership, and factory conventions without being the same kind of object. A client-binding box remains a box package rather than introducing a fifth kind.

The universal ownership rule is:

> One pull request, one accountable package, zero foreign source changes.

Here, source includes every hand-authored or otherwise non-derived file: implementation, manifests, tests, migrations, configuration, quality policy, and similar inputs.

Boxes are the most common contract-bearing owners, but providers, compositions, and platform packages are equally valid owners once their ownership records exist.

## Logical packages and Cargo crates

The word **package** in the ownership model names a semantic owner, not necessarily one Cargo package or crate. A native box owns a handwritten implementation crate and a mechanically generated contract crate. Both compilation units belong to the same logical box, factory area, quality contract, and pull-request owner.

The generated contract crate is a declared derived output of the box's annotated Rust implementation methods. It does not become a separate accountable package merely because Cargo compiles it independently. Conversely, an application composition is a separate logical owner even when it compiles both box implementations into one binary.

This distinction preserves the universal ownership rule while allowing Cargo to enforce that boxes compile only against foreign contracts. The complete crate-role and edge policy is defined in [Rust Build Topology](08-rust-build-topology.md).

## Common ownership manifest

Every logical package root contains exactly one package-local `boxology.toml`, discovered deterministically without a hand-maintained central index. Every package kind participates in the same manifest model, with common ownership fields and kind-specific sections. TOML is the v1 serialization.

The workspace checker walks from the Cargo workspace root, excluding VCS metadata and Cargo build-output directories, and finds every `boxology.toml`. Manifest paths are normalized relative to that root. Absolute paths, `..` escapes, symlink escapes, duplicate package identities, overlapping ownership, and files that classify under no package are rejected.

Every manifest begins with:

```toml
schema = 1
id = "billing"
kind = "box"
owned = ["boxology.toml", "implementation/**", "tests/**"]
```

`id` is a human-readable, workspace-unique, rename-stable slug matching `[a-z][a-z0-9-]*`. Directory names, Cargo package names, and an optional `display_name` may change without changing the identity. Full identity split, merge, transfer, and retirement semantics remain tracked in [issue #3](https://github.com/fontanierh/boxology/issues/3).

`schema` identifies the manifest format. A checker must continue to read every older schema version it claims to support and must reject an unknown newer version rather than silently ignoring it. A format change that cannot be interpreted compatibly increments the schema value and requires an explicit workspace migration.

The common v1 fields encode:

- A stable package identity and package kind.
- Owned non-derived paths, including source, manifests, tests, migrations, configuration, and quality contracts.
- Declared package and contract dependencies.
- Quality-contract entry points.
- Declared derived outputs.
- For every derived output: its output paths, generator identity, complete semantic inputs, and regeneration check. Generated content records the resolved generator version as provenance, while the protected workspace toolchain resolves the current executable and environment consistently for local development and CI.
- Protected control-plane declarations where applicable.

Imports, quality entry points, Cargo crates, and derived outputs use these forms:

```toml
[[imports]]
package = "customer"
contract = "customer"

[quality]
commands = ["cargo test -p billing-implementation"]

[[crates]]
cargo_package = "billing-implementation"
path = "implementation"
role = "box-implementation"

[[crates]]
cargo_package = "billing-contract"
path = "generated/contract"
role = "box-contract"

[[derived]]
id = "contract"
generator = "boxology-contract"
inputs = ["implementation/src/**"]
outputs = ["generated/contract/**", "generated/schema.json"]
```

The generator value is a logical workspace-tool identity, not a per-box version pin. The current workspace tool resolves the executable; generated outputs record its resolved version as provenance.

The foundation crate-role vocabulary is `box-implementation`, `box-contract`, `composition`, and `platform`. The checker reads Cargo metadata and requires every Cargo package to match exactly one manifest `[[crates]]` entry by normalized manifest path and Cargo package name. A role cannot be inferred from a directory or crate-name suffix. Provider crate roles are added when the first provider enters the foundation rather than being guessed now.

A composition adds its selected boxes and bindings:

```toml
[composition]
boxes = ["hello"]

[[composition.bindings]]
box = "hello"
capability = "hello.greet"
transport = "in-process"

[[composition.bindings]]
box = "hello"
capability = "hello.greet"
transport = "http"
exposure = "external"
```

Composition validation checks that every selected identity exists, every binding is compatible with the generated contract, and an exposure does not exceed the box's declared maximum.

The initializer creates a logical `platform` package at the workspace root. Its manifest owns root `Cargo.toml`, repository CI, generator configuration, ownership rules, and other non-derived root machinery, and declares `Cargo.lock` as the workspace's global derived artifact. The Hello box and application composition live under their own package roots with their own manifests. The platform package may initially contain no Rust crate.

Repository-wide ownership policy, CI, build tooling, generator definitions, and merger enforcement belong to platform packages. Ownership, provenance, and quality-policy declarations are protected control-plane data. A pull request cannot weaken or replace the base revision's rules in order to authorize its own changes.

## Ownership and derived-artifact enforcement

The merger evaluates ownership from the base revision of a pull request:

1. Read the ownership and derivation declarations from the base revision.
2. Classify every changed path exactly once as either a non-derived path owned by one package or one declared derived output. Reject ambiguous, overlapping, or unowned classifications.
3. Require the set of non-derived owners to contain exactly one accountable package.
4. Require every changed derived path to be declared as an output attributable to that package.
5. Starting from the base revision plus only the accountable package's complete permitted non-derived diff, run the declared generators as resolved by the protected workspace toolchain and require them to recreate every submitted derived output byte-for-byte.
6. Apply the accountable package's quality contract and every global validation triggered by the change's semantic impact.

Ownership proposed by the pull request does not retroactively authorize other paths in that same pull request. Bootstrapping the ownership record for a newly created package therefore needs a separately defined creation protocol, tracked in [issue #47](https://github.com/fontanierh/boxology/issues/47).

A generated file physically located under another package is not automatically foreign source. It is permitted only when the base revision declares it as non-editable derived output and the regeneration check attributes it to the accountable package. Otherwise it is foreign source and the merger rejects the change. Published client generation still follows the separate-consumer rule: a consumer adopts the generated client in its own package-owned change.

Protected control-plane files cannot be relabeled as derived output to evade their approval policy. Byte reproducibility proves provenance, not limited semantic impact; repository-wide consequences can still trigger broader validation.

## Shared Cargo lockfile

`Cargo.lock` is a global derived artifact with semantic risk, not harmless generated output. An ordinary package pull request must:

1. Begin with the base revision's lockfile.
2. Apply only the accountable package's permitted non-derived changes.
3. Run the repository-defined minimal dependency resolution under pinned Cargo, toolchain, registry, and generator inputs. This resolver must preserve every existing selection from the base lockfile that still satisfies the accountable package's declared dependency change, add newly required selections, and deterministically change only the existing selections that can no longer remain fixed and their required transitive closure.
4. Reproduce the submitted lockfile byte-for-byte.
5. Reject every lockfile change outside the necessary minimal resolution closure.

A transitive change inside that minimal closure remains attributable to the accountable package. A transitive change outside it has no valid owner and is rejected.

The merger compares pinned resolver output and dependency metadata reachable from foreign packages before and after resolution. If a version, source, checksum, selected package, resolved feature set, target-specific selection, or other build-relevant resolver output used by a foreign package changes, the pull request retains one accountable package and contains no foreign source change, but it loses package-local blast-radius treatment. It requires whole-workspace build and tests, all mandatory repository gates, and semantic reassessment of in-progress work involving affected packages.

Deliberate repository-wide dependency maintenance, including security updates and major or routine shared upgrades, belongs to a platform package and normally requires whole-workspace validation. Such a change must include a platform-owned, versioned update declaration naming the requested dependency change and its allowed resolution scope; a lockfile-only mutation is invalid. The pinned resolver treats that declaration as semantic input and preserves every base selection outside its permitted closure. Source edits to foreign package manifests or implementations remain separate package-owned pull requests.

Independent Cargo build roots remain deferred. The factory should measure lockfile-triggered reassessments, rework, and queue delay so separate roots can be introduced if shared resolution repeatedly serializes integration in practice.

## Requirements, providers, and bindings

The initial word "adapter" was carrying too many meanings. The infrastructure discussion was clarified into three concepts:

- A **requirement** states what a box needs.
- A **provider** implements that kind of requirement.
- A **binding** is the configured provider instance assigned to one box.

For example:

```text
billing requires relational-store as db
production binds billing.db to a Postgres-backed instance
```

Or:

```text
agent-loop requires durable-workflows as runs
an environment could bind agent-loop.runs to shared workflow infrastructure
```

The runtime does not need to understand the vendor behind a binding. The broader platform toolchain can scaffold, validate, provision, and configure it, while the runtime receives the resolved handle or configuration needed by the box.

## Semantic requirements

Boxes should normally request semantic capabilities rather than particular vendors.

The examples discussed were:

```text
relational-store -> Postgres provider
cache            -> Redis provider
key-value-store  -> Redis provider
pub-sub          -> Redis provider
```

One provider can implement several requirement types. A box may deliberately request vendor-specific behavior when necessary, but doing so explicitly sacrifices portability.

Postgres and Redis were identified as useful initial providers because they exercise persistent configuration, local development, isolation, migrations or initialization, runtime bindings, and production infrastructure.

## What a provider can supply

A provider package may contribute several kinds of material:

- A configuration schema.
- Box scaffolding and required dependencies.
- A migrations folder or other provider-specific project structure.
- ORM or client configuration.
- Runtime client injection or connection information.
- Local-development setup.
- CI and conformance validation.
- Deployment and health conventions.

These responsibilities span more than the application runtime. Scaffolding and validation belong to the Boxology toolchain and factory. Infrastructure provisioning belongs to deployment tooling. The runtime only needs the resolved binding used when the box executes.

Providers must not silently rewrite arbitrary box source. Their generated or scaffolded outputs must be declared, attributable, and governed by the package ownership and derived-artifact rules above.

## Provider isolation

A private binding is an architectural contract, not a universal security guarantee. A box must use only its own binding, even if the underlying infrastructure is physically shared. The strength of that rule depends on the isolation profile selected by the application composition.

Isolation profiles provide increasing assurance. Later profiles retain the earlier guarantees but may replace their mechanisms:

- **L0 — Convention:** mutually trusted boxes may share a process and credentials. Boundaries rely on code discipline, review, provider conventions, and tests. This is the foundation default.
- **L1 — Credential-enforced:** each binding receives least-privilege credentials, such as a binding-specific database role. Shared administrative credentials remain control-plane material and are not delivered to boxes.
- **L2 — Process-isolated:** L1 plus separate processes or containers and enforced operating-system, network, and resource policy.
- **L3 — Adversarial:** L2 plus sandboxed box code and controlled egress, with box-visible reusable credentials replaced by brokered, unforgeable scoped capabilities. Deploying code deliberately treated as untrusted at runtime would require this profile. L3 remains a future target whose runtime threat model and acceptance criteria are not established here. Sandboxing candidate code, builds, CI, and factory credentials remains separate work in [issue #24](https://github.com/fontanierh/boxology/issues/24).

A composition declares the minimum profile it requires. Its selected providers and deployment topology must supply the corresponding controls, and tooling must not claim a stronger profile than the deployed mechanisms support.

The selected profile and its validation evidence belong in the composition's release or deployment record:

- **L0 evidence** records the applicable ownership checks, linting, and review results.
- **L1 evidence** additionally validates binding-specific roles, default privileges, and attempted cross-binding denial.
- **L2 evidence** additionally validates process identity and the deployed operating-system, network, and resource policies.
- **L3 evidence** cannot be accepted until its separate adversarial threat model and verification criteria have been defined.

These checks are evidence about configured controls, never proof of universal isolation.

A Postgres provider may choose among different isolation mechanisms:

- Separate database servers.
- Separate databases on one server.
- Separate schemas.
- Isolated tables and credentials in one database.

The consumer should not need to know which conforming mechanism was selected. Raw SQL is permitted against a box's own binding at L0 and L1. At L0 the ownership rule is conventional; at L1 provider-issued credentials enforce access for code using those credentials. L1 does not make hostile boxes sharing one process safe from credential theft. Arbitrary networking is technically constrained only when an L2 or L3 network policy does so.

The initial Postgres provider should target L1 by default through binding-specific roles and privileges. Profile-specific negative tests and deployment validation provide evidence that controls are configured correctly; tests alone do not prove non-access.

The runtime does not protect the system from a malicious or defective provider implementation. Providers and platform components enforcing a profile are trusted computing-base components. A provider that claims a profile without supplying its controls is defective.

## Is a provider a box?

A provider is not itself a box, although both are packages.

A provider can contain Rust libraries, tooling, generated code, deployment logic, or supporting services. If one of those services exposes a product capability, that service may itself be implemented as a box. That does not make the provider package and box package equivalent.

This distinction prevents technical provisioning concerns from being confused with the domain interfaces through which application boxes collaborate.

## Application composition packages

An application composition package describes a deployable application assembled from boxes and providers.

It can declare:

- Which boxes and any explicitly selected contract surfaces are included.
- Which endpoints are enabled, within each box's declared maximum exposure.
- Which trust zones exist and how internal routes are constrained to them.
- Which transport bindings are enabled.
- How box requirements are bound to providers.
- The minimum required isolation profile.
- Which authentication realms are configured.
- Application-level configuration.
- Integration tests.
- How the final binary, service, or deployment is built.

For example, a customer web application composition might include identity, billing, and catalog boxes; Postgres and Redis bindings; authentication realm configuration; and HTTP exposure rules.

A composition package should contain almost no business logic. Meaningful logic that accumulates there should become another box. A monolithic deployment can have one composition containing many linked boxes. Independently deployed services can each have a smaller composition package.

Composition packages also give shared assembly artifacts a semantic owner. Changing which boxes are deployed or how they are bound changes the composition package rather than editing the implementation of every included box.

## Deployment topology

Boxes remain deployment-neutral. The application composition decides whether a box is linked into a shared binary, exposed as a separately deployed service, or assembled as another process type such as a worker. This keeps deployment choices out of box business logic and allows the same box contract to participate in different application topologies.

Kubernetes is an important future deployment target for applications built with Boxology, but it is not part of the foundation milestone. Kubernetes support should be expressed through composition and deployment tooling rather than built into individual boxes. The same separation applies when deploying the software factory itself: running the factory on Kubernetes and deploying Boxology applications to Kubernetes are distinct targets.

## Workflow engines

The discussion deliberately avoided making Temporal or another workflow engine a required runtime concept.

A box can use a workflow engine as ordinary implementation code. If several boxes later need consistent shared workflow infrastructure, a provider package can capture its provisioning, binding, worker, namespace, task-queue, testing, and operational conventions. That abstraction should be introduced because repeated use makes it valuable, not because the runtime assumes a particular workflow engine.

## Matters not yet specified

The discussion did not settle:

- The exact lifecycle interface implemented by provider packages.
- How provider bindings are represented across development, testing, and production environments.
- Whether a binding can be switched without rebuilding a composition.
- How vendor-specific requirements are named and governed.
- The exact boundary between composition configuration and deployment configuration.
- The representation and generation of Kubernetes deployment artifacts.
