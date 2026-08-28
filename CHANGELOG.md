# Changelog

This file records notable user-facing changes to Boxology.

## Unreleased

### Fixed

- Made release preflight inspect package archives in Cargo's effective target directory.
- Made base-relative ownership checks accept newly introduced packages and newly declared files,
  while retaining base authority for modified and deleted paths.
- Generated Rust headers and schema provenance now report the actual `boxology-generator` package
  version instead of the stale `0.0.0` placeholder.
- Documented the canonical field-presence wire convention, made explicit `null` on an optional
  object field a dedicated typed decoding error, and added actionable canonical JSON guidance.
- Added `ContractDescriptor::capability` for generic dispatchers and clarified that generated
  factories and runtime import bundles are composition-owned hooks rather than standalone APIs.

## [0.1.1] - 2026-08-13

### Added

- Added the binding-independent `boxology_contract::json` codec. Consumers can now call
  `json::encode(&slot, &descriptor)` and `json::decode(bytes, &descriptor, role, limits)` for IPC,
  CLI output, and files without depending on `boxology-http` or inventing another mapping.
- Allowed a box to use a project-local Cargo name for its generated contract via
  `contract_crate = <crate_name>;`, while preserving `boxology_generated_contract` as the default.
- Generated `Default` for boundary models and domain errors, plus `Eq`, `Hash`, and ordering when
  the complete generated shape supports them. Tolerant enum/error shapes remain intentionally
  partial because they carry an opaque `Unknown` payload.

### Fixed

- Made `boxology --help` and `boxology-init --help` conventional successful invocations: usage is
  written to stdout with exit status 0.
- Allowed generation to bootstrap a missing generated contract crate, while retaining strict
  Cargo-backed validation after the write.
- Corrected ownership and compatibility classification for wholly new nested managed workspaces.
- Made generated contract sources byte-stable across `cargo fmt --all` and serialized the
  regression test's process-wide Cargo environment.
- Moved the canonical descriptor-guided JSON projection into `boxology-contract`; the HTTP binding
  now consumes that shared implementation, including strict/tolerant roles and byte/depth limits.

### Distribution

- Published the ordered `0.1.1` crate closure on crates.io with exact dependency versions and the
  existing resumable, one-crate-at-a-time release safeguards.

### Upgrading an existing setup

To upgrade from `0.1.0`, replace both installed tools with the new registry release:

```sh
cargo install --force --locked --version 0.1.1 boxology-init
cargo install --force --locked --version 0.1.1 boxology-cli
```

Most handwritten boxes only need the authoring facade. Hand-managed consumers that pin the facade
can update it directly:

```toml
[dependencies]
boxology = "=0.1.1"
```

Existing generated projects should keep their exact Git revision pins. Their HTTP example still
uses `boxology-http`, which is outside the registry release closure; do not mechanically replace
those generated dependencies with crates.io versions. Regenerate deliberately when adopting the
new contract alias or generated trait behavior, then run `cargo fmt --all` and `boxology check`.

## [0.1.0] - 2026-08-13

### Foundation

- Completed the V0 framework foundation: typed box contracts, deterministic contract generation,
  runtime composition, in-process and HTTP bindings, compatibility classification, workspace
  validation, deterministic project initialization, and portable onboarding guidance.
- Open-sourced the framework with public CI, dual MIT/Apache-2.0 licensing, security and
  contribution guidance, and package metadata. Agent harnesses and application boxes are separate
  projects rather than framework components.

### Post-V0 refinements

- Standardized handwritten box declarations in `contract.rs` files.
- Added box-like composition wiring and generated typed composition handles, then simplified
  generated compositions so their setup reads like ordinary box code.
- Fixed generated contract type registration, Git dependency resolution in nested workspaces, and
  initializer dependency portability.

### Distribution

- Published the ordered `0.1.0` crate closure on crates.io with exact dependency versions,
  package metadata, license inventories, preflight checks, and one-crate-at-a-time publishing
  safeguards.
- Added normal registry installation with `cargo install boxology-init --locked` and
  `cargo install boxology-cli --locked`.

### Upgrading an existing setup

Replace Git-installed or older copies of both command-line tools with the registry release:

```sh
cargo install --force --locked --version 0.1.0 boxology-init
cargo install --force --locked --version 0.1.0 boxology-cli
```

`--force` replaces the existing `boxology-init` and `boxology` binaries. Keep `--version 0.1.0`
when reproducibility matters; omit it when deliberately updating to the newest release. Confirm the
updated setup by running `boxology check` in each managed project.

Consumers do not add all 19 published crates to an application. Most handwritten box code uses the
`boxology = "=0.1.0"` authoring facade. Generated contracts, runtime compositions, and the two tools
bring in the narrower framework crates they require through normal transitive Cargo dependencies.
For a hand-managed, non-HTTP crate, replace an older facade Git dependency with:

```toml
[dependencies]
boxology = "=0.1.0"
```

Existing projects created by `boxology-init` need no dependency rewrite for this tool update. Keep
their generated exact Git revision pins intact: the generated HTTP example still needs
`boxology-http`, which is not in the `0.1.0` registry closure. Do not mechanically replace those
generated entries with crates.io versions. New projects created by `boxology-init 0.1.0` preserve
the same coherent pinning policy.

### Known boundaries

- Generated projects remain pinned to a Boxology Git revision because `boxology-http` is outside
  the first registry publication closure.
- The supported foundation is a greenfield Rust workspace with unary request-response contracts,
  in-process execution, and HTTP/1.1. Broader contract shapes, foreign-language support, and a
  general cross-platform support claim remain future work.
