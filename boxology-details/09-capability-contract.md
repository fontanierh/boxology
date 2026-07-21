# Canonical Capability Contract

[Back to the white paper](../boxology-whitepaper.md)

This document defines the canonical contract authored by a native Rust box. It fixes the relationship between annotated implementation code, the generated language-neutral schema, typed Rust handles, and transport or client bindings.

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
#[boxology::capability(
    name = "create_user",
    auth = "customer",
    idempotency = "none"
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
#[boxology::contract]
pub struct CreateUser {
    pub email: String,
}
```

Before the normal application build, the generator reads the declaration and emits the real compiled type into the box's generated contract crate. In the implementation crate, the annotated source location resolves to a re-export of that generated type. There is therefore one compiled `CreateUser` type, owned by the contract crate, without asking developers to maintain it in a second file.

Boxology combines structural extraction with a stable-Rust compiler probe. Rust therefore resolves ordinary imports, qualified paths, and type aliases before Boxology accepts the boundary semantics. A resolved type is accepted only when the generated assertions and metadata reporters prove that it belongs to the supported contract model. Target-dependent contract shape remains rejected because it would create more than one compatibility authority.

The generator explicitly propagates only supported attributes and derives to the lifted type. An unknown attribute or derive is a generation error rather than something silently discarded. These restrictions apply to exported declarations, not to internal Rust code.

Every exported type satisfies the platform's contract metadata model, referred to here as `ContractType`. Standard supported types receive platform implementations; user-defined boundary types receive generated implementations and compiler-checked metadata. Whether manually implemented metadata traits may participate remains undecided.

The supported type subset is intentionally smaller than Rust:

- Explicit-width scalar values and strings.
- Contract structs and enums.
- `Option<T>`.
- `Vec<T>`.
- Maps with string keys and contract-safe values.
- Structured contract error enums.
- Dedicated contract types for binary data, streams, sensitive values, and other semantics that ordinary Rust system types cannot carry across bindings.

The generator rejects boundary types it cannot represent faithfully. Unsupported examples include borrowed values and lifetimes, platform-sized integers, arbitrary generics, trait objects, arbitrary `impl Trait`, and standard-library file or I/O handles. These restrictions apply only at exported boundaries; internal box code remains ordinary Rust.

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

Defaults and declarative validation are contract metadata. For example, a field can declare a default plus minimum and maximum values. Every binding applies the same declared rules before invoking the implementation. The provider remains authoritative; generated clients may validate earlier only as a convenience. A client-side validation failure reflects that consumer's schema revision and remains distinguishable from a provider-returned contract or domain error. It is not proof that a differently versioned provider would reject the input.

Changing validation so a previously valid input becomes invalid is a breaking semantic change. Complex business rules remain implementation logic and return structured domain errors rather than being forced into the schema.

## Files, binary data, and sensitive values

Exported capabilities do not accept Rust file handles or host paths. Bounded bytes use a contract binary or blob type; streamed data uses a declared byte-stream type.

Sensitive values use a contract-aware wrapper such as `Secret<String>`. That metadata allows compatible bindings to redact values from logs, traces, help output, and other generated surfaces, and to reject an exposure that cannot preserve the required handling. Infrastructure credentials are injected through provider bindings and do not become capability inputs.

## Structured domain and invocation errors

An exported operation declares a structured domain error type:

```rust
#[boxology::contract(error)]
pub enum CreateUserError {
    EmailAlreadyExists,
    InvalidEmail { reason: String },
}
```

Errors use the same lift-and-re-export mechanism as every other boundary type. The generator supplies both `ContractType` and the error-specific `ContractError` behavior; developers do not maintain a separate derive-based compilation path.

Opaque strings and types such as `anyhow::Error` remain useful internally but cannot cross the box boundary. Callers and generated bindings must be able to identify and handle declared failures.

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

Every operation declares the retry or idempotency property needed to reason about another attempt. Conceptually, the declaration distinguishes:

- **None:** retry is not known to be safe.
- **Inherent:** the operation itself is safe to repeat without platform deduplication state.
- **Keyed:** safety depends on stored deduplication state associated with an idempotency key.

The exact source spelling is a tooling choice. A keyed declaration is not decorative metadata: composition validation rejects it unless an implementation capable of honoring the guarantee is configured. The database-free foundation milestone does not provide keyed deduplication.

These declarations do not authorize automatic retry of every failed call. Retry policy must respect the operation declaration, deadline, and caller intent.

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

- Stable box, capability, type, field, and error identities.
- Inputs, outputs, structured errors, and the complete type graph.
- Interaction shape.
- Authentication, exposure, idempotency, validation, deprecation, and sensitive-value metadata.
- Source documentation.
- Generator provenance and a comparable contract revision.

OpenAPI, Protobuf, generated Rust, and language-native SDKs are outputs of this schema when their mapping is faithful. They are not competing authorities. A generated artifact must preserve the documentation and policy metadata applicable to that artifact.

Documentation remains part of the generated schema and artifact revision, but the change classifier reports a documentation-only change separately from semantic compatibility changes. Editing a Rust doc comment therefore does not create a migration signal.

## Forward-compatible decoding

Generated consumers ignore unknown fields in provider outputs. A provider may therefore add an optional response field without making an older consumer reject the entire response.

Output enums and structured errors include an unknown representation conceptually equivalent to:

```rust
Unknown { tag, payload }
```

An older consumer can report or conservatively handle a new provider variant rather than failing to decode the complete call. Because the older consumer cannot know the classification of a newly introduced payload, an unknown payload is opaque and sensitive by default. Generated debug, logging, and tracing output redacts it; reading or forwarding the raw payload requires an explicit action.

Input is intentionally stricter. If a newer caller sends an unknown field or enum variant to an older provider, the binding rejects it with a structured contract error before invoking the implementation. Silently ignoring requested behavior could produce a successful but incorrect side effect. Adding an optional input field therefore requires provider-first deployment before callers begin sending it, which is the normal expand-migrate-contract order.

These decoding rules do not automatically classify every new enum or error variant as semantically compatible. The generator still reports the precise schema change, and the harness applies the configured compatibility and migration policy.

## Binding conformance and rejection

A binding declares the contract features it can represent faithfully. The generator or application-composition validator rejects an incompatible selection before the application accepts traffic.

Examples include:

- Server-sent events cannot represent a bidirectional streaming capability.
- A basic HTTP request-response binding cannot expose a streaming response unless an appropriate streaming binding is configured.
- A CLI binding can reject an interactive session shape for which it has no faithful terminal representation.
- A binding that cannot carry the complete range of `u64` must use a declared lossless representation, such as a decimal string, or reject the capability. It may not silently coerce the value into a lossy number.

The diagnostic identifies the capability, unsupported feature, and compatible binding kinds. A binding must not silently degrade the contract.

Context propagation is also a conformance dimension. A binding declares how it carries deadlines, tracing, idempotency keys, authentication context, and cancellation. It may not silently discard them. Composition validation rejects a binding when a capability requires a context property that the binding cannot preserve.

## Handwritten customization

Handwritten customization is allowed behind the canonical contract, not beside it as an invisible second public API.

For example, if generated JSON handling is unsuitable for a file upload, a handwritten multipart HTTP adapter may parse the upload, convert it into the declared contract input, invoke the generated typed handle, and map the declared result back to HTTP. The capability, authorization metadata, errors, and compatibility surface remain the same.

An undocumented route that bypasses the contract would be invisible to dependency analysis, compatibility checking, authorization metadata, and SDK generation. It is therefore not a supported managed binding. Internal implementation logic remains unrestricted because it does not create another box boundary.

## Generated outputs

The generator uses this model to produce the box's language-neutral schema, Rust contract crate, typed handles, implementation-neutral dispatch interface, implementation-local adapter, configured binding artifacts, and programmable contract-level test bindings. Their Cargo topology, provenance, and reproducibility rules are defined in [Rust Build Topology](08-rust-build-topology.md).

## Matters deliberately left to later designs

This contract model does not yet specify:

- Detailed cursor, replay, backpressure, delivery, and bidirectional-session semantics.
- Runtime discovery, placement, routing, lifecycle, and overload behavior.
- Per-binding context mappings, including deadline-budget calculation, authentication propagation, and cancellation behavior.
- Keyed-idempotency scope, retention, replay responses, storage, and provider integration.
- Exact wire mappings for every future binding and language SDK.
- The complete permission and resource-authorization model.
- Registry publication and support policy for unmanaged consumers.

Those questions extend the canonical contract; they do not reopen the choice of Rust-first authoring plus a generated language-neutral compatibility schema.
