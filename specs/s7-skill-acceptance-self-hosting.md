# S7 Spec — Skill and Repository Adoption

[Stream definition](../boxology-details/11-v0-streams.md#s7--skill-acceptance-and-stage-2-self-hosting) ·
Status: **delivered in V0**

S7 delivered the portable onboarding skill, behavioral acceptance of its greenfield workflow, and
this repository's root manifest adoption. This specification describes the retained framework
surface; harness-specific operations and application code are outside this repository.

## Boundary

S7 does not build an agent, harness, factory, gateway, or generic development CLI binding. It
consumes S1–S6 as delivered. Agent execution, persistence, communication, review orchestration,
and application-specific dogfooding belong to external consumers.

## Delivered decisions

### D1 — Portable onboarding skill

The product skill is `.agents/skills/boxology/SKILL.md` in shared Agent Skills format. It teaches
the box model, boundaries, compatible contract evolution, the greenfield flow, and names the
hosting agent the lead. It uses source-checkout installation, the explicit initializer interface,
the first Cargo build that creates `Cargo.lock`, and `boxology check`.

Its trigger is limited to managed-project onboarding, so it does not govern development of
Boxology itself. Portability is a content property: the skill has no host-specific instructions.

### D2 — Behavioral acceptance

The acceptance scenario starts from an empty target, initializes a project, completes its first
build, and asks the lead agent to add a backward-compatible `greet(name)` capability. Both Rust and
HTTP calls return `Hello, Ada!`; `boxology check` reports the addition and no foreign package source
changes. This proves the skill's workflow without claiming host certification.

### D3 — Repository adoption

Root `boxology.toml` manifests classify every tracked file exactly once. Framework packages are
platform-kind; fixture projects are opaque owned data, and their nested manifests do not enter root
discovery. Fixture-generated trees are declared by their own manifests. Root `Cargo.lock` is the
root-derived artifact.

The root manifests declare CI and xtask as protected control-plane paths. That declaration reports
ownership; it does not make candidate-writable policy immutable. Human review remains the current
control and [#17](https://github.com/fontanierh/boxology/issues/17) owns stronger semantic
self-protection.

### D4 — Canonical validation

Every canonical `cargo xtask ci` aggregate owns exactly one `boxology check`; xtask retains only
distinct repository semantics the product baseline does not cover. Hosted CI runs the canonical
deep command on Ubuntu. General cross-platform equivalence is not claimed and remains
[#525](https://github.com/fontanierh/boxology/issues/525) scope.

## Delivered acceptance criteria

1. The scoped portable skill exists and passes its content audit.
2. The behavioral scenario proves initialization, compatible evolution, and Rust/HTTP behavior.
3. Root manifests classify the repository with fixture opacity.
4. Canonical validation contains one complete product check without duplicated platform work.

## Live residuals

- Skill distribution, richer onboarding, and host certification remain outside V0.
- Pinned-prior-release generator validation becomes mandatory at the first release boundary.
- Framework self-hosting can proceed as focused product work without importing private application
  operations into this repository.
