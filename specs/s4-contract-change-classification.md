# S4 Spec — Contract-Change Classification

[Stream definition](../boxology-details/11-v0-streams.md#s4--contract-change-classification) · Status: **delivered in V0** · [Completion evidence](../records/2026-08-09-v0-completion-evidence.md)

S4 delivers a pure, deterministic classifier that consumes two S2 schema documents — a base revision and a submitted revision — and reports every semantic difference under a precise compatibility taxonomy. S5's `boxology check` and `boxology generate` pass the report through; the harness applies policy to it. Normative inputs: [Contract Evolution and Deprecation](../boxology-details/04-evolution.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md), [Quality and Authority](../boxology-details/06-quality-and-authority.md), [Rust Build Topology](../boxology-details/08-rust-build-topology.md), and [S2](s2-contract-generator.md) D4/D6.

## Purpose

This is the component that makes "mechanical compatibility check" a fact rather than a promise. The accepted design is explicit that the classifier's findings cannot be suppressed or relabeled: a harness "cannot suppress the underlying classification or claim that the change remained compatible," and an authorized final tightening is still reported honestly as incompatible. S4 therefore ships classification as a policy-free library: it states what changed and how that change class is defined, and nothing in its API can soften the answer.

## Non-goals

- **No policy.** No merge decision, no evidence weighing, no migration-state awareness, no configurable downgrades. The harness and factory own all of that; the classifier's output is their input.
- **No migration machinery.** Durable migration records, consumer discovery, telemetry, and deprecation lifecycle tracking are post-v0 factory work (04-evolution's open matters).
- **No CLI.** `boxology check` / `generate` (S5) wrap the library; S4 exposes a function and a report.
- **No schema emission or format change.** S2 owns schema bytes, the frozen projection, and the revision fingerprint. S4 reads format 1 documents; it never writes one and never recomputes the fingerprint.
- **No history.** One base, one submitted. Revision chains, three-way analysis, and cross-box graph reasoning are out.
- **No source-level impact analysis.** Classification speaks for the language-neutral schema — wire shape, decoding rules, and deployment ordering. Consumer Rust that names a removed variant fails the ordinary workspace build; that discovery channel is the build, not this report. Per-binding and per-language impact facets are post-v0.

## Decisions

### D1 — One schema codec, shared between emitter and classifier

Mirroring S2's one-parser rule: there may not be two independent definitions of what a schema document contains. The schema document model lives in **`boxology-schema`** — the generator serializes its emitted `schema.json` from that model; the classifier deserializes into the same model. S2's checked-in fixture schemas and golden tests guard the bytes. Format authority remains with S2; `boxology-schema` is shared representation, not a second authority.

The read side is **strict**: `schema_format` other than `1`, an unknown field, a malformed identity, a non-`unary` shape, or any value outside the format-1 vocabulary is a coded read error, not a tolerant skip. Schema documents are generated artifacts; an unrecognized field means version skew or drift, and classification over a partially understood document would be dishonest. **Provenance is the one opaque value**: strictness does not extend inside the provenance object (it is outside the compatibility surface, and rejecting a provenance evolution would break classification for no compatibility reason), and the round-trip proof runs under S2 D11's provenance-normalization protocol, so the checked-in goldens' `"@PROVENANCE@"` token parses. A `schema_format` bump and cross-format comparison remain post-v0.

The classifier itself is **`boxology-classifier`**: `classify(base: Option<&SchemaDocument>, submitted: Option<&SchemaDocument>) -> Result<ClassificationReport, Diagnostics>`, pure — no filesystem, environment, network, clock, or execution access, matching the S2 generator's discipline. How S5 obtains the two documents (base from the merge-base checkout, submitted from regeneration) is S5's concern.

### D2 — Pairing

- Both sides present: `box_id` must match; classifying two different boxes against each other is a coded input error, not a finding.
- `base` absent: a contract is being introduced. One finding, *contract introduced*, class `additive`.
- `submitted` absent: a contract is being removed. One finding, *contract removed*, class `incompatible`.
- Both absent: coded input error.

### D3 — Identity alignment, no rename detection

Diffing aligns elements by the identity namespaces S2 D4 defines: capabilities by qualified id (`<box>.<local name>`), types by declared identifier, fields and variants by identifier, import slots not at all (imports are implementation-side and never in the outward schema). There is no rename heuristic: v0 has no type, field, or variant rename override, so a rename is reported as remove-plus-add and classifies accordingly. V0 derives capability identity from the Rust function name, so a Rust fn rename appears as remove-plus-add. A future capability-name override may preserve wire identity across a Rust fn rename and would then produce no schema difference, but that authoring feature is post-v0 ([#480](https://github.com/fontanierh/boxology/issues/480)) and is not S4 completion evidence.

### D4 — Classes and verdict

Every finding carries exactly one class. The classes, their meaning, and their severity order:

| Class | Meaning |
| --- | --- |
| `unchanged` | No semantic, documentation, or deprecation difference. Provenance-only differences are `unchanged` with zero findings: provenance is outside the compatibility surface and outside the revision (S2 D6). |
| `documentation` | Documentation-only difference. Flows to consumers and changes the revision (docs are inside the projection) but creates no migration signal (09-capability-contract). |
| `deprecation` | Deprecation-metadata difference (`#[deprecated]` added, removed, or note changed). A migration *signal*, deliberately distinct from documentation; still compatible. |
| `additive` | Wholly new surface — a new capability, type, or output-side element — that existing conforming counterparties never transmit and can safely ignore under the accepted decoding rules. |
| `compatible_with_conditions` | An addition or widening **inside an existing capability's request or response surface** that migrated counterparties will begin to transmit or must interpret — mechanically survivable **only under a stated condition**. V0's two conditions are provider-first deployment order and unknown-variant tolerance. The finding names its condition; the classifier never promises the condition holds. |
| `incompatible` | Tightening or removal. Reported honestly even when expand-migrate-contract evidence will authorize the merge: the harness authorizes; it does not relabel (04-evolution). |

Severity is totally ordered as listed, `unchanged` lowest. The report's **verdict** is the maximum severity over all findings; over zero findings it is `unchanged` — which is therefore a verdict-only class and never attaches to a finding. The discriminator between `additive` and `compatible_with_conditions` is the definition above: additions to an *existing* capability's exchanged surface carry a condition; wholly new surface does not. All findings are always reported; the verdict summarizes, it never replaces the list.

### D5 — The change-kind table

Classification is structural, over the format-1 document. A type is classified in its **reachability roles** — input-reachable, output-reachable (error enums are output-reachable), or both, computed over the union of base and submitted type graphs. A change to a both-role element produces **one finding per applicable role**, each carrying its own role-specific class and condition; the verdict's maximum then applies across them. A field is **optional** when its type expression is top-level `Option<T>` or `Field<T>`, required otherwise.

**Vocabulary gate.** The delivered format-1 document expresses the V0 corpus plus the modeled
finding vocabulary covered by the mutation corpus. Rows the current `SchemaDocument` cannot
represent remain post-v0 residuals activated by
[#102](https://github.com/fontanierh/boxology/issues/102) and
[#104](https://github.com/fontanierh/boxology/issues/104). The generator's authoring-corpus
narrowing does **not** narrow classification for vocabulary already representable by the model;
named payload, field, payload-kind, metadata, and other modeled findings remain in the taxonomy,
mutation corpus ([#517](https://github.com/fontanierh/boxology/issues/517)), report goldens, and
determinism subject.

| Change | Class |
| --- | --- |
| Contract introduced / removed (D2) | `additive` / `incompatible` |
| Capability added | `additive` |
| Capability removed, or effective name changed (remove+add) | `incompatible` |
| Capability input parameter name changed | `incompatible` (wire-relevant under the JSON input mapping; path `<box>.<capability>/input`) |
| Capability input, output, or declared error changed to a different type expression | `incompatible` |
| `max_exposure` raised (order: `code_only` < `internal` < `external`) | `additive` |
| `max_exposure` lowered | `incompatible` |
| `idempotency` `none` → `inherent` | `additive` |
| `idempotency` `inherent` → `none` | `incompatible` |
| Struct field added, optional — input-reachable | `compatible_with_conditions` (provider-first: callers may populate it only after the provider deploys) |
| Struct field added, optional — output-reachable only | `additive` (consumers ignore unknown output fields) |
| Struct field added, required — input-reachable | `incompatible` (existing callers' payloads become rejectable) |
| Struct field added, required — output-reachable only | `additive` |
| Struct field removed — any role | `incompatible` (strict inputs reject it; consumers may rely on it) |
| Field type expression changed in any way, including `T` ↔ `Option<T>` ↔ `Field<T>` | `incompatible` (v0 admits no loosening/tightening lattice on existing elements; the sanctioned path is add-optional → migrate → tighten, and the final tightening classifies honestly) |
| Enum or error variant added — output-reachable | `compatible_with_conditions` (older consumers decode it as the unknown representation; the decoding rules "do not automatically classify every new … variant as semantically compatible") |
| Enum variant added — input-reachable | `compatible_with_conditions` (provider-first before callers send it) |
| Enum or error variant removed, or payload shape changed | `incompatible` |
| Type added | `additive` (its referencing change classifies separately) |
| Type removed | `incompatible` |
| Type kind changed (struct ↔ enum ↔ error) | `incompatible` |
| Documentation changed on any element | `documentation` |
| Deprecation metadata changed on any element | `deprecation` |
| **Any structural difference not named above** | `incompatible`, with the dedicated *unclassified change* code |

The last row is the fail-closed default and is load-bearing: a difference the table does not name is never silently benign. When a format evolution adds new schema vocabulary, the classifier fails closed until this table gains a row for it. There is deliberately **no shape-change row**: the strict reader admits only `unary`, so a shape difference cannot reach `classify` in v0; when the reader's vocabulary grows with a format evolution, shape changes fall to the fail-closed default until given a row. A declared type reachable from no capability is not emitted by S2 today; if one ever appears, changes to it likewise fall to the fail-closed default.

### D6 — Report, codes, and JSON

Every finding carries: a stable code (`BXC####`, a namespace disjoint from S2's `BXG####`), the identity path, the change kind, the class, base and submitted excerpts, and for `compatible_with_conditions` the named condition. Identity paths use one canonical grammar — `<box>` · `<box>.<capability>` · `<box>.<capability>/input|output|error|shape|exposure|idempotency` · `<box>/type/<Type>` · `<box>/type/<Type>/field/<field>` · `<box>/type/<Type>/variant/<Variant>` (variant payload fields extend with `/field/<field>`). Findings are sorted by identity path, then code; the report is byte-deterministic — no timestamps, absolute paths, environment values, or ordering dependent on map iteration.

Canonical human text is the primary rendering; a machine-readable JSON mirror carries a top-level `schema` field identifying the report format version, aligning with the `boxology check --format json` contract in 08-rust-build-topology. Stable codes cover every finding and error path. Structured role-specific findings allocate `BXC0063` for type-kind changes, `BXC0064`–`BXC0066` for output field add/remove/type-change, `BXC0067`–`BXC0068` for input/output enum-variant additions, and `BXC0069` for output enum-variant removal. Input field rows reuse `BXC0049`–`BXC0051`; input enum-variant removal reuses `BXC0035`.

Two **integrity cross-checks** use the stored revision without recomputing it: any finding present while base and submitted `revision` strings are equal, or zero findings while they differ, is a coded integrity error — the projection and the classifier disagree, and that disagreement must fail loudly rather than be absorbed. **Within `schema_format` 1, revision strings are comparable; a frozen-projection version bump requires a format bump.** Documents of different formats never reach the cross-checks (the reader rejects them first).

### D7 — Determinism

`boxology-classifier` has a registered determinism subject over fixture-pair report bytes. V0 proves repetition, roots, time, locale, and timezone on native macOS ARM64. Report bytes remain required to be platform-independent; [#525](https://github.com/fontanierh/boxology/issues/525) owns restored cross-platform proof. The dependency surface stays minimal and pinned.

## Acceptance criteria

1. Every D5 row expressible in the delivered format-1 vocabulary has a base/submitted fixture-pair test asserting finding code, identity path, and class in native macOS ARM64 V0 evidence; inexpressible rows remain the named #102/#104 residuals.
2. Every **fingerprint-changing** entry of the S2 mutation corpus maps to pinned expected findings, and none of those classifies `unchanged`; the corpus's negative controls (provenance-only, stored-encoding-only) assert `unchanged` with zero findings.
3. Documentation-only, deprecation-only, and provenance-only pairs produce verdicts `documentation`, `deprecation`, and `unchanged` (zero findings) respectively.
4. The fail-closed default is proven: a constructed difference outside the table yields `incompatible` with the *unclassified change* code, never a benign class.
5. Both integrity cross-checks fire on constructed violations.
6. The strict reader rejects, with coded errors and tests: unknown `schema_format`, unknown field, non-`unary` shape, and malformed identity. Mismatched `box_id` pairing is a D2 classify-time input error with its own coded test.
7. The determinism subject is green across V0's native-Mac contexts; cross-platform byte proof is #525 scope.
8. The public API admits no configuration, flag, or input that omits, reorders into hiding, or reclassifies a finding; policy application demonstrably lives outside the crate.
9. Every checked-in S2 fixture schema round-trips through `boxology-schema` **under S2 D11's provenance normalization**, and the extraction is byte-guarded by S2's existing goldens.

## Matters left open

- A loosening/tightening lattice on existing elements (`T` → `Option<T>` as something finer than `incompatible`) — deliberately conservative in v0; revisiting requires evidence from real migrations.
- Cross-`schema_format` comparison and the format-bump migration story — post-v0, arrives with the first format bump.
- Per-binding and per-language impact facets; aggregation across many boxes; policy schema — harness territory.
