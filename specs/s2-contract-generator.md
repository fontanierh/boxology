# S2 Spec — Contract Generator

[Stream definition](../boxology-details/11-v0-streams.md#s2--contract-generator) · Status: **accepted at merge** (two review rounds addressed; cross-stream contract in issue #85)

S2 builds the deterministic compiler-assisted generator: it structurally reads annotated implementation source, obtains compiler-resolved typed metadata, and emits the generated contract crate (types, descriptors, dispatch, handle, test support), the language-neutral schema, and the implementation-side adapter. It is the v0 long pole. Normative inputs: [Rust Build Topology](../boxology-details/08-rust-build-topology.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md). Classification is *not* here — S2 emits schemas and revisions; S4 judges changes.

## Purpose

The generator mechanizes the shape S1 designs by hand: its correctness definition is **byte equality with the S1 fixtures** plus a compile-and-run proof. It also owns, per the first review, the decisions no other stream can invent: the stable identity namespaces (S4's diff keys), the emitted descriptor values (S1/S3's runtime inputs), and the fail-closed rejection of everything v0 does not enforce.

## Non-goals

- No classification or diffing (S4); no OpenAPI/Protobuf/SDK/doc-rendering outputs.
- No streaming/event/session shapes: schema reserves the field; non-`unary` source → "not supported in v0" diagnostic.
- No user-facing CLI (`boxology generate` is S5's wrapper); no incremental generation.
- **Fail-closed rejections, not silent metadata** (resolving the first review's honesty findings): source declaring `Keyed` idempotency (no dedup exists), any authentication/authorization policy annotation (no enforcement exists), and any `default`/`min`/`max`/validation annotation (no executable validator exists in v0) each produce a stable, coded "not supported in v0" error. The canonical capability contract remains the target design; the v0 subset refuses to emit promises the runtime cannot keep. Exposure metadata *is* supported (assembly enforces it); idempotency admits `None | Inherent`.
- **No foreign boundary-type reuse in v0.** A boundary type referencing another box's contract type is a coded "not supported in v0" error. Ordinary capability *imports* (calling foreign capabilities) are fully supported and are a consumer-side artifact (D5). This removes the contract-to-contract dependency surface entirely in v0, which also makes the acyclic contract-edge rule trivially satisfied.

## Decisions

### D1 — Compiler-assisted generation boundary

- **Outer orchestration** owns source resolution, temporary probe construction, stable Cargo invocation, and atomic output projection. `GenerationRequest` still carries box identity/metadata, complete logical source inputs, imported checked-in schemas, the crate root, declared imports, and logical outputs; the exact extension needed to carry compiled metadata remains #107's authority.
- **`boxology-generator`** remains the pure deterministic emitter. It consumes the structural model plus canonical compiled metadata and returns `GeneratedTree` without filesystem, environment, network, clock, or Cargo access. The clippy deny-list and review enforce that implementation rule.
- **`boxology-macros`** supplies `#[boxology::capability]` / `#[boxology::contract]` companions and typed reporters/assertions for the isolated probe. It does not emit checked-in contract content.

In plain language: **Rust resolves type names and aliases; macros report the resolved contract meaning; Boxology still reads annotations and deterministically generates the schema and code.** Prove that on Hello before extending the handwritten type parser.

The stages are fixed:

1. The structural pass produces requests, module reachability, annotations, docs, metadata, stable identities, spans, and an ordered registry.
2. Outer orchestration projects synthetic probe source containing only required imports, aliases, boundary declarations, capability signatures, and typed reporters/assertions, then compiles and runs that projection on stable Rust.
3. The result is canonical normalized metadata containing resolved semantics and stable source identity, independent of host state or discovery order. Its exact ABI is deferred.
4. The pure emitter consumes the structural model and compiled metadata to emit schema, fingerprint, contract crate, and adapter; orchestration owns temporary files and Cargo.
5. The normal build consumes those outputs and exposes one final generated boundary type; probe-local types do not survive.

### D2 — Source discovery, self-containment, and `cfg` rules

Review established that ancestor-`cfg` checks require module resolution, so "every declared file counts" alone was insufficient. Corrected: the structural pass performs **deterministic in-crate module resolution** over the declared inputs — starting exactly at the validated crate root carried by `GenerationRequest`, following plain `mod x;` declarations; `#[path]` is a coded rejection; an annotated item in a file *not reachable* through that module tree is a coded error (`annotated item in unreachable file`), so dead files are visible, not silent. S2 never infers the root from `src/lib.rs`, `boxology.toml`, Cargo metadata, or input order. With the resolved chain, **`cfg`/`cfg_attr` on an exported item, its fields/variants, the surrounding `impl`, or any ancestor `mod` declaration in the resolved chain is an error**. Two contract types resolving to the same lifted name from different modules is a coded collision error (the lifted namespace is flat in v0). The structural pass owns annotations, docs, declared metadata, identities, source spans, and an explicitly sorted registry; it does not try to resolve Rust type names. Macro-generated fields or variants remain outside the structural source model and are rejected. **Attribute policy is a complete allowlist**: `doc`, `boxology::*`, allowlisted derives (`Debug`, `Clone`, `PartialEq`), and `#[deprecated]` (lifted — see D5a); any other attribute is a coded error by name.

### D3 — The supported source grammar is the normative subset, referenced not re-listed

The supported semantic subset is exactly the canonical capability contract's type subset — bool; fixed-width integers (`u8..u64`, `i8..i64`); `f32`/`f64`; `String`; contract structs and enums; structured error enums; `Option<T>`; `Vec<T>`; string-keyed maps; `Field<T>` (object-field position only — top-level use is legal in the type model and rejected by bindings that cannot represent it, per S1 D10); `Secret<T>`; `Blob`. Position restrictions and every accepted/rejected semantic production get golden or error-test coverage.

The compiler probe uses typed macro reporters and assertions, generated from the structural registry, to normalize resolved types into canonical metadata. Ordinary aliases, renamed imports, and qualified paths are accepted when Rust resolves them to the same supported semantics. `CallContext`, `Result<Output, Error>`, and container types are semantic requirements, not spelling rules. The resolved type must satisfy Boxology's metadata contract; Rust validity alone is insufficient.

The probe source replaces or excludes capability bodies and excludes receiver construction and application/user runtime initialization. It neither links nor runs the author application; reporter execution operates only on the synthetic projection. Compile-time constant evaluation remains ordinary Rust and is not described as runtime initialization. It uses stable Rust only: no `rustc_private`, nightly, rustdoc JSON, `.rmeta` parsing, build-script writes to checked-in paths, or inventory/linker discovery order. Probe metadata is sorted by the structural registry and excludes absolute paths, environment, time, address, locale, timezone, and link order. Probe-local types are temporary; final generated output retains one public contract type.

### D4 — Identity namespaces (S4's keys), defined here

- **Box id**: the manifest package id (`[a-z][a-z0-9-]*`), not derived from source.
- **Capability local name**: the Rust fn name, or explicit `name = "..."` override (`[a-z][a-z0-9_]*`); the override is the rename-preserving mechanism for capabilities.
- **Qualified capability id**: `<box id>.<local name>` — used in schemas and manifests; wire routing uses the two segments separately (`/rpc/{box_id}/{capability_local_name}`), reconciling the previously inconsistent spellings.
- **Type identity**: the lifted type's name (PascalCase). **v0 has no type-rename override**: renaming a boundary type is remove+add and will classify as breaking — recorded as an accepted v0 limitation (rename machinery arrives with the #3-adjacent lifecycle work).
- **Import-slot identity**: the imported package id (v0 permits at most one import per foreign package; aliases post-v0). The imported capability set and expected contract revision are read from the imported package's checked-in `schema.json` — a declared generation input — so `ImportDescriptor` values are deterministic.
- **Field identity**: field name. **Variant identity**: variant name. Same v0 no-override rule.

These namespaces are emitted into schema and descriptors identically; S4 diffs on them.

### D5a — Metadata coverage: deprecation supported, defaults defined

Completing the fail-closed table per review: **deprecation is supported, not rejected** — `#[deprecated]` (optionally with `note`) on an exported capability, type, field, or variant lifts into schema and descriptors as classification-relevant metadata. **Omission defaults are fail-safe and explicit**: an unannotated capability's exposure defaults to `code-only` (the narrowest); unannotated idempotency defaults to `None`. Everything outside the D2 allowlist and the supported metadata set errors by name with a stable code.

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

- **Descriptors follow S1's outward/implementation split**: the public contract crate emits the `ContractDescriptor` only; the `ImplementationDescriptor` (with `ImportDescriptor`s) is emitted in the adapter. Private import changes never touch the outward artifact or its revision.
- **Consumer `Imports` support is implementation-local, not contract-crate content.** The first draft put a box's import aggregation and foreign-contract dependencies into its *public* contract crate, recreating exactly the contract-to-contract edges the topology forbids (A-impl imports B and B-impl imports A would have made A-contract ↔ B-contract a Cargo cycle) and leaking private dependencies to every consumer. Corrected: the provider contract crate contains only the outward contract; import wiring (typed import handles bundle) is emitted into the adapter file, which lives in the implementation crate's dependency scope where foreign *contract* dependencies are legal by the edge table.
- **The adapter path is disjoint from inputs by construction**: inputs are `implementation/src/**`; the adapter lives under `generated/`, included via a checked-in handwritten stub (`mod generated { include!("../../generated/adapter/adapter.rs"); }`) that is ordinary owned source. Output never feeds the next input digest; every path classifies exactly once. The normative manifest example gains `generated/adapter/**` in the derived outputs — a reconciliation this PR's merge notes carry to `02-packages.md`.
- **Test support is restored** (an accepted generator output the first draft dropped): a programmable contract-level fake per capability, generated into the contract crate behind a `test-support` feature, hand-modeled first in the S1 fixtures. Behavioral conformance is part of acceptance; #44's reconciliation notes the placement decision.
- **Descriptors are emitted** as `static` values matching S1's ABI — the missing artifact identified across the first reviews; assembly validation and wire decoding both consume them.

### D6 — Schema, and the fingerprint as a frozen projection

One canonical JSON document per box: sorted keys, LF, versioned `schema_format` field, containing identities, the type graph, shape, exposure and idempotency metadata, docs, and a provenance block. The **revision fingerprint is not a hash of the stored document**: it is SHA-256 over a *frozen, versioned canonical projection* of the resolved semantic model — a byte format specified field-by-field in the T2 task spec, independent of the stored schema's encoding, explicitly excluding provenance, the fingerprint itself, and `schema_format`. A stored-encoding upgrade re-encoding identical semantics therefore cannot change the revision (the first draft's subtree hash was representation-sensitive). Docs are inside the projection (they flow to consumers; S4 classifies doc-only changes as their own benign class). Acceptance pins the exact projected bytes for fixtures plus an enumerated mutation corpus.

### D7 — Printing and formatting

Generated Rust is printed by pinned `prettyplease` (a locked library dependency recorded in provenance) — never the toolchain's rustfmt, which would couple generated bytes to toolchain version. The mandatory formatting gate is `cargo fmt --check -p <pkg> …` over an explicit hand-authored package selection maintained in xtask during bootstrap and manifest-derived from S7 onward. Generated Rust is excluded from that selection, not through a rustfmt ignore. Generated files carry the `// Generated by boxology-generator <version>` header, no timestamps.

### D8 — Adapter and the v0 receiver model

The accepted design promises no prescribed internal organization; v0 constrains it honestly rather than silently: **all annotated capabilities of a box must be inherent `&self` methods on a single receiver type** — multi-receiver boxes and free-function capabilities are coded "not supported in v0" diagnostics. The structural call-frame check remains an early, source-local diagnostic: an `async` method with a shared, untyped `&self` receiver, exactly two typed parameters after it, no variadic parameter, and an explicit return type. Compiler assertions provide the final evidence for parameter, return, receiver, and future types. The adapter emits the generated factory surface S1 D11 consumes (`Imports` bundle + `FnOnce(Imports) -> TheService` registration), with **explicit `Send + Sync + 'static` bounds on the receiver and `Send` bounds on method futures**. Construction of the receiver is the composition author's ordinary Rust and is never run by the probe.

### D9 — Determinism

The generator registers a minimal real subject with S0's harness **in T1** (resolving the first draft's day-one/last contradiction) and the subject's output grows as emitters land; final golden coverage stays in T8. Structural registry entries, compiled metadata, maps, and outputs are explicitly sorted. No timestamp, absolute path, environment value, process address, filesystem iteration order, or linker order may affect metadata, output, or diagnostics. Byte-invariance under repeated runs, different roots, time, locale, timezone, Linux, and macOS is an acceptance requirement.

### D10 — Diagnostics

Every structural, probe-construction, Cargo, compiler, reporter, and probe-execution failure is wrapped as a Boxology error with a stable code (`BXG####`), workspace-relative path + source span, offending construct, rule, and rule source; there is no uncoded failure path. Known compiler assertions map through the structural source map. Raw rustc prose is excluded from canonical diagnostics, machine-readable JSON, and determinism surfaces unless a specified normalization redacts unstable and source-sensitive content. The exact new codes and the intermediate metadata ABI remain T1 task-spec work. Machine-readable JSON mirrors canonical human text; S5 passes it through.

### D11 — Golden protocol against S1 fixtures

Inputs: each fixture's `authoring/` tree (source data in S1; probe-compilable only after the extraction macros and probe support land). The compiled metadata is an explicit input to the pure emitter. Expected outputs: the fixture's hand-written `generated/contract/` crate, `generated/schema.json`, and the adapter golden at `generated/adapter/adapter.rs` (the S1 D13 inventory, now aligned) — compared **byte-for-byte under the complete provenance-normalization protocol**: (a) for `schema.json`, the golden stores `"provenance": "@PROVENANCE@"`; comparison parses both documents, replaces the *entire* provenance object (generator version, printer version, input digest) with the token, re-serializes canonically, and compares — exactly one provenance object is required, and extra or missing tokens fail the comparison; (b) for Rust artifacts, the golden's first line is the token line; comparison replaces exactly the first header line matching `^// Generated by boxology-generator .*$` — one occurrence required per artifact, anything else fails; (c) the fingerprint participates in comparison and excludes provenance by construction, so drift outside provenance can never be hidden by normalization. Fixture shape and generator change **atomically in one task PR**. A compile-and-run test regenerates `hello`, builds it, and passes S1's integration suite.

## Acceptance criteria

1. Byte-equal emission (with provenance normalization) for every S1 fixture — contract crate, descriptors, test-support, schema, adapter — on both platforms under S0's full protocol.
2. Regenerated `hello` compiles and passes S1's integration suite unmodified; its emitted descriptors validate under S1 assembly.
3. Every diagnostic code has a failing-input test asserting code and span, including: each fail-closed rejection (Keyed, auth metadata, validation annotations, non-unary shape, foreign boundary type, multi-receiver, free function), ancestor-`cfg`, unsupported resolved type, non-allowlisted derive, duplicate definitions across declared files.
4. Fingerprint properties hold against the frozen projection: the fixture projection bytes match pinned goldens; every mutation in the enumerated corpus changes the fingerprint; provenance-only and stored-encoding-only changes do not.
5. Test-support fakes behaviorally conform: programmed responses round-trip through typed handles; an unprogrammed capability yields `ErasedCallError::Internal` with detail code `unprogrammed_capability` (named per review).
6. The T1-registered determinism subject is green in S0's gating lane from its first PR onward.
7. Before more type-grammar work, canonical, alias, and qualified-path Hello sources across modules produce byte-identical normalized metadata containing, in deterministic order: box identity `hello`; capability identity `hello.greet`; external exposure; semantic `CallContext`; `String` input and output; and local unit error `GreetError::EmptyName`. The proof withholds every pre-generated Hello contract artifact and still succeeds. An unsupported resolved type maps to a stable code/path/span; panic and side-effect sentinels in the original capability body and user runtime initializer prove neither runs; and the cold-tree cross-platform determinism matrix is green.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Structural parse/model + compiler probe: syn ingestion, ordered registry, typed metadata, identities, invariants, spans; minimal determinism subject | active stack; see #102 |
| T2 | Schema emission + frozen fingerprint projection + mutation corpus | 2 |
| T3 | Contract-crate emission: types, `ContractType` impls, descriptors | 2 |
| T4 | Dispatch trait, typed handle, test-support emission | 2 |
| T5 | Adapter emission (single-receiver model, import-handle bundle) + final macro re-export behavior | 2 |
| T6 | Orchestration and `GenerationRequest`/`GeneratedTree` boundary hardening, diagnostics catalog, JSON output | 1–2 |
| T7 | *(merged into T6; number retained for stability of references)* | — |
| T8 | Golden suite with provenance normalization, compile-and-run test, full determinism coverage | 2 |

T1's smallest gate comes first, before more handwritten type-grammar work: the non-vacuous Hello metadata and artifact-withholding proof in AC7 must pass from a deterministic explicit registry; every failure must use the coded D10 surface; and cold-tree stable-Rust runs must remain identical across repetition, roots, time, locale, timezone, Linux, and macOS. This proves architecture, not the final intermediate ABI or diagnostic catalog.

After that gate, T2–T5 may fan out as their inputs become complete; T6 owns outer orchestration alongside them; T8 remains last. S1's fixture shape must be merged before T8.

There is one narrow bootstrap exception. Once the proof gate passes and Hello has complete compiler-resolved semantic metadata, T2/#103 may begin the Hello-only schema, projection, fingerprint, provenance, and revision work while T1/#102 remains open. #103 retains exclusive authority over exact fields and bytes and must specify them before implementation. This does not generally unblock T2 or any later task.

## Matters left open

Remaining load-bearing decisions tracked in [#102](https://github.com/fontanierh/boxology/issues/102) include admission of manually implemented metadata traits; the full semantic subset beyond Hello; self-import policy—which #102 retains after #99 closed without selecting an assembly rule—and transitive presence through `Secret` coordinated with #112; and foreign-boundary reuse, hydration, and general imported-schema sequencing. The exact compiled-metadata ABI and new diagnostic codes are also deferred to the T1 task spec; #107 retains authority over the final `GenerationRequest`/`GeneratedTree` surface. The schema byte format and fingerprint remain #103-owned. No deferred behavior may be inferred from this architecture decision.

- The derive allowlist may grow with fixture-level justification.
- Adapter include-stub ergonomics (module name, path constant) — T5 detail.
- *(f32 is settled: it is in the v0 grammar and the kitchen-sink fixture — removed from open matters per review.)*

## Tracker notes

Normative reconciliation is **in this PR's diff** (review correctly rejected "merge notes carry"): `02-packages.md`'s manifest example now declares `boxology.toml` and imported schemas as inputs and `generated/adapter/**` among outputs; `08-rust-build-topology.md` now records test-support placement (contract crate, `test-support` feature) and formats hand-authored packages by selection rather than `--all`. #44's note stands; #6's note stands. Issue #85's S2 items (import contract, declared inputs, formatting mechanism, fixture inventory, metadata coverage, module resolution, purity honesty, f32, provenance protocol, Send bounds, normative edits, named fake error) are resolved in this revision.
