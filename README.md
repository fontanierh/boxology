# Boxology

Boxology is an early-stage framework for building software as independent boxes that humans define and agents implement. Humans own box boundaries, typed interfaces, data models, and allowed connections; implementations can evolve behind compatible contracts.

V0 was completed on 2026-08-09. The accepted stream specs describe its delivered boundary.
Applications built with Boxology are separate products and are not included in this repository.

Boxology `0.2.1` tools are published on crates.io. Building this checkout requires Rust 1.97.1.

## Quick start

Install the initializer and checker from crates.io, then initialize an existing empty target
directory (a lone `.git` is allowed):

```sh
cargo install boxology-init --locked
cargo install boxology-cli --locked
boxology-init --name example --target <empty-directory>
cd <empty-directory>
cargo build --workspace
boxology check
```

Update with `cargo install --force boxology-init --locked` and
`cargo install --force boxology-cli --locked`; add `--version 0.2.1` to pin this release.
To install directly from the source repository instead:

```sh
cargo install --git https://github.com/fontanierh/boxology --locked boxology-init
cargo install --git https://github.com/fontanierh/boxology --locked boxology-cli
```

The generated project README owns that project's invocation contract.

Hand-managed boxes may name their generated contract dependency after the domain instead of using
a framework-shaped Cargo alias. Put the selected Rust crate name at the start of `contract.rs`:

```rust
boxology::contract! {
    contract_crate = review_contract;
    // declarations and capabilities
}
```

and use the same key in `Cargo.toml`, for example
`review_contract = { package = "review-contract", path = "../generated/contract" }`. Omitting
`contract_crate` preserves the generated-project default, `boxology_generated_contract`. When a
project-local name is selected, pass the same name to the implementation attribute:

```rust
#[boxology::implementation(contract_crate = review_contract)]
impl ReviewService {
    // ordinary async capability methods
}
```

## Release procedure

Run `cargo xtask ci --base origin/main`, then `cargo xtask release preflight`. Preflight checks the
exact `0.2.1` closure and order, inspects every crate's planned file inventory (including README and
both licenses), and creates, verifies, and inspects the real dependency-free root `.crate`. It does
so in Cargo's effective target directory, including an absolute or relative `CARGO_TARGET_DIR`. It
does not claim to package dependent crates: Cargo requires their predecessors to be visible on crates.io.

Configure Cargo's crates.io credentials securely, then publish exactly the next crate shown by the
order using `BOXOLOGY_RELEASE_PUBLISH=1 cargo xtask release publish <crate-name>`. Each invocation
recognizes the already-visible prefix, rejects gaps or an out-of-order name, runs a real crates.io
publish dry-run, and publishes only that crate. Wait for it to become visible before invoking the
next. After the sequence, prove fresh registry installs with the Quick start commands and smoke-test
both tools' `--version` output plus an initialized project. Tag the proven release commit and create
its GitHub Release only after that registry smoke test passes. Tests and preflight never publish.

## Documentation

See the [changelog](CHANGELOG.md) for release history and known boundaries. Start with the
concise [white paper](boxology-whitepaper.md), then use its linked sections to open the detailed
documents. The [product contract](boxology-details/07-product-contract.md) separates long-term
direction from the completed foundation milestone.

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

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) before proposing a change and [SECURITY.md](SECURITY.md)
before reporting a vulnerability. Community participation is governed by the
[Contributor Covenant](CODE_OF_CONDUCT.md).
