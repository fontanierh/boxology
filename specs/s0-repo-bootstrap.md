# S0 Spec — Product-Repo Bootstrap and CI

[Stream definition](../boxology-details/11-v0-streams.md#s0--product-repo-bootstrap-and-ci) ·
Status: **delivered; live repository baseline**

S0 is the live normative baseline for this repository's workspace and validation substrate. PR
[#571](https://github.com/fontanierh/boxology/pull/571) completed the post-V0 #342 absorption.

## Purpose and boundary

S0 supplies the pinned Rust workspace, deterministic repository automation, dependency policy,
review-budget enforcement, and public-safe hosted validation. It does not implement Boxology product
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
  formatting and manifest key order, tracked whitespace, Markdown links, and the 600-line budget.
- `cargo xtask ci --base <revision>` is the canonical local acceptance command. It owns exactly
  one full `boxology check` and runs the separately named repository checks that the product
  baseline does not subsume: skill audit, opaque fixture projects, the generated-style rustfmt
  negative, external-test integrity (including born-valid), whitespace, links, dependency
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

### D3 — Hosted validation

`.github/workflows/ci.yml` is the sole Actions workflow. Pull requests, pushes to `main`, and
manual dispatch run one `validate` job on `ubuntu-latest`, bounded to 30 minutes with redundant
runs cancelled. It installs the repository-pinned Rust toolchain and exact cargo-deny version,
then runs the canonical `cargo xtask ci --no-budget` gate. Pull requests additionally run
`cargo xtask budget --base <base-sha>` against the event's base commit; full checkout history makes
that base available.

The workflow uses only top-level `contents: read`, pins checkout by full commit SHA, and disables
credential persistence. It uses no caches, secrets, write permissions, `pull_request_target`, or
self-hosted runners. Xtask tests bind the exact workflow inventory and required contract, with
mutation cases for the security boundary and canonical gate.

### D4 — Platform evidence

Hosted CI continuously validates Linux. It does not claim Windows, macOS, x86-wide, or general
cross-platform support; broader platform evidence remains deliberate
[#525](https://github.com/fontanierh/boxology/issues/525) work.

### D5 — Links, dependencies, and review budget

`cargo xtask links` requires every tracked Markdown repository-relative link and heading anchor to
resolve; external URLs are not fetched. `cargo-deny` is exact-version pinned with permissive
license, crates.io source, and ban policy. Its full policy suite runs in canonical hosted CI.

`cargo xtask budget --base <revision>` fails above 600 hand-authored added lines with no override.
Markdown counts. `Cargo.lock`, manifest-declared derived outputs, and pure renames are excluded;
oversized work is split rather than exempted. Move accounting also recognizes extracted files when
Git can match the destination to content deleted from a source path. Credit is capped by the
source's deleted lines and consumed once, so copying an unchanged file still counts in full,
partial moves count their edits, and one deletion cannot subsidize duplicate destinations.

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

CI changes must preserve least privilege, pinned inputs, `persist-credentials: false`, bounded
concurrency, and the public-safe hosted execution boundary.

## Delivered acceptance evidence

1. The workspace, toolchain, formatting, links, budget, dependency, and determinism
   checks are implemented and exercised by xtask tests.
2. Hosted CI has one Linux job that runs canonical deep validation and the PR review budget;
   workflow invariant tests pin its inventory, authority, and security boundary.
3. Each canonical `ci` aggregate invokes one product check; the deleted commands and bootstrap
   registries are absent, with manifest-derived selection pinned by tests.
4. PR #571 and closed [#342](https://github.com/fontanierh/boxology/issues/342) preserve the
   absorption evidence.
5. Cross-platform proof, semantic self-protection, and external release support remain explicit
   residuals under #525 and #17 rather than current claims.
