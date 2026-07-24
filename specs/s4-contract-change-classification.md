# S4 Spec — Contract-Change Classification

[Stream definition](../boxology-details/11-v0-streams.md#s4--contract-change-classification) · Status: **accepted** (one independent review round; findings addressed by the amendment of 2026-07-24)

S4 builds the compatibility authority as its own deliverable: a pure, deterministic classifier that consumes two S2 schema documents — a base revision and a submitted revision — and reports every semantic difference under a precise compatibility taxonomy. S5's `boxology check` and `boxology generate` pass the report through; the harness applies policy to it. Normative inputs: [Contract Evolution and Deprecation](../boxology-details/04-evolution.md), [Canonical Capability Contract](../boxology-details/09-capability-contract.md), the classification obligations in [Quality and Authority](../boxology-details/06-quality-and-authority.md) and [Rust Build Topology](../boxology-details/08-rust-build-topology.md), and the identity namespaces and schema format owned by [S2](s2-contract-generator.md) (D4, D6). This spec resolves the taxonomy details those documents left open; it does not reopen them.

## Purpose

This is the component that makes "mechanical compatibility check" a fact rather than a promise. The accepted design is explicit that the classifier's findings cannot be suppressed or relabeled: a harness "cannot suppress the underlying classification or claim that the change remained compatible," and an authorized final tightening is still reported honestly as incompatible. S4 therefore ships classification as a policy-free library: it states what changed and how that change class is defined, and nothing in its API can soften the answer.

## Non-goals

- **No policy.** No merge decision, no evidence weighing, no migration-state awareness, no configurable downgrades. The harness and factory own all of that; the classifier's output is their input.
- **No migration machinery.** Durable migration records, consumer discovery, telemetry, and deprecation lifecycle tracking are post-v0 factory work (04-evolution's open matters).
- **No CLI.** `boxology check` / `generate` (S5) wrap the library; S4 exposes a function and a report.
- **No schema emission or format change.** S2 (#103) owns schema bytes, the frozen projection, and the revision fingerprint. S4 reads format 1 documents; it never writes one and never recomputes the fingerprint.
- **No history.** One base, one submitted. Revision chains, three-way analysis, and cross-box graph reasoning are out.
- **No source-level impact analysis.** Classification speaks for the language-neutral schema — wire shape, decoding rules, and deployment ordering. Consumer Rust that names a removed variant fails the ordinary workspace build; that discovery channel is the build, not this report. Per-binding and per-language impact facets are post-v0.

## Decisions

### D1 — One schema codec, shared between emitter and classifier

Mirroring S2's one-parser rule: there may not be two independent definitions of what a schema document contains. The schema document model is extracted into a dedicated **`boxology-schema`** crate — the generator serializes its emitted `schema.json` from that model; the classifier deserializes into the same model. The extraction changes no bytes: S2's checked-in fixture schemas and golden tests guard it, and any byte difference is out of scope for S4 permanently. Format authority remains with S2/#103; `boxology-schema` is a relocation, not a second authority.

The read side is **strict**: `schema_format` other than `1`, an unknown field, a malformed identity, a non-`unary` shape, or any value outside the format-1 vocabulary is a coded read error, not a tolerant skip. Schema documents are generated artifacts; an unrecognized field means version skew or drift, and classification over a partially understood document would be dishonest. **Provenance is the one opaque value**: strictness does not extend inside the provenance object (it is outside the compatibility surface, and rejecting a provenance evolution would break classification for no compatibility reason), and the round-trip proof runs under S2 D11's provenance-normalization protocol, so the checked-in goldens' `"@PROVENANCE@"` token parses. A `schema_format` bump and cross-format comparison are post-v0 (see matters left open).

The classifier itself is **`boxology-classifier`**: `classify(base: Option<&SchemaDocument>, submitted: Option<&SchemaDocument>) -> Result<ClassificationReport, Diagnostics>`, pure — no filesystem, environment, network, clock, or execution access, matching the S2 generator's discipline. How S5 obtains the two documents (base from the merge-base checkout, submitted from regeneration) is S5's concern.

### D2 — Pairing

- Both sides present: `box_id` must match; classifying two different boxes against each other is a coded input error, not a finding.
- `base` absent: a contract is being introduced. One finding, *contract introduced*, class `additive`.
- `submitted` absent: a contract is being removed. One finding, *contract removed*, class `incompatible`.
- Both absent: coded input error.

### D3 — Identity alignment, no rename detection

Diffing aligns elements by the identity namespaces S2 D4 defines: capabilities by qualified id (`<box>.<local name>`), types by declared identifier, fields and variants by identifier, import slots not at all (imports are implementation-side and never in the outward schema). There is no rename heuristic: v0 has no type, field, or variant rename override, so a rename is reported as remove-plus-add and classifies accordingly. The one sanctioned rename mechanism — the capability `name = "..."` override preserving the effective name across a Rust fn rename — produces no schema difference and therefore no finding, which is exactly the point of the override.

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

**Vocabulary gate.** The current format-1 document (the emitted Hello family) expresses scalar-leaf boundaries, a single error enum, unit-payload variants, and fixed metadata; the rows below covering struct fields, optionality, containers, non-error enums, payload shapes, and multi-type graphs become exercisable only as #103's format-1 field inventory expands with S2's grammar. Until then those rows are **reserved**: implemented against the model where representable, fixture-proven as the vocabulary lands, and S4-COMPLETE may not close with a reserved row unproven unless a recorded decision re-scopes it.

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

Canonical human text is the primary rendering; a machine-readable JSON mirror carries a top-level `schema` field identifying the report format version, aligning with the `boxology check --format json` contract in 08-rust-build-topology. Exact codes are task-spec work; there is no uncoded finding or error path.

Two **integrity cross-checks** use the stored revision without recomputing it: any finding present while base and submitted `revision` strings are equal, or zero findings while they differ, is a coded integrity error — the projection and the classifier disagree, and that disagreement must fail loudly rather than be absorbed. These checks assume revision comparability, which this spec pins as a constraint on #103: **within `schema_format` 1, revision strings are comparable — a frozen-projection version bump requires a format bump.** Documents of different formats never reach the cross-checks (the reader rejects them first).

### D7 — Determinism

`boxology-classifier` registers a real determinism subject with S0's harness in T1 — the report bytes over a fixture pair — and the subject's coverage grows as the table lands. Repetition, roots, time, locale, timezone, Linux, and macOS produce identical report bytes. The dependency surface stays minimal and pinned, per the S2 generator's precedent.

## Acceptance criteria

1. Every D5 row **expressible in the current format-1 vocabulary** has at least one base/submitted fixture-pair test asserting finding code, identity path, and class, green on both platforms; each reserved row gains its fixture pair as #103's vocabulary makes it expressible, and none remains unproven at S4-COMPLETE without a recorded re-scope.
2. Every **fingerprint-changing** entry of the S2 mutation corpus maps to pinned expected findings, and none of those classifies `unchanged`; the corpus's negative controls (provenance-only, stored-encoding-only) assert `unchanged` with zero findings.
3. Documentation-only, deprecation-only, and provenance-only pairs produce verdicts `documentation`, `deprecation`, and `unchanged` (zero findings) respectively.
4. The fail-closed default is proven: a constructed difference outside the table yields `incompatible` with the *unclassified change* code, never a benign class.
5. Both integrity cross-checks fire on constructed violations.
6. The strict reader rejects, with coded errors and tests: unknown `schema_format`, unknown field, non-`unary` shape, and malformed identity. Mismatched `box_id` pairing is a D2 classify-time input error with its own coded test.
7. The determinism subject is green in S0's gating lane from its first PR onward; report bytes are identical across platforms and repetitions.
8. The public API admits no configuration, flag, or input that omits, reorders into hiding, or reclassifies a finding; policy application demonstrably lives outside the crate.
9. Every checked-in S2 fixture schema round-trips through `boxology-schema` **under S2 D11's provenance normalization**, and the extraction is byte-guarded by S2's existing goldens.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | `boxology-schema` extraction (byte-guarded), strict reader with coded diagnostics, round-trip proof, determinism-subject registration | 2 |
| T2 | Diff engine: pairing, identity alignment, reachability roles, raw change-kind detection | 2 |
| T3 | Taxonomy application: the D5 table, fail-closed default, verdict, mutation-corpus classification | 2 |
| T4 | Report: codes catalog, canonical text, JSON mirror, integrity cross-checks | 1–2 |
| T5 | Golden closure across fixture pairs, cross-platform coverage, S4-COMPLETE check against this spec | 1 |

T1 precedes everything (T2–T4 consume the read model). T2–T4 then proceed as a sequential stack; T5 remains last. T2 and T3 carry the **vocabulary gate**: rows beyond the current format-1 inventory activate as #103 lands the expanded fields, and their fixture pairs arrive with that activation. Per the methodology, this spec's merge produces the task issues (S4-T1…T5, S4-COMPLETE) referencing the stream.

## Matters left open

- A loosening/tightening lattice on existing elements (`T` → `Option<T>` as something finer than `incompatible`) — deliberately conservative in v0; revisiting requires evidence from real migrations.
- Cross-`schema_format` comparison and the format-bump migration story — post-v0, arrives with the first format bump.
- Per-binding and per-language impact facets; aggregation across many boxes; policy schema — harness territory.
- Exact `BXC####` codes and the report JSON field inventory — task-spec work under T4's authority.
- The precise S5 handoff (how `check` selects base documents, caches, and renders) — S5's spec; this spec fixes only the library boundary in D1.

## Tracker notes

The stream definition in `11-v0-streams.md` gains its spec link in this PR's diff; no other normative document changes. S2's schema-format authority (#103) and completion check (#109) are unaffected: D1 relocates the document model without changing a byte, guarded by S2's goldens, and the extraction lands as S4-T1 with that guard stated in the task issue. On merge, the operator files the five task issues plus S4-COMPLETE with `stream:s4` labels and reconciles #74's tool-boxification note (the classifier joins the generator and `boxology check` as a future boxification target). No open issue's premise is changed by this spec.

**Amendment of 2026-07-24** (one independent review round): recorded the D5 vocabulary gate on #103 and its AC1/AC2 scoping; removed the unreachable shape-change row and added the input-parameter-name row; stated the `additive`/`compatible_with_conditions` discriminator and the verdict-only nature of `unchanged`; one finding per role for both-role elements; provenance made opaque to reader strictness with AC9 run under provenance normalization; pinned within-format revision comparability as a #103 constraint; moved `box_id` pairing from AC6 to D2. Issues #317 and #318 gain the vocabulary-gate note by operator edit.
