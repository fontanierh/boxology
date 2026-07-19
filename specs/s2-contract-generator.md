# S2 Spec — Contract Generator

[Stream definition](../boxology-details/11-v0-streams.md#s2--contract-generator) · Status: **revised, awaiting re-review** (first review addressed; cross-stream contract in issue #85)

S2 builds the deterministic pre-Cargo generator: it reads annotated implementation source and emits the generated contract crate (types, descriptors, dispatch, handle, test support), the language-neutral schema, and the implementation-side adapter. It is the v0 long pole. Normative inputs: [Rust Build Topology](../boxology-details/08-rust-build-topology.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md). Classification is *not* here — S2 emits schemas and revisions; S4 judges changes.

## Purpose

The generator mechanizes the shape S1 designs by hand: its correctness definition is **byte equality with the S1 fixtures** plus a compile-and-run proof. It also owns, per the first review, the decisions no other stream can invent: the stable identity namespaces (S4's diff keys), the emitted descriptor values (S1/S3's runtime inputs), and the fail-closed rejection of everything v0 does not enforce.

## Non-goals

- No classification or diffing (S4); no OpenAPI/Protobuf/SDK/doc-rendering outputs.
- No streaming/event/session shapes: schema reserves the field; non-`unary` source → "not supported in v0" diagnostic.
- No user-facing CLI (`boxology generate` is S5's wrapper); no incremental generation.
- **Fail-closed rejections, not silent metadata** (resolving the first review's honesty findings): source declaring `Keyed` idempotency (no dedup exists), any authentication/authorization policy annotation (no enforcement exists), and any `default`/`min`/`max`/validation annotation (no executable validator exists in v0) each produce a stable, coded "not supported in v0" error. The canonical capability contract remains the target design; the v0 subset refuses to emit promises the runtime cannot keep. Exposure metadata *is* supported (assembly enforces it); idempotency admits `None | Inherent`.
- **No foreign boundary-type reuse in v0.** A boundary type referencing another box's contract type is a coded "not supported in v0" error. Ordinary capability *imports* (calling foreign capabilities) are fully supported and are a consumer-side artifact (D5). This removes the contract-to-contract dependency surface entirely in v0, which also makes the acyclic contract-edge rule trivially satisfied.

## Decisions

### D1 — Crates and the pure-request invocation model

- **`boxology-generator`** — a library with **no filesystem access at all**. Its API is `generate(GenerationRequest) -> Result<GeneratedTree, Diagnostics>` where `GenerationRequest` carries the box identity/metadata, the complete set of logical inputs as `(relative path, bytes)`, declared imports, and declared logical outputs; `GeneratedTree` is `(relative path, bytes)` pairs plus the schema and computed fingerprint. Purity is structural, not disciplinary — the crate has no `std::fs` dependency, which *is* the fail-closed input rule: an undeclared input cannot be read because inputs arrive by value. Callers own resolution and projection: S5 resolves manifests and projects outputs atomically (preserving mtimes on unchanged files); S0's harness constructs requests in temp roots; tests construct them inline. This replaces the first draft's incoherent pure-but-does-I/O posture.
- **`boxology-macros`** — compile-time companions `#[boxology::capability]` / `#[boxology::contract]`: validate placement/signature shape and expand contract declarations into re-exports. They share the parse/model crate with the generator so the two views cannot drift. They never generate contract content.

### D2 — Source discovery, self-containment, and `cfg` rules

Every file in the declared input set is authoritative — "every declared file counts," deliberately, rather than module-graph traversal (deterministic, simple, and dead-file annotations become *visible* rather than silently ignored; duplicate definitions across files get a dedicated diagnostic). Self-containment rules, each with a coded diagnostic: boundary types spelled from the supported grammar only (no aliases, no macro-generated fields, no `use`-renames in boundary position); **`cfg`/`cfg_attr` anywhere on an exported item, its fields/variants, an enclosing module declaration, or the surrounding `impl` block is an error** — ancestors included, closing the first draft's item-only gap; derives on contract declarations survive only from the allowlist (`Debug`, `Clone`, `PartialEq`), everything else errors by name; doc comments lift verbatim.

### D3 — The supported source grammar is the normative subset, referenced not re-listed

The first draft's illustrative type list was phrased as exhaustive and contradicted the kitchen sink. Corrected: the supported grammar is exactly the canonical capability contract's type subset — bool; fixed-width integers (`u8..u64`, `i8..i64`); `f32`/`f64`; `String`; contract structs and enums; structured error enums; `Option<T>`; `Vec<T>`; string-keyed maps (canonical spelling `BTreeMap<String, T>`); `Field<T>` (object-field position only — top-level use is legal in the type model and rejected by bindings that cannot represent it, per S1 D10); `Secret<T>`; `Blob`. Position restrictions and every accepted/rejected production get golden or error-test coverage. The grammar table in the T1 task spec is the single normative enumeration.

### D4 — Identity namespaces (S4's keys), defined here

- **Box id**: the manifest package id (`[a-z][a-z0-9-]*`), not derived from source.
- **Capability local name**: the Rust fn name, or explicit `name = "..."` override (`[a-z][a-z0-9_]*`); the override is the rename-preserving mechanism for capabilities.
- **Qualified capability id**: `<box id>.<local name>` — used in schemas and manifests; wire routing uses the two segments separately (`/rpc/{box_id}/{capability_local_name}`), reconciling the previously inconsistent spellings.
- **Type identity**: the lifted type's name (PascalCase). **v0 has no type-rename override**: renaming a boundary type is remove+add and will classify as breaking — recorded as an accepted v0 limitation (rename machinery arrives with the #3-adjacent lifecycle work).
- **Field identity**: field name. **Variant identity**: variant name. Same v0 no-override rule.

These namespaces are emitted into schema and descriptors identically; S4 diffs on them.

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

- **Consumer `Imports` support is implementation-local, not contract-crate content.** The first draft put a box's import aggregation and foreign-contract dependencies into its *public* contract crate, recreating exactly the contract-to-contract edges the topology forbids (A-impl imports B and B-impl imports A would have made A-contract ↔ B-contract a Cargo cycle) and leaking private dependencies to every consumer. Corrected: the provider contract crate contains only the outward contract; import wiring (typed import handles bundle) is emitted into the adapter file, which lives in the implementation crate's dependency scope where foreign *contract* dependencies are legal by the edge table.
- **The adapter path is disjoint from inputs by construction**: inputs are `implementation/src/**`; the adapter lives under `generated/`, included via a checked-in handwritten stub (`mod generated { include!("../../generated/adapter/adapter.rs"); }`) that is ordinary owned source. Output never feeds the next input digest; every path classifies exactly once. The normative manifest example gains `generated/adapter/**` in the derived outputs — a reconciliation this PR's merge notes carry to `02-packages.md`.
- **Test support is restored** (an accepted generator output the first draft dropped): a programmable contract-level fake per capability, generated into the contract crate behind a `test-support` feature, hand-modeled first in the S1 fixtures. Behavioral conformance is part of acceptance; #44's reconciliation notes the placement decision.
- **Descriptors are emitted** as `static` values matching S1's ABI — the missing artifact identified across the first reviews; assembly validation and wire decoding both consume them.

### D6 — Schema, and the fingerprint as a frozen projection

One canonical JSON document per box: sorted keys, LF, versioned `schema_format` field, containing identities, the type graph, shape, exposure and idempotency metadata, docs, and a provenance block. The **revision fingerprint is not a hash of the stored document**: it is SHA-256 over a *frozen, versioned canonical projection* of the resolved semantic model — a byte format specified field-by-field in the T2 task spec, independent of the stored schema's encoding, explicitly excluding provenance, the fingerprint itself, and `schema_format`. A stored-encoding upgrade re-encoding identical semantics therefore cannot change the revision (the first draft's subtree hash was representation-sensitive). Docs are inside the projection (they flow to consumers; S4 classifies doc-only changes as their own benign class). Acceptance pins the exact projected bytes for fixtures plus an enumerated mutation corpus.

### D7 — Printing and formatting

Generated Rust is printed by pinned `prettyplease` (a locked library dependency recorded in provenance) — never the toolchain's rustfmt, which would couple generated bytes to toolchain version. The mandatory `cargo fmt --all --check` gate is reconciled by **excluding declared generated Rust from formatting** via the S0 `rustfmt.toml` ignore list (bootstrap) and manifest-derived ignores after S7 — not by claiming a rustfmt fixed point. Generated files carry the `// Generated by boxology-generator <version>` header, no timestamps.

### D8 — Adapter and the v0 receiver model

The accepted design promises no prescribed internal organization; v0 constrains it honestly rather than silently: **all annotated capabilities of a box must be inherent `&self` methods on a single receiver type** — multi-receiver boxes and free-function capabilities are coded "not supported in v0" diagnostics. The adapter emits `pub fn into_dispatch(receiver: TheService) -> impl ErasedTarget + use<>`, the value registration consumes; construction of the receiver is the composition author's ordinary Rust. The broader-organization promise is recorded as narrowed-for-v0 in the merge notes.

### D9 — Determinism

The generator registers a minimal real subject with S0's harness **in T1** (resolving the first draft's day-one/last contradiction) and the subject's output grows as emitters land; final golden coverage stays in T8. Internal rules per S0's catalog: ordered maps, no timestamps, no absolute paths anywhere in output or diagnostics (workspace-relative only), inputs sorted after resolution. Byte-invariance under time/locale/path variation is absolute per the accepted guarantee; `SOURCE_DATE_EPOCH` is simply never consulted.

### D10 — Diagnostics

Every error: stable code (`BXG####`), workspace-relative file + span, offending construct, rule, and rule source. One failing-input test per code; no uncoded error path. Machine-readable JSON mirrors human text; S5 passes it through. Primary consumers are coding agents mid-task; diagnostics are contract surface.

### D11 — Golden protocol against S1 fixtures

Inputs: each fixture's `authoring/` tree (parse-only data in S1; compiles only once `boxology-macros` exists). Expected outputs: the fixture's hand-written `generated/contract/` crate, `generated/schema.json`, and adapter golden — compared **byte-for-byte with provenance normalization**: golden files carry a `@PROVENANCE@` placeholder token; the comparison substitutes the actual provenance line before comparing, so generator-version churn never touches goldens. Fixture shape and generator change **atomically in one task PR** (the first draft's two-PR sequence would strand required checks red). A compile-and-run test regenerates `hello`, builds it, and passes S1's integration suite — proving the emitted code works, not just matches.

## Acceptance criteria

1. Byte-equal emission (with provenance normalization) for every S1 fixture — contract crate, descriptors, test-support, schema, adapter — on both platforms under S0's full protocol.
2. Regenerated `hello` compiles and passes S1's integration suite unmodified; its emitted descriptors validate under S1 assembly.
3. Every diagnostic code has a failing-input test asserting code and span, including: each fail-closed rejection (Keyed, auth metadata, validation annotations, non-unary shape, foreign boundary type, multi-receiver, free function), ancestor-`cfg`, aliased boundary type, non-allowlisted derive, duplicate definitions across declared files.
4. Fingerprint properties hold against the frozen projection: the fixture projection bytes match pinned goldens; every mutation in the enumerated corpus changes the fingerprint; provenance-only and stored-encoding-only changes do not.
5. Test-support fakes behaviorally conform: programmed responses round-trip through typed handles; a fake refusing an unprogrammed capability yields the specified error.
6. The T1-registered determinism subject is green in S0's gating lane from its first PR onward.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Parse/model crate: syn ingestion, grammar table, identity namespaces, invariants, spans; minimal determinism subject registered | 2–3 |
| T2 | Schema emission + frozen fingerprint projection + mutation corpus | 2 |
| T3 | Contract-crate emission: types, `ContractType` impls, descriptors | 2 |
| T4 | Dispatch trait, typed handle, test-support emission | 2 |
| T5 | Adapter emission (single-receiver model, import-handle bundle) + `boxology-macros` companions | 2 |
| T6 | `GenerationRequest`/`GeneratedTree` API hardening, diagnostics catalog completion, JSON output | 1–2 |
| T7 | *(merged into T6; number retained for stability of references)* | — |
| T8 | Golden suite with provenance normalization, compile-and-run test, full determinism coverage | 2 |

T1 first; T2–T5 fan out; T6 alongside; T8 last. S1's fixture shape must be merged before T8; T1 can begin once S1's spec merges.

## Matters left open

*(None load-bearing.)*

- The derive allowlist may grow with fixture-level justification.
- Adapter include-stub ergonomics (module name, path constant) — T5 detail.
- Whether `f32` stays in the v0 grammar or is deferred with a diagnostic — decided by the kitchen-sink fixture at S1 T7; either way the grammar table is the single source.

## Tracker notes

Merge-time reconciliation: `02-packages.md` manifest example gains the adapter/test-support/descriptor outputs and disjoint globs; normative "the generator classifies" phrasings are reworded to classifier-consumes-S2-schemas; #44 notes test-binding placement; #6 notes descriptor/dispatch decisions land in S1/S2. Issue #85 items 2, 4, 5, 6, 7 are resolved here jointly with the S1/S3 revisions.
