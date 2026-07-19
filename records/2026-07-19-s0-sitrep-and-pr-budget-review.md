# S0 Situation Report and Review of the 400-Line PR Budget

Record of the review held on 2026-07-19 between the maintainer and Fable 5 (Claude), after the S0 implementation tasks completed and while the S0 stream completion check was executing. Two subjects: the state of the first stream executed under the [v0 execution methodology](../AGENTS.md#v0-execution-methodology), and the maintainer's direct question — **"Are we being too strict at 400?"** — about the methodology's per-PR review-attention ceiling.

## Situation report: stream S0

S0 (product-repo bootstrap and CI, [spec](../specs/s0-repo-bootstrap.md)) was executed by a pool of agents working through the task issues #86–#92, each as a stack of pull requests under the 400-line budget.

**State at the time of this record:**

- All seven implementation tasks (#86–#92) are closed with acceptance met. The final task, S0-T7 (#92, determinism meta-fixtures and the cross-platform compare lanes), merged as four independently green layers (PRs #136–#139), with the exact merged-`main` validation run fully green.
- Only #93, the stream completion check, remains open. It is executing the spec's acceptance criteria AC1–AC7 against merged `main` — running the checks, not reading PR descriptions — and accumulating the evidence in issue comments. Closing #93 is the declaration that S0 is done.
- Quality outcomes look strong. Splits landed at semantic seams, task issues were updated transparently when plans changed, and acceptance criteria were never weakened to fit the budget.
- The pool invented a practice the methodology did not prescribe: **red-proof PRs** — pull requests that deliberately demonstrate a gate failing (e.g. #140's five isolated red runs covering normal drift, unexpected comparator success, absent retained data, corrupt retained data, and missing run evidence), then are closed unmerged. The gate's ability to fail is thereby proven on the record without polluting `main`. This practice is worth keeping.

**Calibration finding.** Per-task PR-count estimates ran roughly **3× light**. S0-T6 was estimated at two pull requests and took seven layered PRs (#126–#135: A, B1, B2a, B2b, C1, C2, D); S0-T7's fresh specification against the merged verifier established four layers where two had been estimated; the stream will finish at roughly 25 PRs against a ~10 estimate. The decomposition itself reads as coherent — layers, not confetti — so this is an estimation error, not a quality problem. Future task issues should carry PR estimates calibrated against this factor, and estimates remain non-binding.

## The question: is 400 too strict?

The budget under review, verbatim from `AGENTS.md`: every pull request adds at most 400 hand-authored lines, including tests; checked-in derived artifacts are excluded; the budget is a review-attention ceiling, not a stylistic preference; a change that cannot fit is split further or its task re-scoped. The rule is absolute — no override label, no exemption — a deliberate decision from the spec-review round that rejected a proposed Markdown exemption (whose supporting factual claim was wrong, as recorded in the S0 spec's D7).

### What the S0 evidence says the cap is buying

- **The rule's theory of operation fired on a real case.** During T6, review found a scratch-isolation defect whose regression test did not fit the remaining six lines of budget. The result was that the correctness work landed as its own reviewable unit at a pre-agreed seam — instead of being appended to an already-full PR, where it would have received the least review attention of anything in the stack.
- **Splits landed at semantic seams**, with the split points named in the task issue before implementation, not at arbitrary line boundaries.
- **The pool self-enforced.** No gaming, no budget litigation; the pool built methodology (red-proof PRs, layer naming, per-PR budget reporting such as "156/400") *on top of* the constraint. A constraint agents route around produces gaming; one they build on is set about right.

### What it is costing

- The ~3× PR inflation is the honest cost, but it decomposes: most of it is estimation error, not budget friction. The per-PR fixed overhead (CI runs, tracker reconciliation, review setup) multiplies with PR count — and for an agent pool that overhead is cheap and parallelizable, while the resource the budget protects, maintainer review attention per merge, is the one that does not scale. That asymmetry is risk 5 of the [strategy review](../boxology-details/10-strategy-review.md): raising the cap optimizes the abundant resource at the expense of the scarce one.
- **Marginal strictness bites hardest near the boundary.** "Six lines remaining" forcing a split is the cap at its most arbitrary — 400 versus 406 carries no review-attention meaning. But the observed resolution was fine, and a tolerance band would reintroduce exactly the negotiability the review round rejected. An absolute number's value is that nobody spends cycles litigating it.

### Watch conditions

1. **Tests competing with implementation for budget.** If the pool ever starts thinning tests to fit implementation under the cap, that is the failure mode that matters. No evidence of it in S0 — the opposite, if anything. If it appears, the fix is separate test-line accounting, not a bigger shared pot.
2. **Whether S1/S2 change the picture.** S0 is infrastructure and naturally decomposable. S1's ABI and S2's generator have more entangled cores; a genuinely atomic over-budget change may exist there. The methodology's existing escape valve — split further or re-scope the task — is the pressure release. The evidence that would justify revisiting 400 is **repeated pathological splits: splits forced across no semantic seam because none exists.** One awkward six-line case in S0 is not that evidence.
3. **Relaxation under schedule pressure is the specific trap.** Loosening the rule one stream in, on evidence that it is working, would teach the pool that the budget is negotiable exactly when a review-attention ceiling earns its keep.

## Decision

- **The 400-line budget stands unchanged and absolute.** No normative text changes.
- **Revisit trigger recorded:** repeated splits with no semantic seam to land on, or observed test-thinning to fit implementation under the cap. Either is recorded as a dated record here when observed, per the dogfooding pain discriminator (mechanical friction is tooling's future job; semantic friction is thesis data).
- **Estimates recalibrated:** treat pre-execution PR-count estimates as ~3× light until a later stream's data says otherwise; wall-clock expectations follow.
- **Red-proof PRs are endorsed** as pool practice: deliberate failure demonstrations closed unmerged, cited from the task issue.
