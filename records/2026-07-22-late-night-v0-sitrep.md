# Late-Night V0 Situation Report

Record of the review held on 2026-07-22 between the maintainer and Codex,
following the [generated-box critical-path review](2026-07-21-generated-box-critical-path.md).
The review covers the same late evening and early night of July 21–22; it is
deliberately described as a **late-night** update, not an overnight report. It
refreshes repository progress, evaluates the coordinator's response to the
critical-path directive, and records the state after the resulting S2
architecture decision merged.

This is historical and operational analysis. It introduces no new normative
rule, issue dependency, or product decision. PR
[#224](https://github.com/fontanierh/boxology/pull/224) is the authority for
the controlled contract architecture described here, and the existing stream
specifications, tracker decisions, and repository gates remain authoritative.

## Executive assessment

The late-night delivery window was exceptionally productive: fifteen product
PRs and one architecture-decision PR merged after the prior record. S3-T1
completed, S3-T2 moved well into canonical encoding, the real Hello
implementation landed, and S2 advanced exactly far enough to trigger the
coordinator's expansion guard.

The guard worked. Instead of accepting a fourth horizontal S2-T1 slice, the
coordinator stopped, surfaced the underlying authoring/extraction question,
and produced #224. That merged decision selected a controlled
`boxology::contract!` declaration, ordinary Rust implementation, compile-time
conformance, and a shared semantic digest. New issue
[#228](https://github.com/fontanierh/boxology/issues/228) now owns the cold-tree
architecture proof and explicit kill gate.

No generated-box proof rung is complete yet. The architecture decision removes
the authority blocker; it does not itself generate, compile, register, or call
a box. #228 is therefore the primary P0 task from this checkpoint. Continued
S3 progress is valuable but must remain a parallel lane rather than replacing
the generated-box proof.

## Checkpoint comparison

The previous operational record merged on 2026-07-21 at 17:24 CEST as
`1e429b12039a190f37c0d234218a307629b90b8f`. The refreshed checkpoint was
2026-07-22 01:30 CEST at `main`
`2c3aff5769649af00e0272eed919225918115e93`, after PR
[#229](https://github.com/fontanierh/boxology/pull/229). Local and remote main
were synchronized, no pull request was open, and exact-main
[run 29877305655](https://github.com/fontanierh/boxology/actions/runs/29877305655)
passed all six jobs.

| Metric | Prior-record main | Late-night checkpoint | Delta |
| --- | ---: | ---: | ---: |
| PR state | 108 merged, 5 closed unmerged, 0 open | 124 merged, 5 closed unmerged, 0 open | +16 merged |
| Issues | 43 closed, 56 open | 44 closed, 56 open | #110 closed; #228 opened |
| Tracked files | 93 | 98 | +5 |
| Rust lines | 21,081 | 25,445 | +4,364 |
| Tests including doctests | 280 | 335 | +55 |
| Worktrees | 70 | 86 | +16 |
| Local branches | 97 | 113 | +16 |
| Gone-upstream branches | 69 | 77 | +8 |

The exact repository diff after the prior record was 24 files, 4,620
insertions, and 111 deletions across sixteen squash commits. Thirty-five
workflow runs began in the interval: eighteen pull-request runs and seventeen
main pushes. All 35 succeeded.

## What landed

### S1 — real Hello implementation

PR [#212](https://github.com/fontanierh/boxology/pull/212) added the ordinary
Hello implementation crate. The implementation proves the receiver can return
the exact `Hello, Ada!` behavior and domain error locally. It remains
hand-wired fixture substrate rather than generated output, but it is now a real
implementation for the generator and integration proof to consume.

### S2 — guard reached, architecture selected

PRs [#214](https://github.com/fontanierh/boxology/pull/214),
[#218](https://github.com/fontanierh/boxology/pull/218), and
[#219](https://github.com/fontanierh/boxology/pull/219) added capability call-
frame validation, marker metadata parsing, and canonical box-qualified
capability identities. #219 was the third and final additional #102 merge
permitted by the generated-box expansion guard. Generated-box rung one was
still incomplete at that point.

PR #224 then replaced the increasingly costly marker/extraction direction with
a controlled contract-authoring architecture:

- public contracts are written once in a small Rust-like
  `boxology::contract!` grammar;
- implementations remain ordinary Rust and repeat the capability signature
  once;
- ordinary procedural macros and rustc prove implementation conformance;
- a shared semantic digest makes stale generated output fail compilation;
- generation does not require a Rust type resolver, compiler plugin, synthetic
  compiler probe, or execution of project code; and
- transitive presence through `Secret` is rejected in controlled contract
  authoring rather than left ambiguous.

Five independent-review findings on the decision candidate were repaired and a
fresh whole-change rereview returned clean before merge. The decision updated
the affected S1/S2 task premises and created #228 rather than claiming
implementation progress.

#228 must now prove the architecture from a cold Hello tree: generation,
compilation and invocation, ordinary implementation aliases and qualified
paths, compile-fail signature cases, one generated public boundary type,
shared-digest agreement and stale-output failure, non-execution during
generation, cross-context determinism, and acceptable rustfmt/rust-analyzer
behavior. Its kill criteria reject the architecture before emitter expansion
if it requires arbitrary Rust interpretation, parser duplication, pre-
generation compilation, duplicate boundary definitions, or unusable editor
and diagnostic behavior.

### S3 — T1 complete, T2 substantially underway

Eleven product PRs advanced the HTTP binding:

- #215–#217 added scalar, float, and container semantic decoding;
- #220–#221 added structured and enum decoding;
- #222–#223 replayed supported values and strict failures from raw bytes;
- #225 completed canonical Blob decoding and closed S3-T1
  [#110](https://github.com/fontanierh/boxology/issues/110);
- #226–#227 added canonical scalar, Blob, presence, map, list, and struct
  result encoding; and
- #229 added enum encoding and typed domain-error envelopes.

S3-T2 [#111](https://github.com/fontanierh/boxology/issues/111) remains open
for call-error/status mapping, remaining exhaustive golden vectors, and final
reconciliation. This progress is directly useful for the eventual generated
Hello HTTP proof, but it does not complete a generation rung by itself.

## Coordinator and delivery-loop assessment

The critical-path prompt changed behavior in the intended way:

1. It made the generated-box proof, rather than PR volume, the explicit
   optimization target.
2. It enforced the three-merge #102 expansion guard exactly.
3. It caused the coordinator to stop at an authority and architecture seam
   rather than infer policy or continue horizontal parser work.
4. It converted the seam into a reviewed normative decision and a falsifiable
   cold-tree proof with kill criteria.
5. It used otherwise available capacity for dependency-correct S3 work while
   S2 awaited the decision.

The resulting allocation was still heavily weighted toward HTTP: eleven of
fifteen product merges were S3, three were S2, and one was S1. That was
reasonable while the human/authority gate was active. After #224's merge, the
same allocation would become drift. #228 should now occupy the primary lane;
at most one S3 lane should continue in parallel until the cold-tree proof
either passes or kills the architecture.

The current model profile continued to produce strong observable throughput.
All 35 workflows passed, and review remained behaviorally useful: #224
received five repaired findings, while #225 added explicit malformed-padding
vectors after review. The interval still does not provide controlled total-
lead-time, cost, or counterfactual quality evidence, so it supports confidence
in the delivery system rather than a categorical model ranking.

## What is going well

- **The operational guard was real.** The coordinator stopped precisely when
  instructed and escalated a genuine architecture question.
- **The authority answer is falsifiable.** #224 did not merely choose syntax;
  #228 pins a cold-tree proof and explicit reasons to reject the design.
- **CI remained perfect under high throughput.** Every candidate and main run
  in the interval succeeded.
- **Review changed outcomes.** Architecture and wire-codec candidates gained
  substantive corrections before merge.
- **S3 moved from foundation to working semantics.** T1 is complete and T2 now
  has canonical successful and typed-domain output across most supported
  shapes.
- **Hello is becoming an integration subject.** Its contract-side golden,
  fake, handle, and ordinary implementation now exist for #228 to connect.

## What is not going well

- **The generated box still does not exist.** Merge throughput, S3 completion,
  and the architecture decision do not substitute for cold-tree generation,
  compilation, registration, and invocation.
- **S2-T1 remains much larger than estimated.** Nineteen slices preceded the
  architecture gate; the original two-to-three-PR estimate is no longer useful.
- **Complexity is concentrating.** Generator-model `rust.rs` is 4,453 lines;
  HTTP `semantic.rs` is 1,466 and `encoder.rs` is 1,007. The PR budget controls
  review batches but not file-level architecture.
- **Local integration state is worsening.** Before this record worktree was
  created, the repository had 86 worktrees, 113 local branches, 77 gone-
  upstream branches, and three dirty HTTP worktrees. The dirty state must be
  owner-audited rather than treated as disposable clutter.
- **The product remains early.** S4–S7 are still required for complete
  foundation acceptance, and none is needed as an excuse to delay #228.

## Priority from this checkpoint

The next ordering is operationally clear:

1. Make #228 the primary P0 lane and prove or kill the controlled-contract
   architecture from a cold Hello tree.
2. Permit at most one parallel S3 lane to finish #111 and prepare the eventual
   HTTP integration.
3. Do not resume broader S2 grammar or emitter expansion until #228's gate
   passes.
4. Do not treat S4 or S5 specification as a prerequisite for the generated
   Rust or Rust-plus-HTTP proof.
5. Reassess the critical path immediately after #228 passes or fires a kill
   criterion.

A cautious scope-weighted estimate at this checkpoint is approximately
**30–35% of v0**, with a wide error band. The increase reflects a completed S3
task, substantial T2 work, the Hello implementation, and a resolved S2
architecture gate. It does not credit the cold-tree proof before it exists.

## Bottom line

This was a very strong late-night delivery window. The coordinator obeyed the
new control policy, the model-backed loop sustained high throughput and useful
review, S3 gained real depth, and the S2 architecture question was resolved
with a concrete kill gate rather than paper confidence.

The project now has less ambiguity but not yet the integration proof that
matters most. The next meaningful progress event is not another parser or wire
slice: it is #228 generating, compiling, and invoking Hello from a cold tree —
or rejecting the architecture quickly if that cannot be done cleanly.
