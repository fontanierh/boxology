# Full-transcript V0 situation report

Date: 2026-08-04, evening (Europe/Paris)

Repository: `fontanierh/boxology`

Main: `8e9d6a87777d781ab103e1f828dd928e2fdae0f7` (#550)

Prepared by the operator from a Fable 5 medium advisory and exact-tree independent audits.

## Scope and evidence

Fable read the complete redacted Codex session transcript programmatically and chunkwise from beginning to end: 37,529,886 bytes and 31,022 JSONL events, spanning 2026-08-03T11:59:59Z through the transcript-copy turn on 2026-08-04. The copy differed from the session only by replacement of one API secret; no secret is reproduced here.

The advisory was checked against:

- exact main and first-parent history since record #518 (`0f74e16`);
- the eight stream specs, runtime/design authority, CI workflows, delivery-loop configuration, friction log, and prior records;
- live tracker state supplied by the operator;
- independent exact-tree S1/S2, S3, S5, and CI-truth audits;
- local validation and required GitHub checks for #547–#550.

The first Fable draft did not have transcript access and incorrectly called V0 code-complete. It was rejected. This record supersedes that draft, not any earlier committed record.

## Executive verdict

**V0 is not complete, but the remaining path is now small and explicit.** S1, S3, S4, and S6 are code- and audit-complete. S0's lean CI contract is settled. The S7 foundation acceptance is complete. Genuine remaining code is concentrated in:

1. two or three bounded S2 #107 slices;
2. one small S5 #328 closure slice;
3. one shared evidence run after the remaining implementation and normative content merges;
4. the mandatory dated S7 V0 completion record, followed by an exact-main declaration-verification run;
5. tracker-only closure of #108, #328, #109, and #329, then #343.

There are no open pull requests. #101, #103, #115, #116, #272, #319, #320, #321, #327, #335, #336, and #340 are closed. #342 is explicitly the first mandatory **post-V0** task, not a V0 blocker.

At current velocity, the likely V0 declaration is **2026-08-06**. Optimistic is August 5; conservative is August 8.

## What changed since the last full-session record (#518)

Main advanced by 29 first-parent merges through #550, changing 59 files by approximately +8,635/−721 lines.

### Product and evidence

- S4 closed end to end: canonical finding details, report schema v2, report goldens, both-absent pairing, and generate-report classification (#520, #526, #528–#530).
- `boxology check` grew from a partial skeleton into the near-complete S5 product surface: fmt/Clippy, workspace tests, lock freshness, default base resolution, diff ownership, lockfile scope, manifest pairing, JSON reports, and declared quality commands (#533, #535–#546).
- S3 #115 landed in three honest steps:
  - #547: exact raw/named evidence inventories and workspace-isolation proof;
  - #549: empirical bare-414 correction, replacing a falsified universal head-cap claim;
  - #550: compact atomic normative registry with 147 rules, 468 citations, complete reverse closure, digest locks, and fail-closed mutants.
- Exact-tree S3 completion then passed 132/132 `boxology-http` tests and 25/25 conformance tests; #116 closed.
- S1 tracker debt was reconciled: #101 and the satisfied #103 slice closed; #100/#102/#104/#480 were separated as post-V0 breadth rather than allowed to obscure the actual #107 blocker.

### CI and delivery tooling

- Full/deep validation moved off the required PR path (#527).
- Full product check stayed off the required PR path after it expanded into a duplicate workspace sweep (#531).
- The dormant Linux smoke lane was retired (#534).
- S0/S7 living documentation was reconciled to the real lean native-Mac lane (#548).
- Implementation and repair moved to Pi with Grok 4.5 (#540); planning remains Fable and review remains Sol.

Required CI in the closing sequence measured:

| PR | Change | Required CI |
| --- | --- | ---: |
| #548 | CI-truth docs | 1m14s |
| #547 | HTTP evidence inventory | 2m52s |
| #546 | quality-command execution | 3m50s |
| #549 | real-socket 414 evidence | 4m25s |
| #550 | cold HTTP traceability build | 6m07s |

The old Linux/full-workspace duplication is gone. Heavy cold HTTP builds still have a six-minute tail; that is real remaining performance debt, but no longer a reason to restore redundant platforms or full product sweeps on every PR.

## What is going well

- **Independent gates are earning their cost.** They rejected a 2,562-line draft, two false request-head assumptions, a Clippy-dirty compact draft, and a fail-open `splitn(4)` parser before merge.
- **Evidence is becoming harder to fake accidentally.** #550 locks every Wire rule forward to live evidence and every raw/named inventory backward to at least one rule, while pinning both authority documents by digest.
- **The advisory queue converted directly into shipped work.** The S4 report stack, CI acceleration, S5 check stack, and S3 closure all moved from Fable priorities to merged main in one day.
- **Trade-offs are explicit.** Native Mac V0 evidence, deferred Linux restoration (#525), the four-fixture corpus, and dispatch-only deep validation are recorded rather than implied.
- **Local validation is fast when cache-warm.** The final HTTP registry suite and Clippy run in seconds locally; required CI cost is dominated by cold compilation, not test execution.

## What is going less well

### Pi/Grok is useful but not yet reliably one-pass

- PR-A was strong: roughly 16 minutes for a reviewed 540-line raw-evidence patch.
- The first combined PR-B turn ran roughly 30 minutes and produced about 2,562 added lines against a 600-line budget.
- The first 65,534-byte clamp draft asserted 431 but the socket returned 414; the next geometry returned 404. Fable correctly ratified the smaller truthful contract: Hyper's parse buffer and independent URI bound can produce bare 400/414/431.
- The compact registry draft initially carried Clippy errors. Sol then found that `splitn(4)` silently accepted a fifth DSL field. A Pi repair plus Sol re-review fixed it before merge.

The conclusion is not “Pi failed.” It produced the implementation patches across all three merged HTTP closure PRs. The conclusion is that representation constraints, empirical boundary probes, and independent review must stay hard for the remaining S2 work.

### Interrupting a Pi turn leaked a process group

After one long response wait was interrupted, a bash/Cargo process group remained alive in the registry worktree and held the shared Cargo lock. The operator verified its PID, parent, PGID, cwd, and ancestry before terminating the exact owned group. No live Crab or unrelated process was touched.

This needs a durable Reaper, described below.

### Tracker language still trails code

The issue graph contained satisfied slices mixed with genuine blockers. Closing #101/#103/#115/#116 and labeling broader S2 work post-V0 made #107 and #328 visible as the actual remaining code path. #477 and #481 still require explicit disposition rather than assumption.

## Stream-by-stream V0 status

| Stream | Code | Audit/tracker | Remaining V0 work |
| --- | --- | --- | --- |
| S0 — bootstrap/CI | Complete for ratified native-Mac V0 | #272 closed; CI truth reconciled | Shared closure evidence and declaration-verification runs |
| S1 — runtime | Complete | #101 closed | None; #100 is post-V0 |
| S2 — generator | **Incomplete** | #103 closed; #107/#108/#109 open | #107 purity/source closure, diagnostic closure/JSON, atomicity truth; evidence run; audit |
| S3 — HTTP | Complete | #115/#116 closed | None |
| S4 — classification | Complete | #319/#320/#321 closed | None |
| S5 — manifest/check | Nearly complete | #327 closed; #328/#329 open | One closure PR, evidence run, audit |
| S6 — initializer | Complete | #335/#336 closed | None |
| S7 — acceptance | Foundation accepted | #340 closed; #343 open | Residual ledger, dated V0 completion record, declaration-verification run, tracker closure |

## Genuine remaining V0 work

### 1. S5 #328 closure — one small PR

Keep this focused, approximately 120–250 added lines:

- change the no-repository diff-ownership subject from historical `NotImplemented` to `NoRepository` and update exact goldens;
- assert `check quality passed` and `check result passed` in the born-valid proof;
- reconcile S5 D6's stale “CI always passes `--base`” sentence with #531/#548: required PR CI runs no product command; the final exact-main deep job runs the complete default-base check;
- amend D8/AC9/T6 to native macOS ARM64 for V0, with #525 owning cross-platform restoration;
- retain the emitted `ubuntu-latest` workflow bytes, but move execution of that unpublished source-provisioned workflow to the first-release boundary;
- reconcile #477 against the existing regeneration no-op evidence; add only the minimal one-pass stale-import assertion if the current proof is genuinely insufficient.

After merge, #328 waits for the shared closure evidence run; #329's evidence comment can be drafted in parallel.

### 2. S2 #107A — own-source purity and source closure

One 400–600-line slice should extend the purity/source-closure model to the surfaces currently missed:

- `Cargo.toml` and `build.rs`;
- nonstandard `[lib] path` roots;
- `#[path]` module edges;
- relevant source/effect escape paths.

The acceptance test is a live negative corpus that proves these paths cannot smuggle generated or effectful behavior past the current audit.

### 3. S2 #107B — diagnostic catalog and JSON early failures

One 350–550-line slice, split only if the honest patch exceeds budget:

- lock the complete live BXG code catalog;
- prove every rejection path is coded and no uncoded generator diagnostic escapes;
- make early generator failures obey the same stable JSON diagnostic contract as later failures.

### 4. S2 #107C — atomicity truth

Prefer a small normative amendment over a new transaction engine for V0. D1 currently promises whole-tree atomic writes while the implementation provides per-file atomic replacement. State the real crash-consistency boundary, keep deterministic planning/writes, and leave multi-file transactional publication post-V0 unless a concrete consumer requires it.

### 5. Two-run V0 closure handshake

There are not separate per-stream deep runs, but S7 D7 makes one final run insufficient: the mandatory dated V0 completion record must cite completed evidence, while exact-main verification must include that newly merged record. Use two shared runs:

1. after every remaining implementation, normative, audit-repair, and residual-ledger content merge, capture exact `main` and dispatch `deep-validation.yml`;
2. require `cargo xtask ci --no-budget` and full `boxology check` green, and use that evidence run for #108 golden/determinism evidence, #328 closure evidence, and the S0/S2/S5 evidence chain;
3. merge a separate dated S7 V0 completion record that cites the completed evidence run, maps the full S0–S6 chain, and names the ordered residual ledger;
4. capture the new exact `main`, dispatch `deep-validation.yml` again, and require the same commands green so the declared repository—including the completion record—is exact-main verified;
5. merge no further repository content, then close #108, #328, #109, #329, and finally #343 through evidence comments.

The prior run `30872049102` at `3f366867` predates the #327 stack and does not count. If either audit or either run requires a content repair, merge it, repeat the evidence run, refresh the immutable completion record as a new superseding record if one already merged, and repeat declaration verification.

## Distance to V0

Measured recent slices take roughly two to four wall-clock hours including Fable specification, Pi implementation, Sol review, repair, and 3–6 minutes of CI. Docs/audit-only work is faster. The remaining plan is four implementation/normative content PRs in the likely case, one mandatory completion-record PR, two shared deep runs, and tracker closure.

- **Optimistic — 2026-08-05 end of day:** S5 and S2 run in two lanes; no audit repair; both deep runs green first try.
- **Likely — 2026-08-06:** one Pi budget/correctness repair or one audit-driven bounded fix, followed by the two-run closure handshake.
- **Conservative — 2026-08-08:** source closure or diagnostic enumeration exposes a real semantic defect, or the deep run needs two repair cycles.

## Course correction

- Keep the lean native-Mac required lane and dispatch-only full check. Do not restore Linux or full-workspace duplication during V0 closure.
- Put representation strategy and a raw-line target in the Fable directive before Pi starts; do not wait for a 2,500-line draft to discover that data must be compact.
- Treat a silent turn beyond 20 minutes as a checkpoint/reaper decision. Preserve worktree changes, verify process ownership, and resume with a bounded repair prompt rather than waiting indefinitely.
- Keep Sol medium independent and preserve the “reviewer never implements” boundary. It caught the only fail-open parser bug in #550.
- Use normative amendments when the spec over-promises a mechanism V0 does not need, especially S2 whole-tree atomicity and S5 platform/workflow execution claims.
- Do not start Telegram or #342 before #343. Closure focus is worth more than another parallel product lane for the next one to two days.

## Reaper recommendation

Create a separate delivery-worker Reaper task immediately after V0, or run it as a configuration/tooling-only side lane if it cannot delay S2/S5:

1. launch every worker and validation shell in a dedicated recorded process group;
2. persist PID, PGID, start time, harness, worktree, and cwd in the run record;
3. on interruption, enumerate descendants and verify identity by ancestry, start time, and cwd;
4. terminate only the verified owned group (TERM, bounded wait, then KILL if necessary);
5. sweep for surviving `bash`, `cargo`, `rustc`, and harness children rooted in that worktree;
6. verify the shared Cargo lock is released and record the reap result.

Never use broad name-based kills, and never include Crab/runtime processes in this ownership domain. This is a new launch-ownership domain, not an expansion of the existing process reaper: preserve that reaper's individual-PID, PPID=1, `.codex/reviews`-only contract, its `.codex/worktrees` exclusion, and its prohibition on process-group signaling. #481's residue cleanup is adjacent but does not replace worker ownership. #342 remains narrowly scoped to xtask/check absorption; #551 now tracks the separate delivery-worker Reaper.

## When Telegram becomes its own box

`crates/boxology-telegram` is currently a handwritten platform-role crate, not a generated Boxology box. The shortest honest sequence is:

1. declare V0 through #343;
2. land #342, the mandatory post-V0 xtask/check absorption;
3. add the smallest structured/container I/O subset Telegram needs;
4. add named-field payload emission for that subset;
5. split Telegram into a generated contract, injected service implementation, composition wiring, and handwritten CLI/binding adapter;
6. prove it through the normal manifest/check/classification path.

From the likely V0 date:

- dogfood-usable typed Telegram core: **1–2 focused days post-V0**;
- fully evidenced Telegram-owned box: **3–5 focused days post-V0**;
- calendar expectation: approximately August 9–11, stretching to August 13 if structured I/O is larger than the bounded slice.

Live Telegram enablement remains separately human-authorized under #248.

## Delivery-loop models at main

| Phase | Harness | Model | Effort | Fallback |
| --- | --- | --- | --- | --- |
| Specification | Claude CLI | `claude-fable-5` | medium | None; stop if unavailable |
| Implementation | Pi | `xai/grok-4.5` | high | Luna max only on explicit unavailable/exhausted |
| Review | Codex | `gpt-5.6-sol` | medium | None; stop if unavailable |
| Repair | Pi | `xai/grok-4.5` | high | Same explicit fallback as implementation |

The implementation fallback is Codex `gpt-5.6-luna` at max effort. Transport failures such as HTTP 503 retry Pi; they do not trigger fallback. Pure model/config changes remain authorized for direct application without the product delivery loop.

## Post-V0 residual ledger

- #342 — xtask delegates to `boxology check`; first mandatory post-V0 task.
- #525 — restore Linux/x86 validation and cross-platform determinism before the first pinned external release.
- #551 — own and safely reap interrupted delivery-worker process groups without broadening the existing review reaper.
- #100/#102/#104/#480 — broader fixture, grammar, capability-name, and named-field payload breadth.
- #74 — stage-3 tool/factory boxification; Telegram is the first useful rung.
- #248 — live Telegram enablement, separately human-gated.
- #481 — scratch-directory residue and remaining test-integrity cleanup.
- whole-tree transactional publication, generic distribution/publishing, Windows, auth, streaming, SDKs, and foreign-language boxes.

## Next 24 hours

Use four logical lanes, with no more than two compile-heavy jobs on the Mac:

- **Lane 1 — S2 compile-heavy:** #107A purity/source closure, then #107B diagnostic/JSON closure.
- **Lane 2 — S5/Docs:** #328 closure, then #107C atomicity amendment and final residual-ledger text.
- **Lane 3 — review/repair:** Sol reviews and Pi repairs; run the Reaper sweep after every interrupted worker.
- **Lane 4 — audit/read-only:** prepare #109 and #329 mappings, reconcile #477/#481, and stage final tracker comments.

Once implementation and normative content merges, stop every compile lane and run the two-step closure handshake: evidence deep run, dated completion-record merge, then exact-main declaration-verification run. After that second run, close the evidence issues without changing repository bytes.

## Bottom line

The HTTP and classification races are over. CI is lean enough to stop dominating ordinary PRs. The remaining code path is S2 #107 plus one small S5 closure PR. Preserve the current trade-offs, track orphan cleanup separately, run the evidence/record/declaration-verification handshake, and V0 should declare on August 6.
