# Generated-Box Critical-Path Review

Record of the review held on 2026-07-21 between the maintainer and Codex after
the [overnight v0 situation report](2026-07-21-overnight-v0-sitrep.md). The
review refreshed repository progress, examined the early effect of the
delivery-loop model change, and established an operational priority for the
maintainer's running coordinator: reduce time to the first generated,
compiled, runnable Hello box without weakening accepted repository gates.

This is historical and operational analysis. It does not amend a stream spec,
change an issue dependency, or make the coordinator prompt a normative
repository rule. Existing instructions, accepted specifications, tracker
decisions, review requirements, and the 400-hand-authored-line limit remain
authoritative.

## Refreshed situation

The refreshed checkpoint was 2026-07-21 16:59 CEST at `main`
`19b1b47ae8e240c8f5c72686a0c981467411be41`, after PR
[#211](https://github.com/fontanierh/boxology/pull/211). Local `main` and
`origin/main` were synchronized, no pull request was open, and all 70
worktrees were clean. The latest exact-main
[run 29841098365](https://github.com/fontanierh/boxology/actions/runs/29841098365)
passed all six jobs in approximately 89 seconds.

Relative to the 10:12 CEST checkpoint preserved in the overnight record:

| Metric | 10:12 checkpoint | Refreshed checkpoint | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 98 | 107 | +9 |
| Issues | 43 closed, 56 open | 43 closed, 56 open | unchanged |
| Tracked files | 88 | 92 | +4 |
| Rust lines | 18,949 | 21,081 | +2,132 |
| Tests including doctests | 258 | 280 | +22 |
| Worktrees | 62 | 70 | +8 |
| Local branches | 89 | 97 | +8 |
| Gone-upstream branches | 62 | 69 | +7 |

All 20 workflow runs beginning after the earlier checkpoint succeeded. The
nine merges were the overnight record, the delivery-loop configuration change,
and seven product changes. PRs
[#204](https://github.com/fontanierh/boxology/pull/204)–[#207](https://github.com/fontanierh/boxology/pull/207)
and [#211](https://github.com/fontanierh/boxology/pull/211) advanced S2-T1
through contract shapes, documentation, member identities, contract placement,
and capability placement. PR [#209](https://github.com/fontanierh/boxology/pull/209)
added the schema-independent programmable Hello fake. PR
[#210](https://github.com/fontanierh/boxology/pull/210) began S3 implementation
with its lossless JSON syntax layer.

The repository was materially further along, but tracker closure had not
moved. S2-T1 remained open after approximately sixteen slices, S1-T7 remained
incomplete, and there was still no emitted, compiled, end-to-end generated
box. A cautious scope-weighted estimate moved only modestly, from approximately
25–30% to approximately 27–32% of v0, with a wide error band.

## Delivery-loop model review

PR [#208](https://github.com/fontanierh/boxology/pull/208) replaced the
configured Fable-first specification and review routes with direct Codex
routes. The resulting profile was:

- specification: GPT-5.6 Sol at `xhigh`;
- implementation and repair: GPT-5.6 Luna at `max`; and
- review: a fresh GPT-5.6 Sol worker at `high`.

The change removed Fable attempts that had consistently ended in explicit
usage exhaustion. Operationally, it made the configured path match the path
that was actually available and removed predictable retry overhead. It also
moved implementation and repair from Sol `high` to Luna `max`, while reducing
the fallback review effort from Sol `xhigh` to direct Sol `high`.

For the small comparable sample, PR-open-to-merge time averaged approximately
10 minutes 22 seconds for the clean pre-change cohort #204–#206 and 7 minutes
42 seconds for the clean post-change cohort #209–#211, an apparent improvement
of about 26%. That measure excludes specification and implementation before PR
creation, and the post-change cohort included parallel work and tasks of
different complexity, so it is evidence of improved review/CI/merge throughput
rather than a complete model-speed benchmark.

There was no observed quality regression. #209 and #210 passed their first
fresh reviews cleanly. #211 required three review-and-repair cycles, which
caught nested traversal, payload-opacity, unreachable-owner, and regression-
evidence problems before merge. All refreshed CI runs passed and the test
inventory grew. The sample was too small and confounded to conclude that Luna
was categorically higher quality or cheaper. The profile also removed nominal
cross-vendor diversity, although Fable's repeated exhaustion meant that
diversity had already been unavailable in practice.

## The prioritization problem

The coordinator's apparent S1, S2, and S3 lanes were individually sensible,
but broad progress did not minimize time to a generated box. Merge count had
become an increasingly poor proxy for the first falsifiable integration proof:
seven product PRs landed in the interval without completing an issue or
advancing an emitted artifact through compilation and invocation.

The accepted task graph already contained the required path. The missing
ingredient was an explicit selection objective. The maintainer accepted a
temporary operational priority that rewards movement of the lowest unmet
generated-box proof rather than balanced stream coverage, issue fairness,
worker utilization, or PR volume.

## Accepted operational direction

The immediate objective supplied to the running coordinator was defined as the
**generated-box proof**, distinct from complete foundation acceptance:

1. parse the Hello authoring source into the complete semantic model;
2. invoke the S2 `GenerationRequest` to `GeneratedTree` API;
3. emit the Hello contract crate, schema, revision, and adapter;
4. compile the emitted artifacts;
5. register the emitted adapter through S1 composition;
6. return exactly `Hello, Ada!` from `greet("Ada")` through the generated Rust
   path; and
7. prove deterministic byte equality against the accepted Hello goldens.

The next objective is to carry that same generated box through S3 over HTTP.
The complete foundation milestone remains later: S4 supplies compatibility
classification, S5 supplies `boxology generate` and `boxology check`, S6 owns
the installer and generated project, and S7 owns the skill-driven acceptance
run.

The coordinator was directed to rank candidate work as:

- **P0:** directly advances the lowest unmet generated-box proof;
- **P1:** directly removes its next blocker;
- **P2:** advances the generated box's HTTP path without consuming a needed
  P0/P1 lane; and
- **P3:** breadth, cleanup, refactoring, or downstream preparation.

The coordinator must state the current proof rung, exact blocker, proposed
proof delta, newly unblocked work, independence from active lanes, and expected
review budget before accepting a slice. An idle lane is preferable to work
that delays or conflicts with the critical path.

## Existing critical path and concurrency

No new S4 or S5 specification is required for the immediate proof. S2 already
specifies direct library invocation, emission, deterministic output, and the
compile-and-run golden. S3 already specifies the HTTP path and may work against
the hand-written S1 fixtures until generated fixtures are available.

The accepted dependency path was restated, not changed:

```text
#102 complete semantic model
  -> #103 schema and frozen fingerprint projection ----> format-dependent #100
  -> #104 contract-crate emission ----------------------> #105 and #106
  -> #107 final GenerationRequest/GeneratedTree API

#100 plus #103–#107
  -> #108 byte-equal generation, determinism, and compile/run proof
  -> generated-fixture integration through S3 HTTP
```

While #102 remains open, one implementation lane should continuously advance
it, one may advance dependency-correct schema-independent #100 work, and at
most one may advance S3. Once #102 closes, #103, #104, and #107 are the
preferred parallel lanes. Integration should begin at the earliest accepted
point rather than waiting for every emitter to become aesthetically complete.

S4 should not be specified before #103 freezes the schema and fingerprint
format merely to create activity. S5's manifest and ownership portion is
theoretically parallelizable, but it does not accelerate the generated-box
proof and should not consume a critical implementation lane.

## Guard against continued horizontal expansion

The coordinator received an explicit revisit trigger for the repeatedly light
S2-T1 estimate. If three further #102 implementation PRs merge after the
directive without either closing #102 or producing a credible bounded closing
stack, it must pause new breadth and prepare a sequencing proposal for the
maintainer.

That proposal may recommend a minimal fail-closed Hello emitter before the full
grammar is complete, but it may not silently bypass the current dependency.
It must identify the exact dependency change, reject unsupported constructs,
avoid false completion claims, reconcile #102–#108, explain learning value and
churn risk, and wait for explicit human acceptance.

## What did not change

The conversation changed coordinator priority, not repository authority:

- no accepted task or stream dependency was amended;
- no S2, S3, S4, or S5 requirement was narrowed;
- no review, CI, determinism, line-budget, or tracker gate was relaxed;
- no previously undecided product policy was selected; and
- no S4/S5 work was declared unnecessary for complete foundation acceptance.

The priority should be revisited when #102 closes, when the first generated
Rust proof lands, if the three-PR expansion guard fires, or if broader model
telemetry contradicts the early velocity and quality observations.

## Bottom line

The repository had strong throughput and healthy quality evidence, and the
new model profile removed real operational waste. The remaining concern was
that small-slice velocity could continue producing horizontal foundation work
without confronting integration. The accepted coordinator direction makes the
first generated, compiled, invoked Hello box the optimization target while
preserving every existing correctness and authority gate.
