# S1 Spec — Runtime Core and Composition Assembly

[Stream definition](../boxology-details/11-v0-streams.md#s1--runtime-core-and-composition-assembly) · Status: **revised, awaiting re-review** (first review addressed; cross-stream contract in issue #85)

S1 builds the kernel crates: the contract-type model, the descriptor ABI, the call context, the error model, the handle machinery, the composition assembly API, and the in-process binding. This is the definitionally non-box layer — everything the generator will later emit compiles against it. Normative input: [Canonical Capability Contract](../boxology-details/09-capability-contract.md); this spec makes implementation decisions, it does not reopen that contract.

## Purpose

Every later stream consumes S1. Its job is to make the merged semantics *executable and conformance-tested*, and — per the cross-stream repair in issue #85 — to own the **descriptor ABI**: the generated, runtime-consumable description of a box's contract that assembly validation, wire decoding, and conformance checking all require. S1 defines the descriptor types; S2 generates their values; S3 decodes against them.

S1's second deliverable is the fixture suite: hand-written crates in exactly the shape S2 must later emit, which become S2's golden targets (fixture protocol in D10).

## Non-goals

- No authentication, realms, delegation, or authorization semantics — `Caller` is a placeholder pending #8/#9.
- No streaming/event/session runtime types; descriptors carry the shape field, always `Unary` in v0.
- No remote transports (S3), no telemetry export, no providers, no generic CLI.
- No public-API stability promise: v0 crates are `0.x`, consumed from source.
- No `Keyed` idempotency support: descriptors admit `None | Inherent` only; `Keyed` is rejected at generation (S2) because no deduplication implementation exists to honor it — carrying the declaration unenforced would be a false promise.
- No validation/default enforcement: v0 rejects `default`/`min`/`max`/validation annotations at generation with "not supported in v0" diagnostics (S2), rather than S1 carrying unenforced slots. The canonical contract remains the target; the v0 subset excludes it honestly.

## Decisions

### D1 — Two crates; the invocation ABI lives in `boxology-contract`

- **`boxology-contract`** — everything generated code and consumers compile against: the value model (`ContractValue`), `ContractType`/`ContractError`, `Field<T>`, `Secret<T>`, `Blob`, `OpaquePayload`, the **descriptor types** (D5), and the **invocation ABI** — `CallContext`, `CallError<E>`, `Deadline`, `CancelToken`, `TraceContext`, and the erased dispatch trait. Resolves the first review's blocking finding: generated contract crates define handles that take `CallContext` and return `CallError`, so those types must live in the one crate generated code depends on. A third invocation-ABI crate was considered and rejected as speculative; this crate stays free of I/O, servers, and assembly, and its one async-adjacent dependency is `tokio-util` for `CancellationToken` (recorded, challengeable, one wrapped type to replace).
- **`boxology-runtime`** — assembly and execution: the composition builder, import resolution and exposure operations, validation, the in-process binding, the transport-binding trait. Compositions and transports depend on it; generated contract crates never do.

Generated contract crates depend on `boxology-contract` **only** (plus explicitly permitted foreign contract crates for public type reuse — none in v0; see the S2 spec).

### D2 — `ContractValue`: invariant-bearing, not a public enum

The semantic intermediate representation is an opaque struct with a private representation, **fallible constructors, and read-only accessors/visitors** — not a public enum. Illegal states are unrepresentable by construction: non-finite floats are rejected at `ContractValue::f64(x)`, duplicate object keys at insertion, and `Missing` exists only as a field-slot state inside object construction, never as a free value. The first draft's public enum could not carry these invariants; S3's codec relies on them, so they are structural.

Value kinds: null, bool, i64, u64, f64 (finite), string, bytes, list, object (ordered fields with per-field presence), plus the field-slot states. Encode (`typed → ContractValue`) is infallible for well-typed values; decode (`ContractValue → typed`) is fallible and strict.

### D3 — Decoding is descriptor-guided and role-aware

A schema-free syntax mapping is not invertible (issue #85, item 1): `"42"` on a wire is a string or a decimal-encoded `u64` only the expected type can decide; strict-input versus tolerant-output depends on direction. Therefore the layering is:

```text
wire syntax  ←→  ContractValue        (S3: descriptor-guided, role-aware)
ContractValue ←→ typed Rust values    (S1: generated ContractType impls, strict)
```

`boxology-contract` defines `DecodeRole { ProviderInput, ConsumerOutput }` and the descriptor-walking validation used at the `ContractValue` layer: `ProviderInput` rejects unknown fields and unknown variants; `ConsumerOutput` ignores unknown fields and preserves unknown variants as `OpaquePayload`. Bindings own only syntax; every semantic judgment happens here, once.

### D4 — Opaque payloads are actually opaque

Unknown-variant payloads use `OpaquePayload`: private storage, redacted `Debug` (`OpaquePayload(..)`), no accessor that yields the raw value except explicit `reveal()` (documented as sensitive) and `forward()` (re-encoding for pass-through). Leakage tests assert the redaction in `Debug`, `Display`-absence, and diagnostics paths.

### D5 — The descriptor ABI

`boxology-contract` defines immutable descriptor types; S2 generates `static` values in each contract crate; S1/S3 consume them:

- **`TypeDescriptor`** — the runtime type graph: scalars with width, string, blob, secret-wrapped, list, string-keyed map, struct (fields: name, presence kind `Required | Optional | TriState`, type, sensitivity), enum (variants: name, payload type), error enums. Drives role-aware decoding and conformance.
- **`CapabilityDescriptor`** — capability local name, qualified id, input/output/error `TypeDescriptor`s, interaction shape (`Unary`), maximum exposure, idempotency (`None | Inherent`), documentation presence.
- **`ImportDescriptor`** — the imported box id, contract revision expectation, and the imported capability set.
- **`BoxDescriptor`** — box id, capability descriptors, import descriptors, contract revision fingerprint.

Descriptors are plain data (`'static`, no closures), so they are also emitted into fixtures by hand and compared structurally in tests.

### D6 — Error model

`ContractError` is a marker + tag-access trait on generated error enums. `boxology-contract` defines:

```rust
#[non_exhaustive]
pub enum CallError<E> {
    Domain(E),
    Deadline,
    Cancelled,
    Unavailable(Detail),
    ContractViolation(Detail),   // caller-side value rejected before/at the boundary
    InvalidResponse(Detail),     // provider output the contract cannot accept
    Internal(Detail),
}
```

The erased layer's error is defined concretely: `ErasedCallError` carries either `Domain { error_tag: String, payload: ContractValue }` (typed handles decode it to `E` via the error `TypeDescriptor`) or one of the invocation classes with `Detail`. **The in-process binding can produce `InvalidResponse`** — a typed implementation returning output that fails contract encoding (or a domain error that fails encoding) is an invalid provider response wherever it runs; the first draft's claim otherwise is withdrawn. Caller-input violations (`ContractViolation`) are tested separately from provider-output violations (`InvalidResponse`), with no invocation occurring in the former case.

### D7 — `CallContext` and explicit child derivation

Fields: `caller: Caller` (`Anonymous | System(&'static str)` placeholder), `deadline: Option<Deadline>` (absolute; accessor yields remaining budget), `cancellation: CancelToken`, `trace: TraceContext` (opaque W3C strings, carried not interpreted), `idempotency_key: Option<IdempotencyKey>` (transported, never honored in v0 — documented on the accessor).

**Propagation is explicit, not automatic.** The first draft's "transitive propagation is automatic" is replaced by an API: `CallContext::child()` preserves deadline, trace continuity, and caller; derives a child cancellation token (parent cancels child, not vice versa); and **drops the idempotency key** — a key is an operation-scoped decision, never blindly inherited. Constructing a fresh context or a child is the caller's visible choice; there is no ambient state.

### D8 — Erased dispatch is object-safe; handles are generated sugar

```rust
pub trait ErasedTarget: Send + Sync {
    fn call<'a>(&'a self, capability: &'a CapabilityId, ctx: CallContext, input: ContractValue)
        -> Pin<Box<dyn Future<Output = Result<ContractValue, ErasedCallError>> + Send + 'a>>;
}
```

Boxed-future signature because native `async fn` in traits is not dyn-compatible; this is the ABI, stated exactly. Generated handles wrap `Arc<dyn ErasedTarget>` plus descriptor-guided encode/decode. The `ContractValue`-level calling surface is `#[doc(hidden)]`-public for bindings, tests, and fixtures; box code uses typed handles only (a checker rule, not a compiler claim — see D9).

### D9 — “Supported API” is enforced by generation and checking, not by Rust privacy

The first draft claimed a sealed `ContractType` with a compile-fail test. That is impossible: Rust privacy cannot admit sibling generated crates while rejecting handwritten ones. Withdrawn and replaced with the feasible boundary: `ContractType` is a public trait; a handwritten impl compiles, **but can never become part of a managed contract**, because contracts exist only via generation — the schema, descriptors, and contract crate are derived outputs whose byte-reproduction and ownership checks (S5) reject hand-edited or hand-added content. The acceptance criterion becomes a checker-level test, not a compile-fail test.

### D10 — Composition assembly: imports and exposures are different operations

The first draft's single `.bind()` could not represent the merged Hello composition (which binds `greet` both in-process and over HTTP) and conflated two operations with different cardinalities. Corrected API:

```text
CompositionBuilder
  .register(BoxDescriptor, Arc<dyn ErasedTarget>)          // implementation joins the composition
  .resolve_import(consumer_box, import_id, ImportTarget)   // exactly one per declared import slot
  .expose(provider_box, capability_id, Exposure)           // zero or more per capability
  .validate() -> ValidationReport | structured errors
  .start()    -> Composition
```

- **Registration carries the descriptor**, giving validation everything it needs (capabilities, imports, exposure maxima, shapes, idempotency); an erased target alone reveals nothing.
- **Import resolution** (consumer side): every `ImportDescriptor` slot of every registered box resolves to exactly one target — a registered local box or (later) a transport client. Missing and duplicate resolutions are errors with the import identity named.
- **Exposure** (provider side): zero-or-more per capability; each names a transport binding (v0: `InProcessHandle` — mints host-level typed handles from the composition — or an S3-provided server exposure) and an exposure level validated against the descriptor's maximum over the accepted order `code-only < internal < external` (implementing `03-runtime.md`'s already-normative rule — recorded as accepted, not open). Duplicate exposures of one capability through *different* transports are legal by design; duplicates within one transport are that transport's validation call.
- **Transport conformance** happens at `expose`: the transport receives the `CapabilityDescriptor` and may reject shapes or features it cannot represent faithfully, with typed diagnostics (this is where S3 rejects, e.g., a top-level `Field<T>` — a binding-level rule, not a global type-model prohibition; the first draft put that rejection at the wrong layer).

### D11 — In-process binding semantics and future ownership

In-process dispatch **runs inline in the caller's future** — nothing is spawned. Consequences, stated as contract: cancellation is advisory (token observation is the callee's choice); if the caller drops the future, execution stops with it — **continuation-after-abandonment is a transport-binding property, not an S1 guarantee** (S3 provides it via owned tasks; issue #85 item 8). An already-expired deadline short-circuits to `Deadline` with zero invocation; no timeout is imposed around a live call (transports enforce at their boundary; in-process trusts the callee to observe budget — the intended asymmetry). Panics convert to `Internal` at the dispatch boundary via catch-unwind.

### D12 — Fixtures: exact inventory and the pre-macro rule

`crates/fixtures/` contains `hello/` and `kitchen-sink/` (every scalar, bool, f32/f64, `Option`/`Field` positions, nested structs, enums with/without payloads, structured errors, `Secret`, `Blob`, string-keyed maps, multi-capability). Per-fixture inventory — exactly what S2 must later emit, hand-written now:

```text
fixtures/<name>/
  authoring/          # annotated source (#[boxology::...]) — PARSE-ONLY DATA in S1,
                      #   not a workspace member; compiles only once S2's macros exist
  implementation/     # compiled crate, un-annotated methods + handwritten adapter
                      #   at the emitted-adapter shape, incl. the include! stub layout
  generated/contract/ # hand-written generated-style crate: types, ContractType impls,
                      #   descriptors, dispatch trait, handle, test-support module
  generated/schema.json
```

The **test-support module** (programmable contract-level fake per capability — an accepted generator output the first S2 draft dropped) is hand-written here too, so its shape is designed before it is mechanized. Golden-evolution protocol: once S2's byte-equality check exists, fixture shape and generator change **atomically in one task PR** — the first draft's "fixture PR first, generator PR second" would leave required checks red between merges and is withdrawn. Provenance fields in goldens use a placeholder token normalized at comparison (S2 spec, D-golden).

## Acceptance criteria

1. The presence grid passes exhaustively in both directions at the `ContractValue` layer, under **both decode roles** — strict `ProviderInput` (unknown fields/variants rejected) and tolerant `ConsumerOutput` (unknown fields ignored; unknown variants preserved as `OpaquePayload`) — as table tests per cell.
2. Typed round-trips state their domains precisely: for every kitchen-sink type and role-valid value, `decode(encode(v)) = v` (typed domain); tolerant-decode cases additionally assert exactly which information is dropped or preserved.
3. `OpaquePayload` leakage tests pass: redacted `Debug`, no raw value in any diagnostic path, `reveal`/`forward` behave as specified.
4. Assembly validation rejects each failure class with structured diagnostics: missing import, duplicate import resolution, unknown capability, exposure above maximum, transport-conformance rejection (exercised with a stub transport that refuses a marked capability).
5. The Hello-shape composition supports simultaneous in-process and stub-transport exposure of one capability — the merged manifest example is representable.
6. In-process semantics proven: expired deadline → `Deadline` with zero invocations; caller-input violation → `ContractViolation` with zero invocations; provider output failing encoding → `InvalidResponse`; panic → `Internal`; cancellation token observed by a cooperating fixture capability.
7. A handwritten `ContractType` impl in a non-generated crate compiles (negative of the withdrawn sealing claim) and a checker-level fixture demonstrates it cannot enter a managed contract (asserted structurally against descriptors in v0; mechanically once S5 exists).
8. All green under S0's validation on both platforms.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | `boxology-contract` scaffold: invariant-bearing `ContractValue`, constructors, accessors/visitors | 2 |
| T2 | Presence machinery + decode roles: `Field<T>`, grid, `OpaquePayload`, role-aware walking | 2 |
| T3 | `ContractType`/`ContractError`, platform impls (scalars, collections, `Secret`, `Blob`) | 2 |
| T4 | Descriptor types (`TypeDescriptor`, `CapabilityDescriptor`, `ImportDescriptor`, `BoxDescriptor`) | 1 |
| T5 | Invocation ABI: `CallContext` + `child()`, `Deadline`, `CancelToken`, `TraceContext`, `CallError`, `ErasedCallError`, `ErasedTarget` | 2 |
| T6 | `boxology-runtime`: builder, register/resolve_import/expose, validation, transport trait, in-process binding | 2–3 |
| T7 | Fixtures per D12 inventory (incl. authoring data + test-support) + integration/property suites | 2–3 |

T1→T2→T3 sequence; T4/T5 after T1; T6 needs T4+T5; T7 last.

## Matters left open

*(None load-bearing.)*

- The `Caller` shape beyond the placeholder — owned by the post-v0 auth cluster (#8/#9).
- Whether `ContractValue`'s hidden calling surface ever graduates to documented API — revisit at a third consumer.
- Replacing `tokio-util`'s token with an owned primitive — only if executor-agnosticism becomes a real requirement.

## Tracker notes

This spec decides part of what #6 listed as open (the erased-dispatch ABI and in-process execution semantics); #6 retains discovery, placement, routing, multi-version coexistence, and overload. The exposure order implements the accepted `03-runtime.md` rule. Issue #85 items 1–5 and 8 are resolved here jointly with the S2/S3 revisions.
