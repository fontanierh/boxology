# S5 Spec — Manifest and Validation Tooling

[Stream definition](../boxology-details/11-v0-streams.md#s5--manifest-and-validation-tooling) · Status: **accepted** (amended by maintainer decision on 2026-08-03)

S5 builds the workspace tooling: `boxology.toml` parsing and discovery, ownership and path classification, crate-role mapping, the Cargo-edge policy checker, lockfile validation, and the `boxology generate` / `boxology check` commands with the emitted GitHub Actions workflow. The manifest format, discovery walk, ownership and derived-artifact rules, edge-policy table, check baseline, exit codes, and JSON contract are normative in [Packages](../boxology-details/02-packages.md) and [Rust Build Topology](../boxology-details/08-rust-build-topology.md); this spec does not restate them. It records implementation decisions, resolves the details the normative text delegates, and scopes the v0 subset honestly. It also consumes [S2](s2-contract-generator.md) (regeneration, `GenerationRequest` orchestration owned by #107) and [S4](s4-contract-change-classification.md) (classification, the shared `boxology-schema` codec).

## Purpose

S5 turns the merge-discipline substrate into commands a repository actually runs: the same `boxology check` for developers, the lead, and generated CI, with no hidden validation layer. It is also the absorption target fixed by S0 D10 — S7 adopts manifests and gates repository CI with `boxology check`, then the first task immediately post-v0 (#342) delegates platform validation from `cargo xtask ci` and deletes the bootstrap registries. Every check S5 ships must therefore be manifest-derived from birth; S5 may not introduce a second hand-maintained registry.

## Non-goals

- **No merger.** The base-replay enforcement protocol — 02-packages' ownership steps 5–6 (regenerate from base plus the accountable diff) and the pinned minimal-closure lockfile resolver with foreign-impact reassessment — is factory merger machinery. The foundation has no factory service; v0 `check` reports, it does not replay. Deferred explicitly, not silently (see D6 and matters left open).
- **No providers.** 02-packages defers provider crate roles until the first provider enters the foundation; manifest `kind = "provider"` is a coded "not supported in v0" rejection.
- **No isolation profiles beyond recording.** L0 is the foundation default; no profile validation machinery.
- **No package-scoped or impact-selected validation.** V1 `check` always validates the complete workspace, per 08-topology.
- **No branch protection, publication, distribution, or registry.** Recorded v0 exclusions and S0 D9 stand.
- **No additional commands on the `boxology` binary.** Exactly `boxology generate [--package <id>]` and `boxology check [--base <rev>] [--format human|json]`. Other v0 binaries (S6's `boxology-init`) are their own streams' deliverables; further `boxology` subcommands are post-v0.

## Decisions

### D1 — Crate topology and the purity split

Three crates, mirroring the platform's purity discipline:

- **`boxology-manifest`** — the strict v1 manifest model and parser. Pure: bytes in, model or coded diagnostics out.
- **`boxology-workspace`** — discovery classification, ownership and path attribution, crate-role mapping, and the edge-policy checker. Pure over supplied inputs: a normalized file listing, parsed manifests, and a `cargo metadata` JSON document are *data arguments*; the library never spawns a process, reads the filesystem, or consults git.
- **`boxology-cli`** — the binary, named `boxology`. It owns every effect: walking the filesystem, invoking `git` for `--base` material, running `cargo metadata`/`fmt`/`clippy`/`test`, executing `[quality].commands`, and invoking S2's generation orchestration (#107 owns `GenerationRequest`/`GeneratedTree` and atomic writes; S5 supplies manifest-derived requests and never reimplements them).

The existing **`boxology` facade crate stays authoring-only** (macros, `CallContext` re-exports, per 08-topology); the CLI binary shares its name but not its package. Diagnostics use a stable **`BXW####`** namespace, disjoint from `BXG`/`BXC`; there is no uncoded failure path in either library.

### D2 — The v0 manifest subset, fail-closed

`schema = 1` exactly; an unknown newer value is a coded rejection, never a skip (normative). Accepted kinds are `box`, `composition`, and `platform`. Within schema 1, unknown keys reject (normative); this spec adds the delegated details:

- **Glob dialect**: `owned`, `inputs`, and `outputs` patterns use gitignore-style matching with `**`, workspace-relative, case-sensitive, no `..` or absolute segments (rejected at parse). The exact dialect corner cases are pinned in T1's task spec and frozen by fixture goldens.
- **Composition validation** is structural against checked-in artifacts: every selected box identity exists; every binding's capability exists in that box's checked-in `schema.json` (read through `boxology-schema`); transport vocabulary is `in-process` | `http`; binding exposure does not exceed the box's `max_exposure` under the total order fixed by S4 D5. Runtime binding compatibility remains S1 assembly's job at startup; the manifest layer checks what the documents can prove.
- **Identity lifecycle** (split, merge, transfer, retirement) remains #3; v0 knows only creation-by-appearance and workspace-unique ids.
- **Fixture opacity**: a platform-kind package may declare owned subtrees as **fixture data** — `fixtures = [...]` patterns (exact key and grammar under T1's authority). Fixture paths classify as that package's owned non-derived files, and discovery does **not** treat a `boxology.toml` inside them as a workspace package. This is what lets a repository (this one at stage 2) carry fixture projects — including deliberately malformed manifests — without them entering real workspace validation. The corresponding one-sentence extension to 02-packages' discovery walk lands in this amendment's diff.
- **Protected control-plane declarations**: the optional schema-1 key `protected = [...]` is platform-only: only a platform package may declare protected control-plane paths. When present, it must be non-empty and use the existing workspace-relative glob dialect. V0 strength is declaration/reporting-only; cross-list protected-vs-derived enforcement and merger behavior remain out of scope.

### D3 — Deterministic classification

Discovery follows 02-packages' walk verbatim. Every tracked file classifies **exactly once**: a non-derived path owned by one package, or one declared derived output; ambiguous, overlapping, or unowned paths are coded failures naming every candidate. All reporting is ordered by package id, then workspace-relative path — no map-iteration or filesystem ordering leaks. The workspace's `Cargo.lock` classifies as the platform package's declared global derived artifact.

### D4 — Edge policy over the metadata document

The 08-topology edge table is enforced against the `cargo metadata` document plus manifest crate roles — v0 vocabulary exactly `box-implementation`, `box-contract`, `composition`, `platform`. Coverage spans normal, build, dev, renamed, optional, feature-activated, and target-specific edges; a renamed or feature-gated dependency is the same edge. Edge reading is **declaration-based** — `packages[].dependencies`, never the host- and feature-dependent `resolve` graph — which is what makes purity over one metadata document sound; the exact `cargo metadata` invocation (locked, no platform filter) is pinned in T3's task spec. A forbidden dependency concealed without a Cargo edge (e.g. `include!` of foreign source) is outside metadata's reach and is recorded as a known v0 limit. Every Cargo package must match exactly one `[[crates]]` entry by normalized path and package name (normative); unmatched crates and role-impossible edges are coded failures.

### D5 — `generate` orchestration

Candidates are packages with declared derived outputs whose generator is the workspace contract generator; the workspace's `Cargo.lock` declaration (generator identity `cargo`) is validated by D6's freshness rule, never fed to S2. For each candidate, the CLI assembles the `GenerationRequest` from the manifest's `inputs` patterns **and its declared `[[imports]]`, each resolved by package identity to the imported package's checked-in contract schema** — input completeness remains by construction because both halves are manifest-derived, and S2's model-level validation fails closed if traversal wants a file the request does not carry. Regeneration runs into temporary output; only packages whose bytes differ from the checked-in tree are written (this *is* the "declared inputs changed" detection — byte truth, not mtime heuristics); `--package` forces one package. Writes go through S2's atomic orchestration; provenance updates on write. The command then reports the semantic classification per changed package: the previously checked-in `schema.json` is the base, the regenerated document the submitted, classified by `boxology-classifier` and attached to the report unmodified — `generate` has no mechanism to omit or soften it.

**Recorded v0 narrowing (single-generator assumption).** 08-topology's lifecycle lets historical artifacts rest on recorded compatible generator provenance without mass regeneration. V0 assumes one current generator whose releases are byte-stable for unchanged inputs — true while everything ships from this source tree — so byte-diff detection and step 2's unconditional compare are exact. The provenance-recorded compatibility skip becomes mandatory scope at the first generator release whose representation improves for unchanged source; that trigger is recorded in matters left open, not silently dropped.

### D6 — `check` composition and the `--base` posture

`check` composes the eight baseline steps of 08-topology in order, non-mutating, whole-workspace, with the normative exit codes (`0`/`1`/`2`) and `--format json` emitting one document whose top-level `schema` field identifies the diagnostic format version. The delegated details:

- **Regeneration compare** (step 2): temp regeneration, byte compare under the provenance-normalization rules; a stale or tampered artifact fails naming the exact repair command (`boxology generate --package <id>`).
- **`--base` classification** (step 3): the CLI obtains base manifests and schemas via git; contract changes are classified by S4 against the base revision and reported even when harness policy later authorizes the merge — `check` never suppresses or downgrades a finding.
- **Diff ownership reporting**: with `--base`, changed paths are classified under the **base revision's** declarations (merger step 2 semantics): exactly one accountable package, foreign-source detection, derived-output attribution. V0 reports these as findings; the enforcement replay (merger steps 5–6) stays out, per non-goals.
- **Lockfile** (step 4 subset): freshness is proven by pinned `--locked` resolution (an out-of-date lockfile is a coded failure); classification per D3. With `--base`, a **scope finding** honors S0 D6's "mechanical enforcement arrives with S5" at reporting strength: a lockfile diff while no manifest dependency declaration of the accountable package changed is a coded finding (the `cargo update`-drive-by case). The full minimal-closure attribution replay is deferred with the merger.
- **Base default**: without `--base`, local `check` resolves the base to the merge base with `main` — the fixed v0 branch name, a recorded narrowing of 08's "configured main branch" (configurability arrives with a manifest-schema extension, not a guessed key). Where no such revision exists (no repository, unborn branch), base-relative steps are skipped and the report says so explicitly. CI always passes `--base`.
- **Exit-code mapping**: repository-validation defects — discovery/ownership/role/edge violations, unowned or foreign-source paths, stale or tampered artifacts, lockfile failures, failing fmt/clippy/tests/quality commands — exit `1`. **Contract-classification findings of every class are report-only** and do not by themselves change the exit code: per 08's policy boundary the platform reports and the harness gates, and in v0 the harness is the human reading the report. This is what makes "reported even when policy authorizes the merge" operable without override machinery.
- **Formatting** (step 5): the hand-authored package selection is **derived from manifests** — owned crate paths minus declared derived outputs — never a second S5 registry; this is the data that deletes S0's bootstrap lists in immediate-post-v0 #342.
- **Quality commands** (step 8): executed sequentially in package-id order, trusted per the foundation threat boundary, output captured, any nonzero exit failing the run. The checker contains no per-project branch; the generated Hello project's conformance tests enter through its own manifest.

### D7 — The emitted workflow is S5 data

The repository-owned GitHub Actions workflow — `boxology check --base <pull-request-base>` on Linux, per 08-topology — is a golden-pinned document owned by S5 and exposed as library data. S6's initializer writes it into generated projects; S5 owns its content and its conformance to the check contract, including the checkout fetch strategy that guarantees the base revision is locally available to `--base`.

### D8 — Determinism

Both libraries are pure and their reports byte-deterministic: sorted output, workspace-relative normalized paths, no timestamps, environment values, or locale dependence. T2 registers a real determinism subject with S0's harness — the validation report over a fixture workspace — and its coverage grows as checks land. Report bytes are identical across Linux and macOS. **Captured external tool output** (cargo, clippy, tests, quality commands) is outside every determinism claim; its embedding and truncation rules live under T5's JSON authority, and AC5's human/JSON agreement is over findings, not captured text. The JSON field inventory is task-spec work under T5's authority.

## Acceptance criteria

1. A malformed-manifest corpus covers every rejection rule in 02-packages and D2 — unknown key, unknown newer schema, provider kind, duplicate identity, overlapping ownership, unowned file, absolute/`..`/symlink escape, glob violations, role mismatch, unmatched crate — each with a coded failing fixture asserting code and path.
2. The edge-policy matrix is fully covered: every table row crossed with every edge kind (normal, build, dev, renamed, optional, feature-activated, target-specific) has a fixture proving detection.
3. Conforming fixture workspaces validate green end-to-end; a byte-tampered and a stale generated artifact each fail regeneration compare naming the repair command.
4. `generate` rewrites only byte-differing packages, honors `--package`, updates provenance on write, and attaches the S4 classification unmodified; a no-op run writes nothing and reports `unchanged`.
5. All three exit codes are proven; the JSON document carries the top-level `schema` field; human and JSON renderings agree on findings.
6. With `--base`: an incompatible contract change is reported as `incompatible` on a fixture pair that merges anyway; single-accountable-package and foreign-source findings are proven both passing and failing.
7. An out-of-date lockfile fails freshness; `Cargo.lock` classifies as the platform package's derived artifact.
8. Quality commands run from manifests on two distinct fixture projects with no checker branching; a failing command fails `check`.
9. The T2 determinism subject is green in S0's gating lane from its first PR onward; report bytes are identical across platforms and repetitions.
10. The emitted workflow document is golden-pinned and executes `boxology check --base` on Linux in a fixture repository.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | `boxology-manifest`: strict v1 model, parse diagnostics, glob dialect, malformed corpus | 2 |
| T2 | `boxology-workspace`: discovery classification, ownership attribution, crate-role mapping, determinism subject | 2 |
| T3 | Edge-policy checker: full table across all edge kinds | 1–2 |
| T4 | `generate`: manifest-derived requests, byte-diff detection, provenance, S4 classification attachment | 2 |
| T5 | `check`: baseline composition, exit codes and mapping, JSON, `--base` classification/ownership/lockfile-scope reporting, base default, composition and declared-import cross-document validation, quality commands, workflow document | 2–3 |
| T6 | Golden closure: fixture-workspace suite, corpus completeness, cross-platform coverage | 1 |

T1–T3 form a stack independent of S2 and S4 and may begin at once; T1 starts immediately. T4 is blocked by S2 #107 (orchestration surface) and S4 #316/#318/#319 (schema codec, taxonomy, report); T5 consumes T1–T4 and additionally #316 for its cross-document composition validation (D2) and #319 for classification reporting; T6 remains last, followed by the S5-COMPLETE check against this spec.

## Matters left open

- The merger's base-replay enforcement (ownership steps 5–6, minimal-closure lockfile resolver, foreign-impact reassessment) — factory-side, post-v0; v0 `check` reports the same facts without replaying them, including the D6 lockfile-scope finding as the v0 strength of S0 D6's promise.
- The provenance-recorded compatible-generator skip for historical artifacts — mandatory at the first generator release with representational changes for unchanged source (the D5 single-generator narrowing's trigger).
- Provider roles, isolation-profile validation, package-scoped validation, independent build roots — deferred per the normative documents.
- Exact `BXW####` codes, the JSON field inventory, and glob-dialect corner cases — task-spec work (T1/T5 authority).
- Identity lifecycle (#3) and the new-package creation protocol (#47) — unchanged, still open.
- The `xtask ci` delegation itself — executed by immediate-post-v0 S7-T5/#342 per S0 D10, not by this stream; S7's manifest adoption and `boxology check` CI gate precede it.

## Tracker notes

The stream definition in `11-v0-streams.md` gains its spec link in this PR's diff; no other normative document changes. On merge, the operator files the six task issues plus S5-COMPLETE with `stream:s5` labels, recording the T4 dependencies on #107, #316, and #318. #74's boxification list already names `boxology check`; no edit needed. #3 and #47 are cited, not changed. The 2026-08-03 amendment below reconciles this spec to S0 D10's revised absorption schedule.

**Amendment of 2026-07-24** (one independent review round): recorded the D5 single-generator narrowing with its trigger; added the D6 lockfile-scope finding reconciling S0 D6's promise at reporting strength; pinned the no-flag base default (merge base with `main`, recorded narrowing) and the exit-code mapping (validation defects exit `1`; classification findings report-only); added the D2 fixture-opacity declaration with its 02-packages discovery extension in this diff; assigned composition/declared-import cross-document validation to T5 with its #316 dependency; added #319 to T4/T5; pinned declaration-based edge reading and recorded the `include!` residual; scoped determinism claims around captured tool output; reworded the command-surface non-goal and the T1–T3 parallel-start sentence. Operator edits on merge: #326 and #327 gain the #319 dependency; #327 gains cross-document validation scope and the #316 dependency.

**Amendment of 2026-08-03** (maintainer acceleration decision): reconciled the absorption schedule with S0 D10 and S7 D5. S7 adopts root manifests and keeps `boxology check` gating repository CI in v0; S7-T5/#342 deletes the temporary xtask registries and completes delegation as the first task immediately post-v0.
