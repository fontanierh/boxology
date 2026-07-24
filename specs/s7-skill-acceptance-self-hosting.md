# S7 Spec — Skill, Acceptance, and Stage-2 Self-Hosting

[Stream definition](../boxology-details/11-v0-streams.md#s7--skill-acceptance-and-stage-2-self-hosting) · Status: **proposed** (awaiting review)

S7 delivers the three closing pieces of v0: the portable onboarding skill, the end-to-end acceptance run of the foundation milestone, and stage 2 of the self-hosting ladder on this repository. The skill's product role and the milestone scenario are normative in the [Product Contract](../boxology-details/07-product-contract.md); the ladder and the dogfooding pain discriminator in the [Strategy Review](../boxology-details/10-strategy-review.md); the absorption contract in S0 D10. This spec resolves what those documents delegate: the skill's content boundaries and acceptance contract (declared unspecified by AGENTS.md until now — reconciled in this diff), the acceptance-run evidence protocol, and the concrete form of stage-2 adoption.

## Purpose

S7 is where v0 stops being claims. The skill is the product's only user interface; the acceptance run is the milestone's falsification test; stage-2 adoption is the first self-hosting evidence that costs something. All three produce recorded evidence rather than assertions, and the friction they generate is data under the pre-decided discriminator — mechanical friction is the factory's future job, semantic friction is thesis damage, and both get written down.

## Non-goals

- **No factory behavior in the skill.** Per the product contract: no GitHub Issues, task pickup, worker/review/merger roles, pull-request policy, or autonomous-merging guidance. The skill teaches the model and names the lead; the harness and operator keep their ordinary workflow.
- **No host-specific packaging or distribution.** The skill ships as a local file in the shared Agent Skills format; per-host wrappers and distribution channels remain the product contract's open question, post-v0.
- **No stage 3.** Boxifying the tools is the first rung of #74, deliberately after v0; this stream must not blur into it. Stage-2 adoption keeps every crate a platform-kind package.
- **No new validation machinery.** Stage 2 adopts S5's `boxology check` as-is; gaps it exposes are S5 findings, not S7 workarounds.
- **No harness conformance program.** Two recorded acceptance runs demonstrate portability; a certification matrix for "compatible hosts" is post-v0.

## Decisions

### D1 — The skill: scope, location, and shape

The portable skill lives at **`.agents/skills/boxology/SKILL.md`** in the shared Agent Skills format, alongside but distinct from the repository's internal skills (which document how this repository is developed, not the product). Its content is exactly the product contract's list — philosophy, box boundaries, contracts and compatible evolution, the way of working, the five-step onboarding flow, and naming the hosting agent the **lead agent** — plus the operational knowledge the flow requires: obtaining and running `boxology-init` from a source checkout, the first-build lockfile step, and validating with `boxology check`. It stays intentionally small; a topic that needs more than the skill can carry belongs to the generated project's README (S6) or does not belong in v0. The minimal getting-started material permitted by the recorded v0 exclusions is the skill plus that README; S7 adds no separate documentation site or tutorial.

The skill is versioned with the repository and carries no host-specific sections; anything a specific harness needs beyond the shared format is out of scope by the non-goals.

### D2 — The skill's acceptance contract is behavioral

Guidance cannot be unit-tested; it can be falsified. The skill's acceptance is therefore the **acceptance run** (D3) executed with the skill as the only Boxology-specific input to the agent: no coaching beyond the developer role's scripted asks, no repository-internal knowledge, no mid-run skill edits. If the run needs an intervention, the intervention is recorded and the skill is defective until a revision passes cleanly. This is the same fail-honest posture the platform applies to code.

### D3 — The acceptance run and its evidence protocol

The run executes milestone steps 1–7 verbatim, with the maintainer in the developer role following a written runbook (fixed in T3's task spec: the exact asks, including the `greet("Ada")` request; what may be answered; what may not). It is performed **twice, on two different skill-compatible harnesses**, making harness-neutrality demonstrated rather than permitted, per the product contract's own standard.

Evidence is a dated record in `records/` per run, citing: the greenfield repository state before and after; the `boxology check` output at steps 3 and 7; the S4 classification of the `greet(name)` change (expected additive, matching S6 AC7); confirmation that no foreign package source changed and only permitted deterministic artifacts exist outside the Hello box; and both invocation transcripts (Rust and HTTP) returning `Hello, Ada!`. A failed or intervened run is recorded with the same fidelity — a failed acceptance record is evidence, not embarrassment — and the milestone is complete only when a clean run exists on each harness.

### D4 — Stage-2 adoption: manifests and check on this repository

The product source repository adopts `boxology.toml` manifests and `boxology check`:

- Every workspace crate is owned by a **platform-kind** logical package; the runtime core is definitionally non-box and everything else remains platform-kind until stage 3. The package partition (one platform package or several) is T4's task-spec decision, constrained by honesty: every tracked file — including `records/`, `specs/`, `boxology-details/`, and `ops/` — classifies exactly once under D3 of the S5 spec.
- The repository's fixture projects keep their existing box-shaped manifests; they are test data, not stage-2 subjects.
- `boxology check` runs green on this repository as part of its PR validation.

### D5 — The absorption, executed

S0 D10 is executed here, as that spec planned: `cargo xtask ci` delegates platform validation — ownership, edges, regeneration, classification — to `boxology check`; xtask retains only repository-specific checks (links, budget, records, determinism meta-tests) under clearly separate names; and every bootstrap registry xtask duplicated — the derived-output exclusion list and the hand-authored formatting package-selection lists — is replaced by manifest-derived data, with the xtask copies deleted in the same change. Manifests are authoritative from this point; a check that exists in both layers after absorption is a defect.

### D6 — The friction log is instituted first

A standing friction log is created at **`ops/friction-log.md`** before stage-2 adoption begins, so adoption friction is captured from its first minute. Each entry is dated and classified **`mechanical`** (automatable toil — the factory's future job, evidence for continuing) or **`semantic`** (fighting the box boundaries themselves — thesis damage), per the discriminator the strategy review fixed in advance. Every relaxation of the discipline lands with a log entry; an uncategorized relaxation is a process violation. The log is append-mostly working data, deliberately lighter than `records/` (which stays the home of dated analyses); a periodic record summarizes and cites it. The AGENTS.md pointer instituting this obligation lands in T2's PR, since the spec binds the stream but the workflow document binds contributors.

### D7 — S7-COMPLETE is the v0 gate

The stream's completion check verifies this spec, and additionally that every stream S0–S6 has a closed completion check and that the acceptance records exist. S7-COMPLETE closing **is** the v0 milestone declaration; a dated record announces it and cites the evidence chain. If any prior stream's completion is reopened, S7-COMPLETE reopens with it.

## Acceptance criteria

1. The skill exists at the fixed path in the shared format, containing exactly the D1 scope; a content audit confirms no factory-behavior, host-specific, or post-v0 material.
2. Two clean acceptance runs on two different harnesses, each with a dated record satisfying the full D3 evidence protocol, including the additive S4 classification of the `greet(name)` change and both `Hello, Ada!` transcripts.
3. Any intervened or failed run before the clean ones is recorded with the same protocol.
4. Every tracked file in this repository classifies exactly once under the adopted manifests; `boxology check` is green on this repository in PR validation on both platforms.
5. After absorption, no platform check exists in both xtask and `boxology check`; the deleted xtask registries are demonstrably manifest-derived; repository-specific checks (links, budget, records, determinism meta) still run under their own names.
6. `ops/friction-log.md` exists before the first stage-2 adoption PR merges; every discipline relaxation in the stream's history has a categorized entry; the AGENTS.md obligation is in place.
7. S7-COMPLETE's closing comment maps every criterion to evidence, confirms S0–S6 completion checks are closed, and the v0 record exists.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | The portable skill: content per D1, fixed path, content-audit test | 1–2 |
| T2 | Friction log institution: `ops/friction-log.md`, entry format, AGENTS.md obligation | 1 |
| T3 | Acceptance runbook + the two recorded runs (each run's record is its own PR) | 1 + 2 records |
| T4 | Stage-2 adoption: repository manifests, package partition, `check` green in PR validation | 2–3 |
| T5 | The absorption: xtask delegation, registry deletion, name separation | 1–2 |

T2 lands first (D6); T1 and T4 may proceed in parallel after it; T5 follows T4; T3 runs last against the finished S6 installer and S1–S6 artifacts. S7-COMPLETE closes the stream and v0 per D7. Dependencies: T3 consumes S6 in full (#332–#336) and the runbook consumes S6's initial-contract shape (#333); T4/T5 consume S5's `check` (#327) and the fixture manifests; the whole stream depends on S0–S6.

## Matters left open

- Skill distribution, host-specific wrappers, and any certification of compatible harnesses — the product contract's open question, post-v0.
- Stage 3 (tool boxification) — the first rung of #74, immediately after v0; the friction log's early entries are its input.
- The optional lead-authored checkpoint and richer onboarding documentation — open questions in the product contract, untouched.
- Friction-log mechanical enforcement (an xtask check) — deliberately not built now; instituted by convention and review, revisited if entries are skipped in practice.

## Tracker notes

Normative reconciliation in this PR's diff: the stream definition in `11-v0-streams.md` gains its spec link, and AGENTS.md's delivery-method note — which declared the skill's product guidance and acceptance contract unspecified until this spec existed — now points here. On merge, the operator files the five task issues plus S7-COMPLETE with `stream:s7` labels, recording the dependencies above. #74 gains no new obligation but its first-rung input (the friction log) is now scheduled; #57 and the product contract's open questions are cited unchanged.
