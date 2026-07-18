# Boxology

Boxology is a system for building software as independent boxes and operating an autonomous software factory over them.

Humans define box boundaries, typed interfaces, data models, and allowed connections. Agents implement and evolve the code hidden inside each box. A box can be replaced without requiring its consumers to understand its implementation as long as its contract remains compatible.

Start with the concise [white paper](boxology-whitepaper.md), then use its linked sections to open the detailed documents.

The [design interview](boxology-details/00-design-interview.md) records the complete Q&A and decisions that produced the current documents.

The [product contract](boxology-details/07-product-contract.md) separates the long-term direction from the first end-to-end foundation milestone.

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
