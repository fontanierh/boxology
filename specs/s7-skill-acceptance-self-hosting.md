# S7 Spec — Skill, Acceptance, and Stage-2 Self-Hosting

[Stream definition](../boxology-details/11-v0-streams.md#s7--skill-acceptance-and-stage-2-self-hosting) ·
Status: **delivered; V0 complete**

S7 delivered the portable onboarding skill, one clean behavioral acceptance run, this repository's
stage-2 manifest adoption, and the friction log. The exact-main milestone boundary is preserved in
the [V0 completion record](../records/2026-08-09-v0-completion-evidence.md).

## Boundary

S7 did not build an agent, harness certification matrix, new validation layer, factory, or generic
development CLI binding. It consumed S1–S6 as delivered. Harness-specific wrappers, richer
onboarding, stage-3 tool boxification, and pinned-prior-release generator self-validation remain
post-V0 work.

## Delivered decisions

### D1 — Portable onboarding skill

The product skill is `.agents/skills/boxology/SKILL.md` in shared Agent Skills format. It teaches
the box model, boundaries, compatible contract evolution, five-step greenfield flow, and names the
hosting agent the lead. It uses source-checkout installation, the explicit initializer interface,
the first Cargo build that creates `Cargo.lock`, and `boxology check`. Its trigger is limited to
managed-project onboarding, so it does not govern development of Boxology itself.

Portability is a content property: the skill has no host-specific instructions. Additional-harness
runs are useful evidence, not a milestone gate.

### D2 — The skill's acceptance contract is behavioral

The skill is accepted behaviorally through the
[foundation runbook](../ops/s7-foundation-acceptance-runbook.md), with the skill as the only
Boxology guidance. The developer may provide scripted task asks and answer anticipated project,
target, and harness choices. Any utterance that supplies a file, command, procedure, or diagnosis
is an intervention; an intervened run is recorded as failed rather than repaired invisibly.

### D3 — The acceptance run and its evidence protocol

One clean run gates the milestone. Its record identifies the commit and greenfield state, both
`boxology check` runs, the real additive `greet(name)` classification, the permitted file-change
boundary, and Rust/HTTP `Hello, Ada!` transcripts. Failed attempts remain evidence.

### D4 — Stage-2 repository adoption

Root `boxology.toml` manifests classify every tracked file exactly once. The irreducible runtime
core and all other repository packages are platform-kind at this rung; fixture projects are opaque
owned data and their nested manifests do not enter root discovery. Fixture-generated trees are
declared by their own manifests rather than duplicated in the root. Root `Cargo.lock` is the
root-derived artifact.

The root manifests declare CI and xtask as protected control-plane paths. That declaration reports
ownership; it does not make candidate-writable policy immutable. Human review remains the current
control and [#17](https://github.com/fontanierh/boxology/issues/17) owns stronger semantic
self-protection.

Stage-2 product proof is the adopted manifests plus the complete exact-main native-Mac check cited
by the completion record. Required PR CI currently runs zero product commands; it provides lean
hygiene and changed-scope evidence instead. Continuous Linux/cross-platform proof is not claimed
and remains [#525](https://github.com/fontanierh/boxology/issues/525) scope.

### D5 — The absorption, immediately post-v0

PR #571 completed [#342](https://github.com/fontanierh/boxology/issues/342). Every canonical
`cargo xtask ci` aggregate now owns exactly one `boxology check`; xtask retains only distinctly
named repository semantics the product baseline does not cover. The bootstrap derived-output and
format-selection registries are deleted and their selection is manifest-derived. The required PR
lane stays product-free because full checks exceeded its time budget.

### D6 — Friction remains durable evidence

`ops/friction-log.md` records each discipline relaxation as `mechanical` automatable toil or
`semantic` thesis damage. Existing entries are immutable except for permitted appended status
annotations, enforced through the records machinery. Dated analyses live in `records/`; neither
history is rewritten when the normative baseline changes.

### D7 — S7-COMPLETE is the v0 gate

The [clean acceptance record](../records/2026-08-03-foundation-acceptance-clean.md) shows both
invocations returned `Hello, Ada!`, `ping.greet` classified additive, and no foreign package source
changed. The earlier
[classification failure](../records/2026-08-03-foundation-acceptance-failed.md) and
[skill-unavailable failure](../records/2026-08-03-foundation-acceptance-skill-unavailable-failed.md)
remain failed-run evidence. Stream audits and the residual ledger culminated in S7-COMPLETE and
the V0 record; invalidating required evidence would invalidate that completion claim.

## Delivered acceptance criteria

1. The scoped portable skill exists and passes its content audit.
2. One unintervened run satisfies the full behavioral protocol; prior failures are preserved.
3. Root manifests classify the repository with fixture opacity and a green exact-main product
   check in the milestone evidence.
4. The categorized friction log is append-only and contributor guidance points to it.
5. #342 absorption is complete without a duplicated or silently dropped platform check.

## Live residuals

- Stage-3 Telegram/tool/factory dogfood is sequenced by
  [#572](https://github.com/fontanierh/boxology/issues/572) and
  [#74](https://github.com/fontanierh/boxology/issues/74).
- Skill distribution, host wrappers, optional checkpoints, richer onboarding, and harness
  certification remain outside V0.
- Pinned-prior-release generator validation becomes mandatory at the first release boundary.
- Detecting that a relaxation omitted a friction entry remains a human-review responsibility;
  append-only integrity, not entry completeness, is mechanical.
