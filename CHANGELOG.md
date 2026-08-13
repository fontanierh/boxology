# Changelog

This file records notable user-facing changes to Boxology.

## Unreleased

### Contract JSON

- Added the binding-independent `boxology_contract::json` codec. Consumers can now call
  `json::encode(&slot, &descriptor)` and `json::decode(bytes, &descriptor, role, limits)` for IPC,
  CLI output, and files without depending on `boxology-http` or inventing another mapping.
- The codec is the existing canonical HTTP projection moved into the published contract crate:
  descriptor-guided integer/blob/enum/presence rules, deterministic key ordering and escaping,
  strict versus tolerant decode roles, unknown-enum forwarding, and byte/depth caps remain shared.

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
- Allowed a box to use a project-local Cargo name for its generated contract via
  `contract_crate = <crate_name>;`, while keeping the existing alias as the default. Custom names
  are repeated on `#[boxology::implementation(contract_crate = <crate_name>)]` so implementation
  checking remains independent of source-module layout.
- Generated `Default` for every boundary model and domain error, plus `Eq`, `Hash`, and ordering
  whenever the complete generated shape supports them. Tolerant enums and errors remain
  `PartialEq` because their always-present opaque `Unknown` payload is part of that shape;
  floating-point leaves likewise remain intentionally partial.

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
