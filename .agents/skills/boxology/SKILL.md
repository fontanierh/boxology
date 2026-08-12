---
name: boxology
description: Guide greenfield Boxology managed-project onboarding. Use when a developer asks a coding agent to initialize a Boxology-managed project; do not use for development of Boxology itself.
---

# Boxology onboarding

## Philosophy

Code is cheap; safe coordination and human attention are the bottleneck. Boxology makes the unit of change small enough for a coding agent to work on while keeping the decisions that shape a system legible to its human owner.

Human-owned boundaries, contracts, types, data models, and composition are the decisions that shape the system. The coding agent using this skill is the lead agent: it implements within those decisions and keeps communication on declared contracts. The implementation behind a contract is replaceable.

## Box boundaries

A box is a managed package with one accountable owner. A box owns its implementation and declared boundary; a composition owns how boxes are wired and deployed; a platform package owns shared platform machinery. Every managed change has one accountable package and zero foreign source changes, except for deterministic artifacts attributable to the accountable package.

Keep each boundary explicit. Do not reach into another package's implementation or create an undocumented communication path. Human-owned package boundaries and composition decisions are the guardrails that let the lead agent change one box without silently changing its neighbours.

## Contracts and compatible evolution

The authored controlled contract source is the source of truth for the public surface. Generated output is deterministic and checked in for review, but it is never hand-edited: change the authored source and regenerate it. The generated schema is the compatibility authority.

Prefer an additive expansion, then migrate consumers, then contract the old surface: expand-migrate-contract. Preserve compatible evolution, and do not soften or relabel the generator's classifications for a tightening or removal merely because a migration is planned.

## Way of working

The lead agent reads the repository instructions, README, and manifests before editing. It identifies the one accountable package, changes only its authored controlled source, regenerates deterministic outputs, runs the package's declared quality commands, and runs `boxology check`. It surfaces any protected control-plane change for human attention instead of treating that change as ordinary package work.

Author a box's controlled declaration in `implementation/src/contract.rs`. Its
`implementation/src/lib.rs` exposes the declaration with the exact unconditional items
`mod contract;` and `pub use contract::*;`, then contains the ordinary implementation. Both files
are declared generation inputs; the crate root remains explicit rather than inferred from these
paths.

## Five-step onboarding flow

1. **Activate.** Apply this skill to the greenfield onboarding request; the coding agent becomes the lead agent for the new managed project.
2. **Ask only.** Ask for the project name, target root, source checkout, and confirmation that the target is empty except `.git`.
3. **Install both crates.** From the same source checkout, install both tools with the documented paths:
   `cargo install --path <source-checkout>/crates/boxology-init`
   `cargo install --path <source-checkout>/crates/boxology-cli`
4. **Initialize explicitly.** Invoke `boxology-init` through its documented explicit interface with the answers from step 2. Consult that interface for the current flag spellings; this skill does not freeze flag spellings.
5. **Build and check.** In the generated repository, run `cargo build` first so Cargo.lock is materialized, then run `boxology check`. The generated README owns the exact Rust and HTTP invocation detail.
