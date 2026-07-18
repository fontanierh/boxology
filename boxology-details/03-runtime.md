# Runtime

[Back to the white paper](../boxology-whitepaper.md)

This document expands the runtime, endpoint, transport, and authentication model discussed during the design interview.

## Runtime responsibility

The runtime provides a standard way to define and invoke a box capability. It should remain small, generic, and vendor-neutral.

The runtime is not responsible for proving the correctness of box code, choosing a workflow engine, or owning all infrastructure tooling. Those responsibilities belong to boxes, providers, compositions, and the software factory.

The runtime's central object is a typed capability or endpoint definition. That definition contains enough information for configured bindings to expose the capability consistently.

## Capability definition

The desired endpoint API should collect the information required by bindings without requiring handwritten transport-specific adapters.

Rust implementation methods are the authoring source. A method becomes part of the box contract when it is annotated as a capability. No exported service trait, parallel interface file, or prescribed internal organization is required. Unannotated methods remain internal implementation details.

The metadata discussed included:

- Named and documented inputs.
- Types, defaults, and validation.
- Structured outputs and errors.
- Authentication and access policy.
- Request-response, streaming, event, or real-time behavior.
- Progress and cancellation behavior where applicable.
- File, binary, or secret input semantics.
- Side-effect and confirmation metadata where needed by a caller.

The source model, schema-safe type subset, invocation envelope, and binding rules are defined in [Canonical Capability Contract](09-capability-contract.md). Values crossing the capability boundary must implement the generated contract-type model and be representable faithfully; unsupported boundary types are generation errors rather than silently lossy bindings.

Every annotated method receives an explicit `CallContext` and generated caller handles are asynchronous. The implementation returns a declared structured domain error. The generated handle wraps that result in a distinct invocation-error type that can represent deadline, cancellation, availability, contract, or response failures. Local bindings use the same caller type without pretending that local and remote execution have identical operational behavior.

## Generated contract crate

Each native box owns one generated contract crate in addition to its handwritten implementation crate. A deterministic platform generator runs before Cargo, reads annotated implementation methods, and produces the contract-safe Rust types, typed caller handles, implementation-neutral server-side dispatch interface, metadata, language-neutral schema, and programmable test bindings required by configured bindings and box tests.

The contract crate never depends on the implementation. The generator emits separate box-local adapter code inside the implementation crate to connect annotated methods to the contract's dispatch interface; the composition performs the final binding. The generated outputs are checked into Git as declared, reproducible artifacts and are never edited manually. CI regenerates submitted outputs byte-for-byte from the box's permitted source inputs using the generator resolved by the workspace toolchain. Other boxes compile only against the generated contract, never against the providing box's implementation. See [Rust Build Topology](08-rust-build-topology.md).

The workspace supplies one current, backward-compatible generator rather than selecting one per box. Generated outputs record which release produced them. Updating the workspace tool does not rewrite every contract: a box moves to the current generator lazily when its contract is next generated, while older generated crates remain supported.

The contract crate represents the box's complete supported surface. Public `v1` or `v2` crates, traits, and namespaces are optional rather than required. Stable capability identities and generated contract revisions provide change tracking without dictating a public versioning scheme.

## Exposure, identity, and permission

Three concepts were separated:

```text
Exposure   -> where the endpoint can be reached
Identity   -> who or what is calling
Permission -> what the caller may do
```

The exposure examples discussed were code-only, internal, and external. An externally reachable method is not automatically anonymous. Anonymous access must be explicitly allowed.

A box can declare a default access policy and an endpoint can override it. A code-only box can therefore default all methods to code-only access. A service can default to authenticated access while explicitly marking a particular endpoint as anonymous or otherwise public.

The safe posture discussed was default denial: access that is not declared should not be inferred.

An endpoint's effective box declaration—its endpoint override when present, otherwise the box default—sets its maximum reachability. A composition may omit the endpoint or select a binding whose reachability is equal to or narrower than that maximum. It must never widen it. Raising the maximum is a contract change owned by the box.

Reachability is ordered from narrowest to broadest:

- **Code-only:** the platform must not generate or activate a network route for the endpoint. It remains callable through permitted in-process capability handles.
- **Internal:** the endpoint may be routed only inside the composition's declared trust zone. Internal is not an authentication bypass; callers must still authenticate and satisfy the endpoint's authorization policy.
- **External:** the endpoint may be routed externally. This does not imply anonymous access; anonymity must still be explicit.

Composition validation rejects bindings that exceed the box-declared maximum. Under the foundation's convention-level isolation profile, these rules constrain platform-generated routing but do not sandbox mutually trusted code or prevent it from opening its own connections in a shared process.

## Realm-scoped principals

A simple `User(user_id)` identity was considered too limited. The normalized caller identity should be scoped to an application or identity realm:

```text
Principal {
  realm
  subject
  kind
}
```

The realm owns the identity namespace. The subject identifies a principal within that realm. The kind can distinguish humans, services, boxes, or other future principal types.

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

Cross-realm association is domain data. An employee box, for example, can own a record linking a customer-application principal and an internal-administration principal to one employee profile. Other authorized boxes can call that employee box when they need the relationship.

## Authentication adapters

Realms are vendor-neutral configuration backed by flexible authentication adapters. Examples discussed included Auth0, WorkOS, Google authentication, and custom systems.

An adapter converts provider-specific credentials into a normalized external identity. The realm then maps the external identity into its own principal. Boxes depend on the normalized principal and declared permissions rather than on vendor-specific cookies, tokens, or claims.

The intended split is:

- The authentication adapter validates the credential.
- The runtime supplies the normalized caller context and enforces the declared coarse endpoint policy.
- The box enforces resource-specific rules, such as whether the caller owns a particular document or invoice.

## Development authentication

Production authentication methods such as browser cookies are inconvenient when calling a box through a development CLI. The proposed solution is a secondary development-only authentication system that maps to the same principals and authorization behavior.

The principle is:

> Development authentication is an alternative credential source, not alternative authorization.

The agreed model uses named development identities rather than unrestricted impersonation:

```text
alice_customer
support_agent
company_admin
```

An authentication realm owns these non-secret, version-controlled identities and claims. The runtime provides the development credential mechanism. A feature box references the named principal and creates its own domain test data for that principal.

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

CLI was initially described as a major box exposure, then refined into one optional binding among many.

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

Binding compatibility is checked during generation or application-composition validation. Interaction shape, value representation, and propagation of `CallContext` fields are conformance dimensions. An incompatible binding is rejected before the application accepts traffic, with a diagnostic identifying the capability and unsupported contract feature.

The runtime can provide a generic development CLI capable of invoking any compatible endpoint. A box or application can also package a CLI as a real internal product. No handwritten CLI implementation should be required when the endpoint definition already contains the necessary input, output, validation, authentication, and interaction metadata.

CLI is therefore useful but not mandatory. It is neither a special box type nor automatically a public compatibility surface.

## Foundation HTTP binding

The v1 HTTP binding is deliberately RPC-shaped. Every unary capability uses:

```text
POST /rpc/{box_id}/{capability_id}
```

Both identifiers come from the canonical contract and are encoded as individual percent-encoded path segments. V1 has no configurable REST verbs, resource paths, or transport-specific error annotations. A later handwritten REST adapter may translate into and out of the same typed capability, but it does not create a second public contract.

Requests and responses use `application/json`. The composition default request-body limit is 1 MiB and may be lowered or raised explicitly. An oversized body returns `413`; an unsupported media type returns `415`; malformed JSON or a value rejected by contract validation returns `400` before invocation.

The lossless JSON mapping is:

- Strings and booleans use their JSON equivalents.
- Integers through 32 bits and finite floating-point values use JSON numbers. Contract `f32` and `f64` values are finite-only at every capability boundary: a non-finite request value is invalid input, and a non-finite provider result is an invalid contract response.
- `i64` and `u64` use decimal JSON strings so JavaScript clients cannot lose precision. Their use in ordinary API design is discouraged, but the binding remains lossless when they are required.
- String-keyed maps use JSON objects. Other map-key types are not supported by this binding.
- Bounded binary values use `{"base64":"..."}` with standard padded Base64. Streaming binary values are outside the unary foundation binding.
- Every enum uses `{"tag":"variant","payload":...}`. A unit variant has `"payload":null`.

Within an object:

- `T` is required and rejects `null`.
- `Option<T>` maps omission to `None` and rejects explicit `null`.
- `Field<T>` maps omission to `Missing`, `null` to `Null`, and any valid value to `Value`.

At the top level, `Option<T>` uses JSON `null` for `None`. Object fields use omission because an object can distinguish an absent field from a present field; a top-level request body must exist, so `null` is the canonical representation of top-level `None`. A top-level `Field<T>` is rejected during binding validation because an HTTP body cannot represent the distinction between a missing body and a deliberately missing contract value faithfully.

Every valid response uses one of three envelopes. Their format belongs to this HTTP binding and evolves backward-compatibly with it; it is not negotiated per request and does not add a versioned URL. An incompatible future envelope is a distinct binding rather than a silent change to this one.

```json
{"result":{"value":"..."}}
```

```json
{"error":{"kind":"domain","value":{"tag":"invalid_email","payload":{"reason":"..."}}}}
```

```json
{"error":{"kind":"call","code":"invalid_request","message":"..."}}
```

Declared domain errors always return `422`; the structured error variant remains in the envelope. Server-observable invocation failures use:

| Status | Meaning |
| --- | --- |
| `400` | Malformed JSON or contract validation failure. |
| `404` | Unknown box or capability identity, with a diagnostic that distinguishes the two cases. |
| `504` | Deadline exceeded. |
| `503` | Bound target unavailable. |
| `502` | Bound target returned an invalid contract response. |
| `500` | Internal runtime failure. |

Cancellation, disconnection, or transport failure that produces no valid HTTP response becomes a client-side `CallError`; it has no invented HTTP status.

The binding carries remaining deadline budget in `Boxology-Timeout-Ms`, W3C trace context in `traceparent` and `tracestate`, and a declared idempotency key in `Idempotency-Key`. Missing headers mean composition-default deadline, a newly created trace, and no idempotency key respectively. The foundation transports the idempotency key into `CallContext`; it does not provide a deduplication store or imply retry-safe replay by itself. Client disconnection requests advisory cancellation of the in-flight capability. The anonymous Hello endpoint constructs anonymous caller context inside the binding; v1 does not trust caller-supplied identity headers, and authenticated caller propagation remains part of the later authentication design.

Binding conformance tests must exercise routing, every supported value shape, missing/null/value behavior, the three response envelopes, status mapping, context propagation, request limits, and rejection of unsupported or lossy contracts.

## Streaming, events, and real-time behavior

The contract model recognizes unary request-response, server streaming, client streaming, bidirectional streaming, and event subscriptions. The first end-to-end foundation milestone exercises unary request-response through Rust and HTTP only. The first full box-runtime release is expected to implement the additional shapes.

Streaming signatures use platform contract types rather than arbitrary Rust streams so the schema and bindings can identify the required behavior. The required additional interaction shapes include:

- Streaming data associated with a call.
- Streaming events that consumers can observe.
- Real-time systems.

The precise semantics of cursors, replay, backpressure, cancellation, delivery guarantees, and bidirectional sessions remain to be designed.

Durable workflow behavior can be expressed by a box's own endpoints and events. An agent-loop box might expose operations for starting work, sending messages, reading status, and consuming events while using Temporal internally. The runtime should not prescribe Temporal's concepts or require a universal durable-instance lifecycle.

## Rust calls and external clients

Within the Rust ecosystem, box-to-box calls go through a generated asynchronous typed capability handle. The box declares the imported contract, the contract-owning box's generated crate supplies the canonical handle type, and the runtime or composition creates and injects the permitted handle.

The application composition binds that handle to an in-process implementation or a remote transport. Box code remains statically typed and deployment-neutral; it does not perform ambient string-based service lookup.

This gives the factory a static dependency graph while runtime observations provide actual-usage telemetry.

External managed clients participate through client-binding boxes. A binding box imports the Rust-side contract and generates a TypeScript, Swift, Kotlin, or other language-native SDK. The client application imports that generated artifact while the binding box remains visible to the factory's dependency and migration system.

## Matters not yet specified

The discussion did not settle:

- The adapter loading and configuration mechanism.
- Detailed streaming, event replay, and real-time semantics.
- The exact representation of permissions and resource authorization.
- Which CLI behavior is guaranteed by default beyond the generic invocation path.
