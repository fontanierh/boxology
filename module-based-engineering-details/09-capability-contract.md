# Canonical Capability Contract

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document defines the canonical contract authored by a native Rust module. It fixes the relationship between annotated implementation code, the generated language-neutral schema, typed Rust handles, and transport or client bindings.

## Two authorities, one source of human intent

Rust implementation source is the **authoring authority**. Developers and agents annotate ordinary methods and boundary types beside the implementation they maintain. They do not maintain an exported service trait or a separate interface-definition file.

The deterministic generated schema is the **compatibility authority**. Contract diffing, binding validation, generated SDKs, and migration analysis consume the schema rather than interpreting the Rust syntax independently.

The two roles are deliberately different:

```text
annotated Rust implementation source
-> deterministic contract generator
-> language-neutral contract schema
-> generated Rust contract crate and configured bindings
```

The generated schema is derived and checked into Git. It is never an independently edited source of truth.

## Authoring surface

A capability is an annotated asynchronous implementation method. Conceptually:

```rust
/// Creates a customer account.
#[module::capability(
    name = "create_user",
    auth = "customer",
    idempotency = "keyed"
)]
async fn create_user(
    &self,
    context: CallContext,
    input: CreateUser,
) -> Result<User, CreateUserError> {
    // business logic
}
```

The exact macro spelling is a tooling choice, but the source model is fixed:

- The method itself is the implementation and export declaration.
- The public capability name defaults to the Rust function name.
- An explicit public name preserves the contract when the Rust function is renamed internally.
- Rust documentation and declared authentication, idempotency, validation, deprecation, and interaction metadata flow into the generated schema and bindings.
- Unannotated methods and types remain internal implementation details.

The runtime context is an explicit method parameter. It is recognized by the generator and is not part of the serialized input schema.

## Boundary types and type lifting

Developers author exported data types beside the implementation:

```rust
#[module::type]
pub struct CreateUser {
    pub email: String,
}
```

Before Cargo builds, the generator reads the declaration and emits the real compiled type into the module's generated contract crate. In the implementation crate, the annotated source location resolves to a re-export of that generated type. There is therefore one compiled `CreateUser` type, owned by the contract crate, without asking developers to maintain it in a second file.

Every exported type implements the platform's contract-type trait, referred to here as `ModuleType`. Standard supported types receive platform implementations; user-defined boundary types receive generated implementations. A handwritten implementation that can misrepresent the wire shape is not part of the supported API.

The supported type subset is intentionally smaller than Rust:

- Explicit-width scalar values and strings.
- Contract structs and enums.
- `Option<T>`.
- `Vec<T>`.
- Maps with string keys and contract-safe values.
- Structured contract error enums.
- Dedicated contract types for binary data, streams, sensitive values, and other semantics that ordinary Rust system types cannot carry across bindings.

The generator rejects boundary types it cannot represent faithfully. Unsupported examples include borrowed values and lifetimes, platform-sized integers, arbitrary generics, trait objects, arbitrary `impl Trait`, and standard-library file or I/O handles. These restrictions apply only at exported boundaries; internal module code remains ordinary Rust.

## Presence, nullability, defaults, and validation

Missing and `null` have different meanings. An update may need to distinguish:

```text
field missing -> leave the value unchanged
field null    -> clear the value
field value   -> replace the value
```

The contract therefore records presence and nullability separately:

- `T` is required and non-null.
- `Option<T>` represents an ordinary optional value: absence maps to `None`, while an explicit `null` is not silently collapsed into absence.
- A runtime contract type such as `Field<T>` represents `Missing`, `Null`, or `Value(T)` when all three states matter.

Defaults and declarative validation are contract metadata. For example, a field can declare a default plus minimum and maximum values. Every binding applies the same declared rules before invoking the implementation. The provider remains authoritative; generated clients may validate earlier only as a convenience.

Changing validation so a previously valid input becomes invalid is a breaking semantic change. Complex business rules remain implementation logic and return structured domain errors rather than being forced into the schema.

## Files, binary data, and sensitive values

Exported capabilities do not accept Rust file handles or host paths. Bounded bytes use a contract binary or blob type; streamed data uses a declared byte-stream type.

Sensitive values use a contract-aware wrapper such as `Secret<String>`. That metadata allows compatible bindings to redact values from logs, traces, help output, and other generated surfaces, and to reject an exposure that cannot preserve the required handling. Infrastructure credentials are injected through provider bindings and do not become capability inputs.

## Structured domain and invocation errors

An exported operation declares a structured domain error type:

```rust
#[derive(ModuleError)]
enum CreateUserError {
    EmailAlreadyExists,
    InvalidEmail { reason: String },
}
```

Opaque strings and types such as `anyhow::Error` remain useful internally but cannot cross the module boundary. Callers and generated bindings must be able to identify and handle declared failures.

The generated caller handle separates domain failures from invocation failures:

```rust
client
    .create_user(context, input)
    .await
    -> Result<User, CallError<CreateUserError>>
```

`CreateUserError` represents an expected outcome of the capability. `CallError` represents failure to complete or interpret the invocation, such as a deadline, cancellation, unavailable remote target, unsupported contract value, or invalid response.

This is a remote-shaped type contract, not a claim that local and remote execution behave identically. An in-process binding will not normally produce transport failures, but moving a capability behind HTTP does not require every consumer to adopt a new return type.

## Call context, asynchronous handles, and retry safety

Generated capability handles are always asynchronous, including when a composition binds them in-process. The implementation can still perform synchronous work internally.

`CallContext` carries invocation-scoped information needed consistently across bindings:

- The normalized caller and authentication context.
- A deadline.
- A cancellation signal.
- Tracing context.
- An optional idempotency key where the operation supports one.

Cancellation is advisory. It asks work to stop but does not roll back side effects that already occurred.

Every operation declares the retry or idempotency property needed to reason about another attempt. This does not make every operation idempotent and does not authorize automatic retry of every failed call. Retry policy must respect the operation declaration, deadline, and caller intent.

## Interaction shapes

The contract model recognizes five interaction shapes:

- Unary request and response.
- Server streaming.
- Client streaming.
- Bidirectional streaming.
- Event subscription.

Streaming signatures use platform contract types rather than an arbitrary Rust `impl Stream`, so the schema and each binding can identify the required semantics.

The foundation milestone implements unary calls only. Streaming and event shapes are reserved as first-class contract concepts now so the schema does not embed an assumption that every capability is unary. Cursor, replay, backpressure, delivery, and session semantics remain later runtime design work.

## Language-neutral schema

The canonical compatibility schema is a small, platform-owned, versioned representation rather than OpenAPI, Protobuf, or another transport-specific format. It may use deterministic JSON as its stored encoding.

It records at least the contract information established here:

- Stable module, capability, type, field, and error identities.
- Inputs, outputs, structured errors, and the complete type graph.
- Interaction shape.
- Authentication, exposure, idempotency, validation, deprecation, and sensitive-value metadata.
- Source documentation.
- Generator provenance and a comparable contract revision.

OpenAPI, Protobuf, generated Rust, and language-native SDKs are outputs of this schema when their mapping is faithful. They are not competing authorities. A generated artifact must preserve the documentation and policy metadata applicable to that artifact.

## Forward-compatible decoding

Generated consumers ignore unknown fields in received structs. A provider may therefore add an optional response field without making an older consumer reject the entire response.

Output enums and structured errors include an unknown representation conceptually equivalent to:

```rust
Unknown { tag, payload }
```

An older consumer can report or conservatively handle a new provider variant rather than failing to decode the complete call. If a newer caller sends an enum variant to an older provider that does not understand the requested behavior, the binding rejects it with a structured contract error before invoking the implementation.

These decoding rules do not automatically classify every new enum or error variant as semantically compatible. The generator still reports the precise schema change, and the harness applies the configured compatibility and migration policy.

## Binding conformance and rejection

A binding declares the contract features it can represent faithfully. The generator or application-composition validator rejects an incompatible selection before the application accepts traffic.

Examples include:

- Server-sent events cannot represent a bidirectional streaming capability.
- A basic HTTP request-response binding cannot expose a streaming response unless an appropriate streaming binding is configured.
- A CLI binding can reject an interactive session shape for which it has no faithful terminal representation.

The diagnostic identifies the capability, unsupported feature, and compatible binding kinds. A binding must not silently degrade the contract.

## Handwritten customization

Handwritten customization is allowed behind the canonical contract, not beside it as an invisible second public API.

For example, if generated JSON handling is unsuitable for a file upload, a handwritten multipart HTTP adapter may parse the upload, convert it into the declared contract input, invoke the generated typed handle, and map the declared result back to HTTP. The capability, authorization metadata, errors, and compatibility surface remain the same.

An undocumented route that bypasses the contract would be invisible to dependency analysis, compatibility checking, authorization metadata, and SDK generation. It is therefore not a supported managed binding. Internal implementation logic remains unrestricted because it does not create another module boundary.

## Generated outputs

The generator uses this model to produce the module's language-neutral schema, Rust contract crate, typed handles, implementation-neutral dispatch interface, implementation-local adapter, configured binding artifacts, and programmable contract-level test bindings. Their Cargo topology, provenance, and reproducibility rules are defined in [Rust Build Topology](08-rust-build-topology.md).

## Matters deliberately left to later designs

This contract model does not yet specify:

- Detailed cursor, replay, backpressure, delivery, and bidirectional-session semantics.
- Runtime discovery, placement, routing, lifecycle, and overload behavior.
- Exact wire mappings for every future binding and language SDK.
- The complete permission and resource-authorization model.
- Registry publication and support policy for unmanaged consumers.

Those questions extend the canonical contract; they do not reopen the choice of Rust-first authoring plus a generated language-neutral compatibility schema.
