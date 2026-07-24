# S6 Spec — Installer and Generated Project

[Stream definition](../boxology-details/11-v0-streams.md#s6--installer-and-generated-project) · Status: **proposed** (awaiting review)

S6 builds the deterministic initializer and the project it generates: the Cargo workspace, the Hello box (implementation and generated contract), the application composition with in-process and HTTP bindings, the root platform package, all manifests, and repository CI — ending in the working database-free Hello World invocable through Rust and HTTP. The installation flow, generated-project contents, and milestone scenario are normative in the [Product Contract](../boxology-details/07-product-contract.md); the manifest model and check baseline in [Packages](../boxology-details/02-packages.md) and [Rust Build Topology](../boxology-details/08-rust-build-topology.md). This spec records implementation decisions and consumes the outputs of S1–S5; it does not restate them.

## Purpose

The generated project is the platform's first user-facing artifact and its acceptance vehicle: milestone steps 2–3 and 6–7 run inside it, and S7's acceptance task evolves it. The installer must therefore produce a repository that is **born valid** — green under `boxology check` from its first commit, with no post-generation repair step — and **born ordinary**: a plain Cargo workspace in the developer's Git repository, per the reversible-exit contract.

## Non-goals

- **No distribution or publishing.** The recorded v0 exclusion stands: the installer is consumed from a source checkout (`cargo install --path`); packaging, versioning channels, and release publishing arrive with the first post-v0 release for outside users.
- **No interactivity.** The onboarding skill's agent asks the developer the minimum necessary questions and passes explicit flags; the CLI itself prompts for nothing. Harness-neutrality lives in the skill (S7), not here.
- **No options.** One generated shape: the database-free Hello project. No templates, feature menus, provider scaffolding, persistence, or alternative layouts. Every option is a post-v0 decision with its own evidence.
- **No git, network, or toolchain execution.** The installer runs no `git`, `cargo`, `rustc`, or network access; it writes files. Brownfield and non-empty-target operation are excluded (fail-closed below).
- **No Windows**, per the merged validation baseline.

## Decisions

### D1 — A pure library and a thin CLI, mirroring S2

- **`boxology-init`** (library) is pure: `initialize(InitRequest) -> Result<GeneratedTree, Diagnostics>` — the complete project file tree as relative-path/byte pairs, with no filesystem, environment, network, clock, or execution access. It embeds S2's pure generation to emit the Hello box's contract crate, `schema.json`, and adapter, so the project is born with valid derived artifacts rather than a "run generate first" instruction.
- The **CLI** (a `boxology-init` binary in the same crate) owns the effects: request assembly from flags, fail-closed target validation, and atomic write of the complete tree through the same write discipline S2's orchestration uses (#107). It is a **separate binary from S5's `boxology`**: the release bundle names the installer as its own deliverable, the strategy review's stage 3 plans it as a standalone composition, and S5's command surface stays exactly `generate`/`check`.
- Diagnostics carry stable codes in a **`BXI####`** namespace, disjoint from `BXG`/`BXC`/`BXW`; there is no uncoded failure path.

`InitRequest` is minimal: target-relative project name and the workspace parameters the task specs prove necessary — every addition needs a use in the milestone scenario. The installer records its own version in the generated provenance.

### D2 — Fail-closed target, atomic write

The CLI refuses a target directory that is not empty (VCS metadata `.git/` alone is permitted — the developer's greenfield repository may already be initialized, per milestone step 1). It never overwrites, merges into, or repairs an existing tree; re-running against a generated project is a coded error, not an idempotent no-op. The complete tree is written atomically: a partially initialized project must not be observable.

### D3 — The generated tree

One workspace, three logical packages, per the normative model:

- **Root platform package**: `boxology.toml` owning root `Cargo.toml`, the repository CI workflow, and root machinery; `Cargo.lock` declared as the workspace's global derived artifact. It contains no Rust crate, as 02-packages permits.
- **Hello box**: `boxology.toml`, implementation crate (one controlled contract block plus ordinary inherent implementation), and generated `contract/` crate, `schema.json`, and `adapter/` — emitted by the embedded S2 generator, byte-identical to what regeneration produces. Its `[quality].commands` declare the in-process Rust and HTTP conformance tests, per check baseline step 8 — the checker has no Hello-specific branch.
- **Application composition**: `boxology.toml` and a composition crate that assembles the Hello box through S1's assembly API, binds it in-process, and exposes it over HTTP through S3's server binding — both bindings declared in the manifest's `[composition]` section.

The initial contract declares **one capability distinct from `greet`**, externally exposed and invocable through both bindings, chosen so that S7's acceptance task — adding `greet(name)` — is a purely additive change under S4's taxonomy. The exact initial contract shape, crate names, and directory layout are pinned by T2's task spec and frozen as goldens; where possible they align with the S1 fixture corpus (#100) so installer output and fixture proofs share machinery.

The generated README is the project's own minimal document: what was generated, how to build, invoke (Rust and HTTP), and validate (`boxology check`). Broader getting-started material is S7's.

### D4 — CI and the lockfile

The generated repository CI workflow is **S5's golden-pinned document** (S5 D7), written verbatim — S6 owns its placement, not its content. `Cargo.lock` is not emitted: the installer runs no toolchain, and the lockfile materializes on the developer's first ordinary Cargo invocation, exactly as the platform manifest declares it — a derived artifact attributed to the platform package. The generated README states this one-step expectation; `boxology check`'s freshness rule then holds from the first build onward.

### D5 — Determinism

Identical `InitRequest` and installer version produce byte-identical trees on Linux and macOS: sorted emission, no timestamps, absolute paths, environment values, or locale dependence — S2's D9 discipline applied to the whole project. T1 registers the generated tree as a real determinism subject with S0's harness. Provenance normalization follows S2's protocol so goldens stay comparable across generator releases.

### D6 — Born-valid conformance

The complete end-to-end proof is an S6-owned integration test in this repository: initialize into a temporary directory, run the real toolchain (`cargo build`, the generated quality commands), invoke the capability through the in-process Rust binding and through HTTP against the running composition, and run `boxology check` — all green, on both platforms. This is milestone steps 2–3 and 7 made mechanical; S7 owns steps 1, 4, 5, and 6.

## Acceptance criteria

1. The generated tree is byte-identical across repetition, roots, time, locale, timezone, Linux, and macOS; the T1 determinism subject is green in S0's gating lane from its first PR onward.
2. The generated project passes `boxology check` from its initial state with no repair step, on both platforms.
3. The capability answers correctly through the in-process Rust binding and through HTTP against the same composition, via the generated quality commands — with no Hello-specific branch in any checker.
4. The embedded generation is byte-identical to standalone regeneration: `boxology generate` on the fresh project rewrites nothing.
5. Fail-closed behavior is proven: non-empty target (beyond `.git/`), re-run against a generated project, and invalid request each fail with asserted `BXI` codes; an interrupted write leaves no partial project observable.
6. The emitted CI workflow byte-matches S5's golden document; the generated README documents build, both invocation paths, validation, and the first-build lockfile step.
7. Adding `greet(name)` to the generated Hello box classifies as additive under S4 against the initial schema — proven by a fixture pair, pre-validating S7's acceptance task.
8. The installer builds and runs from a source checkout via `cargo install --path` on both platforms.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | `boxology-init` pure library: `InitRequest`, tree assembly, embedded S2 generation, determinism subject | 2 |
| T2 | Generated project content: workspace, three packages, initial contract shape, composition wiring, README, quality commands, CI placement | 2–3 |
| T3 | CLI binary: flags, fail-closed target validation, atomic write, provenance/version | 1 |
| T4 | Born-valid closure: end-to-end integration proof (build, both invocations, check), goldens, cross-platform coverage | 2 |

T1 and T2 interleave as one stack (content is assembled by the library); T3 follows; T4 remains last, followed by the S6-COMPLETE check against this spec. The stream depends on S1–S5 artifacts: T1 consumes S2's generation and write orchestration (#107); T2 consumes S1 assembly, S3's server binding (#112), S5's workflow document (#327), and coordinates with the fixture corpus (#100); T4 consumes S5's `check` (#327) and S4's classification for AC7.

## Matters left open

- The exact initial contract shape, crate names, directory layout, and `InitRequest` fields — T2/T1 task-spec authority, frozen by goldens.
- Exact `BXI####` codes — task-spec work.
- Host-specific installer distribution, wrappers, and any interactive mode — post-v0 with the first outside-user release.
- Boxifying the installer itself — stage 3 of the self-hosting ladder, first rung of #74, deliberately after v0.

## Tracker notes

The stream definition in `11-v0-streams.md` gains its spec link in this PR's diff; no other normative document changes. On merge, the operator files the four task issues plus S6-COMPLETE with `stream:s6` labels, recording the cross-stream dependencies above. #100's premise gains a consumer (installer/fixture alignment) but no obligation; #74 and the recorded v0 exclusions are cited unchanged.
