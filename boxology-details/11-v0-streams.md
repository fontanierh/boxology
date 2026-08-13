# V0 Streams

[Back to the white paper](../boxology-whitepaper.md)

**Status: V0 complete (2026-08-09).** This document describes the capabilities delivered by the
high-level workstreams. The linked specs are the current normative baselines.

Streams partition the delivered v0 scope defined by the [product contract](07-product-contract.md).
The dependency ordering below is historical.

## S0 — Product-repo bootstrap and CI

The infrastructure every other stream stands on: the Cargo workspace scaffold, pinned toolchain,
determinism harness, and repository validation. Hosted CI runs the canonical validation command in
one public Ubuntu job. General cross-platform equivalence is not claimed; #525 owns that evidence.
Spec: [S0 — Product-Repo Bootstrap and CI](../specs/s0-repo-bootstrap.md).

## S1 — Runtime core and composition assembly

The kernel crates that everything else consumes: `CallContext`, the `ContractType` and `ContractError` model, the presence model (`T` / `Option<T>` / `Field<T>`), dedicated binary and sensitive-value types, and the asynchronous fallible handle contract with the domain/invocation error split. S1 also owns the **composition assembly API**: binding registration and validation (reject missing, duplicate, or incompatible bindings before accepting traffic), exposure-versus-declared-maximum checks, handle wiring, and the in-process binding. Transports implement their bindings against this API rather than defining their own assembly semantics. This is the definitionally non-box layer — the substrate boxes are made of. Normative inputs: [Canonical Capability Contract](09-capability-contract.md). Spec: [S1 — Runtime Core and Composition Assembly](../specs/s1-runtime-core.md).

## S2 — Contract generator

The delivered deterministic contract generator structurally discovers one controlled contract block and its ordinary inherent implementation, performs pure pre-Cargo parsing and emission, and proves signatures through normal compilation. It emits the sole public boundary types, language-neutral schema, stable identities and revision, typed handles, dispatch interface, adapter, and provenance. V0 proved generation, provenance, reproducibility, compilation, and typed invocation as one path on native macOS ARM64; cross-platform proof remains #525 scope. Contract-change *classification* is deliberately not here — S2 emits the schema and revision; S4 judges changes. Normative inputs: [Rust Build Topology](08-rust-build-topology.md), [Canonical Capability Contract](09-capability-contract.md). Spec: [S2 — Contract Generator](../specs/s2-contract-generator.md).

## S3 — HTTP binding

The v1 HTTP transport, implemented against S1's assembly API: `POST /rpc/{box_id}/{capability_id}` routing, the lossless JSON mapping, the three response envelopes, the status mapping, context headers (`Boxology-Timeout-Ms`, `traceparent`/`tracestate`, `Idempotency-Key`), request limits, advisory cancellation on disconnect, and the binding-conformance test suite. Normative inputs: the Foundation HTTP binding section of [Runtime](03-runtime.md). Depends on S1 and S2. Spec: [S3 — HTTP Binding](../specs/s3-http-binding.md).

## S4 — Contract-change classification

The compatibility authority as its own deliverable: consuming S2's schemas, diffing a submitted revision against a base revision, and classifying every change under the compatibility taxonomy — additive, compatible-with-conditions, incompatible tightening or removal — with precise diagnostics. The taxonomy details left open by the capability-contract design are resolved by this stream's spec. Classification output is consumed by S5's `check` and reported even when harness policy later authorizes an incompatible change. This is the most thesis-critical single component: it is what makes "mechanical compatibility check" a fact rather than a promise. Depends on S2's schema format. Spec: [S4 — Contract-Change Classification](../specs/s4-contract-change-classification.md).

## S5 — Manifest and validation tooling

`boxology.toml` parsing and workspace discovery, ownership and path classification, crate-role mapping against Cargo metadata, the Cargo-edge policy checker, shared-lockfile rules, and the `boxology generate` / `boxology check` commands with their exit codes, diagnostics, JSON output, and the emitted GitHub Actions workflow. Normative inputs: [Packages](02-packages.md), [Rust Build Topology](08-rust-build-topology.md). Depends on S2 for regeneration checks and S4 for classification; the manifest/ownership half is parallelizable from the start. Spec: [S5 — Manifest and Validation Tooling](../specs/s5-manifest-and-validation.md).

## S6 — Installer and generated project

The deterministic initializer: creating the Cargo workspace, the `ping` box (implementation and generated contract), the `ping-app` composition with in-process and HTTP bindings, the root platform package, all manifests, and repository CI — ending in the working database-free Hello World scenario invocable through Rust and HTTP. Normative inputs: [Product Contract](07-product-contract.md) and the outputs of S1–S5. Depends on all of S1–S5 producing usable artifacts. Spec: [S6 — Installer and Generated Project](../specs/s6-installer-and-generated-project.md).

## S7 — Skill, acceptance, and stage-2 self-hosting

The delivered portable Agent Skills-format onboarding skill; the clean end-to-end foundation
acceptance run, including the `greet(name)` task; and the adopted root manifests and
`boxology check`. Spec:
[S7 — Skill, Acceptance, and Stage-2 Self-Hosting](../specs/s7-skill-acceptance-self-hosting.md).

## The v0 evidence corpus

V0's required behavioral evidence corpus is exactly these four fixtures, each with a verified role. This narrowing changes proof breadth, not the shipped kernel: already-implemented runtime representations, codec behavior, classifier rows, mutation tests, and determinism subjects remain required and are not deleted or weakened.

- **`hello`** (`crates/fixtures/hello`) — `greet(String)` with `GreetError::EmptyName`; S1's golden-target shape, S2's byte-equality and compile-and-run subject, and S3's typed/raw conformance subject.
- **`greeter`** (`crates/fixtures/greeter`) — imports `hello` through a resolved import; S1's assembly/injection end-to-end proof.
- **`ping`** (`crates/fixtures/ping`) — `ping(u64)`; the S6 installer's initial contract, deliberately distinct from `greet` so S7's `greet(name)` addition is purely additive under S4's taxonomy.
- **`ping-app`** (`crates/fixtures/ping-app`) — composition binding `ping` in-process and over HTTP; S6 born-valid and quality-command evidence.

S1's required fixture inventory is `hello` and `greeter`, with `ping`/`ping-app` consumed from S6/S7. S2 completion proves generation over that exact corpus; parser/model branches beyond it do not create an end-to-end support claim. S3's required end-to-end fixture corpus is the scalar/unit-error `hello`/`ping` surface and `ping-app`, while existing raw protocol and codec tests remain gating. S4 classification remains broader than generator authoring wherever the schema model already represents a change.

## Post-v0 residuals recorded at the corpus decision

These claims were previously written as v0 obligations but are not delivered by the landed corpus. They are durable post-v0 residuals, not silent omissions. Each is named again in [#343](https://github.com/fontanierh/boxology/issues/343)'s S7-COMPLETE residual ledger with this section as its normative basis.

| Residual | Owner | V0 posture |
| --- | --- | --- |
| `kitchen-sink` full-grammar fixture (structs, data enums, containers, `Field`/`Secret`/`Blob` authoring corpus) | [#100](https://github.com/fontanierh/boxology/issues/100) | Not built; fail-closed codes and kernel-level suites stand |
| Named-field payload emission (`BXG0048`) and `Blob`/`Secret` end-to-end generator/binding coverage (`BXG0040`) | [#104](https://github.com/fontanierh/boxology/issues/104) | Fail-closed at generation; `Secret` redaction proven at kernel level only |
| Capability `name` override (wire-vs-Rust identity split) | [#480](https://github.com/fontanierh/boxology/issues/480) | Partial parser/model handling is not v0 support; wire name = Rust fn name in v0 |
| Transitive dependency-graph purity / identity-value extraction (`boxology-contract` → `tokio`) | [#358](https://github.com/fontanierh/boxology/issues/358) | [#107](https://github.com/fontanierh/boxology/issues/107) keeps own-source purity, source closure, uncoded-path catalog, deterministic generation, and no-project-code-execution gating; only the transitive graph-hygiene half is deferred |
| Cross-platform determinism comparison | [#525](https://github.com/fontanierh/boxology/issues/525) | Hosted CI validates Linux; equivalence across operating systems and architectures is not yet claimed |

## Recorded v0 exclusions

Decided during stream review; recorded so their absence reads as intent rather than oversight:

- **Distribution and publishing.** V0 was proved from a source checkout. The post-V0 `0.1.0`
  tool release uses crates.io for `boxology-cli` and `boxology-init`; the README documents install,
  update, pinning, and the Git source-build fallback. Initializer-generated projects remain pinned
  to their proven Git revision because their HTTP dependency is deliberately outside this first
  registry closure. Broader registry, provenance, and revocation design remains #12 scope.
- **The generic development CLI binding** — outside the V0 boundary.
- **Human-facing getting-started documentation** beyond the skill and the generated project's own README — minimal, inside S7.
- **Cross-platform support.** V0 is evidenced only on native macOS ARM64. Linux/x86 and any wider
  support claim require the deliberate proof owned by [#525](https://github.com/fontanierh/boxology/issues/525).
- **Authentication, providers, streaming, client-binding SDKs, foreign-language boxes** — all post-v0 per the merged scope.

## Historical sequencing notes

- The dependency spine is S0 → S1 → S2 → {S3, S4} → S5 → S6 → S7, with S5's manifest/ownership portion parallelizable from the start.
- Stream boundaries are also review boundaries: a pull request belongs to one task, a task to one stream, and cross-stream interface changes are made in the owning stream. S1 owns assembly semantics; transport streams implement against them.
