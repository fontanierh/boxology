# S2 Spec — Contract Generator

[Stream definition](../boxology-details/11-v0-streams.md#s2--contract-generator) · Status: **accepted at merge** (two review rounds addressed; cross-stream contract in issue #85)

S2 builds the deterministic contract generator: it parses one controlled contract block, emits the generated contract crate (types, descriptors, dispatch, handle, test support), language-neutral schema, and implementation-side adapter, then relies on the normal Rust build to prove the ordinary implementation matches. It is the v0 long pole. Normative inputs: [Rust Build Topology](../boxology-details/08-rust-build-topology.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md). Classification is *not* here — S2 emits schemas and revisions; S4 judges changes.

## Purpose

The generator mechanizes the shape S1 designs by hand: its correctness definition is **byte equality with the S1 fixtures** plus a compile-and-run proof. It also owns, per the first review, the decisions no other stream can invent: the stable identity namespaces (S4's diff keys), the emitted descriptor values (S1/S3's runtime inputs), and the fail-closed rejection of everything v0 does not enforce.

## Non-goals

- No classification or diffing (S4); no OpenAPI/Protobuf/SDK/doc-rendering outputs.
- No streaming/event/session shapes: schema reserves the field; non-`unary` source → "not supported in v0" diagnostic.
- No user-facing CLI (`boxology generate` is S5's wrapper); no incremental generation.
- **Fail-closed rejections, not silent metadata** (resolving the first review's honesty findings): source declaring `Keyed` idempotency (no dedup exists), authentication/authorization policy metadata (no enforcement exists), or `default`/`min`/`max`/validation metadata (no executable validator exists in v0) produces a stable, coded "not supported in v0" error. The canonical capability contract remains the target design; the v0 subset refuses to emit promises the runtime cannot keep. Exposure metadata *is* supported (assembly enforces it); idempotency admits `None | Inherent`.
- **No foreign boundary-type reuse in v0.** A boundary type referencing another box's contract type is a coded "not supported in v0" error. Ordinary capability *imports* (calling foreign capabilities) are fully supported and are a consumer-side artifact (D5). This removes the contract-to-contract dependency surface entirely in v0, which also makes the acyclic contract-edge rule trivially satisfied.

## Decisions

### D1 — Controlled declaration plus ordinary implementation

- **`boxology-generator`** remains pure: `generate(GenerationRequest) -> Result<GeneratedTree, Diagnostics>`. The request carries box identity/metadata, complete logical source inputs, imported checked-in schemas, the crate root, declared imports, and logical outputs. The result contains generated relative-path/byte pairs. The function has no filesystem, environment, network, clock, Cargo, rustc, or code-execution access.
- **`boxology-contract-syntax`** is the one parser for the controlled contract grammar. The generator and `boxology-macros` both use it; they may not maintain separate acceptance rules.
- **`boxology-macros`** supplies the `boxology::contract!` facade and `#[boxology::implementation]` compile-time checks. It does not discover resolved types or produce checked-in artifacts.
- A future author-facing **`boxology` facade crate** re-exports those macros and public kernel/runtime authoring names, including `CallContext`. It is distinct from the existing `boxology-contract` ABI crate. Generated contract packages depend on `boxology-contract`; implementation crates use `boxology` plus the fixed dependency alias `boxology_generated_contract` for their box-specific generated package (for example `package = "hello-contract"`).

The selected v0 shape is exact:

```rust
boxology::contract! {
    #[error]
    pub enum GreetError { EmptyName, }

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

The signatures intentionally repeat: the declaration supplies a small deterministic contract language, while the method stays ordinary executable Rust. Generated glue makes rustc prove exact agreement, so drift is a compile error.

The stages are fixed:

1. Existing deterministic module traversal finds exactly one reachable direct `boxology::contract!` and one direct `#[boxology::implementation]` inherent impl.
2. The shared parser produces the complete ordered semantic model from contract tokens; implementation bodies remain opaque.
3. The pure generator emits schema, fingerprint, box-specific sibling contract crate, adapter, and semantic-digest-keyed checking glue.
4. The caller writes the complete generated tree atomically.
5. The normal Cargo build expands the source facade and implementation attribute and lets rustc check the generated assertions.

Generation runs no Cargo, rustc, build script, user procedural macro, user code, runtime initializer, or implementation body.

### D2 — Source discovery, self-containment, and `cfg` rules

Review established that ancestor-`cfg` checks require module resolution, so "every declared file counts" alone was insufficient. Deterministic traversal starts exactly at the validated request crate root, follows plain `mod x;`, rejects `#[path]`, and diagnoses recognized contract or implementation sites in unreachable files. S2 never infers the root from `src/lib.rs`, Cargo metadata, or input order.

Each box has exactly one reachable direct `boxology::contract!` invocation and one direct `#[boxology::implementation]` inherent impl. Indirect paths, aliases of the macros, user-macro-generated sites, and multiple or missing sites are rejected. `cfg` or `cfg_attr` on either site or anywhere in its resolved module ancestry is rejected. Contract tokens own their own allowlist; implementation bodies are opaque to generation.

### D3 — Controlled contract grammar

The block admits only:

- `pub struct Name { pub field: Type, ... }` with named public fields;
- `pub enum Name` and no-argument `#[error] pub enum Name`, with unit, one-value, or named-field variants;
- `#[capability(...)] pub async fn name(input: Type) -> Result<Output, ErrorType>;`, where `ErrorType` directly names an in-block `#[error]` enum;
- doc comments/direct string `#[doc = "..."]`, `#[deprecated]` or `#[deprecated(note = "...")]`, `#[error]`, and `#[capability(...)]`; and
- canonical leaves `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`, `String`, `Blob`; containers `Option<T>`, `Vec<T>`, `BTreeMap<String, T>`, `Field<T>`, `Secret<T>`; and a supported in-block type.

A capability has one input; multiple logical inputs require a named struct. Context is implicit. Metadata arguments are unique, comma-separated, order-independent, and allow a trailing comma: `name = "[a-z][a-z0-9_]*"`, `exposure = code_only | internal | external`, and `idempotency = none | inherent`. Exposure defaults to `code_only`; idempotency defaults to `none`. Every declaration/member/capability identifier is an ordinary non-raw Rust identifier; the identity rules in D4 still apply.

The grammar rejects aliases, imports, qualified paths, re-exports, references, lifetimes, arbitrary generics, associated types, `impl Trait`, `cfg`, user macros, derives, unknown/duplicate metadata arguments, and duplicate type/capability/field/variant names. It also rejects self-import, `Keyed`, authentication policy, validation/default metadata, non-unary shapes, foreign boundary types, and manual metadata implementations. `Field<T>` is legal only at a top-level capability input/output or named struct field; it is rejected in list/map elements, enum payloads, and `Secret`. Presence may wrap `Secret<T>`; `Secret<T>` may not transitively contain `Option` or `Field`; nested presence is rejected.

The required capability-to-error-enum link resolves over the complete block. Beyond it, the target subset includes local data-type references, but accepted authority does not yet settle forward-reference resolution or recursive types. The architecture proof fails closed on those references until its grammar task specifies and tests both cases. It may not claim the whole target grammar is implemented merely because Hello passes.

Those restrictions stop at the contract boundary. The implementation is ordinary Rust and may freely use aliases, imports, qualified paths, macros, helpers, and private types.

### D4 — Identity namespaces (S4's keys), defined here

- **Box id**: the manifest package id (`[a-z][a-z0-9-]*`), not derived from source.
- **Capability local name**: the Rust fn name, or explicit `name = "..."` override (`[a-z][a-z0-9_]*`); the override is the rename-preserving mechanism for capabilities.
- **Qualified capability id**: `<box id>.<local name>` — used in schemas and manifests; wire routing uses the two segments separately (`/rpc/{box_id}/{capability_local_name}`), reconciling the previously inconsistent spellings.
- **Type identity**: the declaration's exact ordinary non-raw Rust identifier. **v0 has no type-rename override**: renaming a boundary type is remove+add and will classify as breaking.
- **Import-slot identity**: the imported package id (v0 permits at most one import per foreign package; aliases post-v0). The imported capability set and expected contract revision are read from the imported package's checked-in `schema.json` — a declared generation input — so `ImportDescriptor` values are deterministic.
- **Field identity**: the exact ordinary non-raw field identifier. **Variant identity**: the exact ordinary non-raw variant identifier. Same v0 no-override rule.

These namespaces are emitted into schema and descriptors identically; S4 diffs on them.

### D5a — Metadata coverage: deprecation supported, defaults defined

Completing the fail-closed table per review: **deprecation is supported, not rejected** — `#[deprecated]` (optionally with `note`) on an exported capability, type, field, or variant lifts into schema and descriptors as classification-relevant metadata. **Omission defaults are fail-safe and explicit**: a capability without those arguments has `code_only` exposure and `none` idempotency. Everything outside the D3 allowlist errors by name with a stable code.

### D5 — Emission inventory and placement

Per box, from one resolved internal model (single source for all outputs; invariants enforced once):

```text
generated/contract/          # crate: lifted types + ContractType impls, descriptors (static
                             #   BoxDescriptor et al.), dispatch trait, typed handle,
                             #   test-support module (feature "test-support")
generated/schema.json        # canonical schema document
generated/adapter/adapter.rs # implementation-side adapter, include!-d via a handwritten
                             #   one-line stub inside the implementation crate
```

Each generated Cargo package remains box-specific and workspace-unique (for example `hello-contract`); `boxology_generated_contract` is only its fixed dependency alias inside the implementation crate. That generated crate owns the sole compiled public definition of every boundary type, generated `ContractType`/`ContractError` implementations, provider-checking glue, and the semantic-digest-keyed marker. During normal compilation, `boxology::contract!` emits only imports/facades for those types and requires that marker; it never emits an independent boundary definition. Foreign-language clients consume `schema.json` and generated bindings, not the Rust authoring macro.

- **Descriptors follow S1's outward/implementation split**: the public contract crate emits the `ContractDescriptor` only; the `ImplementationDescriptor` (with `ImportDescriptor`s) is emitted in the adapter. Private import changes never touch the outward artifact or its revision.
- **Consumer `Imports` support is implementation-local, not contract-crate content.** The first draft put a box's import aggregation and foreign-contract dependencies into its *public* contract crate, recreating exactly the contract-to-contract edges the topology forbids (A-impl imports B and B-impl imports A would have made A-contract ↔ B-contract a Cargo cycle) and leaking private dependencies to every consumer. Corrected: the provider contract crate contains only the outward contract; import wiring (typed import handles bundle) is emitted into the adapter file, which lives in the implementation crate's dependency scope where foreign *contract* dependencies are legal by the edge table.
- **The adapter path is disjoint from inputs by construction**: inputs are `implementation/src/**`; the adapter lives under `generated/`, included via a checked-in handwritten stub (`mod generated { include!("../../generated/adapter/adapter.rs"); }`) that is ordinary owned source. Generated output never becomes a generator input; every path classifies exactly once.
- **Test support is restored** (an accepted generator output the first draft dropped): a programmable contract-level fake per capability, generated into the contract crate behind a `test-support` feature, hand-modeled first in the S1 fixtures. Behavioral conformance is part of acceptance; #44's reconciliation notes the placement decision.
- **Descriptors are emitted** as `static` values matching S1's ABI — the missing artifact identified across the first reviews; assembly validation and wire decoding both consume them.

### D6 — Schema, and the fingerprint as a frozen projection

One canonical JSON document per box: sorted keys, LF, versioned `schema_format` field, containing identities, the type graph, shape, exposure and idempotency metadata, docs, and a provenance block. The **revision fingerprint is not a hash of the stored document**: it is SHA-256 over a *frozen, versioned canonical projection* of the resolved semantic model — a byte format specified field-by-field in the T2 task spec, independent of the stored schema's encoding, explicitly excluding provenance, the fingerprint itself, and `schema_format`. A stored-encoding upgrade re-encoding identical semantics therefore cannot change the revision (the first draft's subtree hash was representation-sensitive). Docs are inside the projection (they flow to consumers; S4 classifies doc-only changes as their own benign class). Acceptance pins the exact projected bytes for fixtures plus an enumerated mutation corpus.

### D7 — Printing and formatting

Generated Rust is printed by pinned `prettyplease` (a locked library dependency recorded in provenance) — never the toolchain's rustfmt, which would couple generated bytes to toolchain version. The mandatory formatting gate is `cargo fmt --check -p <pkg> …` over an explicit hand-authored package selection maintained in xtask during bootstrap and manifest-derived from S7 onward. Generated Rust is excluded from that selection, not through a rustfmt ignore. Generated files carry the `// Generated by boxology-generator <version>` header, no timestamps. The pinned toolchain also includes `rust-analyzer`; `cargo xtask ci` runs a quiet, fixed-flag `analysis-stats` batch probe against the controlled Hello implementation with build scripts and proc macros disabled. A successful probe proves the analyzer executable is available and can load the editor project under those flags; it does not assert zero diagnostics, and it does not format generated Rust.

### D8 — Adapter and the v0 receiver model

The accepted design promises no prescribed internal organization behind the boundary; v0 requires one direct `#[boxology::implementation]` on one non-generic inherent impl for a concrete receiver. Capability methods are non-generic async methods with no `where` clause or `impl Trait`, exactly `&self`, `boxology::CallContext`, one input, and the declared result—no extra parameter. The implementation macro structurally rejects violations before emitting any generated adapter call. The implementation may spell types through normal Rust aliases, imports, and qualified paths; generated calls then make rustc prove alias-resolved exact nominal input/output/error equality, `Send + Sync + 'static` on the receiver, and `Send` on each future. The attribute preserves the ordinary impl and bodies. Compile-fail fixtures include generic impls/methods and compatible-looking `impl Into<DeclaredInput>` signatures, so coercibility cannot replace nominal equality.

### D9 — Determinism

The generator registers a minimal real subject with S0's harness **in T1** and the subject's output grows as emitters land; final golden coverage stays in T8. Semantic entries, diagnostics, maps, and outputs are explicitly sorted. There is one generation-consistency digest: domain-separated SHA-256 over the shared parser's ordered normalized model—declaration kinds/names/order, semantic docs/deprecation, fields/variants/types, and capability signatures/metadata. Whitespace, non-doc comments, spans, paths, implementation/private code, and unrelated source are excluded. Generator and `contract!` call the same digest function; its versioned canonical byte encoding is pinned by architecture-proof goldens. This digest is not the public contract revision owned by #103. No timestamp, absolute path, environment value, process address, filesystem iteration order, locale, or timezone may affect output or diagnostics.

### D10 — Diagnostics

Every generation failure has a stable code (`BXG####`), workspace-relative path and source span, offending construct, rule, and rule source; there is no uncoded generation path. Machine-readable JSON mirrors canonical human text; S5 passes it through. Exact new codes remain task-spec work. Normal-Cargo implementation mismatches are rustc diagnostics and are deliberately not promised byte-stable wording; compile-fail tests assert the failure class rather than frozen prose.

### D11 — Golden protocol against S1 fixtures

Inputs are each fixture's implementation source containing the controlled contract block and ordinary inherent implementation. The architecture proof atomically consolidates the temporary S1 `authoring/` input and duplicate implementation copy into that one source. Expected outputs are the fixture's hand-written `generated/contract/` crate, `generated/schema.json`, and `generated/adapter/adapter.rs`, compared **byte-for-byte under the complete provenance-normalization protocol**: (a) for `schema.json`, the golden stores `"provenance": "@PROVENANCE@"`; comparison parses both documents, replaces the *entire* provenance object with the token, re-serializes canonically, and compares — exactly one provenance object is required; (b) for Rust artifacts, comparison replaces exactly the first matching generator-header line — one occurrence required; (c) the fingerprint participates and excludes provenance, so drift outside provenance cannot be hidden. Fixture shape and generator change atomically. A compile-and-run test regenerates `hello`, builds it, and passes S1's integration suite.

## Acceptance criteria

1. Byte-equal emission (with provenance normalization) for every S1 fixture — contract crate, descriptors, test-support, schema, adapter — on both platforms under S0's full protocol.
2. Regenerated `hello` compiles and passes S1's integration suite unmodified; its emitted descriptors validate under S1 assembly.
3. Every generation diagnostic code has a failing-input test asserting code and span, including: each fail-closed rejection (aliases/imports/qualified paths in contract tokens, Keyed, auth metadata, validation/default metadata, non-unary shape, foreign boundary type, self-import, transitive presence inside `Secret`, multiple/missing or invalid sites, `cfg` ancestry, unsupported type, derive, duplicate definitions).
4. Fingerprint properties hold against the frozen projection: the fixture projection bytes match pinned goldens; every mutation in the enumerated corpus changes the fingerprint; provenance-only and stored-encoding-only changes do not.
5. Test-support fakes behaviorally conform: programmed responses round-trip through typed handles; an unprogrammed capability yields `ErasedCallError::Internal` with detail code `unprogrammed_capability` (named per review).
6. The T1-registered determinism subject is green in S0's gating lane from its first PR onward.
7. The architecture proof starts with no generated Hello contract and runs the exact D1 source through generation and normal compilation. It proves the complete semantic model (`hello`, `hello.greet`, external exposure, implicit context, `String` input/output, `GreetError::EmptyName`), sole-public-type and dependency-alias invariants, matching and deliberately mismatching implementations, semantic-digest marker mismatch, and exact signature bounds. Alias/qualified-path implementations pass; generic impls/methods, `where`, altered/extra parameters, `impl Trait`/`impl Into<String>`, wrong nominal types, non-`Send` receiver/future, and stale digest fail. Whitespace/non-doc-comment changes preserve the digest; semantic docs or contract changes alter it. Sentinels prove generation runs no Cargo, rustc, build script, user proc macro, body, or initializer. Repetition, roots, time, locale, timezone, Linux, and macOS produce identical bytes. If one shared grammar/digest, one-type facade, pure generation, or pointed compile checking cannot be maintained without special-case source interpretation, this architecture is rejected before emitters expand.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Controlled contract syntax/model: direct-site discovery, shared parser, grammar, identities, invariants, spans, architecture proof, minimal determinism subject | active stack; see #102 |
| T2 | Schema emission + frozen fingerprint projection + mutation corpus | 2 |
| T3 | Contract-crate emission: types, `ContractType` impls, descriptors | 2 |
| T4 | Dispatch trait, typed handle, test-support emission | 2 |
| T5 | Adapter emission (single-receiver model, import-handle bundle) + facade/implementation compile-check macros | 2 |
| T6 | `GenerationRequest`/`GeneratedTree` boundary hardening, atomic write orchestration, diagnostics catalog, JSON output | 1–2 |
| T7 | *(merged into T6; number retained for stability of references)* | — |
| T8 | Golden suite with provenance normalization, compile-and-run test, full determinism coverage | 2 |

T1's architecture gate in AC7 comes before further grammar or emitter work. It must prove the complete Hello model from the controlled block, pure generation, the one-type facade, and normal-rustc implementation checking. Every generation failure uses D10; cross-platform bytes remain identical.

After that gate, T2–T5 may fan out as their inputs become complete; T6 owns outer orchestration alongside them; T8 remains last. S1's fixture shape must be merged before T8.

There is one narrow bootstrap exception. Once the proof gate passes and Hello has complete semantic metadata, T2/#103 may begin the Hello-only schema, projection, fingerprint, provenance, and revision work while T1/#102 remains open. #103 retains exclusive authority over exact fields and bytes and must specify them before implementation. This does not generally unblock T2 or any later task.

## Matters left open

The architecture decision settles the direct macro shape, canonical leaf/container spellings, self-import, transitive presence through `Secret`, manual metadata, dependency alias/facade topology, semantic digest, and normal-compile authority boundary. Forward/local-reference and recursion rules remain architecture-proof-gated and fail closed; imported-capability schema hydration and sequencing also remain. Exact new diagnostic codes remain task-spec work; #107 retains authority over the final `GenerationRequest`/`GeneratedTree` surface, and #103 owns schema bytes and fingerprint. No behavior beyond D3 may be inferred.

- Adapter include-stub ergonomics (module name, path constant) — T5 detail.
- *(f32 is settled: it is in the v0 grammar and the kitchen-sink fixture — removed from open matters per review.)*

## Tracker notes

Normative reconciliation is **in this PR's diff** (review correctly rejected "merge notes carry"): `02-packages.md`'s manifest example now declares `boxology.toml` and imported schemas as inputs and `generated/adapter/**` among outputs; `08-rust-build-topology.md` now records test-support placement (contract crate, `test-support` feature) and formats hand-authored packages by selection rather than `--all`. #44's note stands; #6's note stands. Issue #85's S2 items (import contract, declared inputs, formatting mechanism, fixture inventory, metadata coverage, module resolution, purity honesty, f32, provenance protocol, Send bounds, normative edits, named fake error) are resolved in this revision.

This architecture supersedes the old direct `#[boxology::contract]` item and `#[boxology::capability]` method authoring model. Existing merged work on deterministic Rust ingestion, module traversal/reachability, coded diagnostics and spans, documentation/deprecation decoding, identity grammars/collisions, and ordering remains input to the replacement. Marker-placement, call-frame, and marker-metadata behavior must be replaced by direct `contract!`/`implementation` site discovery and the shared controlled parser; old diagnostics are not evidence that the new grammar is implemented.

Before merge, the operator must reconcile #102 and every open tracker whose premise mentions the old markers, spelling decision, compiler probe, self-import, or transitive-`Secret` deferral. #102 remains open for implementation; no unfinished generator task closes merely because the authority decision is recorded. GitHub comments and issue-body edits are operator actions outside this documentation diff.
