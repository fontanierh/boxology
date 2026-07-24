# Multi-Capability Generation Situation Report

Record of the situation report held on 2026-07-24 between the maintainer and the
assisting agent (Claude Opus 4.8), following a request for an updated status
report after the factory resumed. The baseline is the refreshed checkpoint of
the [S2 architecture proof and v0 reassessment](2026-07-23-s2-arch-proof.md):
`origin/main` after PR [#299](https://github.com/fontanierh/boxology/pull/299),
recorded at approximately 15:40 CEST on 2026-07-23. The refreshed checkpoint is
approximately 14:40 CEST on 2026-07-24 at `origin/main`
`e83c67b`, after PR [#311](https://github.com/fontanierh/boxology/pull/311).

This is historical and operational analysis. It introduces no product,
architecture, stream-dependency, or normative process decision. Existing
specifications, tracker decisions, review gates, and the 400-line budget remain
authoritative. The v0 estimate below is an assessment, not a normative change;
it binds nothing.

## Executive assessment

The interval is a single coherent generator thread: taking S2 emission from the
single-capability Hello slice to a **proven multi-capability box**. It retired
two of the six remaining-S2 items named in the previous record — multiple
capabilities per contract and box-generic naming — while leaving the data-type
grammar, non-default metadata, JSON diagnostics, T6 hardening, and the T8
golden/completion closure still open. The generator remains an incremental
emitter-generalization effort against a proven architecture, not open
architecture risk.

Main checks remain green throughout. The delivery loop is running; at the
checkpoint it was mid-task on the T8 multi-capability adapter-implementation
proof. No new product-stream risk was introduced, and no S3, S4, S5, S6, or S7
work landed.

## Checkpoint comparison

| Metric | 2026-07-23 checkpoint (#299) | Refreshed checkpoint (#311) | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 191 | 203 | +12 |
| Open PRs | 0 | 0 | 0 |
| Issues closed/open | 48 / 54 | 48 / 54 | unchanged |
| Tracked files | 147 | 148 | +1 |
| Rust lines | 38,474 | 39,312 | +838 |
| First-parent merges | 0 | 11 | +11 |

The aggregate diff from the baseline is 6 files, 1,336 insertions, and 367
deletions — entirely inside the generator stack. At the refreshed checkpoint,
exact-main run
[30093595656](https://github.com/fontanierh/boxology/actions/runs/30093595656)
passed all protected jobs.

## What landed: multi-capability generation, end-to-end

The controlled contract grammar and every emitter were extended from one
capability to many under a single error enum, then the single-capability guard
was lifted and a real multi-capability box proven:

- [#302](https://github.com/fontanierh/boxology/pull/302) models multiple
  capabilities under one error enum in the shared `boxology-contract-syntax`
  parser.
- [#303](https://github.com/fontanierh/boxology/pull/303) emits schema for
  multiple capabilities.
- [#305](https://github.com/fontanierh/boxology/pull/305) emits the descriptor
  for multiple capabilities.
- [#306](https://github.com/fontanierh/boxology/pull/306) emits the dispatch
  trait and typed handle for multiple capabilities.
- [#307](https://github.com/fontanierh/boxology/pull/307) emits the test-support
  fake for multiple capabilities.
- [#308](https://github.com/fontanierh/boxology/pull/308) emits adapter routing
  for multiple capabilities.
- [#309](https://github.com/fontanierh/boxology/pull/309) emits the
  implementation checker for multiple capabilities.
- [#310](https://github.com/fontanierh/boxology/pull/310) derives generated type
  names from the box id, retiring the literal `Hello`-prefixed identifiers.
- [#311](https://github.com/fontanierh/boxology/pull/311) lifts the
  single-capability guard and proves a multi-capability box end-to-end.

## Remaining-S2 burndown

Against the six remaining-S2 items named in the previous record:

| Remaining-S2 item | Status at this checkpoint |
| --- | --- |
| Multiple capabilities per contract | Landed (#302–#311) |
| Box-generic naming | Landed (#310) |
| Data structs/enums and containers (`Option`/`Vec`/`BTreeMap`/`Field`/sensitive) | Open; emission still scalar-shaped |
| Non-default exposure/idempotency emission | Open; still fixed at `external`/`none` |
| Machine-readable JSON diagnostics (D10/T6) | Open; text-only |
| T6 boundary hardening, atomic-write orchestration, import hydration | Open |
| T8 golden suite and S2-COMPLETE | In progress at the checkpoint (issue #108 branch) |

Tracker state: #105 (S2-T4) is closed COMPLETED; #102, #103, #104, #106, #107,
#108, and #109 remain open. The previous record's remark that #102–#109 were all
open is corrected here: #105 had already closed at 2026-07-23T00:11:47Z.

## Velocity and process

Daily first-parent merges were 52 on 07-22 (peak), 22 on 07-23, and 8 on 07-24
by mid-afternoon. The continued taper reflects concentration on deep,
individually compile-and-run-proven generator increments rather than a
slowdown; every merge in the interval advanced the S2 long pole directly, with
no horizontal-slice or CI churn. The delivery loop's planning role moved to
Fable-High ([#304](https://github.com/fontanierh/boxology/pull/304)), joining the
earlier move of the spec, implement, and review roles to Opus 4.8; the factory
is now a mixed-model pipeline.

## Other streams and operational state

- **S3 (HTTP binding)** stayed frozen; T3, T5, T6, and the completion check
  remain open. No S3 implementation landed.
- **S0** still has the isolated ARM64 self-hosted-runner task
  ([#272](https://github.com/fontanierh/boxology/issues/272)) open.
- **S4, S5, S6, S7** remain unstarted: no crates, no tracker issues. They are
  the majority of the remaining v0 volume. S4 (contract-change classification)
  depends only on S2's schema format and is the most thesis-critical single
  component; scoping it in the tracker is the natural next planning step as S2
  approaches completion.
- **Telegram delivery tooling** was unchanged this interval; issue
  [#248](https://github.com/fontanierh/boxology/issues/248) remains open pending
  human-authorized live credential exchange, which did not occur.

The wider checkout still carries substantial operational clutter — well over a
hundred worktrees and local branches. It was not cleaned as part of this review.

## Assessment and next actions

A scope-weighted estimate at this checkpoint remains approximately **40–45% of
v0**, unchanged from the previous record. This interval was real forward
progress on S2 grammar generalization, but it is incremental burndown of
known-shape emitter work rather than a newly retired risk, so there is no basis
to move the number. The next genuine inflection points are the full data-type
grammar landing, S2-COMPLETE closing, or the first S4 issue existing.

The next bounded sequence is:

1. Continue S2 emitter generalization: the data-type grammar (structs, enums,
   containers), non-default exposure/idempotency, JSON diagnostics, and T6
   hardening, through #102–#107.
2. Land the T8 golden suite and the S2-vs-spec completion check (#108, #109).
3. Scope S4–S7 in the tracker before beginning them, starting with S4.
4. Close out S1 fixtures and completion (#100, #101) as their S2 golden targets
   solidify.
5. Re-baseline the estimate once the full grammar is general or S2-COMPLETE
   closes.

This record preserves the evidence and analysis only. No normative document,
stream specification, issue dependency, or product scope is changed by the
record itself.
