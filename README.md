# Boxology

Boxology is an early-stage, source-only framework for building software as independent boxes that humans define and agents implement. Humans own box boundaries, typed interfaces, data models, and allowed connections; implementations can evolve behind compatible contracts.

V0 was completed on 2026-08-09. The accepted stream specs describe its delivered boundary.
Applications built with Boxology are separate products and are not included in this repository.

The workspace packages are currently unpublished development packages: every package is version `0.0.0` with `publish = false`. Install only from a source checkout. Building this checkout requires Rust 1.97.1.

## Source-checkout quick start

Install the initializer and checker from a Boxology source checkout, then initialize an existing empty target directory (a lone `.git` is allowed):

```sh
cargo install --path <boxology-source>/crates/boxology-init
cargo install --path <boxology-source>/crates/boxology-cli
boxology-init --name example --dependency-source <absolute-boxology-source> --target <empty-directory>
cd <empty-directory>
cargo build --workspace
boxology check
```

The generated project README owns that project's invocation contract.

## Documentation

Start with the concise [white paper](boxology-whitepaper.md), then use its linked sections to open
the detailed documents. The [product contract](boxology-details/07-product-contract.md) separates
long-term direction from the completed foundation milestone.

- [Boxes](boxology-details/01-boxes.md)
- [Packages, providers, and compositions](boxology-details/02-packages.md)
- [Runtime](boxology-details/03-runtime.md)
- [Contract evolution and deprecation](boxology-details/04-evolution.md)
- [Software factory](boxology-details/05-software-factory.md)
- [Quality and authority](boxology-details/06-quality-and-authority.md)
- [Product contract and foundation milestone](boxology-details/07-product-contract.md)
- [Rust build topology](boxology-details/08-rust-build-topology.md)
- [Canonical capability contract](boxology-details/09-capability-contract.md)
- [V0 streams](boxology-details/11-v0-streams.md)

## License

Boxology is dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option.
