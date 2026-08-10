# Canonical Capability Contract

[Back to the white paper](../boxology-whitepaper.md)

This document defines the target canonical contract authored by a native Rust box; that target exceeds the shipped S1-S4 subset. Delivered behavior is defined by [S1](../specs/s1-runtime-core.md), [S2](../specs/s2-contract-generator.md), [S3](../specs/s3-http-binding.md), and [S4](../specs/s4-contract-change-classification.md). Grammar, metadata, interaction shapes, and binding behavior beyond those specifications remain target design.

## Two authorities, one source of human intent

The declaration-only Rust contract block is the **authoring authority**. Developers and agents keep it beside the ordinary implementation they maintain. They do not maintain an exported service trait or a separate interface-definition file.

The deterministic generated schema is the **compatibility authority**. Contract diffing, binding validation, generated SDKs, and migration analysis consume the schema rather than interpreting the Rust syntax independently.

The two roles are deliberately different:

```text
controlled Rust contract block
-> deterministic contract generator
-> language-neutral contract schema
-> generated Rust contract crate and configured bindings
```

The generated schema is derived and checked into Git. It is never an independently edited source of truth.

## Authoring surface

A box has one direct declaration-only contract block and one direct ordinary inherent implementation:

```rust
boxology::contract! {
    #[error]
    pub enum GreetError {
        EmptyName,
    }

    #[capability(exposure = external)]
    pub async fn greet(name: String) -> Result<String, GreetError>;
}

pub struct HelloService;

#[boxology::implementation]
impl HelloService {
    async fn greet(
        &self,
        context: boxology::CallContext,
        name: String,
    ) -> Result<String, GreetError> {
        // ordinary Rust body
        todo!()
    }
}
```

The two signatures intentionally repeat. The declaration is the deterministic, language-neutral boundary; the inherent method is executable Rust. Generated assertions make rustc reject any mismatch.

The v0 source model is fixed where listed and fail-closed elsewhere. This section states the canonical **target** grammar; v0's **implementation evidence corpus** is the scalar boundary subset proven by the four fixtures named in [V0 Streams](11-v0-streams.md#the-v0-evidence-corpus). Post-V0 work now parses and projects to format-1 schema the narrow structured subset of named-field structs, unit enums, declaration-before-use local references, and `Base`/`Option<Base>`/`Vec<Base>`/`Option<Vec<Base>>`; its deterministic emitter also produces the corresponding public Rust types and strict codecs. Complete structured call surfaces remain fail-closed under `BXG0038` until descriptors and dispatch/handle/fake/adapter glue land; named-field error payloads remain gated by `BXG0048`, `Blob` end-to-end generation by `BXG0040`, and `Secret` authoring/end-to-end generation remains outside the narrow grammar under `BXG0038`. Core `ContractValue`, presence, `Blob`, `Secret`, redaction, and codec semantics remain shipped and tested; this scoping does not remove them.

- Every declared type, field, variant, and capability uses an ordinary non-raw Rust identifier. A struct is `pub struct Name { pub field: Type, ... }`; all fields are named and public. An enum is `pub enum Name` with unit, one-value, or named-field variants. `#[error]` has no arguments and marks an enum as an error.
- A capability is `#[capability(...)] pub async fn name(input: Type) -> Result<Output, ErrorType>;`, has exactly one input, and names an in-block `#[error]` enum directly as `ErrorType`. Multiple logical inputs use a struct.
- Context is implicit in the contract declaration. Its implementation receives `&self`, `boxology::CallContext`, then exactly the declared input.
- The public capability name defaults to the Rust function name.
- Capability arguments are unique, comma-separated, order-independent, and allow a trailing comma: `name = "[a-z][a-z0-9_]*"`, `exposure = code_only | internal | external`, and `idempotency = none | inherent`. No other key or value is admitted. An explicit name is the target rename-preserving mechanism when the Rust function is renamed internally. In v0 the public capability name is the declared Rust function name; the explicit `name = "..."` override is post-v0 ([#480](https://github.com/fontanierh/boxology/issues/480)), and partial parser/model handling carries no v0 support claim.
- Exposure defaults to `code_only`; idempotency defaults to `none`.
- Rust doc comments/direct string `#[doc = "..."]`, `#[deprecated]` or `#[deprecated(note = "...")]`, `#[error]`, and `#[capability(...)]` are the complete attribute set. No derive is admitted.
- The implementation body and all code behind the boundary remain ordinary Rust and may use imports, aliases, qualified paths, macros, helpers, and private types.

The canonical leaf types are exactly `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`, `String`, and `Blob`. Containers are `Option<T>`, `Vec<T>`, `BTreeMap<String, T>`, `Field<T>`, and `Secret<T>`, plus a supported in-block declared type. The grammar rejects aliases, imports, qualified paths, re-exports, references, lifetimes, arbitrary generics, associated types, `impl Trait`, `cfg`, user macros, and derives. These are authoring restrictions only, not restrictions on the implementation.

V0 also rejects self-imports, `Keyed` idempotency, authentication or validation/default metadata, non-unary capabilities, and handwritten metadata implementations. `Field<T>` is legal only as a top-level capability input/output or a named struct field—not in lists, maps, enum payloads, or `Secret`. Presence may wrap `Secret<T>`; `Secret<T>` may not transitively contain `Option` or `Field`; nested presence wrappers are rejected.

Duplicate type names, effective capability names, struct fields, and enum variants are reported in deterministic declaration order. The required capability-to-error-enum link is resolved over the complete block. The current narrow subset resolves local data references only to an earlier declaration, which rejects self, forward, and recursive references. Broader resolution and recursion semantics remain later grammar work; no support for them is inferred here.

The implementation site is one non-generic inherent impl for one concrete receiver. Capability methods are non-generic, have no `where` clause or `impl Trait`, and use exactly `&self`, `boxology::CallContext`, one input, and the declared result. The implementation macro rejects structural deviations before generated calls let rustc prove alias-resolved nominal type equality and required `Send` bounds. A signature that merely accepts `impl Into<Input>` is not equivalent.

## Boundary types and type lifting

Before the normal application build, the generator reads each declaration and emits the real compiled type into the box-specific generated package. The implementation names that package through the fixed Cargo dependency alias `boxology_generated_contract`. During normal compilation, the source macro imports the type rather than defining another one and requires the generated marker keyed by the shared parser's canonical semantic contract digest. There is therefore one compiled public definition, and stale generated output fails compilation. The author-facing `boxology` facade re-exports exactly the `contract` and `implementation` macros plus `boxology_contract::CallContext` as `boxology::CallContext`; it exposes no wider kernel or runtime APIs.

Every exported type satisfies the generated contract-type model, referred to here as `ContractType`. Standard supported types receive platform implementations; user-defined boundary types and their metadata implementations are generated. Handwritten implementations are not admitted in v0.

The supported type subset is intentionally smaller than Rust:

- The explicit v0 leaves listed above.
- Contract structs and enums.
- `Option<T>`.
- `Vec<T>`.
- Maps with string keys and contract-safe values.
- Structured contract error enums.
- Dedicated `Blob`, `Field<T>`, and `Secret<T>` semantics.

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
boxology::contract! {
    #[error]
    pub enum CreateUserError {
        EmailAlreadyExists,
        InvalidEmail { reason: String },
    }
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
