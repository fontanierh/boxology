# S0 Spec — Product-Repo Bootstrap and CI

[Stream definition](../boxology-details/11-v0-streams.md#s0--product-repo-bootstrap-and-ci) ·
Status: **delivered; current baseline after PR #571**

S0 is the live normative baseline for this repository's workspace and validation substrate. V0
completion is evidenced in the
[2026-08-09 record](../records/2026-08-09-v0-completion-evidence.md); PR
[#571](https://github.com/fontanierh/boxology/pull/571) completed the post-V0 #342 absorption.

## Purpose and boundary

S0 supplies the pinned Rust workspace, deterministic repository automation, dependency policy,
review-budget enforcement, and a fast merge-critical check. It does not implement Boxology product
features, publish releases, claim Windows/Linux/cross-platform support, or make the quality policy
immutable. Cross-platform evidence is owned by
[#525](https://github.com/fontanierh/boxology/issues/525); semantic self-protection is owned by
[#17](https://github.com/fontanierh/boxology/issues/17).

## Current decisions

### D1 — Workspace and toolchain

The root is one edition-2024 Cargo workspace with a committed `Cargo.lock`, exact stable pin in
`rust-toolchain.toml`, and pinned `rustfmt`, Clippy, and rust-analyzer components. `.gitattributes`
is authoritative for LF normalization; `.editorconfig` is guidance. `crates/xtask` owns repository
automation through the checked-in `cargo xtask` alias. Generated Rust is printed by the generator,
not reformatted as hand-authored source.

Pull requests use squash merge and `main` is expected to remain linear. Repository settings allow
only squash PR merges, but branch protection is unavailable and direct pushes are not prevented.

### D2 — Canonical command ownership

- `cargo xtask ci-hygiene --base <revision>` is the cheap PR tier: repository audit, root
  formatting and manifest key order, tracked whitespace, Markdown links, records/friction
  integrity, and the 600-line budget.
- `cargo xtask ci --base <revision>` is the canonical local acceptance command. It owns exactly
  one full `boxology check` and runs the separately named repository checks that the product
  baseline does not subsume: skill audit, opaque fixture projects, the generated-style rustfmt
  negative, external-test integrity (including born-valid), whitespace, links, records, dependency
  policy, determinism, and the review budget.
- `cargo xtask ci --no-budget` is the canonical full local/deep command. It owns exactly one full
  `boxology check`, omits the base-relative review budget, and adds editor loading, ignored
  generator matrices, and repository/fixture rustdoc to the retained repository suites.

The former split capstone commands do not exist. Their unique evidence moved into the retained
aggregate. Root fmt/test/Clippy and manifest key-order work is not duplicated
inside that aggregate when the product check already owns it. Manifest data, not bootstrap
registries, selects hand-authored fixture formatting and budget-derived outputs.

Commands that need a base take it explicitly; they do not infer GitHub event state. The
determinism subcommands remain available for local experiments and future cross-platform proof,
but no active workflow currently compares platforms.

### D3 — Required pull-request validation

`.github/workflows/pr.yml` has one required `validation` job on
`[self-hosted, macOS, ARM64, boxology-macos-pr]`, with a 20-minute timeout and no Actions cache.
It always runs `ci-hygiene` against the pull-request base. Markdown-only changes stop there.

Code changes run the xtask invariant suite and ordinary tests for directly changed crates. The CLI
end-to-end target, CLI/workspace/classifier surface locks, and generator-model purity lock are
dispatch-only regardless of the changed path; their four crates retain library/binary tests,
explicit doctests, and every other integration target in required CI. `boxology-init` likewise
retains those tests around its existing deep-only born-valid exclusion. `boxology-generator`
retains ordinary unit, binary, doctest, and integration coverage but sends its two named
nested-Cargo end-to-end unit tests to deep validation. `cargo xtask ci --no-budget` executes the
non-ignored exclusions once through product workspace tests; integrity-only source/body/list guards bind all seven new exclusions without duplicate Cargo execution. A root `Cargo.toml`, `Cargo.lock`, or toolchain change
conditionally checks the complete workspace build graph; opaque fixture/golden changes
conditionally run `ci-fixtures`; process-reaper changes run that fixture suite. The required job
runs **zero product commands**. It therefore does not claim pre-merge full regeneration,
classification, Cargo-edge, workspace-wide, declared-quality, mutation-lock, or CLI end-to-end
enforcement.

`.github/workflows/deep-validation.yml` is dispatch-only, non-required, and native-Mac-only. It
runs only `cargo xtask ci --no-budget`; there is no separate `boxology check` step. Measured full
checks exceeded the PR timeout, so the product baseline remains local/deep rather than acquiring a
weakened policy-only mode.

### D4 — Runner topology

Four native Apple-silicon Mac JIT slots are active. Each slot has an isolated disposable checkout
and a private persistent Cargo target cache; concurrency is bounded to four build/test jobs per
slot. Linux JIT source and assets remain in `ops/ci-runner/`, but Linux services, registrations,
and the Colima profile are dormant. There is no active Linux, x86, or cross-platform workflow.
Any reactivation is deliberate [#525](https://github.com/fontanierh/boxology/issues/525) work.

Runner labels are capabilities, not immutable platform images. Actions are pinned by full commit
SHA; checkout does not persist credentials. Native host execution is trusted only for this private
repository and trusted collaborators. Exact runner, toolchain, and cargo-deny pins and the
credential/JIT/rollback boundary are maintained in the
[runner runbook](../ops/ci-runner/README.md).

### D5 — Links, dependencies, and review budget

`cargo xtask links` requires every tracked Markdown repository-relative link and heading anchor to
resolve; external URLs are not fetched. `cargo-deny` is exact-version pinned with permissive
license, crates.io source, and ban policy. Its bans/licenses/sources suite is deep/local rather
than per-PR; the scheduled advisory workflow is non-gating.

`cargo xtask budget --base <revision>` fails above 600 hand-authored added lines with no override.
Markdown counts. `Cargo.lock`, manifest-declared derived outputs, and pure renames are excluded;
oversized work is split rather than exempted.

### D6 — Determinism

Determinism subjects vary exactly one controlled condition per experiment across repeat, path,
time, locale, and timezone contexts, with controlled inputs and scrubbed environments. Real-subject
findings carry observational labels; causal labels are reserved for explicit fault-injection
fixtures. Output manifests are sorted path-to-hash/size documents;
platform evidence is separate from compared bytes. Fault-injection subjects force known mismatch
classes so the comparator's detection power is tested, not assumed. V0 proved the registered real
subjects across contexts on native macOS ARM64. Continuous Linux/x86 comparison is not current
evidence and remains #525 scope.

### D7 — Quality authority and security

CI and checker code are candidate-writable: a pull request can change the checks that judge it.
Protected-path declarations and human review are reporting-level controls, not immutable
evaluation. No same-named green check proves semantic self-protection; #17 remains open.

Runner credentials stay outside the checkout; JIT workers are single-job and cleaned after use.
Dependency advisories never fail unrelated PRs. CI changes must preserve least privilege, pinned
inputs, `persist-credentials: false`, bounded concurrency, and fail-closed cleanup.

## Delivered acceptance evidence

1. The workspace, toolchain, formatting, links, records, budget, dependency, and determinism
   checks are implemented and exercised by xtask tests.
2. Required PR validation is one lean native-Mac job with conditional positive scopes and zero
   product commands; workflow invariant tests pin that topology.
3. Each canonical `ci` aggregate invokes one product check; the deleted commands and bootstrap
   registries are absent, with manifest-derived selection pinned by tests.
4. Exact-main native-macOS V0 proof is preserved by the completion record; PR #571 and closed
   [#342](https://github.com/fontanierh/boxology/issues/342) preserve the absorption evidence.
5. Cross-platform proof, semantic self-protection, and external release support remain explicit
   residuals under #525, #17, and the post-V0 roadmap rather than current claims.

Historical task planning and superseded CI topologies remain available through
[#93](https://github.com/fontanierh/boxology/issues/93),
[#272](https://github.com/fontanierh/boxology/issues/272), and the repository's append-only
records and friction log. They are not part of this live operational baseline.
