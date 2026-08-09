# V0 Streams

[Back to the white paper](../boxology-whitepaper.md)

This document defines the high-level workstreams for implementing the v0 foundation milestone. It is the top level of the [v0 execution methodology](../AGENTS.md#v0-execution-methodology): each stream will receive its own spec and task list before implementation, and every task resolves into a stack of small, individually reviewable pull requests.

Streams partition the v0 scope defined by the [product contract](07-product-contract.md). They are workstreams, not strict phases: later streams depend on earlier ones, but work overlaps where dependencies allow.

## S0 — Product-repo bootstrap and CI

The infrastructure every other stream stands on: the Cargo workspace scaffold for Boxology's own crates, the pinned toolchain, the determinism harness, and pull-request validation for this repository. After V0, #342 makes `cargo xtask ci` the sole owner of one full `boxology check`, while the required native Apple-silicon Mac PR lane stays lean: hygiene, repository invariants, directly changed crates, conditional root build-graph and opaque-fixture checks, and scoped process-reaper coverage. The full product check remains dispatch-only because measured runs exceed the 20-minute PR timeout; no policy-only product mode exists. Continuous Linux/x86/cross-platform validation and determinism comparison are post-V0 residual work. Lands first; serves everyone. Spec: [S0 — Product-Repo Bootstrap and CI](../specs/s0-repo-bootstrap.md).

## S1 — Runtime core and composition assembly

The kernel crates that everything else consumes: `CallContext`, the `ContractType` and `ContractError` model, the presence model (`T` / `Option<T>` / `Field<T>`), dedicated binary and sensitive-value types, and the asynchronous fallible handle contract with the domain/invocation error split. S1 also owns the **composition assembly API**: binding registration and validation (reject missing, duplicate, or incompatible bindings before accepting traffic), exposure-versus-declared-maximum checks, handle wiring, and the in-process binding. Transports implement their bindings against this API rather than defining their own assembly semantics. This is the definitionally non-box layer — the substrate boxes are made of. Normative inputs: [Canonical Capability Contract](09-capability-contract.md). Spec: [S1 — Runtime Core and Composition Assembly](../specs/s1-runtime-core.md).

## S2 — Contract generator

The deterministic contract generator: structural discovery of one controlled contract block and its ordinary inherent implementation, pure pre-Cargo parsing and emission, and normal-compile signature proof. It emits the sole public boundary types, language-neutral schema, stable identities and revision, typed handles, dispatch interface, adapter, provenance, and cross-platform deterministic bytes. The v0 long pole; the milestone is not complete until generation, provenance, reproducibility, compilation, and typed invocation work as one path. Contract-change *classification* is deliberately not here — S2 emits the schema and revision; S4 judges changes. Normative inputs: [Rust Build Topology](08-rust-build-topology.md), [Canonical Capability Contract](09-capability-contract.md). Depends on S1 for the types the generated code targets. Spec: [S2 — Contract Generator](../specs/s2-contract-generator.md).

## S3 — HTTP binding

The v1 HTTP transport, implemented against S1's assembly API: `POST /rpc/{box_id}/{capability_id}` routing, the lossless JSON mapping, the three response envelopes, the status mapping, context headers (`Boxology-Timeout-Ms`, `traceparent`/`tracestate`, `Idempotency-Key`), request limits, advisory cancellation on disconnect, and the binding-conformance test suite. Normative inputs: the Foundation HTTP binding section of [Runtime](03-runtime.md). Depends on S1 and S2. Spec: [S3 — HTTP Binding](../specs/s3-http-binding.md).

## S4 — Contract-change classification

The compatibility authority as its own deliverable: consuming S2's schemas, diffing a submitted revision against a base revision, and classifying every change under the compatibility taxonomy — additive, compatible-with-conditions, incompatible tightening or removal — with precise diagnostics. The taxonomy details left open by the capability-contract design are resolved by this stream's spec. Classification output is consumed by S5's `check` and reported even when harness policy later authorizes an incompatible change. This is the most thesis-critical single component: it is what makes "mechanical compatibility check" a fact rather than a promise. Depends on S2's schema format. Spec: [S4 — Contract-Change Classification](../specs/s4-contract-change-classification.md).

## S5 — Manifest and validation tooling

`boxology.toml` parsing and workspace discovery, ownership and path classification, crate-role mapping against Cargo metadata, the Cargo-edge policy checker, shared-lockfile rules, and the `boxology generate` / `boxology check` commands with their exit codes, diagnostics, JSON output, and the emitted GitHub Actions workflow. Normative inputs: [Packages](02-packages.md), [Rust Build Topology](08-rust-build-topology.md). Depends on S2 for regeneration checks and S4 for classification; the manifest/ownership half is parallelizable from the start. Spec: [S5 — Manifest and Validation Tooling](../specs/s5-manifest-and-validation.md).

## S6 — Installer and generated project

The deterministic initializer: creating the Cargo workspace, the Hello box (implementation and generated contract), the application composition with in-process and HTTP bindings, the root platform package, all manifests, and repository CI — ending in the working database-free Hello World invocable through Rust and HTTP. Normative inputs: [Product Contract](07-product-contract.md) and the outputs of S1–S5. Depends on all of S1–S5 producing usable artifacts. Spec: [S6 — Installer and Generated Project](../specs/s6-installer-and-generated-project.md).

## S7 — Skill, acceptance, and stage-2 self-hosting

The portable Agent Skills-format skill that teaches a coding agent the box model and names it the lead; the end-to-end acceptance run of the foundation scenario, including the `greet(name)` task; and stage 2 of the [self-hosting ladder](10-strategy-review.md#self-hosting-ladder) — adopting `boxology.toml` manifests and `boxology check` on this product repository itself, including the standing friction log required by the dogfooding pain discriminator. Depends on S0–S6. Spec: [S7 — Skill, Acceptance, and Stage-2 Self-Hosting](../specs/s7-skill-acceptance-self-hosting.md).

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
| General structured/container authoring grammar (struct/data-enum declarations, `Option`/`Vec`/`BTreeMap`/`Field`/`Secret` authoring, forward-reference and recursion resolution) | [#102](https://github.com/fontanierh/boxology/issues/102) | No v0 E2E claim; unsupported emission paths retain their coded gates |
| Named-field payload emission (`BXG0048`) and `Blob`/`Secret` end-to-end generator/binding coverage (`BXG0040`) | [#104](https://github.com/fontanierh/boxology/issues/104) | Fail-closed at generation; `Secret` redaction proven at kernel level only |
| Capability `name` override (wire-vs-Rust identity split) | [#480](https://github.com/fontanierh/boxology/issues/480) | Partial parser/model handling is not v0 support; wire name = Rust fn name in v0 |
| Transitive dependency-graph purity / identity-value extraction (`boxology-contract` → `tokio`) | [#358](https://github.com/fontanierh/boxology/issues/358) | [#107](https://github.com/fontanierh/boxology/issues/107) keeps own-source purity, source closure, uncoded-path catalog, deterministic generation, and no-project-code-execution gating; only the transitive graph-hygiene half is deferred |
| Continuous Linux/x86/cross-platform validation and determinism comparison (including whether to reactivate, replace, or decommission the dormant Mac-hosted Linux ARM64 JIT lane) | [#525](https://github.com/fontanierh/boxology/issues/525) | V0 gates on native macOS ARM64 only; deep Mac validation is dispatch-only; cross-platform evidence required before the first pinned external release |

## Recorded v0 exclusions

Decided during stream review; recorded so their absence reads as intent rather than oversight:

- **Distribution and publishing.** V0 is for this project's bootstrap phase; it is consumed from a source checkout (`cargo install --path`, local skill file). Packaging, versioning, crates.io/GitHub-release publishing, and skill delivery become in-scope with the first release intended for users who are not us, after v0. The product contract's release-bundle section describes that bundle's *contents*, not a v0 distribution channel.
- **The generic development CLI binding** — post-v0; it arrives as part of the tool-boxification rung of [issue #74](https://github.com/fontanierh/boxology/issues/74).
- **Human-facing getting-started documentation** beyond the skill and the generated project's own README — minimal, inside S7.
- **Windows** — the supported matrix is Linux (factory and CI) and macOS (local), per the merged validation baseline.
- **Authentication, providers, streaming, client-binding SDKs, foreign-language boxes** — all post-v0 per the merged scope.

## Sequencing notes

- The dependency spine is S0 → S1 → S2 → {S3, S4} → S5 → S6 → S7, with S5's manifest/ownership portion parallelizable from the start.
- Stage 3 of the self-hosting ladder — boxifying the tools themselves — deliberately comes **after** v0: tools are built conventionally as platform packages first, then boxified as the first rung of the [issue #74](https://github.com/fontanierh/boxology/issues/74) commitment.
- Stream boundaries are also review boundaries: a pull request belongs to one task, a task to one stream, and cross-stream interface changes are made in the owning stream. S1 owns assembly semantics; transport streams implement against them.
