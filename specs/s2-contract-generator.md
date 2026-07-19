# S2 Spec — Contract Generator

[Stream definition](../boxology-details/11-v0-streams.md#s2--contract-generator) · Status: **proposed**

S2 builds the deterministic pre-Cargo generator: it reads annotated implementation source and emits the generated contract crate, the language-neutral schema, and the implementation-side adapter. It is the v0 long pole. Normative inputs: [Rust Build Topology](../boxology-details/08-rust-build-topology.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md). Classification is *not* here — S2 emits schemas and revisions; S4 judges changes.

## Purpose

The generator is the mechanism behind the platform's central promise: one annotated implementation method becomes a schema, a typed handle, a dispatch surface, and (later) wire bindings — with no second hand-maintained API. Everything the thesis needs from "machine-extracted contract" is only as real as this component. Its correctness definition is unusually crisp thanks to S1: **the generator is correct when it byte-emits the fixture contract crates S1 wrote by hand.**

## Non-goals

- No classification or diffing (S4). S2's schema must be *diffable*; it does not diff.
- No OpenAPI, Protobuf, SDK, or documentation rendering outputs — the schema is designed so they can exist later; none ship in v0.
- No streaming/event/session shapes: the schema's shape field exists and is always `unary` in v0; any other shape is a "not yet supported" generation error (reserved, not invalid).
- No user-facing CLI: S2 is a library crate plus a thin internal binary for tests; `boxology generate` (S5) wraps the library.
- No incremental generation: v0 regenerates a box wholesale. Incrementality is a performance optimization with a determinism cost, deferred until measured need.

## Decisions

### D1 — Crates and invocation model

- **`boxology-generator`** — the library: parse → model → emit, pure functions over paths and strings, no global state. S5's CLI and the S0 determinism harness both call it.
- **`boxology-macros`** — the proc-macro crate providing `#[boxology::capability]` and `#[boxology::contract]` as *compile-time companions*: at Cargo build time they validate placement and signature shape and expand `#[boxology::contract]` declarations into re-exports of the generated types. They never generate contract content — Cargo resolves its package graph before macro expansion, which is exactly why generation is pre-Cargo. The macro and the generator share one parsing/model crate internally so the two views of an annotation cannot drift.

The generator runs strictly *before* Cargo and needs no `cargo metadata`: its inputs are the manifest's declared generation inputs, resolved as files.

### D2 — Parsing is textual and the contract surface must be syntactically self-contained

The generator parses declared input files with `syn`, without type resolution — the pre-Cargo constraint. Consequences, made explicit rules with dedicated diagnostics rather than discovered behaviors:

- Boundary declarations must be **syntactically self-contained**: field and signature types must be spelled from the supported subset (`u32`, `String`, `Option<...>`, `Vec<...>`, `Field<...>`, `Secret<...>`, `Blob`, locally-declared contract types, declared foreign contract types via their import path). A type alias, a macro-generated field, or a `use`-renamed type in boundary position is a generation **error**, not a silent guess.
- `cfg`-varied contract surfaces are rejected: a `#[cfg]` attribute on an annotated item or its fields is an error. A contract that differs by platform would split the compatibility authority.
- Handwritten derives and attributes on `#[boxology::contract]` items: only an allowlist survives onto the generated type (`Debug`, `Clone`, `PartialEq`); anything else is an error naming the attribute. Nothing is silently dropped — the lift-and-re-export model means the handwritten body is never compiled, so silence would be deception.
- Documentation comments are lifted verbatim onto generated items and into the schema.

### D3 — The internal model is the single source for all outputs

Parsing produces a resolved **contract model** (boxes → capabilities → type graph, with metadata, docs, and source spans). Every output — schema, contract crate, adapter — is emitted from this model, never from re-inspecting source. The model is where invariants are enforced once: identifier validity (`[a-z][a-z0-9_]*` for capability names, id syntax per the manifest spec), duplicate capability ids, name collisions after lifting, foreign-type references resolvable against declared imports.

### D4 — Schema format and the revision fingerprint

One JSON document per box: deterministic (sorted keys, LF, no floats-with-ambiguous-formatting), schema-versioned, containing identities, the full type graph, interaction shape, metadata (exposure, idempotency, validation, deprecation, sensitivity), documentation, and provenance. Two structural rules with teeth:

- **The revision fingerprint is computed over the canonicalized *semantic* content — excluding the provenance block.** Provenance records which generator version produced the artifact; semantics record what was promised. A newer, backward-compatible generator that emits identical semantics must yield an identical revision, or the lazy-regeneration model (untouched boxes keep older artifacts) would make every generator upgrade look like a contract change. Fingerprint = SHA-256 over the canonical semantic subtree; recorded inside the document and echoed into generated-crate provenance headers.
- **Documentation is semantic.** Doc strings live inside the fingerprinted subtree — docs flow into SDKs and are part of what consumers see — but S4's taxonomy will classify doc-only diffs as their own benign class. The schema layout groups docs so that classification can do this cheaply.

### D5 — Generated contract crate emission

Layout per box (exactly the S1 fixture shape):

```text
generated/contract/
  Cargo.toml            # depends on boxology-contract only (+ declared foreign contract crates)
  src/lib.rs            # types, dispatch trait, handle, all generated
generated/schema.json
```

- Types module: lifted boundary types implementing `ContractType` via generated impls.
- Dispatch: the typed dispatch trait (the inversion — contract defines the interface; the implementation's generated adapter implements it).
- Handle: the canonical typed client over the erased layer, plus the consumer `Imports` struct when imports exist.
- **Printing is via a pinned code-printing library (`prettyplease`), never rustfmt.** Formatting with the toolchain's rustfmt would couple generated bytes to the toolchain version, so a toolchain bump would rewrite every generated crate. The printer is a locked library dependency of the generator; its version is part of generator provenance. Generated files carry a `// Generated by boxology-generator <version>. Do not edit.` header with no timestamp.

The implementation-side adapter is emitted **into the implementation crate** at a fixed path (`src/generated_adapter.rs`), declared as a derived output in the manifest, wiring the annotated methods to the dispatch trait. Same determinism rules apply to it.

### D6 — I/O discipline is fail-closed

The generator may read only the manifest-declared generation inputs and write only the declared outputs. Reading any other path is an **error**, enforced by routing all file access through an access-tracking layer — not a convention. This is what makes "regenerate byte-for-byte from declared inputs" sound: if an undeclared file could influence output, the reproducibility check would be fiction. The tracking layer also produces the input digest for provenance.

### D7 — Determinism is a registered, self-tested property

`boxology-generator` registers as an S0 determinism subject on day one of its implementation — repeat runs, path variation, environment variation, Linux/macOS manifest comparison — and inherits S0's fixture-proven detection. Internal rules follow S0's catalog: ordered maps everywhere in the model (`BTreeMap`/sorted `Vec`), no timestamps, no absolute paths in any output (paths in diagnostics are workspace-relative), no reliance on directory-listing order (inputs are sorted after resolution).

### D8 — Golden-fixture correctness against S1

S2's primary test suite parses the S1 fixtures' implementation crates and asserts **byte equality** with their hand-written contract crates and schemas. The fixtures are the specification of emitted shape; a deliberate shape change is a reviewed fixture change first, generator change second. Secondary suites: model-level unit tests, error-catalog tests (D9), and a compile-and-run test that regenerates the hello fixture, builds it, and drives it through S1's assembly — proving generated code actually works, not just matches.

### D9 — Diagnostics are a contract surface

Primary consumers of generation errors are coding agents mid-task; diagnostic quality directly gates the platform's usability. Every error carries: a stable error code (`BXG####`), the workspace-relative file and span, the offending construct, why it is rejected, and where the rule comes from. The catalog is enumerated in the S2 task specs and covered by one test per code (an input reproducing it, asserting code + span). Machine-readable JSON output mirrors the human text — S5's `check --format json` passes it through rather than re-parsing prose.

### D10 — Idempotence and staleness

Running the generator twice over unchanged inputs is a no-op (byte-identical outputs; unchanged files are not rewritten, preserving mtimes for build caching). The generator itself never answers "is the checked-in artifact stale?" — that is `check`'s job (S5), done by regenerating to a temporary location and comparing. Keeping the generator a pure function of inputs→outputs, with all workflow logic outside it, is what keeps both S5 and the determinism harness simple.

## Acceptance criteria

1. Byte-equal emission for every S1 fixture (contract crate, adapter, schema), on both platforms, under S0's full determinism protocol.
2. The regenerated hello fixture compiles and passes S1's integration suite unmodified.
3. Every diagnostic code in the catalog is exercised by a failing-input test asserting code and span; no error path exists without a code.
4. The fail-closed I/O rule demonstrably fires: a test input that tricks generation into touching an undeclared file fails with the access-violation diagnostic.
5. Provenance/fingerprint separation holds: a provenance-only change (simulated newer generator, identical semantics) leaves the revision fingerprint unchanged; any semantic change changes it.
6. A `cfg`-varied contract, an aliased boundary type, and a non-allowlisted derive each produce their specific diagnostic, not a generic failure.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Shared parse/model crate: syn ingestion, contract model, invariants, spans | 2 |
| T2 | Schema emission: canonical JSON, fingerprint-over-semantics, provenance block | 2 |
| T3 | Contract-crate emission: types + `ContractType` impls, printer integration | 2 |
| T4 | Dispatch trait, handle, `Imports` emission | 1–2 |
| T5 | Implementation-side adapter emission + `boxology-macros` companions (validate/re-export) | 2 |
| T6 | Fail-closed I/O layer, input digests, idempotent writes | 1 |
| T7 | Diagnostics catalog + machine-readable output | 1–2 |
| T8 | Golden-fixture suite, compile-and-run test, S0 determinism subject registration | 1–2 |

T1 first; T2–T5 fan out from T1; T6/T7 alongside; T8 last. S1's fixtures must exist before T8; T1 can begin as soon as S1's fixture *shape* is merged.

## Matters left open

- `SOURCE_DATE_EPOCH`: proposed posture is that the generator ignores wall-clock time entirely (nothing dated is emitted), making the variable irrelevant; confirmed or revised at T2.
- The exact allowlisted derive set on lifted types — seeded as `Debug`/`Clone`/`PartialEq`; extended only with fixture-level justification.
- Whether the adapter file is `include!`-free standalone module or macro-included — decided at T5 with compile-time-cost evidence.
- Foreign-import type references in v0: the hello scope has none; the kitchen-sink fixture decides how much of the import path surface v0 must actually parse (may be reserved-but-erroring like non-unary shapes).
