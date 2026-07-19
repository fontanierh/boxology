# V0 Streams

[Back to the white paper](../boxology-whitepaper.md)

This document defines the high-level workstreams for implementing the v0 foundation milestone. It is the top level of the [v0 execution methodology](../AGENTS.md#v0-execution-methodology): each stream will receive its own spec and task list before implementation, and every task resolves into a stack of small, individually reviewable pull requests.

Streams partition the v0 scope defined by the [product contract](07-product-contract.md). They are workstreams, not strict phases: later streams depend on earlier ones, but work overlaps where dependencies allow.

## S1 — Runtime core

The kernel crates that everything else consumes: `CallContext`, the `ContractType` and `ContractError` model, the presence model (`T` / `Option<T>` / `Field<T>`), dedicated binary and sensitive-value types, the asynchronous fallible handle contract with the domain/invocation error split, and the in-process binding. This is the definitionally non-box layer — the substrate boxes are made of. Normative inputs: [Canonical Capability Contract](09-capability-contract.md).

## S2 — Contract generator

The deterministic pre-Cargo generator: parsing annotated implementation methods and boundary-type declarations, type lifting into the generated contract crate with implementation-side re-exports, the language-neutral schema, typed caller handles, the dispatch interface and implementation-local adapter, generator provenance, cross-platform byte determinism, and semantic contract-change classification against a base revision. The v0 long pole; the milestone is not complete until generation, provenance, reproducibility, typed invocation, and classification work as one path. Normative inputs: [Rust Build Topology](08-rust-build-topology.md), [Canonical Capability Contract](09-capability-contract.md). Depends on S1 for the types the generated code targets.

## S3 — HTTP binding

The v1 HTTP transport: `POST /rpc/{box_id}/{capability_id}` routing, the lossless JSON mapping, the three response envelopes, the status mapping, context headers (`Boxology-Timeout-Ms`, `traceparent`/`tracestate`, `Idempotency-Key`), request limits, advisory cancellation on disconnect, and the binding-conformance test suite. Normative inputs: the Foundation HTTP binding section of [Runtime](03-runtime.md). Depends on S1 and S2.

## S4 — Manifest and validation tooling

`boxology.toml` parsing and workspace discovery, ownership and path classification, crate-role mapping against Cargo metadata, the Cargo-edge policy checker, shared-lockfile rules, and the `boxology generate` / `boxology check` commands with their exit codes, diagnostics, JSON output, and the emitted GitHub Actions workflow. Normative inputs: [Packages](02-packages.md), [Rust Build Topology](08-rust-build-topology.md). Depends on S2 for regeneration checks; the manifest/ownership checker itself can start alongside S1.

## S5 — Installer and generated project

The deterministic initializer: creating the Cargo workspace, the Hello box (implementation and generated contract), the application composition with in-process and HTTP bindings, the root platform package, all manifests, and repository CI — ending in the working database-free Hello World invocable through Rust and HTTP. Normative inputs: [Product Contract](07-product-contract.md), S1–S4 outputs. Depends on all of S1–S4 producing usable artifacts.

## S6 — Skill, acceptance, and stage-2 self-hosting

The portable Agent Skills-format skill that teaches a coding agent the box model and names it the lead; the end-to-end acceptance run of the foundation scenario, including the `greet(name)` task; and stage 2 of the [self-hosting ladder](10-strategy-review.md#self-hosting-ladder) — adopting `boxology.toml` manifests and `boxology check` on this product repository itself. Depends on S1–S5.

## Sequencing notes

- The dependency spine is S1 → S2 → {S3, S4} → S5 → S6, with S4's manifest/ownership portion parallelizable from the start.
- Stage 3 of the self-hosting ladder — boxifying the tools themselves — deliberately comes **after** v0: tools are built conventionally as platform packages first, then boxified as the first rung of the [issue #74](https://github.com/fontanierh/boxology/issues/74) commitment.
- Stream boundaries are also review boundaries: a pull request belongs to one task, a task to one stream, and cross-stream interface changes are made in the owning stream.
