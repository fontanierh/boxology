# V0 Planning Completion and the Spec Review Round

Record of the situation report held on 2026-07-25 between the maintainer and the
assisting agent (Claude Opus 4.8), covering the interval since the
[multi-capability generation situation report](2026-07-24-multi-capability-generation.md).
The baseline is that record's refreshed checkpoint: `origin/main`
`e83c67b`, after PR [#311](https://github.com/fontanierh/boxology/pull/311),
approximately 14:40 CEST on 2026-07-24. The refreshed checkpoint is
`origin/main` `d16042f`, after PR
[#352](https://github.com/fontanierh/boxology/pull/352), merged 22:10 CEST on
2026-07-24.

This is historical and operational analysis. The two decisions it reports —
the acceptance-run gate and manifest-declared fixture opacity — are binding
through the specification and design-document changes that already merged in
PRs [#349](https://github.com/fontanierh/boxology/pull/349) and
[#347](https://github.com/fontanierh/boxology/pull/347); this record cites them
and changes no normative document itself. The v0 estimate below is an
assessment, not a normative change.

## Executive assessment

Two things completed in this interval that had been outstanding since the
project began implementation.

First, **every v0 stream now has a merged, accepted specification.** S4, S5,
S6, and S7 were specified, reviewed, amended, and filed as tracker tasks. The
remaining v0 scope is enumerated for the first time; no stream is unscoped, and
the planning risk repeatedly flagged in the last three records is retired.

Second, **the specifications went through an independent review round before
any of them reached implementation.** Four reviewers — one per stream, run on
Fable-High — audited each spec against its normative inputs. All four returned
*sound-with-findings*; none returned unsound. Eleven confirmed high-severity
defects were found and fixed by amendment. Three of them would have blocked
implementation outright, and two were silent contradictions of merged normative
text. The round cost roughly two hours and is the clearest evidence so far that
the review discipline earns its price on documents as well as code.

Alongside the planning work, the generator's foreign-import chain completed and
the S3 freeze ended: the HTTP server binding is now under active construction.
The repository's checks remain green throughout.

## Checkpoint comparison

| Metric | 2026-07-24 checkpoint (#311) | Refreshed checkpoint (#352) | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 203 | 220 | +17 |
| Open PRs | 0 | 0 | 0 |
| Issues closed/open | 48 / 54 | 48 / 78 | 0 / +24 |
| Tracked files | 148 | 155 | +7 |
| Rust lines | 39,312 | 41,359 | +2,047 |
| First-parent merges | 0 | 17 | +17 |

The aggregate diff from the baseline is 20 files, 2,679 insertions, and 25
deletions. The +24 open issues are the four new streams' task sets, not
unresolved defects. At the refreshed checkpoint, exact-main run
[30123069092](https://github.com/fontanierh/boxology/actions/runs/30123069092)
passed all protected jobs.

## What landed

### Generator: the import chain closed

The foreign-import hydration gap named in the 2026-07-23 record is now closed:
[#313](https://github.com/fontanierh/boxology/pull/313) compile-proved the
multi-capability adapter and implementation,
[#314](https://github.com/fontanierh/boxology/pull/314) hydrated declared
imports into a fail-closed import model,
[#331](https://github.com/fontanierh/boxology/pull/331) and
[#344](https://github.com/fontanierh/boxology/pull/344) emitted import
descriptors and typed import handles into the adapter, and
[#345](https://github.com/fontanierh/boxology/pull/345) proved a sealed
`greeter`-imports-`hello` composition end to end. Consumer-side import wiring
is therefore real generated code with a cross-box proof, not a stub.

### The four stream specifications

[#315](https://github.com/fontanierh/boxology/pull/315) (S4 — contract-change
classification), [#322](https://github.com/fontanierh/boxology/pull/322) (S5 —
manifest and validation tooling),
[#330](https://github.com/fontanierh/boxology/pull/330) (S6 — installer and
generated project), and [#337](https://github.com/fontanierh/boxology/pull/337)
(S7 — skill, acceptance, and stage-2 self-hosting) merged with their stream
definitions linked, following the established pattern of not restating
normative text. Twenty-four task issues were filed across `stream:s4` through
`stream:s7`, each with scope, acceptance, dependencies, and the 400-line
method note.

### The review round and its amendments

Four independent reviewers audited the merged specs against their normative
inputs, the filed issues, and the actual emitted artifacts. Their confirmed
high-severity findings, and the amendments that resolved them:

- **S4** ([#346](https://github.com/fontanierh/boxology/pull/346)): most of the
  change-kind table could not be exercised against anything schema format 1 can
  currently express, with no dependency recorded — the spec now carries an
  explicit vocabulary gate on #103 with reserved rows; the capability-shape row
  was unreachable through the spec's own strict reader and was removed; the
  input-parameter-name change, which is present-day vocabulary and alters the
  revision, had no row and would have fallen to the anomaly catch-all.
- **S5** ([#347](https://github.com/fontanierh/boxology/pull/347)): byte-diff
  regeneration silently dropped the lazy-regeneration clause of the build
  topology, which would have mass-failed historical artifacts at the first
  representational generator change — now a recorded narrowing with its
  trigger; and the lockfile deferral voided S0's promise that minimal-closure
  enforcement "arrives with S5" — now honored at reporting strength by a
  lockfile-scope finding. The unowned no-flag `--base` default and the
  undecided finding-to-exit-code mapping were also resolved.
- **S6** ([#348](https://github.com/fontanierh/boxology/pull/348)): the
  generated project's dependency on the unpublished platform crates was decided
  nowhere — now an explicit `InitRequest` parameter carrying path dependencies
  to the source checkout; "born valid from first commit" contradicted the
  spec's own decision not to emit a lockfile — born-valid is now defined around
  the documented first Cargo invocation; and atomic write was unachievable as
  stated beside a preserved `.git/` — now a staged-rename mechanism with a
  completion sentinel.
- **S7** ([#349](https://github.com/fontanierh/boxology/pull/349)): the
  repository's fixture manifests would be found by stage-2 discovery, making
  "check green on this repository" unreachable; the ladder's pinned-prior-release
  generator validation was silently dropped; and the v0 gate was vacuous for S0,
  since a closed completion check does not mean a complete stream — S0-T8
  (#272) was filed after S0-COMPLETE closed.

All four specifications now read `Status: accepted`, and thirteen tracker edits
propagated the amendments into the affected issues.

### S3 unfrozen

[#351](https://github.com/fontanierh/boxology/pull/351) and
[#352](https://github.com/fontanierh/boxology/pull/352) began the HTTP server
binding — serving box exposures over a real socket, then exposing composed
boxes through the binding. This is S3-T3 ([#112](https://github.com/fontanierh/boxology/issues/112)),
the largest frozen chunk, resuming after the freeze held since 2026-07-22. The
sequencing was correct: expansion restarted only after the generator's
critical path was proven and the planning backlog was full.

## Decisions reported

Two decisions were taken during the review round. Both are binding through the
merged documents cited, not through this record.

**The acceptance gate is one clean run.** The S7 specification had required the
foundation scenario to pass on two different coding-agent harnesses, presenting
that as the product contract's standard. The reviewer found the attribution
false: the contract defines success as the scenario working, once, and its
harness-neutrality language constrains the *skill's content*, not a run matrix.
The maintainer's decision, recorded in the amended specification: Boxology ships
a skill, not an agent, so there is nothing to validate in a harness. Portability
is a content property enforced by the skill's content audit. One clean run gates
the milestone; runs with additional agents are welcome evidence and never gates.

**Fixture subtrees are opaque to workspace discovery.** The stage-2 collision
required a mechanism, not an assertion. The accepted resolution extends the
manifest model: a platform-kind package may declare owned subtrees as fixture
data, and a `boxology.toml` inside such a subtree is that package's owned test
material rather than a workspace package. The one-sentence normative extension
landed in [Packages](../boxology-details/02-packages.md) in the same pull
request as the S5 amendment that specifies it. This is what allows this
repository — and any repository carrying fixture projects or deliberately
malformed manifest corpora — to satisfy `boxology check` at stage 2.

## Velocity and process

Daily first-parent merges were 52 on 07-22, 22 on 07-23, and 25 on 07-24. The
07-24 total divides into roughly eight specification and amendment merges, one
process change, and the remainder product work — a planning-weighted day rather
than a slow one. A billing interruption cost several hours mid-day; the loop
resumed immediately after it was resolved and produced both S3 merges that
evening.

The delivery loop's model configuration changed twice in two days: planning
moved to Fable-High on 07-24 ([#304](https://github.com/fontanierh/boxology/pull/304)),
and implementation and review moved to Opus 5 that evening
([#350](https://github.com/fontanierh/boxology/pull/350)). The loop has been
idle since 22:10 on 07-24 with a clean tree and no in-flight branch; startable
runway exists and is untouched.

## Other streams and operational state

- **S2** saw no generator merges after #345; issues #102–#109 remain open, with
  the data-type grammar, T6 hardening, and T8 goldens outstanding.
- **S1** retains #100 and #101; #100 now sits on S6's critical path as well as
  S2's, since the installer's box is necessarily a new corpus fixture.
- **S0** still has [#272](https://github.com/fontanierh/boxology/issues/272)
  open, which the amended S7 gate now explicitly counts against v0 completion.
- **S4–S7** have no implementation. Four task issues are startable with no
  unmet dependency: #316, #323, #338, and #339.
- **Telegram** delivery tooling was unchanged;
  [#248](https://github.com/fontanierh/boxology/issues/248) remains open pending
  human-authorized live credential exchange, which did not occur.

The wider checkout still carries substantial operational clutter — well over a
hundred worktrees and local branches. It was not cleaned as part of this review.

## Assessment and next actions

A scope-weighted estimate at this checkpoint is approximately **45–48% of v0**,
a small increase over the prior 40–45%. The increase reflects two things that
are not implementation volume: the retirement of planning risk across four
streams, and the hardening of those plans by independent review before any code
depends on them. S3's resumption adds real implementation progress. What caps
the number is unchanged — S4, S5, S6, and S7 have no code at all.

The next bounded sequence is:

1. Restart the delivery loop; it has been idle since 22:10 with startable work
   available.
2. Continue S3-T3 to completion, then T5 and T6.
3. Begin S4 (#316) and S5 (#323) — neither depends on the S2 tail — and S7's
   friction log (#339), which the S7 specification requires before stage-2
   adoption begins.
4. Continue the S2 tail: data-type grammar, T6, and the T8 goldens gated on
   S1 #100.
5. Re-baseline the estimate when the first of S4 or S5 lands its first task, or
   when S3-COMPLETE closes.

This record preserves the evidence and analysis only. No normative document,
stream specification, issue dependency, or product scope is changed by the
record itself.
