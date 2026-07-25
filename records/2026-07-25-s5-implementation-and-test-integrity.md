# S5 Implementation Start and the Test-Integrity Sweep

Record of the situation report held on 2026-07-25 in the afternoon between the
maintainer and the assisting agent (Claude Opus 5), covering the interval since
this morning's [v0 planning completion and spec review round](2026-07-25-v0-planning-and-spec-review-round.md).
The baseline is that record's own merge commit, `origin/main` `3d6b296` after PR
[#353](https://github.com/fontanierh/boxology/pull/353) at 10:44 CEST. The
refreshed checkpoint is `origin/main` `d4cf218`, after PR
[#362](https://github.com/fontanierh/boxology/pull/362) at 14:48 CEST.

This is historical and operational analysis. It introduces no product,
architecture, stream-dependency, or normative process decision. The v0 estimate
is an assessment, not a normative change.

## Executive assessment

Implementation of the four newly specified streams began. The delivery loop took
S5-T1 ([#323](https://github.com/fontanierh/boxology/issues/323)) and landed
four substantive pull requests on `boxology-manifest` in under four hours. Among
them, the manifest model now implements the **`fixtures` declaration** — the
fixture-opacity mechanism decided during yesterday's review round and specified
in the S5 amendment the evening before. A normative resolution reached
implementation inside a day, which is the review round's value made concrete
rather than argued.

The interval's most consequential item is not a feature. A mutation sweep filed
as [#361](https://github.com/fontanierh/boxology/issues/361) proves **nine tests
pass while the behaviour they protect is broken, four of them guarding this
repository's own validation gates** — including the 400-line pull-request budget
and the whitespace gate. The gates function today; what the sweep establishes is
that nothing would detect them regressing. For a project whose thesis is that
mechanical checks make discipline a fact rather than a promise, unprotected
gates are a first-order defect, and they are now enumerated with reproductions
and minimal fixes.

The repository's checks remain green.

## Correction to this morning's record

The morning record stated that the delivery loop "has been idle since 22:10 on
07-24 with a clean tree and no in-flight branch" and made restarting it the
first next action. The maintainer confirmed shortly after that merge that the
loop was in fact already running. The inference had been drawn from repository
state alone — no push since 22:02, no open pull requests, no new remote branches
— which lags a loop working in a local worktree before its first push. The
observation was therefore accurate about the repository and wrong about the
loop; its next action was already moot when written. Recorded here per the
convention that a record is corrected by a later record citing it, not by
rewriting.

## Checkpoint comparison

| Metric | Morning checkpoint (`3d6b296`) | Refreshed checkpoint (#362) | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 221 | 227 | +6 |
| Open PRs | 0 | 0 | 0 |
| Issues closed/open | 48 / 78 | 48 / 81 | 0 / +3 |
| Tracked files | 156 | 160 | +4 |
| Rust lines | 41,359 | 42,765 | +1,406 |
| First-parent merges | 0 | 6 | +6 |

The aggregate diff is 9 files, 1,471 insertions, and 46 deletions. The +3 open
issues are the three findings described below. At the refreshed checkpoint,
exact-main run
[30158586180](https://github.com/fontanierh/boxology/actions/runs/30158586180)
passed all protected jobs.

## What landed

### S5-T1: the manifest crate

- [#357](https://github.com/fontanierh/boxology/pull/357) added the
  `boxology-manifest` crate and its glob dialect.
- [#359](https://github.com/fontanierh/boxology/pull/359) parsed the schema-1
  manifest model.
- [#360](https://github.com/fontanierh/boxology/pull/360) modelled
  `display_name`, **`fixtures`**, and quality commands.
- [#362](https://github.com/fontanierh/boxology/pull/362) modelled the
  `[[crates]]` and `[[derived]]` sections.

#323 remains open; the strict-rejection corpus and the remaining parse
diagnostics are outstanding. This is the first implementation work on any of
S4–S7, and it began on the task the specification identified as startable with
no unmet dependency.

### S3 and hygiene

[#355](https://github.com/fontanierh/boxology/pull/355) made the HTTP server
join connection tasks before shutdown returns, continuing S3-T3
([#112](https://github.com/fontanierh/boxology/issues/112)).
[#354](https://github.com/fontanierh/boxology/pull/354) ignores
harness-created agent worktrees — a small measure against the worktree clutter
every recent record has noted.

## The test-integrity sweep

#361 follows an earlier round in which five vacuous tests were found and fixed.
Each of the nine new findings was verified by applying a minimal mutation to
production code, running the guarding suite, observing green, and reverting;
four further hypotheses went red under mutation and are reported as refuted
rather than padded into the list. The work was done in a throwaway worktree at
`origin/main` `ada9c718`.

The four that guard repository gates:

- The **400-line budget** can be disabled wholesale. Adding `"crates/"` to
  `DERIVED_OUTPUT_PATHS` excludes every source file in the workspace from the
  hand-authored line count, and `cargo test -p xtask --all-features` reports 67
  passed, 0 failed. The guarding test never passes the real constant to the
  function it appears to protect.
- The **whitespace gate** can be made a total no-op; the only test asserts the
  gate passes on an already-clean repository, with no paired negative.
- The **editor gate** can be repointed at a different crate.
- **Determinism-manifest sorting** can be deleted.

The honest consequence for this project's own reporting: the budget and gate
results cited in recent records and pull requests were real runs that genuinely
passed, and the gates are correct at this checkpoint. What was overstated is the
implied durability — a regression of the gates themselves would have been
silent. The finding is exactly the class of evidence the quality discipline
exists to surface, produced by systematic mutation rather than by accident.

## Also filed

[#358](https://github.com/fontanierh/boxology/issues/358) records that
`boxology-contract` bundles pure identity types with runtime cancellation,
forcing `tokio-util` onto pure consumers — noticed while building the
specified-as-pure `boxology-manifest`. It is dependency-hygiene friction, and
under the pre-decided discriminator it is **mechanical** rather than semantic:
automatable toil, not evidence against the box boundaries. It is the first
natural candidate entry for the friction log, which does not yet exist
([#339](https://github.com/fontanierh/boxology/issues/339)).

[#356](https://github.com/fontanierh/boxology/issues/356) records that a
post-abort dispatch spawn can hang `join_tasks` unboundedly in S3; #355
addresses part of the shutdown path.

## Velocity and process

Seven first-parent merges by mid-afternoon, against 52 on 07-22, 22 on 07-23,
and 25 on 07-24. The S5-T1 cadence — four substantive pull requests in under
four hours, each inside the 400-line ceiling — is the first evidence on the
delivery loop with Opus 5 in the implement and review roles
([#350](https://github.com/fontanierh/boxology/pull/350)), and it produced three
recorded findings alongside the code rather than code alone.

## Other streams and operational state

- **S2** saw no generator merges this interval; #102–#109 remain open.
- **S1** retains #100 and #101.
- **S0** retains [#272](https://github.com/fontanierh/boxology/issues/272),
  which the amended S7 gate counts against v0 completion.
- **S4, S6, S7** have no implementation; #316, #338, and #339 remain startable
  with no unmet dependency.
- **Telegram** tooling was unchanged;
  [#248](https://github.com/fontanierh/boxology/issues/248) remains open pending
  human-authorized live credential exchange, which did not occur.

## Assessment and next actions

A scope-weighted estimate at this checkpoint is approximately **48% of v0**, up
slightly from this morning's 45–48% on the strength of real S5 implementation
rather than planning. S4, S6, and S7 remain at zero code, which continues to cap
the figure.

The next bounded sequence is:

1. Prioritize #361 above the next feature slice. Four broken gates on a platform
   whose claim is mechanical verification outrank another manifest increment, and
   the issue supplies reproductions and minimal fixes.
2. Continue S5-T1 to the strict-rejection corpus, then T2 and T3.
3. Land #339 so findings like #358 are captured with a category from the start,
   as the S7 specification requires before stage-2 adoption.
4. Continue S3-T3, including #356.
5. Re-baseline the estimate when S5-T1 closes or S3-COMPLETE closes.

This record preserves the evidence and analysis only. No normative document,
stream specification, issue dependency, or product scope is changed by the
record itself.
