# Runtime

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document expands the runtime, endpoint, transport, and authentication model discussed during the design interview.

## Runtime responsibility

The runtime provides a standard way to define and invoke a module capability. It should remain small, generic, and vendor-neutral.

The runtime is not responsible for proving the correctness of module code, choosing a workflow engine, or owning all infrastructure tooling. Those responsibilities belong to modules, providers, compositions, and the software factory.

The runtime's central object is a typed capability or endpoint definition. That definition contains enough information for configured bindings to expose the capability consistently.

## Capability definition

The desired endpoint API should collect the information required by bindings without requiring handwritten transport-specific adapters.

The metadata discussed included:

- Named and documented inputs.
- Types, defaults, and validation.
- Structured outputs and errors.
- Authentication and access policy.
- Request-response, streaming, event, or real-time behavior.
- Progress and cancellation behavior where applicable.
- File, binary, or secret input semantics.
- Side-effect and confirmation metadata where needed by a caller.

The exact Rust interface or interface-definition language was not chosen. The important requirement is that the contract be rich enough to generate or validate its configured bindings.

## Exposure, identity, and permission

Three concepts were separated:

```text
Exposure   -> where the endpoint can be reached
Identity   -> who or what is calling
Permission -> what the caller may do
```

The exposure examples discussed were code-only, internal, and external. An externally reachable method is not automatically anonymous. Anonymous access must be explicitly allowed.

A module can declare a default access policy and an endpoint can override it. A code-only module can therefore default all methods to code-only access. A service can default to authenticated access while explicitly marking a particular endpoint as anonymous or otherwise public.

The safe posture discussed was default denial: access that is not declared should not be inferred.

## Realm-scoped principals

A simple `User(user_id)` identity was considered too limited. The normalized caller identity should be scoped to an application or identity realm:

```text
Principal {
  realm
  subject
  kind
}
```

The realm owns the identity namespace. The subject identifies a principal within that realm. The kind can distinguish humans, services, modules, or other future principal types.

How the caller authenticated is separate evidence:

```text
Authentication {
  provider
  external_subject
}
```

For example, a Google identity can be mapped into a customer-facing application principal, while the same external person can be mapped separately into an internal administration realm.

```text
(google, external-subject)
-> (customer-app, user-123)

(work-provider, external-subject)
-> (internal-admin, employee-789)
```

The runtime does not automatically infer that principals in different realms represent the same person. In particular, identities must not be linked merely because they share an email address.

Cross-realm association is domain data. An employee module, for example, can own a record linking a customer-application principal and an internal-administration principal to one employee profile. Other authorized modules can call that employee module when they need the relationship.

## Authentication adapters

Realms are vendor-neutral configuration backed by flexible authentication adapters. Examples discussed included Auth0, WorkOS, Google authentication, and custom systems.

An adapter converts provider-specific credentials into a normalized external identity. The realm then maps the external identity into its own principal. Modules depend on the normalized principal and declared permissions rather than on vendor-specific cookies, tokens, or claims.

The intended split is:

- The authentication adapter validates the credential.
- The runtime supplies the normalized caller context and enforces the declared coarse endpoint policy.
- The module enforces resource-specific rules, such as whether the caller owns a particular document or invoice.

## Development authentication

Production authentication methods such as browser cookies are inconvenient when calling a module through a development CLI. The proposed solution is a secondary development-only authentication system that maps to the same principals and authorization behavior.

The principle is:

> Development authentication is an alternative credential source, not alternative authorization.

The agreed model uses named development identities rather than unrestricted impersonation:

```text
alice_customer
support_agent
company_admin
```

An authentication realm owns these non-secret, version-controlled identities and claims. The runtime provides the development credential mechanism. A feature module references the named principal and creates its own domain test data for that principal.

For example, a billing test can authenticate as `alice_customer` and seed invoices owned by that identity. Billing does not define the credential or the realm identity.

The safety properties discussed were:

- Development credentials are short-lived and local to the development runtime.
- Production deployments do not load or trust the development issuer.
- Production validation rejects development authentication configuration.
- Development-authenticated calls are distinguishable in audit data.
- Privileged development identities must be explicitly declared.
- Tests can create temporary identities from approved templates.

Development authentication tests the normal invocation and authorization path after credential verification. Production authentication adapters still need their own integration tests.

## Transport bindings

CLI was initially described as a major module exposure, then refined into one optional binding among many.

The conceptual layers are:

```text
Capability contract
-> interaction shape
-> transport binding
-> wire encoding
```

Examples discussed were:

- Transport bindings: Rust, HTTP, gRPC, CLI, and server-sent events.
- Wire encodings or schemas: JSON and Protobuf.
- Interaction shapes: request-response, data streaming, event streaming, and real-time communication.

Protobuf is treated as a schema and encoding rather than as the transport itself.

Every binding must preserve the capability contract, but a binding can only be selected when it supports the endpoint's interaction shape. For example, server-sent events support a server-to-client stream but do not provide a general bidirectional session.

The runtime can provide a generic development CLI capable of invoking any compatible endpoint. A module or application can also package a CLI as a real internal product. No handwritten CLI implementation should be required when the endpoint definition already contains the necessary input, output, validation, authentication, and interaction metadata.

CLI is therefore useful but not mandatory. It is neither a special module type nor automatically a public compatibility surface.

## Streaming, events, and real-time behavior

The first end-to-end foundation milestone exercises unary request-response through Rust and HTTP only. The first full module-runtime release is expected to support more than ordinary request-response calls. Its required additional interaction shapes are:

- Streaming data associated with a call.
- Streaming events that consumers can observe.
- Real-time systems.

The precise semantics of cursors, replay, backpressure, cancellation, delivery guarantees, and bidirectional sessions remain to be designed.

Durable workflow behavior can be expressed by a module's own endpoints and events. An agent-loop module might expose operations for starting work, sending messages, reading status, and consuming events while using Temporal internally. The runtime should not prescribe Temporal's concepts or require a universal durable-instance lifecycle.

## Rust calls and external clients

Within the Rust ecosystem, module-to-module calls should go through a runtime-provided typed function or capability handle. The module declares the imported contract, and the runtime or generated code supplies the typed caller.

This gives the factory a static dependency graph while runtime observations provide actual-usage telemetry.

External managed clients participate through client-binding modules. A binding module imports the Rust-side contract and generates a TypeScript, Swift, Kotlin, or other language-native SDK. The client application imports that generated artifact while the binding module remains visible to the factory's dependency and migration system.

## Matters not yet specified

The discussion did not settle:

- The exact endpoint declaration syntax.
- Whether the canonical interface is Rust-first or language-neutral.
- The adapter loading and configuration mechanism.
- Detailed streaming, event replay, and real-time semantics.
- The exact representation of permissions and resource authorization.
- How code-only access is technically enforced when modules share one process.
- Which CLI behavior is guaranteed by default beyond the generic invocation path.
