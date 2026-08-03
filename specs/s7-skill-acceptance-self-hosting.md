# S7 Spec — Skill, Acceptance, and Stage-2 Self-Hosting

[Stream definition](../boxology-details/11-v0-streams.md#s7--skill-acceptance-and-stage-2-self-hosting) · Status: **accepted** (amended by maintainer decision on 2026-08-03)

S7 delivers the three closing pieces of v0: the portable onboarding skill, the end-to-end acceptance run of the foundation milestone, and stage 2 of the self-hosting ladder on this repository. The skill's product role and the milestone scenario are normative in the [Product Contract](../boxology-details/07-product-contract.md); the ladder and the dogfooding pain discriminator in the [Strategy Review](../boxology-details/10-strategy-review.md); the bootstrap-absorption contract in S0 D10. This spec resolves what those documents delegate: the skill's content boundaries and acceptance contract (declared unspecified by AGENTS.md until now — reconciled in this diff), the acceptance-run evidence protocol, the concrete form of stage-2 adoption, and the explicit immediate-post-v0 absorption boundary.

## Purpose

S7 is where v0 stops being claims. The skill is the product's only user interface; the acceptance run is the milestone's falsification test; stage-2 adoption is the first self-hosting evidence that costs something. All three produce recorded evidence rather than assertions, and the friction they generate is data under the pre-decided discriminator — mechanical friction is the factory's future job, semantic friction is thesis damage, and both get written down.

## Non-goals

- **No factory behavior in the skill.** Per the product contract: no GitHub Issues, task pickup, worker/review/merger roles, pull-request policy, or autonomous-merging guidance. The skill teaches the model and names the lead; the harness and operator keep their ordinary workflow.
- **No host-specific packaging or distribution.** The skill ships as a local file in the shared Agent Skills format; per-host wrappers and distribution channels remain the product contract's open question, post-v0.
- **No stage 3.** Boxifying the tools is the first rung of #74, deliberately after v0; this stream must not blur into it. Stage-2 adoption keeps every discovered package platform-kind.
- **No new validation machinery.** Stage 2 adopts S5's `boxology check` as-is; gaps it exposes are S5 findings, not S7 workarounds.
- **No harness validation.** Boxology ships a skill, not an agent; there is nothing to certify. The skill's portability is a **content property** — shared format, no host-specific sections — enforced by the T1 content audit, not by a run matrix. The milestone gate is the product contract's scenario, once; runs with additional agents are welcome evidence, never gates.

## Decisions

### D1 — The skill: scope, location, and shape

The portable skill lives at **`.agents/skills/boxology/SKILL.md`** in the shared Agent Skills format, alongside but distinct from the repository's internal skills (which document how this repository is developed, not the product). Its content is exactly the product contract's list — philosophy, box boundaries, contracts and compatible evolution, the way of working, the five-step onboarding flow, and naming the hosting agent the **lead agent** — plus the operational knowledge the flow requires: obtaining and running `boxology-init` from a source checkout, the first-build lockfile step, and validating with `boxology check`. It stays intentionally small; a topic that needs more than the skill can carry belongs to the generated project's README (S6) or does not belong in v0. The minimal getting-started material permitted by the recorded v0 exclusions is the skill plus that README; S7 adds no separate documentation site or tutorial.

The skill is versioned with the repository and carries no host-specific sections; anything a specific harness needs beyond the shared format is out of scope by the non-goals. Because it lives in the directory this repository's own contributor agents load skills from, its frontmatter **scopes its trigger to managed-project onboarding** — it is a product artifact, not guidance for developing this repository — and the T1 content audit asserts that scoping.

### D2 — The skill's acceptance contract is behavioral

Guidance cannot be unit-tested; it can be falsified. The skill's acceptance is therefore the **acceptance run** (D3) executed with the skill as the only Boxology *guidance* given to the agent: no repository-internal knowledge, no mid-run skill edits. The falsifiability discriminator is fixed here, not in the runbook: the developer role may utter the scripted task asks (which necessarily use product vocabulary — that is the *what*) and answer the questions the onboarding flow anticipates (project name, target directory, harness-native choices); an **intervention** is any developer utterance conveying *how* — a file, command, procedure, or diagnosis. An intervened run is recorded and the skill is defective until a revision passes cleanly. This is the same fail-honest posture the platform applies to code.

### D3 — The acceptance run and its evidence protocol

The run executes milestone steps 1–7 verbatim, with the maintainer in the developer role following a written runbook whose exact ask wording is T3's task spec (the discriminator itself is fixed by D2). The skill also owns the run's precondition: guiding the developer's agent to an empty target (S6 D2's fail-closed rule). **One clean run gates the milestone** — exactly the product contract's own success definition. Runs with additional agents are welcome evidence recorded the same way, never gates.

Evidence is a dated record in `records/` per run, citing — by commit hash, command, and trimmed excerpt, so the record fits the 600-line budget — the greenfield repository state before and after; the `boxology check` output at steps 3 and 7; the S4 classification of the real `greet(name)` change (expected additive); confirmation that no foreign package source changed and only permitted deterministic artifacts exist outside the Hello box; and both invocation transcripts (Rust and HTTP) returning `Hello, Ada!`. This real evolution is the sole fixture-pair/classification acceptance proof; S6 does not pre-validate it synthetically. A failed or intervened run is recorded with the same fidelity — a failed acceptance record is evidence, not embarrassment — and the milestone is complete only when a clean run exists.

### D4 — Stage-2 adoption: manifests and check on this repository

The product source repository adopts `boxology.toml` manifests and `boxology check`:

- Every **discovered** package is **platform-kind**; the runtime core is definitionally non-box and everything else remains platform-kind until stage 3. The package partition (one platform package or several) is T4's task-spec decision, constrained by honesty: every tracked file — including `records/`, `specs/`, `boxology-details/`, and `ops/` — classifies exactly once under D3 of the S5 spec.
- Fixture projects are kept out of workspace validation by S5 D2's **fixture-opacity declaration**: their subtrees are a platform package's owned fixture data, their `boxology.toml` files (including deliberately malformed corpus entries and duplicate ids) invisible to discovery. Their Cargo crates leave the root workspace's membership — each fixture is its own Cargo project, which is what S6's installer output naturally is — with platform test crates reaching them by path dependency or process-level `cargo` invocation; the migration mechanics are T4 scope coordinated with #100. Edge policy applies to discovered packages' roles; fixture crates have none and are reachable only from platform-owned test crates. A fixture's `generated/` tree is fixture-owned data under that opacity and is **not** a platform package's derived output — amending the stage-2 decision that said otherwise (#463). It is not expressible: `plan()` requires a generation candidate to declare exactly one `box-implementation` crate (`BXW0067`), a role a platform package cannot host (`BXW0054`), so the only entry that parses is `generator = "cargo"`, which would classify the files while stating something untrue about how they are produced. The fixture's own manifest declares them derived; the root delegates rather than duplicating the claim. The cost is recorded on #329: the root `Cargo.lock` remains the single root-derived artifact, and fixture trees never enter the derived-output registry that D5's absorption deletes.
- The T4 manifests declare the repository's CI workflow and `crates/xtask` as **protected control-plane paths** (02-packages). V0 `check` reports; with no merger, S0 non-goal 4's human-review posture stands until one exists — resolved explicitly, not silently.
- `boxology check` runs green on this repository as part of its PR validation on both platforms; the `pr.yml` changes this needs (including the macOS lane, which today runs only the determinism command) are named T4/T5 scope.

### D5 — The absorption, immediately post-v0

S0 D10's xtask absorption is deliberately deferred to **the first task immediately after v0**, tracked as #342. V0 retains the adopted root manifests and requires `boxology check` in repository PR validation; the existing xtask registries may remain as a temporary, explicitly named duplication until #342 lands. That bounded deferral cannot remove or bypass `boxology check`, classification evidence, or any retained repository check.

At #342 completion, `cargo xtask ci` delegates platform validation — ownership, edges, regeneration, classification — to `boxology check`, and every duplicated bootstrap registry — the derived-output exclusion list and the hand-authored formatting package-selection lists — is replaced by manifest-derived data, with the xtask copies deleted in the same change. The retention rule is subsumption, not a short list: **xtask retains, under clearly separate names, every current `ci` check whose semantics `boxology check`'s baseline does not cover** — links, budget, records, `cargo-deny`, doc tests, whitespace, the rust-analyzer probe, and the determinism subjects with their meta-tests. A check that exists in both layers after absorption is a defect, and a check that exists in neither is a silent drop — also a defect.

### D6 — The friction log is instituted first

A standing friction log is created at **`ops/friction-log.md`** before stage-2 adoption begins, so adoption friction is captured from its first minute. Each entry is dated and classified **`mechanical`** (automatable toil — the factory's future job, evidence for continuing) or **`semantic`** (fighting the box boundaries themselves — thesis damage), per the discriminator the strategy review fixed in advance. Every relaxation of the discipline lands with a log entry; an uncategorized relaxation is a process violation. The log is deliberately lighter than `records/` (which stays the home of dated analyses), but its integrity is mechanical, not conventional: **merged entries are immutable — the only permitted mutations are appending new entries and appending a status annotation to an existing one — enforced by extending the existing `cargo xtask records` merge-base machinery to this one path** (T2 scope; pointing existing enforcement at one more file, not new validation machinery). A `semantic` entry cannot be quietly edited into a `mechanical` one. A periodic record summarizes and cites the log. The AGENTS.md pointer instituting the obligation lands in T2's PR, since the spec binds the stream but the workflow document binds contributors.

### D7 — S7-COMPLETE is the v0 gate

The stream's completion check verifies this spec and that every stream S0–S6 has a closed evidence audit through its completion check. Every acceptance criterion in an accepted S0–S6 spec must be satisfied by cited evidence or explicitly re-scoped by a merged normative amendment; an open issue, closing comment, or residual-ledger entry is not itself a re-scope. Open stream-labelled issues do not mechanically block v0 only when the accepted spec already marks their remaining work excluded, deferred, or otherwise non-gating, and the closing audit explicitly names each residual, states the normative basis for that status, and records its post-v0 order or dependency. Unresolved required evidence — including, but not limited to, a born-valid run, clean acceptance run, `boxology check`, determinism, or classification proof — cannot be converted into residual debt. #342 is the first mandatory post-v0 task; any other qualifying residual is named beside it rather than hidden behind a zero-open-issue assertion.

S7-COMPLETE closing **is** the v0 milestone declaration; its dated record cites the evidence chain and the residual ledger. If evidence underlying a closed stream completion check is invalidated, S7-COMPLETE reopens with it. The ladder's stage-2 text also names pinned-prior-release generator self-validation; that is impossible before a first pinned release exists (the recorded v0 exclusions publish nothing) and is **deferred to the first-release boundary under #74's first rung — recorded here so S7-COMPLETE's stage-2 claim is scoped, not overstated**.

## Acceptance criteria

1. The skill exists at the fixed path in the shared format, containing exactly the D1 scope; a content audit confirms no factory-behavior, host-specific, or post-v0 material, and asserts the managed-project trigger scoping.
2. One clean acceptance run with a dated record satisfying the full D3 evidence protocol, including the additive S4 classification of the `greet(name)` change and both `Hello, Ada!` transcripts; any additional-agent runs are recorded under the same protocol as non-gating evidence.
3. Any intervened or failed run before the clean one is recorded with the same protocol.
4. Every tracked file in this repository classifies exactly once under the adopted manifests, with fixture subtrees opaque per S5 D2; `boxology check` is green on this repository in PR validation on both platforms.
5. The v0 record names #342 as the first mandatory post-v0 task. #342's own completion requires that no platform check exists in both xtask and `boxology check`, and none exists in neither: the full retained set of D5 (links, budget, records, deny, doc tests, whitespace, analyzer probe, determinism subjects and meta-tests) runs under its own names, and the deleted xtask registries are demonstrably manifest-derived.
6. `ops/friction-log.md` exists before the first stage-2 adoption PR merges under the extended append-only enforcement; every discipline relaxation in the stream's history has a categorized entry; the AGENTS.md obligation is in place.
7. S7-COMPLETE's closing comment maps every criterion to evidence or its merged normative re-scope, confirms every stream S0–S6 has a closed evidence audit, and names and orders every residual follow-up with the accepted exclusion, deferral, or other non-gating basis. The examples protected in D7 are non-exhaustive: any unsatisfied accepted criterion remains blocking. The comment also scopes the stage-2 claim per D7 and cites the v0 record.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | The portable skill: content per D1, fixed path, content-audit test | 1–2 |
| T2 | Friction log institution: `ops/friction-log.md`, entry format, AGENTS.md obligation | 1 |
| T3 | Acceptance runbook + the recorded gating run (each run's record is its own PR; additional-agent runs optional evidence) | 1 + records |
| T4 | Stage-2 adoption: repository manifests, package partition, `check` green in PR validation | 2–3 |
| T5 | Immediate post-v0 absorption (#342): xtask delegation, registry deletion, name separation | 1–2 |

T2 lands first for stage-2 adoption's sake (D6); T1 may proceed immediately and in parallel; T3 follows T1 and the finished S6 installer — it does not depend on T4/T5, since the acceptance run happens in a greenfield repository. T4 and the clean T3 run precede S7-COMPLETE, which closes the stream and v0 per D7. T5/#342 then runs first immediately post-v0, before stage-3 tool boxification. Dependencies: T3 consumes S6 in full (#332–#336), S4's classifier, and S6's initial-contract shape (#333); T4/T5 consume S5's `check` (#327) and the fixture-opacity mechanism; the whole stream depends on S0–S6.

## Matters left open

- Skill distribution, host-specific wrappers, and any certification of compatible harnesses — the product contract's open question, post-v0.
- Stage 3 (tool boxification) — the first rung of #74, immediately after v0.
- Xtask absorption (#342) — the first mandatory post-v0 task, sequenced before stage-3 tool boxification.
- The optional lead-authored checkpoint and richer onboarding documentation — open questions in the product contract, untouched.
- Friction-log *entry-completeness* enforcement (detecting a relaxation that skipped the log) — human review; only the log's append-only integrity is mechanical (D6).
- Pinned-prior-release generator self-validation — deferred to the first-release boundary under #74's first rung (D7); the friction log's early entries are stage 3's input.

## Tracker notes

Normative reconciliation in this PR's diff: the stream definition in `11-v0-streams.md` gains its spec link, and AGENTS.md's delivery-method note — which declared the skill's product guidance and acceptance contract unspecified until this spec existed — now points here. On merge, the operator files the five task issues plus S7-COMPLETE with `stream:s7` labels, recording the dependencies above. #74 gains no new obligation but its first-rung input (the friction log) is now scheduled; #57 and the product contract's open questions are cited unchanged.

**Amendment of 2026-07-24** (one independent review round and a maintainer decision): the two-harness protocol is removed — the milestone gate is the contract's scenario, once, with portability as a content property (maintainer decision; the prior text also misattributed the two-run rule to the product contract); the D2 intervention discriminator is fixed at spec level; D4 gains the fixture-opacity mechanism (S5 amendment), the protected control-plane declaration resolving S0 non-goal 4 explicitly, and the named `pr.yml`/macOS-lane scope; D5's retention becomes a subsumption rule with the full retained-check set; D6's log gains mechanical append-only enforcement via the extended records machinery; D7's gate closes the closed-completion-check loophole (#272) and scopes the stage-2 claim around the deferred pinned-prior-release validation; T3's ordering matches its real dependencies; evidence records are budget-bounded by citation. Operator issue edits on merge of this amendment: #340 (single gating run, wording), #338 (drop the #339 blocker; T1 starts immediately).

**Amendment of 2026-08-03** (maintainer acceleration decision): the real #340 `greet(name)` evolution becomes the sole additive-classification proof; #342's xtask absorption moves to the first task immediately post-v0 while root manifests and `boxology check` remain gating; and S7-COMPLETE accepts closed stream evidence audits plus an explicit residual ledger instead of literal zero-open tracker debt. Required behavioral, classification, check, and determinism evidence cannot be deferred. Operator reconciliation updates #335, #336, #340, #342, and #343.
