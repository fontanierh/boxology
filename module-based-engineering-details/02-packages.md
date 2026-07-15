# Packages, Providers, and Compositions

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the package kinds and infrastructure model discussed during the design interview.

## Package kinds

The ecosystem contains at least three distinct package kinds:

- **Module package:** implements a product or domain capability.
- **Provider package:** satisfies a technical requirement using a particular technology or strategy.
- **Application composition package:** assembles modules and providers into a deployable application.

They can share packaging, versioning, testing, and factory conventions without being the same kind of object.

The distinction is:

```text
Module      -> what capability does the product provide?
Provider    -> how does the environment satisfy a technical requirement?
Composition -> what deployable system is assembled from them?
```

## Requirements, providers, and bindings

The initial word "adapter" was carrying too many meanings. The infrastructure discussion was clarified into three concepts:

- A **requirement** states what a module needs.
- A **provider** implements that kind of requirement.
- A **binding** is the configured provider instance assigned to one module.

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

The runtime does not need to understand the vendor behind a binding. The broader platform toolchain can scaffold, validate, provision, and configure it, while the runtime receives the resolved handle or configuration needed by the module.

## Semantic requirements

Modules should normally request semantic capabilities rather than particular vendors.

The examples discussed were:

```text
relational-store -> Postgres provider
cache            -> Redis provider
key-value-store  -> Redis provider
pub-sub          -> Redis provider
```

One provider can implement several requirement types. A module may deliberately request vendor-specific behavior when necessary, but doing so explicitly sacrifices portability.

Postgres and Redis were identified as useful initial providers because they exercise persistent configuration, local development, isolation, migrations or initialization, runtime bindings, and production infrastructure.

## What a provider can supply

A provider package may contribute several kinds of material:

- A configuration schema.
- Module scaffolding and required dependencies.
- A migrations folder or other provider-specific project structure.
- ORM or client configuration.
- Runtime client injection or connection information.
- Local-development setup.
- CI and conformance validation.
- Deployment and health conventions.

These responsibilities span more than the application runtime. Scaffolding and validation belong to the module toolchain and factory. Infrastructure provisioning belongs to deployment tooling. The runtime only needs the resolved binding used when the module executes.

Providers must not silently rewrite arbitrary module source. The intended model is that their generated or scaffolded outputs are declared, attributable, and governed by the same module ownership rules as other files.

## Provider isolation

Every binding is private to one consuming module, even if the underlying infrastructure is physically shared.

A Postgres provider may choose among different isolation mechanisms:

- Separate database servers.
- Separate databases on one server.
- Separate schemas.
- Isolated tables and credentials in one database.

The consumer should not need to know which mechanism was selected. It assumes that it can touch only its own state.

The runtime does not protect the system from a malicious or defective provider implementation. Providers are trusted platform packages. A provider that allows one binding to access another has violated its contract and is a bad provider. Provider conformance tests should exercise the promised isolation and behavior.

## Is a provider a module?

A provider is not itself a module, although both are packages.

A provider can contain Rust libraries, tooling, generated code, deployment logic, or supporting services. If one of those services exposes a product capability, that service may itself be implemented as a module. That does not make the provider package and module package equivalent.

This distinction prevents technical provisioning concerns from being confused with the domain interfaces through which application modules collaborate.

## Application composition packages

An application composition package describes a deployable application assembled from modules and providers.

It can declare:

- Which modules and versions are included.
- Which endpoints are exposed.
- Which transport bindings are enabled.
- How module requirements are bound to providers.
- Which authentication realms are configured.
- Application-level configuration.
- Integration tests.
- How the final binary, service, or deployment is built.

For example, a customer web application composition might include identity, billing, and catalog modules; Postgres and Redis bindings; authentication realm configuration; and HTTP exposure rules.

A composition package should contain almost no business logic. Meaningful logic that accumulates there should become another module. A monolithic deployment can have one composition containing many linked modules. Independently deployed services can each have a smaller composition package.

Composition packages also give shared assembly artifacts a semantic owner. Changing which modules are deployed or how they are bound changes the composition package rather than editing the implementation of every included module.

## Deployment topology

Modules remain deployment-neutral. The application composition decides whether a module is linked into a shared binary, exposed as a separately deployed service, or assembled as another process type such as a worker. This keeps deployment choices out of module business logic and allows the same module contract to participate in different application topologies.

Kubernetes is an important future deployment target for module-based applications, but it is not part of the first viable slice. Kubernetes support should be expressed through composition and deployment tooling rather than built into individual modules. The same separation applies when deploying the software factory itself: running the factory on Kubernetes and deploying module-based applications to Kubernetes are distinct targets.

## Workflow engines

The discussion deliberately avoided making Temporal or another workflow engine a required runtime concept.

A module can use a workflow engine as ordinary implementation code. If several modules later need consistent shared workflow infrastructure, a provider package can capture its provisioning, binding, worker, namespace, task-queue, testing, and operational conventions. That abstraction should be introduced because repeated use makes it valuable, not because the runtime assumes a particular workflow engine.

## Matters not yet specified

The discussion did not settle:

- The common manifest format shared by package kinds.
- The exact lifecycle interface implemented by provider packages.
- How provider bindings are represented across development, testing, and production environments.
- Whether a binding can be switched without rebuilding a composition.
- How vendor-specific requirements are named and governed.
- The exact boundary between composition configuration and deployment configuration.
- The representation and generation of Kubernetes deployment artifacts.
