# Boxology

Boxology is a platform for building software as independent boxes that humans define and agents implement. It works with any coding-agent harness. An autonomous, Boxology-based software factory is the project's committed flagship application — built on the platform, not inside it.

Humans define box boundaries, typed interfaces, data models, and allowed connections. Agents implement and evolve the code hidden inside each box. A box can be replaced without requiring its consumers to understand its implementation as long as its contract remains compatible.

Start with the concise [white paper](boxology-whitepaper.md), then use its linked sections to open the detailed documents.

The [design interview](boxology-details/00-design-interview.md) records the complete Q&A and decisions that produced the current documents.

The [product contract](boxology-details/07-product-contract.md) separates the long-term direction from the first end-to-end foundation milestone.

## Current status

**V0 completed on 2026-08-09.** The
[completion record](records/2026-08-09-v0-completion-evidence.md) preserves the exact-main
native-macOS evidence, accepted boundary, and post-V0 residuals. PR
[#571](https://github.com/fontanierh/boxology/pull/571) subsequently completed
[#342](https://github.com/fontanierh/boxology/issues/342): `cargo xtask ci` now owns one full
`boxology check`, with the required PR lane intentionally kept lean.

Current product work follows the pragmatic
[post-V0 self-hosting roadmap](boxology-details/12-post-v0-self-hosting-roadmap.md), tracked by
[#572](https://github.com/fontanierh/boxology/issues/572), and the standing factory dogfood
commitment in [#74](https://github.com/fontanierh/boxology/issues/74).

## Source-checkout quick start

V0 is not published. From a Boxology source checkout, install the initializer and checker, then
initialize an existing empty target directory (a lone `.git` is allowed):

```sh
cargo install --path <boxology-source>/crates/boxology-init
cargo install --path <boxology-source>/crates/boxology-cli
boxology-init --name example --dependency-source <absolute-boxology-source> --target <empty-directory>
cd <empty-directory>
cargo build --workspace
boxology check
```

The generated README owns the invocation contract. Its current end-to-end command is
`cargo test -p ping-app assembled_ping_answers_in_process_and_over_real_http`.

## Detailed documents

- [Boxes](boxology-details/01-boxes.md)
- [Packages, providers, and compositions](boxology-details/02-packages.md)
- [Runtime](boxology-details/03-runtime.md)
- [Contract evolution and deprecation](boxology-details/04-evolution.md)
- [Software factory](boxology-details/05-software-factory.md)
- [Quality and authority](boxology-details/06-quality-and-authority.md)
- [Product contract and foundation milestone](boxology-details/07-product-contract.md)
- [Rust build topology](boxology-details/08-rust-build-topology.md)
- [Canonical capability contract](boxology-details/09-capability-contract.md)
- [Strategy review and self-hosting ladder](boxology-details/10-strategy-review.md)
- [V0 streams](boxology-details/11-v0-streams.md)
