# S2 Architecture Proof and V0 Reassessment

Record of the situation report held on 2026-07-23 between the maintainer and the
assisting agent (Claude Opus 4.8), following a request for a complete Boxology
status report and an account of how the project evolved since the last record.
The baseline is the refreshed checkpoint of the
[generated-box and tracker-integrity situation report](2026-07-22-generated-box-and-tracker-sitrep.md):
approximately 17:31 CEST at `main` `6ac05bacf1ddd5f31e47ba6bebf35ec003d0160f`,
after PR [#270](https://github.com/fontanierh/boxology/pull/270). The refreshed
checkpoint is approximately 15:40 CEST at `origin/main`
`a9d0b30ecec22f80cc76315e6a679b2edeae86ba`, after PR
[#299](https://github.com/fontanierh/boxology/pull/299).

This is historical and operational analysis. It introduces no product,
architecture, stream-dependency, or normative process decision. Existing
specifications, tracker decisions, review gates, and the 400-line budget remain
authoritative. The revised v0 estimate below is an assessment, not a normative
change; it binds nothing.

## Executive assessment

The dominant gap named in the previous record closed. That record put v0 at a
cautious 35–40% and identified the missing increment as generated
descriptor-to-invocation evidence: no generated `ContractDescriptor`, adapter,
dispatch, registration, or end-to-end generated `greet("Ada")` call, with the
checked-in Hello fixture explicitly discounted as hand-written substrate rather
than generator output.

Since then the S2 contract-generator **architecture proof landed and was
accepted.** Issue [#228](https://github.com/fontanierh/boxology/issues/228)
(`[S2-ARCH] Prove controlled contract syntax before emitter expansion`) was
closed as `COMPLETED` by the maintainer at 2026-07-23T12:38:43Z, with a full
AC7 verification table and the note that emitter and grammar expansion may
proceed. This is the first cleared architecture rung of the v0 long pole, and
unlike the schema/revision and Telegram slices reviewed in the previous record,
it is a falsifiable proof of the hardest single component rather than another
horizontal slice.

The repository's main checks remain green. The generator now emits and routes
real generated boundaries for the Hello vertical and its scalar generalization.
The remaining S2 work — broader grammar, non-default metadata, box-generic
naming, JSON diagnostics, orchestration, and the golden/completion tasks — is
known-shape emitter work rather than open architecture risk. The larger
outstanding v0 volume is the four unstarted greenfield streams S4–S7.

## Checkpoint comparison

| Metric | 2026-07-22 checkpoint (#270) | Refreshed checkpoint (#299) | Delta |
| --- | ---: | ---: | ---: |
| Merged PRs | 165 | 191 | +26 |
| Open PRs | 2 | 0 | −2 |
| Issues closed/open | 46 / 56 | 48 / 54 | +2 / −2 |
| Tracked files | 128 | 147 | +19 |
| Rust lines | 35,839 | 38,474 | +2,635 |
| First-parent merges | 0 | 26 | +26 |

The interval's aggregate diff from the baseline is 51 files, 4,552 insertions,
and 299 deletions. First-parent history over the interval contains the isolated
ARM64 self-hosted-runner bring-up (#277–#285), the previous record itself
(#276), the generator increments #286–#291, editor-tooling verification
#292–#293, the delivery-loop model change #295, and the scalar-grammar
expansion #296–#298. At the refreshed checkpoint, exact-main run
[30019468077](https://github.com/fontanierh/boxology/actions/runs/30019468077)
passed all protected jobs.

## The S2 architecture proof

The accepted proof (#228) establishes, on the Hello vertical slice, the full
path the previous record found missing:

```text
cold controlled contract
  -> shared semantic model/digest (boxology-contract-syntax)
  -> generated contract crate: types, ContractType/ContractError, ContractDescriptor
  -> generated dispatch trait, typed handle, test-support fake
  -> implementation-local adapter
  -> registration and in-process composition (boxology-runtime)
  -> generated greet("Ada") -> "Hello, Ada!" through Rust and through the adapter
```

The proof runs `generate()` on cold source, then real `cargo check`/`cargo run`
on the emitted crate and a consumer. It covers matching and deliberately
mismatching implementations, the sole-public-type and dependency-alias
invariants, semantic-digest staleness rejection, purity sentinels (no Cargo,
rustc, build script, user proc macro, implementation body, or initializer runs
during generation), cross-platform byte-identical output through the registered
determinism subject, and a pinned rust-analyzer/rustfmt editor gate (#292).

### Correction to the previous record's fixture caveat

The previous record cautioned that the checked-in
`crates/fixtures/hello/generated/` contract crate, `schema.json`, and adapter
were hand-written generated-style substrate that must not be counted as
generator-produced evidence. **As of the cold-generation proof (PR #289) and the
#228 closure, that caveat no longer holds:** those files are generator-produced
goldens, verified by a cold-generation test that runs `generate()` on only the
two owned inputs (`boxology.toml` and the implementation source, asserting no
generated files are fed back) and byte-compares the output against the
checked-in files under the provenance-normalization protocol. The only remaining
hand-written "generated-style" artifact in the tree is the unrelated
`crates/fixtures/generated-style-fmt/` formatting-gate fixture.

## Generator state and remaining S2 work

The T1 architecture gate, T2 schema and frozen-fingerprint projection, T3
descriptors and `ContractType` implementations, T4 dispatch/handle/test-support,
and T5 adapter and companion macros are implemented and green for the Hello
shape, and #296–#298 extended emission and routing to canonical scalar-leaf
boundaries beyond `String`. What remains in S2 is emitter generalization rather
than architecture:

- the broader D3 grammar — data structs and enums, containers
  (`Option`/`Vec`/`BTreeMap`/`Field`/sensitive types), multiple types, and
  multiple capabilities per contract;
- non-default exposure and idempotency emission (currently fixed at
  `external`/`none` despite the model carrying the real fields);
- box-generic naming (identifiers are still literally `Hello*`);
- machine-readable JSON diagnostics (D10/T6, text-only today);
- `GeneratedTree` boundary hardening with atomic-write orchestration (T6) and
  foreign-import handle hydration;
- the T8 golden suite, full determinism coverage, and the S2-vs-spec completion
  check.

Issues #102–#109 remain open and track exactly this work. #100/#101 retain the
S1 fixture and completion tasks; the S1 kernel crates are otherwise in place.

## Velocity trajectory

Development throughput stayed high and is now deliberately concentrating. Daily
first-parent merges were 33 on 07-19, 36 on 07-20, 33 on 07-21, a peak of 52 on
07-22, and 15 on 07-23 by mid-afternoon. The 07-23 reduction is a shift from
many small horizontal slices to a few deep, load-bearing generator increments,
each gated by real compile-and-run proofs, not a stall. The previous records'
repeated caution — that velocity can hide remaining scope and that the estimate
should not rise on slice count alone — held: the estimate moves now because a
falsifiable architecture rung cleared, not because more slices landed.

The delivery loop's spec, implement, and review roles were moved to Opus 4.8
(#295) during the interval.

## Other streams and operational state

- **S3 (HTTP binding)** stayed frozen; T3, T5, T6, and the completion check
  (#112, #114, #115, #116) remain open. No S3 product implementation landed.
- **S0** has one task open: the isolated ARM64 self-hosted runner
  ([#272](https://github.com/fontanierh/boxology/issues/272)); its bring-up PRs
  merged over the interval but the completion issue is not yet closed.
- **S4, S5, S6, S7** are unstarted: no crates and no tracker issues exist. The
  `boxology` CLI crate is a placeholder. These four streams are the majority of
  the remaining v0 scope and are not yet scoped in the tracker.
- **Telegram delivery tooling** advanced (durable-state validation, protected
  paths) but is coordinator infrastructure, not a v0 product stream. Issue
  [#248](https://github.com/fontanierh/boxology/issues/248) remains open pending
  human-authorized live credential exchange; none occurred.

The wider checkout still carries substantial operational clutter — well over a
hundred worktrees and local branches. It was not cleaned as part of this review.

## Assessment and next actions

The critical-path bet paid off: the hardest architecture risk in v0 — that a
deterministic, pure, one-type-facade contract generator with normal-rustc
implementation checking could be built at all — is now proven and accepted on
the Hello slice. A scope-weighted estimate at this checkpoint is approximately
**40–45% of v0**, a modest increase over the prior 35–40%. The increase reflects
risk retirement more than scope completion: S2's architecture is proven but its
emitters are not general, and half the streams (S4–S7) remain entirely
greenfield.

The next bounded sequence is:

1. Continue S2 emitter generalization through #102–#108: full grammar,
   non-default metadata, box-generic naming, JSON diagnostics, orchestration,
   and the T8 golden and completion closure.
2. Close out S1 fixtures and the S1-vs-spec completion check (#100, #101).
3. Scope S4–S7 in the tracker before beginning them, since none has issues yet;
   S4 (contract-change classification) is the most thesis-critical and depends
   only on S2's schema format.
4. Keep #248 open until human-authorized live Telegram acceptance occurs.
5. Re-baseline the estimate and coordinator policy again once the full grammar
   is general or once S4 exists.

This record preserves the evidence and analysis only. No normative document,
stream specification, issue dependency, or product scope is changed by the
record itself.
