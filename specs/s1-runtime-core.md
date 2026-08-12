# S1 Spec — Runtime Core and Composition Assembly

[Stream definition](../boxology-details/11-v0-streams.md#s1--runtime-core-and-composition-assembly) · Status: **delivered in V0**

S1 delivers the kernel crates: the contract-type model, descriptor ABI, call context, error model, handle machinery, composition assembly/lifecycle API, and in-process binding. Normative input: [Canonical Capability Contract](../boxology-details/09-capability-contract.md); this spec records the delivered baseline and does not reopen that contract.

## Purpose

Every later stream consumes S1. It owns the descriptor ABI (S2 generates values, S3 decodes against them), the presence and opacity representations, the invocation and error ABI, and the composition lifecycle that transports plug into. Its fixture surfaces match S2's emitted shape, and their checked-in generated artifacts are S2 golden targets.

## Non-goals

- No auth/realms/delegation (`Caller` placeholder pending #8/#9); no telemetry export; no providers; no generic CLI; no remote transports (S3).
- Streaming shapes: **representable in descriptors, never authorable or executable in v0.** The shape enum carries reserved non-unary variants so S3 can prove conformance rejection against a synthetic descriptor; S2 rejects authoring them; the runtime rejects executing them.
- No `Keyed` idempotency (descriptors admit `None | Inherent`; S2 rejects `Keyed` at generation).
- No validation/default enforcement (S2 rejects that contract metadata fail-closed).
- No public-API stability promise: `0.x`, consumed from source.

## Decisions

### D1 — Two kernel crates; the invocation ABI lives in `boxology-contract`

- **`boxology-contract`**: value model, `ContractType`/`ContractError`, presence and opacity types, descriptor types, `CallContext`, `CallError`, `Deadline`, `CancelToken`, `TraceContext`, the erased dispatch ABI. No I/O, no server; one async-adjacent dependency (`tokio-util` for `CancellationToken`, recorded and replaceable).
- **`boxology-runtime`**: composition builder, import resolution, exposure, validation, lifecycle, in-process binding, `TransportBinding`.

The author-facing `boxology` facade re-exports exactly the `contract` and `implementation` macros plus `boxology_contract::CallContext`; it does not re-export the wider kernel or runtime APIs and owns no competing ABI. Generated contract crates depend on `boxology-contract` only. Each implementation uses the facade and aliases its box-specific generated package to the fixed dependency name `boxology_generated_contract`.

### D2 — Value model: `ContractValue`, `SlotValue`, and presence at every position

`ContractValue` is an opaque struct (private representation, fallible constructors, read-only visitors): null, bool, i64, u64, f64/f32 (finite — non-finite rejected at construction), string, bytes, list, object (ordered, duplicate keys rejected), enum node (tag + payload slot), opaque node (D4), sensitive node (D9).

**Presence is modeled at every legal position, not only struct fields:**

- `TypeDescriptor` has wrapper nodes `Optional(inner)` and `TriState(inner)` legal wherever a type appears.
- **Position rule:** `TriState` (`Field<T>`) is legal at object-field and top-level call-slot positions only — a list or map element cannot be "missing" — enforced by descriptor construction and S2 generation. `Optional` is legal anywhere; inside lists/maps its `None` is carried as a null value guided by the descriptor wrapper.
- **Call slots use `SlotValue`** — `Missing | Null | Value(ContractValue)` — the type that crosses the erased boundary, so a top-level `Field::Missing` is representable and transportable in-process. A binding that cannot represent top-level tri-state (HTTP) rejects the *descriptor* at expose-time conformance; the ABI itself carries it.

**Encoding is fallible.** `ContractType::encode -> Result<SlotValue, EncodeError>`: ordinary Rust `f32`/`f64` can be non-finite, so infallibility was withdrawn. Mapping: caller-input encode failure → `ContractViolation` before invocation; provider-output or domain-error encode failure → `InvalidResponse`. Decode (`SlotValue → typed`) is fallible and strict as before.

### D3 — Decoding is descriptor-guided and role-aware

Unchanged from the prior revision: `DecodeRole { ProviderInput, ConsumerOutput }`; strict input (unknown fields/variants rejected), tolerant output (unknown fields ignored, unknown variants preserved as opaque). All semantic judgment at this layer; bindings own syntax only.

### D4 — Opacity has a transport-neutral representation

`OpaquePayload` stores an **`OpaqueTree`**: a `boxology-contract`-owned raw-value tree (null, bool, number-as-decimal-string, string, list, ordered multimap object preserving duplicate keys) — expressive enough for any binding to capture unparsed payload syntax without leaking a transport's own types into the kernel. Captured exactly when tolerant decoding meets an unknown variant (no payload descriptor exists, by definition). `reveal() -> &OpaqueTree` (documented sensitive); `forward()` re-emits through the encoding binding. `Debug` is redacted. Unknown *domain-error* variants cross `ErasedCallError::Domain` the same way: the typed layer decodes known tags to `E` and wraps unknown tags in the generated error's `Unknown` variant with `OpaquePayload`.

### D5 — Descriptors: outward contract split from implementation registration

Resolving the public/private leak found in review:

- **`ContractDescriptor`** (public, emitted in the generated *contract* crate): box id, capability descriptors (name, qualified id, input/output/error `TypeDescriptor` slots, shape — including reserved non-unary variants — max exposure, idempotency `None | Inherent`, deprecation flags), contract revision fingerprint. **Contains no import data.**
- **`ImplementationDescriptor`** (implementation-local, emitted in the generated adapter): reference to the `ContractDescriptor` plus `ImportDescriptor`s. Private import changes therefore never touch the outward contract artifact or its revision.
- **`ImportDescriptor`**: slot id = the imported package id (v0 permits at most one import per foreign package, so the package id *is* the slot identity; aliases are post-v0), expected contract revision, imported capability set — all sourced from the manifest's `[[imports]]` plus the imported package's checked-in schema, which is a declared generation input (S2).

### D6 — Error model

As previously revised (`CallError<E>` non-exhaustive; concrete `ErasedCallError`), with two clarifications: `ErasedCallError::Domain { error_tag, payload: SlotValue }`; and **panic ownership is decided** — catch-unwind lives at the S1 dispatch boundary (uniform for in-process and transports), producing `Internal`; a transport's `JoinError` is reserved for genuine task aborts (post-drain), not handler panics. S3 conforms to this.

### D7 — `CallContext` and explicit child derivation

Unchanged: explicit construction, absolute deadline/remaining budget, advisory cancellation, opaque trace carriage, transported-never-honored idempotency key, `child()` derivation (inherits deadline/trace/caller, derives child token, drops the key).

### D8 — Erased dispatch

```rust
pub trait ErasedTarget: Send + Sync {
    fn call<'a>(&'a self, capability: &'a CapabilityId, ctx: CallContext, input: SlotValue)
        -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>;
}
```

`SlotValue` at the boundary (D2). The ABI requires receivers `Send + Sync + 'static` and `Send` futures; S2's generated adapter carries these as explicit bounds so a violating receiver (`Rc`, non-`Send` guard across `.await`) fails at implementation-crate compile time with a pointed error, backed by an S2 compile-fail fixture.

### D9 — Sensitive values are redacted end to end

`Secret<T>` encodes to a **sensitive node**: same wire behavior when a binding encodes it, but `Debug`/`Display` of any `ContractValue`/`SlotValue` containing it redacts the subtree, `Detail` payloads in `CallError`/diagnostics never embed flagged subtrees, and visitors expose sensitivity so bindings can honor it. Leakage tests cover debug formatting, error details, and validation diagnostics. (This implements the canonical contract's redaction requirement at the semantic layer; per-binding surfaces add their own.)

### D10 — Supported-API enforcement, stated at its true strength

`ContractType`, descriptors, and targets are public; trusted Rust can construct a manual surface before S5 exists. The rule is **generated-surface policy**: contracts exist only via generation, enforced by S2 acceptance and S5's reproduction/ownership checks. The mechanical acceptance criterion lives with S5; S1 carries only a structural demonstration, labeled as such.

### D11 — Composition: registration with factories, import injection, fallible lifecycle

Resolving the injection and lifecycle gaps:

```text
CompositionBuilder
  .add_box(ImplementationDescriptor, factory)      // factory: FnOnce(Imports) -> Receiver-adapter
  .resolve_import(consumer_box, import_slot, ImportTarget)
  .expose(provider_box, capability, transport_binding, exposure_level)
  .start() -> Result<Composition, AssemblyErrors>  // consumes builder; THE validation boundary
```

- **Import injection:** typed import handles are composition-bound lazy references (internally `Arc<OnceLock<…>>`) sealed at `start()`. The builder constructs each box's generated `Imports` bundle immediately and passes it to that box's generated factory, which returns the receiver adapter — so construction order is independent of import topology and approved live-invocation cycles remain constructible. Invoking a handle before `start()` completes returns `Unavailable`.
- **`start()` is authoritative and fallible.** It consumes the builder, runs all validation (missing/duplicate import resolution, unknown capability, exposure over maximum, transport conformance), then drives transport lifecycle: each `TransportBinding` gets `prepare(descriptors) -> Result`, then `start(TransportRuntime) -> Result<TransportHandle>` where `TransportRuntime` supplies the **composition-owned task tracker** and the transport's config (defaults such as request deadline and size limits live in the transport's config object, held by the composition). Any prepare/bind/start failure fails `start()` with a structured error; a separately inspectable `validate()` report exists but cannot authorize traffic.
- **Shutdown:** `Composition::shutdown(drain_timeout)` — stop intake on every transport, await the tracker up to the timeout, then cancel tokens, grace-wait, abort; abort-`JoinError`s map to `Internal` per D6.

### D12 — In-process binding

As previously revised (inline execution; advisory cancellation; expired-deadline short-circuit with zero invocations; no mid-call timeout; continuation-after-abandonment is a transport property), plus panic ownership per D6.

### D13 — Fixtures: exact inventory, including the adapter golden and a two-box fixture

```text
crates/fixtures/<name>/
  boxology.toml                       # manifest (inputs incl. itself + imported schemas, per S2)
  implementation/
    src/…                             # contract macro + ordinary implementation methods
                                      #   + one-line include stub:
                                      #   mod generated { include!("../../generated/adapter/adapter.rs"); }
  generated/
    contract/                         # hand-written generated-style crate: types, ContractType impls,
                                      #   ContractDescriptor, dispatch trait, handle, test-support feature
    adapter/adapter.rs                # hand-written expected generated adapter (ImplementationDescriptor,
                                      #   factory, Imports bundle, erased glue) — S2's byte-equal golden
    schema.json                       # hand-written golden schema (provenance placeholder token)
```

The required S1 v0 fixtures are exactly **`hello`** and **`greeter`**. `hello` proves one generated scalar/unit-error box; `greeter` imports `hello` through a resolved import, proving injection end to end. The S6/S7 `ping` and `ping-app` projects complete the [v0 evidence corpus](../boxology-details/11-v0-streams.md#the-v0-evidence-corpus). The `kitchen-sink` full-grammar fixture is a post-v0 residual ([#100](https://github.com/fontanierh/boxology/issues/100)); S1's presence-grid and sensitivity acceptance evidence lives in the kernel's descriptor-level suites and never depended on that fixture. Fixture and golden evolution stays atomic, with provenance normalized by S2's protocol.

## Acceptance criteria

1. Presence grid exhaustively green in both roles, **including** `Optional`/`TriState` wrapper positions (top-level slots, nested `Vec<Option<T>>`) and the position-rule rejections (TriState in a list).
2. Typed round-trips with stated domains; tolerant-decode information-drop cases explicit; **fallible-encode cases**: non-finite caller input → `ContractViolation` with zero invocations; non-finite provider output and failing domain-error encode → `InvalidResponse`.
3. Opacity: unknown-variant capture into `OpaqueTree`, redacted `Debug`, `reveal`/`forward` behavior, unknown domain-error tags surfacing as generated `Unknown` variants.
4. Sensitivity: leakage tests over debug/detail/diagnostic surfaces for `Secret`-flagged subtrees.
5. Assembly: every failure class rejected with structured diagnostics **via fallible `start()`**; a bypass attempt (invoking a pre-seal handle) yields `Unavailable`; transport prepare/start failures fail `start()`.
6. The Hello composition supports simultaneous in-process and stub-transport exposure; **the greeter fixture invokes hello through a resolved import** (local target), exercising factory injection.
7. In-process semantics: expired deadline (zero invocations), caller-input violation (zero invocations), provider-output `InvalidResponse`, panic → `Internal` at the dispatch boundary, cooperative cancellation observation.
8. A synthetic reserved non-unary descriptor is constructible and rejected by the stub transport's conformance hook (S3 reuses this pattern).
9. Structural demonstration of generated-surface policy, labeled as such (mechanical criterion owned by S5).
10. Green in final exact-main native macOS ARM64 V0 validation; cross-platform re-proof is owned by [#525](https://github.com/fontanierh/boxology/issues/525).

## Matters left open

*(None load-bearing for v0.)* `Caller` shape (auth cluster); hidden calling-surface graduation; token-dependency replacement. Post-v0 residuals recorded with the corpus decision: extended `kitchen-sink` fixture ([#100](https://github.com/fontanierh/boxology/issues/100)); `Blob`/`Secret` end-to-end fixture paths and generated structured/container fixture coverage ([#100](https://github.com/fontanierh/boxology/issues/100), [#104](https://github.com/fontanierh/boxology/issues/104)).
