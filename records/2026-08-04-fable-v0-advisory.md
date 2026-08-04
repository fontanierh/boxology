# Fable V0 advancement and course-correction advisory

Date: 2026-08-04

Repository state reviewed: main at b84807a

Advisor: Claude Fable 5, medium reasoning

This is an append-only record of an independent advisory audit requested after the
full Codex delivery session. The advisor read the session transcript at
/Users/jim/.codex/sessions/2026/08/03/rollout-2026-08-03T13-59-58-019fc77e-b765-7ca0-a1e5-e2feec6ae3f8.jsonl,
which measured 19,315,711 bytes at the audit monitoring checkpoint, plus the
repository instructions and delivery-loop configuration, current main and its
history, the V0 stream documents and specs, the records, the live GitHub tracker,
and the Telegram crate. The exact Fable prompt is retained locally at
/Users/jim/.codex/boxology-fable-v0-report-prompt.md with SHA-256
63176d6eee7c8d0922782ad8a78144a7761e631c8e7c66d6abeaf3c273b6a27c.
The raw advisory output is preserved in that Codex session. Because the session
transcript remained live and append-only after the call, this record does not
claim a stable hash for the whole transcript.

This file is an editorial condensation of Fable's raw output, not a byte-for-byte
transcript. A fresh independent factual review corrected stale tracker claims
before commit; those corrections are identified below. Every course correction
and queue item is advisory unless a cited merged specification already makes it
normative. At recording time the operator accepted #319 as the next delivery task
after an independent live-tracker audit; that execution choice does not make the
other recommendations binding. The advisory pass itself was read-only.

## Executive verdict

GO.

The existential V0 question is answered: the merged, replayable clean acceptance
run in #516 proved that an uncoached lead can initialize a generated project and
additively evolve it with mechanical classification. S1, S2, S3, and S6 are
substantially evidence-complete for the re-scoped four-fixture corpus. S4's
classifier semantics are strong, but report and golden code remain.

Fable estimated the remaining V0 work at approximately five to seven mergeable
code or documentation slices, six stream audits, tracker reconciliation, and the
#343 residual ledger and dated completion record. Fresh review notes that #115's
required PR-B breadth makes the low end optimistic.

One correction is required before V0 can be declared honestly. The emergency CI
rewrite in #522 made pull-request validation fast, but it also orphaned the deep
code-validation tier. No automatic workflow runs the full xtask CI aggregate, and
the S0 spec and friction log do not describe the new code-validation topology.
Scheduled advisory and storage-hygiene workflows still exist, as do manually
dispatched runner smoke workflows. The lean pull-request gate should remain. Deep
code validation needs a bounded off-critical-path trigger and the normative
documents must be reconciled.

## Advancement since the previous record

The previous full-session record froze its analysis at 0513573. Seven first-parent
merges followed:

- #517 closed the S4 generator mutation corpus and deliberately closed #318.
- #521 made Fable 5 Medium the specification planner. Review remains GPT-5.6 Sol
  Medium with no fallback.
- #522 collapsed the multi-job PR workflow to one native-Mac validation job.
  Measured hosted time fell to 5m42s for a code PR and 1m11s for a docs-only PR.
- #518 merged the full-session progress record.
- #519 fixed the V0 evidence corpus to hello, greeter, ping, and ping-app, while
  recording the broader grammar and payload work as explicit post-V0 residuals.
- #520 delivered the first #319 reporting slice by attaching canonical detail
  fields to every classifier finding.
- #523 closed external-test digest reachability gaps and closed #441.

The most important proximity changes are that #318 is closed, the evidence corpus
is bounded by merged policy, generated-project acceptance is clean, and hosted PR
latency is no longer the dominant ten-to-fifteen-minute bottleneck.

## What is going well

Independent review continues to catch consequential defects before merge:
fail-open base classification, duplicate-schema fail-open behavior, reaper safety,
and anchor-integrity defects were found and repaired during the session.

Failure honesty held under pressure. Two failed acceptance attempts remain
preserved as records, false keyword-based closures were caught and reopened, and
the final clean run is replayable.

The classifier is the strongest component. It has exhaustive coded-error corpora,
surface locks against suppression, deterministic reports, and a shared
generator/classifier mutation corpus.

The evidence-corpus reduction in #519 was the right speed tradeoff. It replaced an
open-ended full-grammar proof obligation with proof over the four fixtures that
actually define V0, while naming the deferred breadth rather than hiding it.

Velocity is real: 27 pull requests merged in roughly nine hours on 2026-08-03.
The lean pull-request gate should make the next closure phase materially faster.

## What is going less well

The #522 CI topology is not yet represented in the S0 spec or friction log, and no
off-path hosted trigger currently runs the deep validation aggregate. This is a
process and evidence gap, not an argument to restore the old PR matrix.

Tracker hygiene trails implementation. Several issue bodies still describe
dependencies or fixture requirements superseded by merged changes.

S5's boxology check command executes only three of the eight baseline steps in its
current spec. Cargo-graph, formatting, clippy, tests, and quality execution exist
elsewhere as library or xtask behavior but are not all wired into the command.
There is also no JSON output mode. The fastest honest path is to wire the cheap
cargo-graph step and amend the spec so duplicated quality execution is deferred
to #342, where xtask absorption belongs.

The product repository invokes boxology check without an explicit base in its PR
workflow even though the generated-project workflow uses one. The product should
hold itself to the same classification standard.

The raw Fable output proposed closing #248 as implemented. Fresh tracker review
rejected that proposal: #248 was intentionally reopened after #274 for the
separately authorized live credential exchange and operational enablement. That
external action remains open and must not be performed without explicit user
authorization.

## Stream audit

### S0 — partial and temporarily regressed

The workspace, toolchain, xtask surface, budget mechanism, records machinery, and
determinism subjects exist. The current hosted topology no longer runs the
aggregate, budget, deny, links, records, born-valid, clippy, or determinism checks.
The normative S0 CI description contradicts the merged workflow.

Required closure evidence: adopt the lean topology in the S0 spec, add the #522
friction entry, pass an explicit base to the product check, and provide a
scheduled or manually dispatched deep validation cadence outside the PR merge
path. The former Linux-per-PR migration issue #272 has lost its premise.

### S1 — complete

The presence grid, sensitive-value redaction, child contexts, fallible startup,
unsealed-import failure, panic mapping, assembly validation, and fixture import
proofs cover the re-scoped V0 contract. Issue #101's dependency on kitchen-sink is
stale after #519.

### S2 — complete for the re-scoped corpus

Byte-equal goldens, compile-and-run conformance, the frozen projection, mutation
corpus, purity locks, fail-closed generator diagnostics, and the four-fixture
contract are present. The broader structured authoring grammar is explicitly
post-V0. Stream closure should follow restoration of a deep determinism cadence
and tracker reconciliation.

### S3 — complete for the re-scoped corpus, closure stack pending

The raw-socket and raw-server suites, traceability gates, typed and raw hello
proof, headers, lifecycle, and current scalar/unit-error corpus are present.
Issue #115 remains the last concrete evidence task before #116. A reviewed,
budget-compliant PR-A candidate exists locally and its former #114 dependency is
closed, but #115 was deliberately scoped as an estimated two-PR stack. PR-B's
broader raw-case and traceability coverage remains required after PR-A.

### S4 — classifier semantics are mature; report and golden code remain

Issues #317 and #318 are closed. Issue #319 is partially delivered by #520. Its
remaining report-schema and rendering slice is real code work at the head of the
serial S4 queue. #320 is a second real code task for golden and determinism
coverage; the #321 audit follows.

### S5 — partial

Manifest parsing, workspace validation, generated workflow behavior, regeneration
checks, and fail-closed explicit-base classification exist. The command baseline
is only partially wired. Close the gap by combining a normative narrowing with
the cheapest missing live step, then audit #329.

### S6 — complete

The installer emits a deterministic 17-file project with composition, README,
workflow, staged-rename safety, a born-valid integration test, in-process and HTTP
invocation, regeneration no-op, additive evolution, and a negative control.

### S7 — partial

The lead skill, runbook, acceptance records, friction log, root manifest, crate
classification, and fixture opacity are present. The product check needs an
explicit base, the CI decision needs a friction entry, the stream audits remain,
and #343 must record the final residual ledger and V0 declaration.

## Self-hosting and bootstrapping critical path

Stage 1 process self-hosting is complete. Stage 2 ownership and validation
self-hosting is the V0 target: it is adopted, but its evidence needs the CI/S0
correction above. Stage 3 tool boxification remains post-V0 under #74.

The minimum truthful path is:

1. Reconcile S0 with the lean PR topology, restore off-path deep validation, add
   explicit-base product classification, and record the friction.
2. Complete #319's report renderer, then #320's goldens and #321's audit.
3. Land the ready #115 PR-A evidence slice, complete PR-B's remaining breadth,
   then audit #116.
4. Narrow S5's duplicated quality obligations, wire cargo-graph, then audit #329.
5. Close the S1 and S2 audits after reconciling the #519 scope decision.
6. Reconcile stale tracker premises and close #272 or rewrite it for a real
   residual rather than a deleted Linux PR lane.
7. Merge #343's residual ledger and dated V0 completion record last.

These workstreams are substantially parallel. The serial edges are #320 after
#319, each audit after its stream's final code slice, and #343 after all audits.

## Telegram as its own box

The existing Telegram bridge is not blocked by factory machinery. It is blocked
primarily by contract expressiveness: the V0 contract syntax admits scalar leaf
input and output, while Telegram's command/event payloads need structured and
container types and named-field payload emission.

The fastest post-V0 path is:

1. Absorb xtask delegation into boxology check under #342.
2. Implement the smallest structured-I/O subset needed by Telegram.
3. Implement named-field payload emission for that subset.
4. Split Telegram into a capability contract, injected service implementation,
   composition wiring, and a handwritten CLI/binding adapter over typed handles.

The advisor estimates a dogfood-usable typed Telegram core one to two focused days
after V0, and a fully evidenced Telegram-owned box three to five focused days
after V0. The structured-I/O grammar is the largest uncertainty.

## Velocity forecast

At the observed delivery rate, V0 is one to two focused working days away, with a
medium-high-confidence target of 2026-08-05 or 2026-08-06. A conservative case is
three to four days, by 2026-08-08, if stream audits expose genuine repair slices.

Honest stage-2 self-hosting should land inside that same window because the S0
correction is a V0 prerequisite, not a follow-on.

Telegram as a fully evidenced box is estimated for roughly 2026-08-09 through
2026-08-13 on the fast path, contingent on structured I/O remaining bounded.

## Course corrections

Keep independent Sol review, the ordinary 600-line implementation budget,
fail-closed classifier/check behavior, immutable records, and acceptance
NO-GO honesty.

Keep the one-lane native-Mac pull-request gate. Do not restore a multi-platform
matrix on every PR.

Fable recommended restoring deep validation outside the pull-request critical
path and updating the S0 spec plus friction log to match the chosen topology. The
operator's current proposed tradeoff is a Mac-only cadence for V0; cross-platform
breadth need not block every change. This remains a proposal until the S0
normative amendment is independently reviewed and merged.

Amend S5 so that it does not duplicate xtask work that #342 will absorb. Wire the
cheap cargo-graph step now and defer redundant quality execution explicitly.

Reconcile the scope promises from #519 in the tracker. Fable proposed closing
#272 because its Linux-PR-lane premise is gone; tracker review must confirm any
transferred residual before closure. Do not close #248: its intentionally
reopened live-enablement acceptance remains.

Do not spend a critical lane before V0 on more CI restructuring, stage-3
boxification, broad new grammar, or Telegram implementation.

## Prioritized execution queue

1. #319 PR2: classification report schema version 2, human/JSON rendering, and
   hostile-control escaping.
2. S0 CI-truth slice: normative lean topology, #522 friction entry, explicit-base
   product check, and an off-path deep-validation trigger.
3. #115: update and land the already reviewed PR-A candidate, then deliver the
   remaining PR-B breadth.
4. #320: classifier golden fixture-pair suite after #319.
5. #327: S5 amendment plus cargo-graph wiring.
6. Audits #101, #109, #116, #321, #329, and S0 closure.
7. Tracker reconciliation covering the scope and dependency drift.
8. #343: final residual ledger and dated V0 completion record.
9. Post-V0: #342, structured-I/O slices, named-field emission, then Telegram box
   slices. Keep #248 open until the separately authorized live enablement is
   actually evidenced.

## Parallel lane plan

Use at most two compile-heavy jobs on the ten-core Mac:

- Lane 1: the serial S4/S3 code queue.
- Lane 2: the compile-light S0/spec and S5-spec work.
- Lane 3: independent review and repair.
- Lane 4: read-only audits, tracker reconciliation, and record drafting.
- Logical lanes 5 through 8 remain queued and time-share the four live slots.

Merges serialize through the single required Mac validation job. Keep local Cargo
concurrency bounded and do not overlap two nested-Cargo suites with hosted CI.

## Immediate implementation directive accepted by the operator

GO on #319 PR2.

Complete the report schema version 2 renderer and serializer using #520's
canonical finding detail fields. Emit the fields in human output and the JSON
mirror. Escape control and non-printable content so hostile contract text cannot
corrupt terminal or report bytes. Preserve stable ordering and byte determinism.
Add positive rendering tests, exact hostile-input expected bytes, and surface
locks that fail if a detail field is dropped.

Do not change classifier semantics, add BXC codes, add CLI flags, implement check
JSON mode, alter workflows, or edit tracker bodies in the code slice.

Acceptance:

- classifier package tests pass with all features and the lockfile;
- the classifier-report determinism subject produces identical bytes twice;
- hostile excerpts render as exact expected escaped bytes in both formats;
- the surface lock detects removal of any detail field;
- the ordinary implementation remains within the 600-line budget.

If the human and JSON renderers do not fit honestly, split them rather than hiding
scope or tests.

## Final advisory verdict

Proceed toward V0. The clean generated-project acceptance proof is already
merged. Preserve the lean pull-request gate, correct its missing off-path
validation and documentation, then execute the bounded closure queue above.

The advisory worktree was clean before and after the Fable pass. The report made
no repository, tracker, or external changes.
