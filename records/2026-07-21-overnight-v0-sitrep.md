# Overnight V0 Situation Report

Record of the review held on 2026-07-21 between the maintainer and Codex,
comparing the repository with the [July 20 checkpoint](2026-07-20-v0-sitrep-and-s2-decision-gate.md).
The review covers the work that landed overnight, its effect on v0 progress,
and the delivery process's current strengths and weaknesses. This is historical
analysis. It introduces no product or process decision; accepted normative
documents and tracker decisions remain authoritative.

## Executive assessment

Overnight execution was unusually strong. S1's runtime and composition
lifecycle was materially completed, S2's deterministic source model advanced
through declaration validation, and the first schema-independent pieces of the
Hello golden fixture landed.

The result is still an early foundation rather than a usable end-to-end
product. There is no generator emission, schema or fingerprint output, HTTP
binding, compatibility classification, validation CLI, installer, generated
application, or self-hosting proof.

A cautious scope-weighted estimate at this checkpoint is approximately
**25–30% of v0**, with a wide error band. This is analysis, not a milestone
declaration or schedule promise. S4–S7 remain insufficiently specified for
precise forecasting, and completed streams have already shown substantial
estimation error.

## Checkpoint comparison

The previous checkpoint was 2026-07-20 16:26 CEST at `main`
`53d2df52fcbf3520c930f8fe1e716cd1afb4eacf`, immediately after PR
[#187](https://github.com/fontanierh/boxology/pull/187). The current checkpoint
was 2026-07-21 10:12 CEST at `main`
`8dd179728eff454172bdf6737c8f38d24f2bb9e9`, after PR
[#202](https://github.com/fontanierh/boxology/pull/202). Local `main` and
`origin/main` were synchronized, no pull request was open, and all 62
worktrees were clean. Later candidates or merges are outside this frozen
snapshot.

| Metric | July 20 checkpoint | July 21 checkpoint | Delta |
| --- | ---: | ---: | ---: |
| PR state | 83 merged, 5 closed unmerged, 0 open | 98 merged, 5 closed unmerged, 0 open | +15 merged |
| Issues | 42 closed, 57 open | 43 closed, 56 open | #99 alone closed |
| Tracked files | 77 | 88 | +11 |
| Rust lines | approximately 14,450 | 18,949 | approximately +4,499 |
| Tests including doctests | 213 | 258 | +45 |
| Worktrees | 49 | 62 | +13 |
| Local branches | 76 | 89 | +13 |
| Gone-upstream branches | 50 | 62 | +12 |

The 15 merges comprise the July 20 record PR #188 and 14 implementation PRs
#189–#202. The checkpoint-to-checkpoint repository diff was 27 files, 4,996
insertions, and 133 deletions, including the record and derived lockfile
changes. The 14 implementation PRs reported 4,732 hand-authored additions
under the repository budget rules.

## What landed overnight

- PR [#188](https://github.com/fontanierh/boxology/pull/188) preserved the July
  20 checkpoint as a repository record.
- PRs [#189](https://github.com/fontanierh/boxology/pull/189)–[#193](https://github.com/fontanierh/boxology/pull/193)
  completed S1-T6: assembled in-process semantics, transport lifecycle
  carriers, validation and ordered preparation/startup, rollback and stub
  transport evidence, and consuming drain/cancel/grace/abort shutdown. Issue
  [#99](https://github.com/fontanierh/boxology/issues/99) closed.
- The accepted human shutdown decision and explicit crate-root decision were
  recorded before dependent implementation proceeded. The latter made the
  crate root a validated, caller-supplied generator input rather than an
  inferred filesystem convention.
- PRs [#194](https://github.com/fontanierh/boxology/pull/194),
  [#195](https://github.com/fontanierh/boxology/pull/195), and
  [#197](https://github.com/fontanierh/boxology/pull/197) added crate-root
  validation, deterministic module traversal, `#[path]` rejection, unreachable
  annotated-file detection, ancestor conditional rejection, and declaration
  reachability validation.
- PRs [#196](https://github.com/fontanierh/boxology/pull/196) and
  [#198](https://github.com/fontanierh/boxology/pull/198) began S1-T7 with the
  schema-independent Hello manifest and authoring root, a hand-written
  generated-style contract substrate, the fixture harness, a dispatch trait,
  the caller-side erased target, and a typed handle. This is a hand-written S1
  golden target, not generated output.
- PRs [#199](https://github.com/fontanierh/boxology/pull/199)–[#202](https://github.com/fontanierh/boxology/pull/202)
  added contract declaration discovery, flat-name collision detection,
  attribute and derive validation, typed Value/Error declaration roles, and
  exact deprecation syntax validation through `BXG0025`. Deprecation syntax and
  ownership are validated; semantic deprecation metadata is not yet fully
  modeled.

## Current stream status

### S0 — complete

S0 remains complete. Its repository, CI, determinism, and review-budget gates
continued to support the overnight work without a stream-level change.

### S1 — runtime complete through T6; T7 underway

T1–T6 are complete. T7 is partially implemented through two Hello slices. It
still lacks outward descriptors, revision and schema material, a programmable
fake, implementation/adapter/composition evidence, kitchen-sink, greeter, and
the resolved-import proof. The S1 completion audit
[#101](https://github.com/fontanierh/boxology/issues/101) remains blocked only
by T7 [#100](https://github.com/fontanierh/boxology/issues/100).

### S2 — T1 substantial but incomplete

T1 has eleven merged slices and remains open. Remaining work includes invalid
placement, documentation and semantic deprecation lowering, PascalCase and
type grammar plus positions, field and variant lowering, capability discovery
and metadata, identities, and imported-schema assembly. Self-import and
transitive presence through `Secret` remain explicitly undecided.

T2–T6 and T8 remain unimplemented as deliverables. In particular, no generator
emission exists yet.

### S3–S7 — downstream work remains ahead

S3 has an accepted specification but no HTTP implementation. S1's completed
lifecycle now supplies part of its future substrate. S4–S7 remain stream
definitions without task specs or implementation.

At the snapshot no PR was open. Active work was represented by open tasks and
clean worktrees, not by an in-flight delivered candidate.

## Quality and delivery health

Thirty-two workflow runs began after the prior checkpoint: 16 pull-request
runs, 15 `main` pushes, and one advisory run. All 32 succeeded. Exact-main
[run 29813258179](https://github.com/fontanierh/boxology/actions/runs/29813258179)
passed all six jobs in approximately 92 seconds.

Its test inventory was 110 contract tests, 10 Hello fixture tests, 46
generator-model tests, 18 runtime unit tests, six assembled in-process
integration tests, 66 xtask tests, and two doctests: 258 total. All
implementation PRs stayed within the 400-hand-authored-line ceiling.

Tracker reconciliation remained conservative: only the completed issue #99
closed. Independent review also changed outcomes rather than serving only as
ceremony:

- #194 had displaced earlier determinism evidence;
- #195 missed raw `r#path` recognition;
- #197 missed inner conditional attributes owned by `syn`;
- #200 lacked subject-level input-order evidence; and
- #201 drifted a previously authoritative artifact.

Each candidate was repaired and freshly reviewed before merge.

## What is going well

1. **Authority gates are stopping guesses.** Crate-root and shutdown semantics
   were decided explicitly before dependent code proceeded. This preserved a
   clear authority boundary instead of turning implementation convenience into
   policy.
2. **Independent review is behaviorally useful.** Five overnight candidates
   received material evidence or correctness repairs because of review.
3. **CI and determinism evidence are consistently healthy.** Every workflow
   run in the interval passed, including the six-job exact-main run.
4. **Small PRs and tracker handoffs preserve inspectability.** High throughput
   did not weaken the review ceiling or cause partial tasks to be closed.
5. **Lifecycle completion removes a real dependency.** S1-T6 did more than add
   scaffolding: it completed runtime startup, rollback, transport, and shutdown
   behavior needed by both S1 composition proofs and future S3 work.
6. **S2's explicit-input model is becoming concrete.** Root selection, module
   traversal, reachability, and declaration roles now operate through
   deterministic declared inputs.

## What is not going well

1. **Estimates remain substantially light.** S2-T1 was estimated at two or
   three PRs and remains open after eleven slices. S1-T7's original three-PR
   estimate is already obsolete.
2. **Model diversity disappeared overnight.** Fable was unavailable through
   explicit usage exhaustion for every specification and review attempt on PRs
   #189–#202. The configured Sol fallback preserved delivery gates, but both
   Fable-backed roles used the fallback path.
3. **Review evidence remains prose-only on GitHub.** This work has no native
   GitHub review objects; its independent review evidence lives in PR bodies
   and comments.
4. **Server-side enforcement remains incomplete.** Branch protection and
   rulesets are unavailable on the current private-repository plan, leaving
   required-check and direct-push discipline dependent on the operator.
5. **Local integration debt is growing.** Worktrees increased by 13, local
   branches by 13, and gone-upstream branches by 12. All worktrees were clean,
   so this is operational clutter rather than corruption.
6. **Complexity is concentrating.** `boxology-generator-model/src/rust.rs` is
   2,122 lines, runtime `composition.rs` is 665, and runtime `lib.rs` is 976.
   This is refactoring pressure, not a demonstrated defect, but the per-PR line
   ceiling does not itself prevent file-level concentration.
7. **Dependency noise persists.** Cargo continues to warn that the `toml_edit`
   requirement's `+spec-1.1.0` metadata is ignored.
8. **Product-level proof remains far ahead.** Merge volume and foundation
   quality do not validate the many-agent thesis. The emitted, compiled,
   end-to-end generated box and every downstream product stream remain absent.

## Bottom line

The project is healthier and materially further along than at the July 20
checkpoint. Execution was excellent, review caused meaningful corrections,
S1's lifecycle completed, and S2 now has explicit deterministic source
authority.

It remains an early foundation. The largest gap is still the absence of an
emitted, compiled, end-to-end generated box and every downstream product
stream. The overnight results increase confidence in construction quality; they
do not yet establish product readiness or validate Boxology's many-agent
thesis.
