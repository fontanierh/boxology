# S1 Spec — Runtime Core and Composition Assembly

[Stream definition](../boxology-details/11-v0-streams.md#s1--runtime-core-and-composition-assembly) · Status: **proposed**

S1 builds the kernel crates: the contract-type model, the call context, the error model, the handle machinery, the composition assembly API, and the in-process binding. This is the definitionally non-box layer — everything the generator will later emit compiles against it. Normative input: [Canonical Capability Contract](../boxology-details/09-capability-contract.md); this spec makes implementation decisions, it does not reopen that contract.

## Purpose

Every later stream consumes S1: generated contract crates depend on its type model, the generator emits code against its traits, bindings implement its assembly API, and the classification stream trusts that its presence and error semantics are what the schema says they are. S1's job is to make those semantics *executable and conformance-tested* so that downstream streams inherit tested behavior, not prose.

S1 has a second, less obvious deliverable: **hand-written fixture boxes in exactly the shape the generator will later emit.** Since no generator exists yet, S1's integration tests are written against hand-authored "generated-style" contract crates. These fixtures then become S2's golden targets — the generator is correct when it byte-emits what S1 wrote by hand. This inverts the usual codegen risk: the emitted shape is designed, reviewed, and tested as ordinary code first, then mechanized.

## Non-goals

- No authentication, realms, delegation, or authorization semantics — `CallContext` carries a minimal caller slot whose real design is post-v0 (#8/#9).
- No streaming, event, or session shapes — the type model reserves nothing for them beyond what the schema needs; their runtime types arrive with the full-runtime release (#11).
- No remote transports (S3), no telemetry export, no providers, no generic CLI.
- No public-API stability promise: v0 crates are `0.x` and consumed from source; API churn during v0 is expected and cheap.

## Decisions

### D1 — Two crates: `boxology-contract` and `boxology-runtime`

- **`boxology-contract`** — the pure type model: `ContractValue`, `ContractType`, `ContractError`, `Field<T>`, `Secret<T>`, `Blob`, presence/validation machinery. No async, no I/O, minimal dependencies. This is what *generated contract crates* depend on, so it must stay lightweight: a consumer compiling against a contract crate should not pull in an HTTP stack or an executor.
- **`boxology-runtime`** — invocation and assembly: `CallContext`, `CallError`, cancellation, the erased dispatch layer, the composition builder and validation, the in-process binding. Implementation crates and compositions depend on it; contract crates do not.

The dependency direction (`runtime → contract`, never the reverse) mirrors the box edge rules at the platform layer and keeps the contract crate compile-time cheap.

### D2 — `ContractValue` is the semantic intermediate representation

Contract types encode to and decode from a boxology-owned value tree rather than implementing serde directly:

```rust
enum ContractValue {
    Null,
    Bool(bool),
    I64(i64), U64(u64), F64(f64),   // finite only; non-finite is rejected at construction
    String(String),
    Bytes(Vec<u8>),
    List(Vec<ContractValue>),
    Object(Vec<(String, ContractValue)>),   // ordered, duplicate keys rejected
    Missing,                                 // presence marker, legal only as an object field
}
```

Rationale: the wire rules are bespoke — `u64` as decimal strings on the wire, adjacent enum tagging, the `Missing`/`Null`/`Value` tri-state, reject-unknown-input-fields, preserve-unknown-variants-as-opaque-payloads. Pushing those rules into per-binding serde impls would scatter the single most conformance-critical logic across every transport. With an IR: `ContractType` implements the *semantics* once (`encode → ContractValue`, `decode ← ContractValue` with strictness rules); each binding implements only a syntax mapping (`ContractValue ↔ JSON bytes`), and the S1 conformance grid tests semantics independently of any wire. Serde interop is explicitly a non-goal for boundary types in v0.

`ContractType` is **sealed against handwritten implementations** (private supertrait in a hidden module): the merged contract says a handwritten impl that can misrepresent the wire is not supported API, and sealing makes that mechanical. Platform types and generated types implement it; nothing else can.

### D3 — The presence grid is the conformance core

The decode rules for object fields form a fixed grid, implemented once in `boxology-contract` and tested exhaustively:

| Declared | absent | `Null` | value |
| --- | --- | --- | --- |
| `T` | error: required | error: required non-null | `T` |
| `Option<T>` | `None` | error: explicit null not accepted | `Some(T)` |
| `Field<T>` | `Missing` | `Null` | `Value(T)` |

Top-level (non-field) positions: `Option<T>` maps `Null` ↔ `None`; `Field<T>` is rejected at generation time, so the runtime treats a top-level `Missing` as a contract violation. Generated output enums and errors carry `Unknown { tag: String, payload: ContractValue }`; the payload is treated as opaque and sensitive (never logged, never re-inspected) per the merged decoding rules. Every cell of this grid, both directions, is a table test; S3's wire suite later reuses the same cases at the JSON layer.

### D4 — Error model

```rust
// boxology-contract
trait ContractError: ContractType { /* marker + tag access */ }

// boxology-runtime
#[non_exhaustive]
enum CallError<E> {
    Domain(E),                     // declared, expected outcome
    Deadline,
    Cancelled,
    Unavailable { detail },        // bound target unreachable
    ContractViolation { detail },  // input/value rejected before or at the boundary
    InvalidResponse { detail },    // target returned something the contract cannot accept
    Internal { detail },
}
```

Handles return `Result<Output, CallError<DomainError>>`. `CallError` is `#[non_exhaustive]`: it is a platform type whose variants may grow (it is not a contract enum and does not use the `Unknown` mechanism). The in-process binding constructs only `Domain`, `Deadline`, `Cancelled`, `ContractViolation`, and `Internal`; `Unavailable`/`InvalidResponse` exist for remote bindings but the *type* is shared — the remote-shaped-everywhere decision, realized.

### D5 — `CallContext`

```rust
struct CallContext {
    caller: Caller,             // v0: Anonymous | System(&'static str); real principal model is post-v0
    deadline: Option<Deadline>, // absolute instant; accessor returns remaining budget
    cancellation: CancelToken,
    trace: TraceContext,        // opaque W3C traceparent/tracestate strings in v0
    idempotency_key: Option<IdempotencyKey>,
}
```

- Constructed by bindings or test code via a builder; there is no ambient/global context — explicitness is the merged design.
- **Deadline semantics:** stored absolute, exposed as remaining budget (`context.remaining()`); child calls inherit the same deadline by default (transitive propagation is automatic because the instant is shared). No deadline means the composition default applies at the binding, not in core.
- **Cancellation is advisory:** observing it is the implementation's choice; the runtime never kills work. `CancelToken` is a thin wrapper over `tokio_util::sync::CancellationToken` (see D8).
- **Trace context is carried, not interpreted:** v0 stores and propagates the header strings; no span API, no exporter. This keeps the #29 observability surface open without blocking on it.
- **The idempotency key is transported, never honored:** no dedup exists in v0; the accessor's documentation says so, preventing the false-promise failure mode flagged in review.

### D6 — Handle machinery and erased dispatch

The runtime defines one erased call shape; everything typed is generated sugar over it:

```rust
// what a dispatch target implements (contract crates define typed dispatch
// traits; generated adapters erase into this):
async fn call(&self, capability: CapabilityId, ctx: CallContext, input: ContractValue)
    -> Result<ContractValue, ErasedCallError>;
```

Generated handle types (`CustomerClient`) wrap an `Arc<dyn ErasedTarget>` plus the capability's encode/decode glue: encode input → erased call → decode output/domain error. The typed handle is the *only* public calling surface; `ContractValue`-level calling is `#[doc(hidden)]` (bindings and tests need it; box code must not). The dispatch inversion from the build topology holds: the contract crate defines the typed dispatch trait, the implementation's generated adapter implements it, the composition erases and wires.

### D7 — Composition assembly API

`boxology-runtime` owns assembly:

```text
CompositionBuilder
  .register(box_id, erased_dispatch)        // from the impl's generated adapter
  .bind(box_id, capability_id, Binding)     // InProcess | transport-provided
  .validate()  -> ValidationReport | errors
  .start()     -> Composition (typed handle source, transport attach points)
```

Validation, before any traffic: every declared import of every registered box has exactly one binding (missing and duplicate are errors); every binding's capability exists in the target's contract; every binding's declared exposure does not exceed the capability's declared maximum, over the fixed lattice `code-only < internal < external`; binding conformance hooks let a transport reject a capability it cannot represent (the S3 entry point). Reports are structured (machine-readable), because `boxology check` and the installer's generated composition both consume this validation. Transports plug in through a `TransportBinding` trait owned here — S3 implements it; S1 ships only `InProcess`.

### D8 — Async posture

Handles and dispatch are `async` everywhere, including in-process. The core crates are executor-agnostic — they spawn nothing and depend on no runtime — with one pragmatic exception: `tokio_util` for `CancellationToken` rather than hand-rolling a synchronization primitive. Tests and S3 use tokio. If executor-agnosticism ever matters enough to remove that dependency, it is one wrapped type. Flagged as challengeable rather than agonized over.

### D9 — In-process binding semantics

Direct dispatch through the erased layer with exactly these behaviors: an already-expired deadline yields `Deadline` without invoking; cancellation is checked before invoke and offered to the implementation via context, never enforced mid-flight; no timeout is imposed around the call (deadline *enforcement* is a transport concern — in-process trust means the callee is expected to honor budget, and wrapping every local call in a timer buys overhead, not honesty). Panics in the target convert to `Internal` at the dispatch boundary (a box panic must not tear down a composition thread pool silently).

### D10 — Fixtures as the generator's golden targets

S1 ships `crates/fixtures/` containing at least: a `hello`-shaped box (one unary capability, the acceptance shape) and a `kitchen-sink` box exercising the full type subset — every scalar, `Option`/`Field` positions, nested structs, enums with and without payloads, structured errors, `Secret`, `Blob`, string-keyed maps. Each fixture has a hand-written implementation crate and a hand-written contract crate **in exactly the layout the generator must later emit**, plus integration tests driving them through assembly and handles. These are explicitly labeled as S2 golden targets; changing a fixture's shape after S2 exists is a generator-contract change and gets reviewed as one.

## Acceptance criteria

1. The presence grid passes exhaustively in both directions at the `ContractValue` layer, including `Unknown`-variant preservation and opaque-payload handling.
2. The kitchen-sink fixture round-trips every supported type through encode/decode with property tests (arbitrary values, `encode ∘ decode = id`).
3. The hello fixture runs end-to-end: builder → validation → typed handle → correct output; every `CallError` class constructible by the in-process binding is exercised, including panic-to-`Internal` and expired-deadline short-circuit.
4. Assembly validation rejects each failure class (missing, duplicate, unknown capability, exposure exceeding maximum) with structured diagnostics.
5. Handwritten `ContractType` impls outside the platform do not compile (sealing verified by a compile-fail test).
6. All of it green under S0's PR validation on both platforms.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | `boxology-contract` scaffold: `ContractValue`, construction invariants (finite floats, duplicate-key rejection) | 1 |
| T2 | Presence machinery: `Field<T>`, the decode/encode grid, `Unknown` preservation | 1–2 |
| T3 | `ContractType`/`ContractError` traits, sealing, platform impls for scalars/collections, `Secret`, `Blob` | 2 |
| T4 | `boxology-runtime`: `CallContext`, `Deadline`, `CancelToken`, `TraceContext` | 1 |
| T5 | `CallError`, erased dispatch layer, typed-handle support machinery | 2 |
| T6 | Composition builder, validation lattice and report, `TransportBinding` trait, in-process binding | 2 |
| T7 | Fixture boxes (hello, kitchen-sink) + integration and property test suites | 2 |

T1→T2→T3 sequence; T4/T5 can start after T1; T6 needs T5; T7 needs everything and lands last.

## Matters left open

- The exact `Caller` shape beyond `Anonymous`/`System` — deliberately minimal until the auth cluster (#8/#9) designs the real principal model.
- Whether `ContractValue` remains `#[doc(hidden)]`-public or graduates to a documented API — revisit when a third consumer (beyond bindings and tests) appears.
- Validation metadata (defaults, min/max) representation in the type model — S1 carries the slots; enforcement semantics land with S2's schema emission, since the schema is where declared validation lives.
- Panic policy configurability (abort vs. convert) — v0 converts; a composition-level strict mode can come later if wanted.
