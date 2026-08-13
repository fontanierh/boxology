# S6 Spec — Installer and Generated Project

[Stream definition](../boxology-details/11-v0-streams.md#s6--installer-and-generated-project) · Status: **delivered in V0**

S6 delivers the deterministic initializer and generated project: a Cargo workspace, the `ping` box (implementation and generated contract), the `ping-app` composition with in-process and HTTP bindings, the root platform package, manifests, README, and golden-pinned CI workflow bytes. The installation flow and milestone scenario are normative in the [Product Contract](../boxology-details/07-product-contract.md); the manifest/check baseline lives in [Packages](../boxology-details/02-packages.md) and [Rust Build Topology](../boxology-details/08-rust-build-topology.md).

## Purpose

The generated project is the platform's first user-facing artifact and its acceptance vehicle: milestone steps 2–3 and 6–7 run inside it, and S7's clean acceptance run evolves a generated instance. The installer must therefore produce a repository that is **born valid** — green under `boxology check` after the documented first ordinary Cargo invocation, with no repair step beyond that one build — and **born ordinary**: a plain Cargo workspace in the developer's Git repository, per the reversible-exit contract.

## Non-goals

- **No crates.io publishing in V0.** Registry packaging and versioned release channels remain post-v0. The current outside-user bridge uses standard `cargo install --git`; this does not retroactively widen the V0 evidence claim.
- **No interactivity.** The onboarding skill's agent asks the developer the minimum necessary questions and passes explicit flags; the CLI itself prompts for nothing. Harness-neutrality lives in the skill (S7), not here.
- **No options.** One generated shape: the database-free `ping` project. No templates, feature menus, provider scaffolding, persistence, or alternative layouts. Every option is a post-v0 decision with its own evidence.
- **No git, network, or toolchain execution.** The installer runs no `git`, `cargo`, `rustc`, or network access; it writes files. Brownfield and non-empty-target operation are excluded (fail-closed below).
- **No cross-platform support claim.** V0 evidence is native macOS ARM64; #525 owns wider proof.

## Decisions

### D1 — A pure library and a thin CLI, mirroring S2

- **`boxology-init`** (library) is pure: `initialize(InitRequest) -> Result<GeneratedTree, Diagnostics>` — the complete project file tree as relative-path/byte pairs, with no filesystem, environment, network, clock, or execution access. It embeds S2's pure generation to emit `ping`'s contract crate, `schema.json`, and adapter, so the project is born with valid derived artifacts rather than a "run generate first" instruction.
- The **CLI** (a `boxology-init` binary in the same crate) owns the effects: request assembly from flags, fail-closed target validation, and sentinel-gated staged write through D2's own staged-directory-plus-sentinel mechanism. It is a **separate binary from S5's `boxology`**: the release bundle names the installer as its own deliverable, the strategy review's stage 3 plans it as a standalone composition, and S5's command surface stays exactly `generate`/`check`.
- Diagnostics carry stable codes in a **`BXI####`** namespace, disjoint from `BXG`/`BXC`/`BXW`; there is no uncoded failure path.

`InitRequest` now contains exactly the project name. The generated workspace keeps exact `=0.0.0` dependency declarations and obtains framework crates from one full revision of the public Boxology Git repository. The first ordinary Cargo build also records that commit in `Cargo.lock`. No host checkout path is embedded and no root `[patch.crates-io]` override is emitted. The portable repository source is recorded in generated provenance. A versioned registry release can replace this Git bridge later without changing member-crate dependency declarations. The installer records its own version in generated provenance.

### D2 — Fail-closed target, sentinel-gated staged write

The installer writes into the target root directly — the greenfield repository root becomes the workspace root; the project name names the workspace and packages, not a created subdirectory. The CLI refuses a target that is not empty (VCS metadata `.git/` alone is permitted — the developer's greenfield repository may already be initialized, per milestone step 1), and the diagnostic names every offending entry; ensuring the target is actually empty is the onboarding flow's job, owned by S7's skill. It never overwrites, merges into, or repairs an existing tree; re-running against a generated project is a coded error, not an idempotent no-op.

The write mechanism is staged: the complete tree is materialized in a same-filesystem staging directory, moved into place by ordered top-level renames, and finalized by writing the test-pinned completion sentinel last. A crash mid-sequence cannot yield a tree bearing the sentinel; the fail-closed re-run check treats a sentinel-less partial tree as a non-empty target and reports it for manual cleanup. AC5's no-partial-project claim is scoped to this mechanism.

### D3 — The generated tree

One workspace, three logical packages, per the normative model:

- **Root platform package**: `boxology.toml` owning root `Cargo.toml`, `rust-toolchain.toml` (the pinned toolchain check baseline step 6 requires), the repository CI workflow, the platform generator configuration recording the boxology version and dependency source (02-packages assigns generator configuration to the platform manifest), and root machinery; `Cargo.lock` declared as the workspace's global derived artifact with the non-S2 generator identity `cargo` and a freshness-only regeneration check, per S5 D5/D6. It contains no Rust crate, as 02-packages permits.
- **`ping` box**: `boxology.toml`, implementation crate (one controlled contract block plus ordinary inherent implementation), and generated `contract/` crate, `schema.json`, and `adapter/` — emitted by the embedded S2 generator, byte-identical to what regeneration produces.
- **`ping-app` composition**: `boxology.toml` and a composition crate that assembles `ping` through S1's assembly API, binds it in-process, and exposes it over HTTP through S3's server binding — both bindings declared in the manifest's `[composition]` section. Its `[quality].commands` own the real Rust/HTTP conformance test; the checker has no project-specific branch.

The initial contract is externally exposed `ping(nonce: u64) -> Result<u64, HelloError>`, distinct from `greet`, so S7's real `greet(name)` addition is `additive` under S4. The contract shape, crate names, and layout are frozen by generated-project goldens.

The generated README is the project's own minimal document: what was generated, how to build, invoke (Rust and HTTP), and validate (`boxology check`). Broader getting-started material is S7's.

### D4 — CI and the lockfile

The generated repository CI workflow is **S5's golden-pinned document** (S5 D7), written verbatim — S6 owns its placement, not its content. V0 pins its bytes but does not claim execution of the emitted Linux workflow; that first-release proof is #525 scope. `Cargo.lock` is not emitted: the installer runs no toolchain, and the lockfile materializes on the developer's first ordinary Cargo invocation, exactly as the platform manifest declares it — a derived artifact attributed to the platform package. The generated README states this one-step expectation; `boxology check`'s freshness rule then holds from the first build onward.

### D5 — Determinism

Identical `InitRequest` and installer version produce byte-identical trees: sorted emission, no timestamps, absolute paths, environment values, or locale dependence — S2's D9 discipline applied to the whole project. The generated-tree determinism subject proves repetition, roots, time, locale, and timezone on native macOS ARM64. Provenance normalization follows S2's protocol; cross-platform re-proof is [#525](https://github.com/fontanierh/boxology/issues/525) scope.

### D6 — Born-valid conformance

The complete end-to-end proof is one S6-owned integration test run natively on macOS ARM64: initialize into a temporary directory, `git init` and commit the initial tree (so `check`'s base default resolves per S5 D6), run `cargo build` first to materialize the declared lockfile, run generated quality commands, invoke through the in-process Rust binding and real HTTP composition, and run `boxology check` — all green. The proof also asserts that `Cargo.lock` classifies as the platform package's derived artifact.

## Acceptance criteria

1. The generated tree is byte-identical across repetition, roots, time, locale, and timezone in native macOS ARM64 V0 evidence; cross-platform proof is #525 scope.
2. In one native-macOS born-valid run, the generated project passes `boxology check` after the documented first ordinary Cargo invocation, with no other repair step.
3. The capability answers correctly through the in-process Rust binding and through HTTP against the same composition, via the generated quality commands — with no generated-project-specific branch in any checker.
4. The embedded generation is byte-identical to standalone regeneration: `boxology generate` on the fresh project rewrites nothing. V0 assumed same-checkout tools; the current Git-installed bridge uses the generated project's lockfile to pin framework dependencies while registry release compatibility remains future work.
5. Fail-closed behavior is proven: non-empty target (beyond `.git/`), re-run against a generated project, and invalid request each fail with asserted `BXI` codes; an interrupted write never yields a tree bearing the completion sentinel, and a sentinel-less partial tree is detected and reported by the re-run check.
6. The emitted CI workflow byte-matches S5's golden document; the generated README documents build, both invocation paths, validation, and the first-build lockfile step.
7. The historical V0 proof installed from a source checkout. Current onboarding additionally proves the documented `cargo install --git` path; the real `greet(name)` evolution and its additive classification remain proved once in S7's acceptance run (#340), not duplicated here.

## Matters left open

- Host-specific installer distribution, wrappers, and any interactive mode — post-v0 with the first outside-user release.
- Boxifying the installer itself is future framework dogfooding work.
