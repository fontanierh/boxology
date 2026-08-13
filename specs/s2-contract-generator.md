# S2 Spec — Contract Generator

[Stream definition](../boxology-details/11-v0-streams.md#s2--contract-generator) · Status: **delivered in V0**

S2 delivers the deterministic contract generator: it parses one controlled contract block, emits the generated contract crate (types, descriptors, dispatch, handle, test support), language-neutral schema, and implementation-side adapter, then relies on the normal Rust build to prove the ordinary implementation matches. Normative inputs: [Rust Build Topology](../boxology-details/08-rust-build-topology.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md). Classification is *not* here — S2 emits schemas and revisions; S4 judges changes.

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
- The author-facing **`boxology` facade crate** re-exports exactly those two macros plus `boxology_contract::CallContext`; it does not re-export the wider kernel or runtime APIs. It is distinct from the `boxology-contract` ABI crate. Generated contract packages depend on `boxology-contract`; implementation crates use `boxology` plus the fixed dependency alias `boxology_generated_contract` for their box-specific generated package (for example `package = "hello-contract"`).

The authoring convention places the controlled declaration in
`implementation/src/contract.rs`; `implementation/src/lib.rs` contains the exact unconditional
items `mod contract;` and `pub use contract::*;`. These are ordinary declared logical Rust inputs;
the generation API still starts traversal at the explicit request crate root and does not infer
paths from this convention. The selected declaration and implementation shape is exact:

```rust
// implementation/src/contract.rs
boxology::contract! {
    #[error]
    pub enum GreetError { EmptyName, }

    #[capability(exposure = external)]
    pub async fn greet(name: String) -> Result<String, GreetError>;
}
```

```rust
// implementation/src/lib.rs
mod contract;
pub use contract::*;

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
4. The caller publishes the generated tree through per-file staged commit plus per-file prune. All changed files stage as same-directory temporary siblings before any declared path changes; each declared path is then replaced by one atomic same-directory rename, so readers see complete old or complete new bytes of that file, never torn bytes. Byte-identical files are untouched. ASCII-case-rival aliasing is refused before staging on every platform. Stale files under declared outputs patterns are pruned per file after commit. Cross-file atomicity is not claimed: a rename failing mid-commit can leave a reported mixed tree and a re-run converges it. Multi-package execution is sequential in dependency order and terminal on first error. Whole-tree transactional publication is post-V0 under [#555](https://github.com/fontanierh/boxology/issues/555).
   Staging names use exclusive creation and residue is never adopted. Parent listing, entry/type
   inspection, and prune-enumeration errors fail closed. A staging failure preserves the original
   error and attempts cleanup; unmatched `.boxology-write-*` residue may remain visible for manual
   cleanup without changing a declared path.
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

**Delivered V0 authoring subset.** Byte-stable generation of `hello`, `greeter`, and `ping`, with `ping-app` consuming the generated `ping` surface, proves scalar/unit-error emission (`hello`, `ping`), import resolution (`greeter`), external exposure, and default `none` idempotency. Documentation/deprecation and idempotency (including `inherent`) remain covered by mutation/parser/schema tests; multi-capability evolution is proven by S7's real `greet(name)` additive acceptance run. At the V0 boundary, general structs/non-error enums, containers, named payload emission, `Blob`/`Secret` end-to-end emission, and the capability-name override remained post-v0 ([V0 Streams residuals](../boxology-details/11-v0-streams.md#post-v0-residuals-recorded-at-the-corpus-decision)).

**Current narrow structured subset.** The shared parser/model, format-1 schema writer/strict reader, public generator, and contract macro facade accept ordered named-field structs (including empty structs), nonempty unit-only normal enums, and declaration-before-use local references end to end. Type expressions are exactly a scalar or earlier local base, `Option<Base>`, `Vec<Base>`, or `Option<Vec<Base>>`; generated Rust types/codecs, macro re-exports, descriptors, checker/dispatch glue, typed handles, fakes, and adapters all use that same recursive model. Broader containers, payload-bearing normal enums, forward references, and recursion remain residual work under [#102](https://github.com/fontanierh/boxology/issues/102) and [#104](https://github.com/fontanierh/boxology/issues/104).

For the current generator, one-value error payloads over emittable leaves are emittable and may ship; a `Blob` value payload remains fail-closed under `BXG0040`. Named-field payloads (including empty-named) remain fail-closed under `BXG0048`. Both codes are the v0 contract; the support they gate is a post-v0 residual owned by [#104](https://github.com/fontanierh/boxology/issues/104).

A capability has one input; multiple logical inputs require a named struct. Context is implicit. Metadata arguments admitted in v0 are unique, comma-separated, order-independent, and allow a trailing comma: `exposure = code_only | internal | external` and `idempotency = none | inherent`. Exposure defaults to `code_only`; idempotency defaults to `none`. The canonical target contract reserves `name = "[a-z][a-z0-9_]*"`, but v0 completion does not admit it: the existing partial parser/model path is not a support claim, and the override plus its per-site wire-vs-Rust proof are post-v0 ([#480](https://github.com/fontanierh/boxology/issues/480)). Every declaration/member/capability identifier is an ordinary non-raw Rust identifier; the identity rules in D4 still apply.

Here **ordinary non-raw Rust identifier** has the exact Rust 2024 lexical meaning: `(XID_Start | _) XID_Continue*`, using the Unicode `XID_Start` and `XID_Continue` properties, with `_` alone excluded (see the [Rust Reference identifier grammar](https://doc.rust-lang.org/reference/identifiers.html)). It is a non-keyword identifier, not a `r#...` raw spelling; the strict keywords `_`, `as`, `async`, `await`, `break`, `const`, `continue`, `crate`, `dyn`, `else`, `enum`, `extern`, `false`, `fn`, `for`, `if`, `impl`, `in`, `let`, `loop`, `match`, `mod`, `move`, `mut`, `pub`, `ref`, `return`, `self`, `Self`, `static`, `struct`, `super`, `trait`, `true`, `type`, `unsafe`, `use`, `where`, and `while`, and the reserved keywords `abstract`, `become`, `box`, `do`, `final`, `gen`, `macro`, `override`, `priv`, `try`, `typeof`, `unsized`, `virtual`, and `yield` are rejected. The Rust 2024 weak keywords `macro_rules`, `raw`, `safe`, and `union` remain valid identifier spellings in contexts where Rust treats them as identifiers, while `'static` is a lifetime spelling and not an identifier. Rust's explicit ZWNJ (`U+200C`) and ZWJ (`U+200D`) exclusions also apply. Before validation and identity comparison, the spelling is normalized to Unicode NFC; the canonical NFC spelling is stored and emitted, so canonically equivalent declarations are one identity and duplicate detection cannot be bypassed. This matches the NFC-normalized identifiers delivered in compiler macro tokens. `boxology_contract::canonicalize_ordinary_rust_identifier` is the single implementation for S2 producers and readers, and `is_ordinary_rust_identifier` delegates to it; ASCII character-class approximations and validation without canonicalization are not equivalent.

The grammar rejects aliases, imports, qualified paths, re-exports, references, lifetimes, arbitrary generics, associated types, `impl Trait`, `cfg`, user macros, derives, unknown/duplicate metadata arguments, and duplicate type/capability/field/variant names. It also rejects self-import, `Keyed`, authentication policy, validation/default metadata, non-unary shapes, foreign boundary types, and manual metadata implementations. `Field<T>` is legal only at a top-level capability input/output or named struct field; it is rejected in list/map elements, enum payloads, and `Secret`. Presence may wrap `Secret<T>`; `Secret<T>` may not transitively contain `Option` or `Field`; nested presence is rejected.

The required capability-to-error-enum link resolves over the complete block. Local data references resolve only to an earlier declaration, so self, forward, and recursive references fail closed. Forward-reference resolution and recursive types remain a broader post-v0 residual ([#102](https://github.com/fontanierh/boxology/issues/102)). The narrow structured subset above is generated end to end; it is not a claim that the whole target grammar is implemented.

Those restrictions stop at the contract boundary. The implementation is ordinary Rust and may freely use aliases, imports, qualified paths, macros, helpers, and private types.

### D4 — Identity namespaces (S4's keys), defined here

- **Box id**: the manifest package id (`[a-z][a-z0-9-]*`), not derived from source.
- **Capability local name**: in v0, the Rust function name (`[a-z][a-z0-9_]*`). Changing that name changes the public identity. The explicit rename-preserving `name = "..."` override is post-v0 ([#480](https://github.com/fontanierh/boxology/issues/480)).
- **Qualified capability id**: `<box id>.<local name>` — used in schemas and manifests; wire routing uses the two segments separately (`/rpc/{box_id}/{capability_local_name}`), reconciling the previously inconsistent spellings.
- **Type identity**: the declaration's canonical NFC ordinary non-raw Rust identifier. **v0 has no type-rename override**: renaming a boundary type is remove+add and will classify as breaking.
- **Import-slot identity**: the imported package id (v0 permits at most one import per foreign package; aliases post-v0). The imported capability set, structured declarations, full boundary expressions, and expected contract revision are read strictly from the imported package's checked-in `schema.json` — a declared generation input — so `ImportDescriptor` values and typed import methods are deterministic. A consumer implementation names each provider contract dependency through `boxology_import_<package>`, mapping package hyphens to underscores; generated adapters reference provider-owned local types through that alias and never duplicate them.
- **Field identity**: the canonical NFC ordinary non-raw field identifier. **Variant identity**: the canonical NFC ordinary non-raw variant identifier. Same v0 no-override rule. The error variant name `Unknown` is separately reserved, even though it satisfies the ordinary identifier grammar: S1 D4 requires the runtime-generated typed error to use `Unknown` to wrap an unknown domain-error tag and its `OpaquePayload` during tolerant decoding. S2 must therefore reject a user-declared `Unknown` with the dedicated reserved-variant diagnostic, rather than hiding the collision behind the generic identifier rule. This reservation belongs to the shared producer/parser path; under S4 D1's one schema codec and fail-closed reader, the strict reader must not accept a format that the sole emitter cannot produce.

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
- **Structured imports use the same narrow boundary subset.** Hydration delegates format-1 validation to the shared strict schema reader, preserves declaration and member order, and accepts only scalars, earlier local named structs or unit enums, and recursive `Option`/`Vec` expressions. Unsupported, malformed, duplicate, or unknown imported shapes fail closed as `BXG0047`; foreign domain errors remain erased at this boundary.
- **The adapter path is disjoint from inputs by construction**: inputs are `implementation/src/**`; the adapter lives under `generated/`, included via a checked-in handwritten stub (`mod generated { include!("../../generated/adapter/adapter.rs"); }`) that is ordinary owned source. Generated output never becomes a generator input; every path classifies exactly once.
- **Test support is restored** (an accepted generator output the first draft dropped): a programmable contract-level fake per capability, generated into the contract crate behind a `test-support` feature, hand-modeled first in the S1 fixtures. Behavioral conformance is part of acceptance; #44's reconciliation notes the placement decision.
- **Descriptors are emitted** as `static` values matching S1's ABI — the missing artifact identified across the first reviews; assembly validation and wire decoding both consume them.

### D6 — Schema, and the fingerprint as a frozen projection

One canonical JSON document per box: sorted keys, LF, versioned `schema_format` field, containing identities, the type graph, shape, exposure and idempotency metadata, docs, and a provenance block. The **revision fingerprint is not a hash of the stored document**: it is SHA-256 over a frozen, versioned, field-by-field canonical projection of the resolved semantic model, independent of the stored schema's encoding and explicitly excluding provenance, the fingerprint itself, and `schema_format`. A stored-encoding upgrade re-encoding identical semantics therefore cannot change the revision. Docs are inside the projection; exact projected fixture bytes and the mutation corpus are pinned.

### D7 — Printing and formatting

Generated Rust is printed by pinned `prettyplease` (a locked library dependency recorded in provenance) — never the toolchain's rustfmt, which would couple generated bytes to toolchain version. Every top-level item in the generated contract crate carries the stable outer `#[rustfmt::skip]` attribute, making an incidental `cargo fmt --all` a byte-for-byte no-op while regeneration remains the sole formatting authority. The mandatory formatting gate selects hand-authored packages from live manifests; PR #571 deleted the bootstrap registry. Generated Rust is excluded from that selection rather than accepted after formatter drift. Generated files carry the `// Generated by boxology-generator <version>` header, no timestamps. The pinned toolchain also includes `rust-analyzer`; deep `cargo xtask ci` runs a quiet, fixed-flag `analysis-stats` batch probe against the controlled Hello implementation with build scripts and proc macros disabled. A successful probe proves the analyzer executable is available and can load the editor project under those flags; it does not assert zero diagnostics or format generated Rust.

### D8 — Adapter and the v0 receiver model

The accepted design promises no prescribed internal organization behind the boundary; v0 requires one direct `#[boxology::implementation]` on one non-generic inherent impl for a concrete receiver. Capability methods are non-generic async methods with no `where` clause or `impl Trait`, exactly `&self`, `boxology::CallContext`, one input, and the declared result—no extra parameter. The implementation macro structurally rejects violations before emitting any generated adapter call. The implementation may spell types through normal Rust aliases, imports, and qualified paths; generated calls then make rustc prove alias-resolved exact nominal input/output/error equality, `Send + Sync + 'static` on the receiver, and `Send` on each future. The attribute preserves the ordinary impl and bodies. Compile-fail fixtures include generic impls/methods and compatible-looking `impl Into<DeclaredInput>` signatures, so coercibility cannot replace nominal equality.

### D9 — Determinism

The generator's registered real determinism subjects and final goldens gate in dispatch-only deep validation rather than lean PR CI. Semantic entries, diagnostics, maps, and outputs are explicitly sorted. There is one generation-consistency digest: domain-separated SHA-256 over the shared parser's ordered normalized model—declaration kinds/names/order, semantic docs/deprecation, fields/variants/types, and capability signatures/metadata. Whitespace, non-doc comments, spans, paths, implementation/private code, and unrelated source are excluded. Generator and `contract!` call the same digest function; its versioned canonical byte encoding is pinned by architecture-proof goldens. This digest is distinct from the public contract revision. No timestamp, absolute path, environment value, process address, filesystem iteration order, locale, or timezone may affect output or diagnostics.

### D10 — Diagnostics

Every generation failure has a stable code (`BXG####`), workspace-relative path and source span, offending construct, rule, and rule source; there is no uncoded generation path. Machine-readable JSON mirrors canonical human text; S5 passes it through. Normal-Cargo implementation mismatches are rustc diagnostics and are deliberately not promised byte-stable wording; compile-fail tests assert the failure class rather than frozen prose.

### D11 — Golden protocol against the v0 evidence corpus

Inputs are the implementation sources of `hello`, `greeter`, and `ping` containing the controlled contract block and ordinary inherent implementation; `ping-app` supplies the generated-project consumer proof through S6/S7. Expected outputs are the fixture's `generated/contract/` crate, `generated/schema.json`, and `generated/adapter/adapter.rs`, compared **byte-for-byte under the complete provenance-normalization protocol**: (a) for `schema.json`, the golden stores `"provenance": "@PROVENANCE@"`; comparison parses both documents, replaces the *entire* provenance object with the token, re-serializes canonically, and compares — exactly one provenance object is required; (b) for Rust artifacts, comparison replaces exactly the first matching generator-header line — one occurrence required; (c) the fingerprint participates and excludes provenance, so drift outside provenance cannot be hidden. Fixture shape and generator change atomically. A compile-and-run test regenerates `hello`, builds it, and passes S1's integration suite.

## Acceptance criteria

1. Byte-equal emission (with provenance normalization) for `hello`, `greeter`, and `ping` — contract crate, descriptors, test-support, schema, adapter — in final exact-main native macOS ARM64 V0 evidence. Generated output remains platform-independent by product requirement; [#525](https://github.com/fontanierh/boxology/issues/525) owns restored cross-platform proof before the first pinned external release.
2. Regenerated `hello` compiles and passes S1's integration suite unmodified; its emitted descriptors validate under S1 assembly.
3. Every generation diagnostic code has a failing-input test asserting code and span, including each fail-closed rejection on the supported v0 grammar and all currently shipped fail-closed paths (aliases/imports/qualified paths in contract tokens, Keyed, auth metadata, validation/default metadata, non-unary shape, foreign boundary type, self-import, transitive presence inside `Secret`, multiple/missing or invalid sites, `cfg` ancestry, unsupported type, derive, duplicate definitions, `BXG0040`, `BXG0048`). Exhaustive coverage beyond the current narrow structured subset—broader containers, recursion, forward references, payload-bearing data enums, and `Secret` authoring—remains post-v0; the uncoded-path prohibition is not weakened.
4. Fingerprint properties hold against the frozen projection: the fixture projection bytes match pinned goldens; every mutation in the enumerated corpus changes the fingerprint; provenance-only and stored-encoding-only changes do not.
5. Test-support fakes behaviorally conform: programmed responses round-trip through typed handles; an unprogrammed capability yields `ErasedCallError::Internal` with detail code `unprogrammed_capability` (named per review).
6. The registered determinism subject is green in final exact-main dispatch-only deep validation for V0; it is not required in lean PR CI.
7. The architecture proof starts with no generated Hello contract and runs the exact D1 source through generation and normal compilation. It proves the complete semantic model (`hello`, `hello.greet`, external exposure, implicit context, `String` input/output, `GreetError::EmptyName`), sole-public-type and dependency-alias invariants, matching and deliberately mismatching implementations, semantic-digest marker mismatch, and exact signature bounds. Alias/qualified-path implementations pass; generic impls/methods, `where`, altered/extra parameters, `impl Trait`/`impl Into<String>`, wrong nominal types, non-`Send` receiver/future, and stale digest fail. Whitespace/non-doc-comment changes preserve the digest; semantic docs or contract changes alter it. Sentinels prove generation runs no Cargo, rustc, build script, user proc macro, body, or initializer. Repetition, roots, time, locale, and timezone produce identical bytes in native-Mac V0 evidence; [#525](https://github.com/fontanierh/boxology/issues/525) owns restored cross-platform comparison before the first pinned external release. Any later change that cannot preserve the shared grammar/digest, one-type facade, pure generation, and pointed compile checks is rejected.

## Matters left open

The delivered architecture settles the direct macro shape, dependency-alias/facade topology,
semantic digest, normal-compile authority boundary, own-source purity, source closure,
uncoded-path catalog, deterministic generation, and no-project-code-execution gate. No behavior
beyond the delivered D3 subset may be inferred.

- *(f32 is settled: it is in the v0 grammar, emittable as a scalar leaf, and wire-pinned by S3 D3's Ryu `f32` golden vectors; the full-grammar `kitchen-sink` fixture that was to carry it is the [#100](https://github.com/fontanierh/boxology/issues/100) residual.)*
- Extended `kitchen-sink` corpus — [#100](https://github.com/fontanierh/boxology/issues/100).
- Broader struct/non-error-enum/container authoring, forward references, and recursion beyond [#574](https://github.com/fontanierh/boxology/issues/574)'s generated narrow subset — [#102](https://github.com/fontanierh/boxology/issues/102).
- Named payload emission and `Blob`/`Secret` generator-to-binding E2E — [#104](https://github.com/fontanierh/boxology/issues/104).
- Capability-name override with its per-emission-site wire-vs-Rust proof — [#480](https://github.com/fontanierh/boxology/issues/480).
- Truthful transitive dependency-graph hygiene / identity-value extraction currently contradicted by `boxology-contract` → `tokio` — [#358](https://github.com/fontanierh/boxology/issues/358); not a reason to weaken [#107](https://github.com/fontanierh/boxology/issues/107).
- Whole-tree transactional publication for generated trees — post-V0 under [#555](https://github.com/fontanierh/boxology/issues/555); V0 keeps the implemented per-file staged-rename guarantee in D1 stage 4. Existing writer evidence pins staging failure, repeated-write isolation, refusal-before-change, and prune-failure/live-committed-tree behavior; the CLI surface lock anchors the sole writer call. The forced mid-commit rename-failure edge is owned by #555 and is not claimed as tested cleanup.
