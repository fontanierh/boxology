# Contributing to Boxology

Thanks for helping improve Boxology. The project is an early-stage, source-only Rust framework;
applications built with it live in separate repositories.

## Before opening a change

- Search existing issues and open or comment on one before undertaking a substantial change.
- Keep changes focused. Explain the problem, the chosen approach, and any contract or compatibility
  impact.
- Treat box boundaries and generated contracts as public design surfaces. Box contracts live in a
  dedicated `contract.rs`; implementation details stay behind that boundary.
- Do not include credentials, private data, generated build output, or unrelated formatting changes.

## Development

Use the pinned Rust toolchain from the repository root. The canonical local validation command is:

```sh
cargo xtask ci --no-budget
```

For a pull request, also check its authored-line budget against the current base:

```sh
cargo xtask budget --base origin/main
```

Pull requests should add or update tests for observable behavior, include documentation when a
public surface changes, and stay within 600 hand-authored added lines. Generated artifacts and
`Cargo.lock` are excluded from that review budget but must remain reproducible.

## Pull requests

Complete the pull request template, link the relevant issue, and make sure CI is green. Maintainers
may ask for a smaller change or a clearer contract. By contributing, you agree that your work is
licensed under the repository's MIT OR Apache-2.0 terms.

For security vulnerabilities, do not open a public issue; follow [SECURITY.md](SECURITY.md).
