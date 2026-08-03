# 2026-08-03 — Full-session progress and V0 course correction

## Context and scope

Henry asked for speed: up to eight logical lanes, native macOS as the canonical
host where practical, fewer platform constraints, an independent CI-speed lane,
and an orphan reaper. He delegated coordinator authority to make the ordinary
project calls and accepted bounded V0 trade-offs. This record captures the
resulting session and its course correction; it does not introduce or narrow a
normative rule. Existing authority remains in the linked specs, issues,
[`AGENTS.md`](../AGENTS.md), and [`ops/friction-log.md`](../ops/friction-log.md).
Any new normative narrowing belongs in a separate PR.

Fable 5 performed the final advisory pass read-only: all 9,137 events in the
preserved Codex transcript, then the repository, GitHub tracker, runner state,
launchd, and acceptance worktrees. Its snapshot ended at `614d9f0`; this record
updates that analysis through `main` at
[`0513573`](https://github.com/fontanierh/boxology/commit/05135735be46e6ca2812be2715f77024c292df8b).

The latest pre-session record is the
[2026-07-27 S4 identifier prerequisite](2026-07-27-s4-identifier-prerequisite.md).
Sixty-nine PRs merged from July 28 through August 2 without another operational
record. That gap is acknowledged, not retroactively reconstructed here. This
session itself began at `29117d3` ([#484](https://github.com/fontanierh/boxology/pull/484))
and merged 27 first-parent PRs through
[#515](https://github.com/fontanierh/boxology/pull/515).

## What shipped

The 27 session PRs are #485, #486, #487, #490–#496, #498–#501,
#504–#516 excluding #502 and #503, which are issues rather than session PRs.
They fall into four connected outcomes.

### Delivery policy and model configuration

[#486](https://github.com/fontanierh/boxology/pull/486) moved independent
review to native `gpt-5.6-sol` at medium reasoning. It merged early in this
same session and is already the current policy; a later repeated request was
correctly treated as satisfied rather than used to manufacture a no-op change.
[#487](https://github.com/fontanierh/boxology/pull/487) changed the allowed
Kimi exhaustion/unavailability fallback to Fable 5 medium through Claude CLI.
The current role order is:

- specification: Kimi K3 high, then Fable 5 medium only on usage exhaustion or
  model unavailability;
- implementation: Cursor Grok 4.5 High Fast, then GPT-5.6 Luna max under the
  same bounded fallback conditions;
- review: GPT-5.6 Sol medium, with no silent fallback; and
- repair: the implementation configuration.

Pure model/configuration changes use the authorized direct configuration path
rather than the repository delivery loop. Code-bearing work retained the
independent specification, implementation, review, repair, and validation
gates.

### A born-valid generated project and the foundation proof

S6 moved from a hollow template to a working project:
[#491](https://github.com/fontanierh/boxology/pull/491) emitted the runnable
`ping` package, [#493](https://github.com/fontanierh/boxology/pull/493) the
application composition, [#495](https://github.com/fontanierh/boxology/pull/495)
the generated README and workflow, [#496](https://github.com/fontanierh/boxology/pull/496)
the born-valid end-to-end proof, and
[#500](https://github.com/fontanierh/boxology/pull/500) the tightened complete-tree
Linux/macOS evidence. Issues #332–#336 are closed with cited evidence.

S7 then exposed and removed its real blockers. The fail-honest runbook landed
in [#501](https://github.com/fontanierh/boxology/pull/501); the fast-V0 gate was
focused in [#494](https://github.com/fontanierh/boxology/pull/494). Composition
validation landed in [#507](https://github.com/fontanierh/boxology/pull/507) and
[#509](https://github.com/fontanierh/boxology/pull/509). All-capability selection
and composition landed in [#504](https://github.com/fontanierh/boxology/pull/504)
and [#510](https://github.com/fontanierh/boxology/pull/510), closing #502. The
classifier's raw metadata/field split landed in
[#512](https://github.com/fontanierh/boxology/pull/512), closing #317, and
[#513](https://github.com/fontanierh/boxology/pull/513) made `check --base`
execute real, fail-closed classification.

Two failed starts were retained rather than laundered: the invalid candidate in
[#511](https://github.com/fontanierh/boxology/pull/511), and the technically
single-agent host run that could not see the onboarding skill in #516. The next
fresh lead used only the pinned skill, initialized an empty `hello-v0`, and then
added `ping.greet(name)` in the same non-delegating session. The operator
replayed both `Hello, Ada!` bindings; `boxology check --base e7abfad` reported
`BXC0039 ping ping.greet additive`; baseline `e7abfad`, evolved `9a0caec`, and
the skill hash remained unchanged. The full proof is in the
[clean acceptance record](2026-08-03-foundation-acceptance-clean.md).
[#516](https://github.com/fontanierh/boxology/pull/516) merged as `2eadda1` and
properly closed [#340](https://github.com/fontanierh/boxology/issues/340).

### CI, runners, reaper, and orphans

The session found ten CPU-saturating orphan test shells about twenty hours old
and removed them. [#492](https://github.com/fontanierh/boxology/pull/492) shipped
the review-orphan reaper inert, then six clean dry-run observations preceded
live launchd enforcement. Review found and fixed symlink ownership borrowing,
false-success signal logging, and vacuous race tests before activation.

[#498](https://github.com/fontanierh/boxology/pull/498) bounded the repo-owned
fleet at eight JIT slots—four Linux and four Mac—under a drain-gated migration
with rollback snapshots. [#499](https://github.com/fontanierh/boxology/pull/499)
added explicit fail-closed acknowledgment for stale GitHub runs before the
11-day zero-job zombie `29990172003` was deleted. The stale merged branch
`feat/issue228-editor-evidence` was also deleted and remains recoverable at
`d027b62`.

Mac-first and split validation landed through
[#485](https://github.com/fontanierh/boxology/pull/485),
[#490](https://github.com/fontanierh/boxology/pull/490), and
[#505](https://github.com/fontanierh/boxology/pull/505)–
[#508](https://github.com/fontanierh/boxology/pull/508). Linux fell from 10m42s
to 1m07s; Mac lanes measured about 5m57s–6m17s primary, 3m08s–3m49s born-valid,
and 6m24s–8m13s capstone. [#514](https://github.com/fontanierh/boxology/pull/514)
capped Cargo concurrency and removed a 738 MB Linux target cache.

That is improvement, not completion. On #515's authoritative run
[`30851735169`](https://github.com/fontanierh/boxology/actions/runs/30851735169),
the first capstone attempt ran 15m15 and timed out/cancelled after its internal
summary had already printed PASS; aggregate validation therefore failed. The
failed-job rerun was queued and in progress as this record began, then completed
green in 4m26 before finalization. This is a real breach of Henry's explicit
"over ten minutes is unacceptable" threshold, not a reason to wait for a
second uncontended breach.

CI round four therefore proceeds now. At the record cutoff Kimi's specification
was GO: three compile-time-pinned capstone groups estimated at 5.6, 6.8, and
7.2 minutes plus an independent reaper lane, complete exact-once assignment,
fail-closed unknown groups, and unchanged main-push depth. The configured Cursor
implementation worker was active on branch `ci/split-capstone`. This is an
active lane, not a proposal; its result still requires ordinary review and
validation.

### S4 delivery now in flight

[#515](https://github.com/fontanierh/boxology/pull/515) is PR-A for #318:
capability metadata and field taxonomy `BXC0044`–`BXC0052`, 572/600 lines, with
independent Fable and native GO reviews. Its failed first capstone and successful
failed-job rerun are both part of the evidence. It merged as `0513573` while
this record was being prepared.

PR-B is no longer an uncommitted risk. Its reviewed pre-rebase commit was
[`b0790ae`](https://github.com/fontanierh/boxology/commit/b0790aea23c3dd2ea856726d0f1fa6e072d480d0);
it is now rebased and pushed as `54c218c` on `feat/s4-generator-corpus` and open
as [#517](https://github.com/fontanierh/boxology/pull/517): the exact shared 11 legacy and 21 mixed
generator mutations, error rename, pinned classifier findings, and four
negative-control categories. Sol-medium review found one anchor-integrity
defect; the repair made one exact-two-anchor `mixed_error_rename` feed both
revision and classifier proofs, and re-review returned GO. The slice is 562/600
hand-authored lines; hosted validation is in progress.

#515's negated tracker prose prematurely auto-closed #318 even though PR-B was
still pending. The coordinator caught and reopened it; #517 now carries the
deliberate `Closes #318` transition. This is a third concrete instance of the
closing-keyword trap described below, not evidence that PR-B was already done.

## What is working

- Independent review is paying for itself. It caught a fail-open Git-command
  exit path in `check --base`, duplicate-schema fail-open behavior, a future
  non-exhaustive classifier match, three reaper safety defects, PR-B's split
  mutation proof, and a CI-split coverage regression. CI alone would not have
  found those defects.
- Failure honesty held under schedule pressure. The coordinator stopped the
  first acceptance run, retained both failed records, reopened false tracker
  closures, and did not relabel a timeout as usage exhaustion.
- The failed S7 run produced useful discovery: #317–#320 were still open despite
  S4 feeling nearly complete. #512 and #513 turned that discovery into a clean
  proof within the same day.
- Parallelism had a useful shape. Eight logical lanes were time-sliced through
  the actual four-live-agent limit; read-only audits, record work, review, and
  critical-path code advanced without pretending eight agents could execute
  simultaneously on this host.
- Destructive or autonomous operations used explicit gates: reaper dry-run
  before enforcement, drain and rollback for the runner migration, and an
  acknowledgment proof for the zombie-run deletion.

## What is not working well enough

- Mac contention remains the dominant tax. Several heavy jobs contend on one
  ten-core machine; runs exceeded ten minutes, reached 15m15, or hung with no
  useful children. Four CI rounds in one session are expensive, but the latest
  timeout makes round four corrective work rather than optional polish.
- #492, #493, #495, and #496 were administratively merged with a Mac gate
  incomplete or superseded, backed by local evidence and independent review;
  #500 later supplied complete cross-platform proof. That bounded exception
  worked, but "the next PR will prove it" is not a general substitute for a
  code-bearing PR's own full evidence.
- The acceptance harness itself caused one rejected CLI configuration attempt
  and one fresh lead refusal because the skill was not registered. Both were
  preserved, but a separate normative PR should decide whether a host-skill and
  delegation preflight belongs in the runbook.
- Fable xhigh advisory calls took 8–12 minutes or timed out without output.
  Those were backend/CLI timeouts, not Kimi exhaustion. Fable medium later
  returned useful GO advice in about 2m30, validating the configured fallback
  effort.
- GitHub closing-keyword parsing falsely closed #333 and #340 from negated
  prose. Both were reopened; #340 is now legitimately closed by #516. Closing
  language needs deliberate review during tracker reconciliation.
- One worker exceeded its boundary with a broad process-name kill and was
  terminated. The reviewed reaper's positive ownership gates—not broad names—
  are the reusable mechanism.

## Distance to an honest V0

The clean foundation proof removes the existential question: Boxology can guide
a fresh lead through initialization and compatible evolution. It does not by
itself declare V0. Under [#343](https://github.com/fontanierh/boxology/issues/343),
the remaining dependency-critical work is:

1. shepherd reviewed PR-B #517 through CI and merge it to close #318, then
   reconcile the tracker after GitHub applies the closing keyword;
2. reconcile #319's report evidence, close #320's golden corpus, and perform
   the S4 completion audit #321;
3. resolve the remaining S5 scope in #326–#329—currently estimated at one to
   three bounded slices after audit;
4. audit S0 and S1–S3 completion (#101, #109, #116, including #272 and open
   tracker debt), citing merged evidence or opening a bounded repair slice;
5. verify every V0 relaxation is represented in the friction log; and
6. merge #343's dated V0 record and ordered residual ledger.

Fable estimated 8–14 mergeable slices at `614d9f0`; #516 and #515 have since
removed two, while the newly active CI round-four implementation adds one slice
that was not in Fable's enumerated range. The current planning range is therefore
roughly **7–13 slices**, about **4–7 carrying code** and the rest audits or
records. At this session's measured velocity—25
merged PRs at Fable's cutoff, 26 when #516 merged, and 27 before finalization,
in roughly nine hours with the review gate still finding real
defects—the honest estimate is **one to three focused working days**. The range
depends on completion audits surfacing no more than about three genuine repair
slices and on round four preventing more Mac timeout churn. Audit closure must
not be rubber-stamped merely to hit the lower bound.

## Milestones that must remain distinct

- **`hello-v0` foundation proof:** complete and recorded by #516. It proves a
  generated project can be initialized and evolved by one uncoached lead.
- **Formal V0:** not yet declared. It additionally requires the stream audits,
  remaining S4/S5 closure, friction completeness, and #343's dated record.
- **Absorption (#342):** first mandatory post-V0 work—delegate repository xtask
  behavior to `boxology check`, remove temporary registries, and separate
  names. Its post-V0 deferral is already recorded in the friction log.
- **Full bootstrap:** Boxology's own crates become managed boxes through
  Boxology itself. Stage-2 self-classification has begun, but full bootstrap is
  a post-V0 continuum, not the V0 gate.
- **`boxology-telegram` as a box:** after absorption basics, give the existing
  tested Telegram core a typed contract/provider and a composition while
  retaining its current CLI as a handwritten binding. The minimal core shape
  is about three slices; including command parity and evidence is closer to
  five focused slices, plus #342. Dogfood use remains plausible one to two days
  after V0; a fully evidenced own-box result is roughly three to five focused
  days after V0. Streaming `listen` can remain binding orchestration over typed
  polling rather than blocking the first box form.

## Course correction and priority

CI round four is already in motion because a real 15m15 timeout crossed the
user-set threshold. After that bounded repair, freeze new CI ambition unless a
required path again breaches the target; the critical path is closure. Do not
weaken independent review, fail-closed `check`/drain/reaper behavior, the
born-valid lane, acceptance NO-GO honesty, or the 600-line review budget.

The current order is:

1. shepherd the reviewed #517 through hosted validation and deliberate #318 closure;
2. reconcile any remaining #318 tracker premise after that merge;
3. close S4 through #319, #320, and #321, then S5 through #326–#329;
4. run S0/S1/S2/S3 evidence audits in parallel under "cite merged evidence or
   open one bounded repair slice";
5. draft #343's residual ledger in parallel and merge it only after the audits;
6. keep #342, Telegram migration, and other post-MVP scope after formal V0.

Use four live lanes on the ten-core host: one serial critical-path coding lane,
one read-only audit lane, one independent review/repair lane, and one
records/tracker lane. Up to eight logical lanes may queue through them. Keep no
more than two heavy Mac jobs concurrent and retain #514's local Cargo caps.

Henry has delegated coordinator authority over these ordinary choices. Optional
human ratification of the already-taken run/branch deletions, live reaper
enforcement, Telegram's exact start time, or the public V0 announcement can be
useful reputationally; none blocks the authorized engineering path. The public
V0 declaration remains a natural point for Henry's signature if he wants it.

## Verdict

**GO.** The clean self-hosting proof is merged and replayable, S4's remaining
work is in reviewed or concrete slices, and the V0 remainder is enumerable.
The immediate risks are Mac timeout churn and evidence audits becoming closure
theater. Round four addresses the first; strict cited-evidence audits address
the second. Continue toward formal V0, then absorb and box Telegram.
