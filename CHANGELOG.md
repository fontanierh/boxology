# Changelog

This file records notable user-facing changes to Boxology.

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

### Known boundaries

- Generated projects remain pinned to a Boxology Git revision because `boxology-http` is outside
  the first registry publication closure.
- The supported foundation is a greenfield Rust workspace with unary request-response contracts,
  in-process execution, and HTTP/1.1. Broader contract shapes, foreign-language support, and a
  general cross-platform support claim remain future work.
