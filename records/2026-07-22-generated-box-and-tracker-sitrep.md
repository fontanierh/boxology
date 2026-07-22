# Generated-Box Progress and Tracker-Integrity Situation Report

Record of the review held on 2026-07-22 between the maintainer and Codex,
following the request for a full Boxology situation report since the
[morning coordinator course correction](2026-07-22-morning-coordinator-course-correction.md).
The baseline was the 09:26 CEST checkpoint at `main` `098687a`, immediately
after PR [#247](https://github.com/fontanierh/boxology/pull/247). The refreshed
checkpoint is approximately 17:31 CEST at `origin/main`
`6ac05bacf1ddd5f31e47ba6bebf35ec003d0160f`, after PR
[#270](https://github.com/fontanierh/boxology/pull/270).

This is historical and operational analysis. It introduces no product,
architecture, stream-dependency, or normative process decision. Existing
specifications, tracker decisions, review gates, and the 400-line budget
remain authoritative.

## Executive assessment

The coordinator correction produced the intended critical-path movement. The
controlled-contract proof advanced from parser and cold discovery through a
generated boundary, stale-output rejection, implementation conformance,
domain-error ABI, and the canonical Hello schema/public revision. The
repository's main checks remain green.

The generated-box proof is still not complete. There is no generated
`ContractDescriptor`, generated adapter, registration/composition path, or
end-to-end generated `greet("Ada")` invocation. The checked-in Hello contract
fixture remains hand-written generated-style substrate; it is not evidence
that the generator has produced and run the complete product path.

The tracker incident around #228 was corrected during the review. GitHub
auto-closed the issue because PR #273 contained the negated phrase “does not
close #228”; the maintainer reopened it and recorded the reason. #228 is now
open again and correctly retains the remaining architecture-proof work.

## Checkpoint comparison

| Metric | 09:26 checkpoint | Refreshed checkpoint | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 142 | 165 | +23 |
| Closed unmerged PRs | 5 | 5 | unchanged |
| Open PRs | 0 | 2 | +2 |
| Issues closed/open | 46 / 54 | 46 / 56 | 0 / +2 |
| Tracked files | 102 | 128 | +26 |
| Rust lines | 30,209 | 35,839 | +5,630 |
| First-parent merges | 0 | 23 | +23 |

The interval's first-parent history contains the morning record, generator
PRs #250–#253, Telegram and delivery slices #254–#266, conformance and
delivery-process changes #260 and #263, CI cleanup #269, the domain-error ABI
#268, schema/revision #273, and the later Telegram merges #267 and #270. The
aggregate diff from the overnight checkpoint is 39 files, 6,549 insertions,
and 136 deletions.

At the refreshed checkpoint, exact-main run
[29932590234](https://github.com/fontanierh/boxology/actions/runs/29932590234)
passed all six protected jobs: Linux, macOS, deny, validation, and both
determinism consumers.

## Generated-box ladder

### Landed

- [#250](https://github.com/fontanierh/boxology/pull/250) established the
  controlled Hello parser and owned semantic model.
- [#251](https://github.com/fontanierh/boxology/pull/251) added cold-tree
  discovery, canonical semantic bytes, SHA-256 generation consistency, and
  deterministic diagnostics.
- [#252](https://github.com/fontanierh/boxology/pull/252) emitted the first
  deterministic in-memory contract-package artifact.
- [#253](https://github.com/fontanierh/boxology/pull/253) proved a real
  generated public boundary through the fixed dependency alias and facade,
  including stale-output compilation failure.
- [#260](https://github.com/fontanierh/boxology/pull/260) added ordinary
  `#[boxology::implementation]` conformance checking and the negative
  signature matrix.
- [#268](https://github.com/fontanierh/boxology/pull/268) added the model-derived
  `ContractType`/`ContractError` ABI, including opaque future-error
  preservation, tagging, forwarding, and redaction.
- [#273](https://github.com/fontanierh/boxology/pull/273) added the canonical
  Hello `schema.json` and separately frozen public contract revision, with
  exact projection, mutation, invariance, determinism, and missing-schema
  evidence.

The highest proven rung is:

```text
cold controlled contract
  -> shared semantic model/digest
  -> generated Rust boundary
  -> facade and stale-output rejection
  -> implementation conformance
  -> domain-error ABI
  -> canonical schema and public revision
```

### Still missing

The next load-bearing increment is generated descriptor material:

- generated `TypeDescriptor`, capability descriptor, and `ContractDescriptor`;
- generated dispatch, typed handle, and test-support surface;
- implementation-local adapter;
- registration and runtime composition;
- a complete generated `greet("Ada")` invocation;
- final purity/no-execution, cross-platform, editor, and full golden closure.

The current [Hello manifest](../crates/fixtures/hello/boxology.toml) declares
`generated/adapter/**` and `generated/schema.json`, but the checked-in fixture
does not yet contain those outputs. The existing
[generated contract source](../crates/fixtures/hello/generated/contract/src/lib.rs)
contains hand-written dispatch, handle, fake, and ABI code; it must not be
counted as generator-produced end-to-end evidence.

Issues #102–#109 remain open. In particular, #109 still records that S2 is not
complete, and #100/#101 retain the unfinished S1 fixture/completion work.

## Tracker correction for #228

PR #273's body explicitly said that it did not close #228, but GitHub's
closing-issue parser treated the negated sentence as a closing reference. The
issue timeline records the mistaken closure at 14:46:03Z, followed by the
maintainer's correction and reopen at 15:28:17Z.

The current tracker state is correct: #228 is open with the remaining
`ContractDescriptor`, dispatch, adapter, composition, runtime, and completion
requirements. The incident is recorded here so a future reviewer does not
mistake the transient closed state for an accepted proof completion.

The operational lesson is to avoid GitHub closing-keyword phrases, including
negated forms, in reconciliation comments and PR bodies when an issue must
remain open. This is guidance from the incident, not a new binding repository
rule.

## Telegram and delivery tooling

The parallel Telegram stack advanced substantially without being mistaken for
Boxology product-stream completion:

- [#267](https://github.com/fontanierh/boxology/pull/267) merged the bounds,
  redaction, failure-class, skill-schema, and monitoring-reference slice.
- [#270](https://github.com/fontanierh/boxology/pull/270) merged delivery
  structure, bounded retention, revocation/conflict hardening, and stale
  offset handling.
- [#271](https://github.com/fontanierh/boxology/pull/271) is ready but needs
  fresh exact-main checks after the #270 merge.
- [#274](https://github.com/fontanierh/boxology/pull/274) remains a dependent
  draft; its determinism checks were not yet terminal at the checkpoint.

Issue [#248](https://github.com/fontanierh/boxology/issues/248) remains open.
The implementation has explicit enablement, private pairing, durable state,
ordered receive, deduplicated send, structured asks, correlated replies,
listener support, and deterministic fake-API evidence. Human-authorized live
credential exchange remains pending. No credentials, Telegram enablement,
deployment, or live exchange was used.

## Other streams and operational state

No new S3 product implementation landed after the overnight baseline. S3-T1,
T2, and T4 remain closed; T3, T5, T6, and final completion remain open. The
freeze therefore held while the generator proof advanced.

S0 issue [#272](https://github.com/fontanierh/boxology/issues/272) remains open
for the isolated ARM64 self-hosted runner. No activation or completion
evidence exists.

The main checkout is clean and the current upstream tip is the #270 merge.
The wider repository still has substantial operational clutter: 117 worktrees
were visible at the checkpoint, including three unrelated dirty worktrees for
old S3 experiments and the CI-runner work. They were not cleaned or treated as
disposable.

## Assessment and next actions

The critical-path correction succeeded at the coordinator layer: focused S2
work moved, competing S3 expansion stopped, and the generated ladder gained
multiple falsifiable rungs. It did not yet establish the product-level proof
that matters most. The prior cautious 35–40% v0 estimate should not be raised
solely because schema/revision and Telegram slices landed; generated
descriptor-to-invocation evidence remains the dominant gap.

The next bounded sequence is:

1. Continue #228 through descriptors, adapter, composition, and generated
   `greet("Ada")`.
2. Keep S3 expansion frozen unless #228 exposes a real dependency.
3. Restack #271 and #274 sequentially after exact-main reconciliation; do not
   treat stacked checks as product proof.
4. Keep #248 open until the human-authorized live Telegram acceptance occurs.
5. Revisit the estimate and coordinator policy after the first complete
   generated invocation, or immediately if a #228 kill criterion fires.

This record preserves the evidence and analysis only. No normative document,
stream specification, issue dependency, or product scope is changed by the
record itself.
