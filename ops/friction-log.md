# Friction log

This is the standing record of friction found while applying the Boxology discipline. Every entry is dated and classified exactly once:

- `mechanical`: automatable toil — the factory's future job, and evidence for continuing.
- `semantic`: fighting the box boundaries themselves — thesis damage.

Each entry is, in order: a `## YYYY-MM-DD — #issue` heading, one blank line, one `Classification`, one `Observation`, and one `Evidence` line. Exact `- Status (YYYY-MM-DD): ...` annotations may follow any of those three fields. After merge, established bytes are immutable; complete entries may only be appended at EOF, separated by one blank line. Periodic records summarize and cite this log.

## 2026-07-26 — #358

- Classification: `mechanical`
- Observation: `boxology-manifest` needs canonical identity types from `boxology-contract`, which transitively pulls `tokio-util` into a specified-pure consumer.
- Evidence: [#358](https://github.com/fontanierh/boxology/issues/358) and the [S5 implementation record](../records/2026-07-25-s5-implementation-and-test-integrity.md#also-filed).
- Status (2026-07-26): Deferred; revisit only if the T4/T5 `boxology` CLI has a genuine minimal-dependency constraint.

## 2026-08-03 — #488

- Classification: `mechanical`
- Observation: Eight generator tests each create isolated Cargo targets and repeat deep compile or negative-matrix evidence, while documentation, editor, and fixture lint checks extend every pull request's critical path despite being independently recoverable after merge.
- Evidence: [#488](https://github.com/fontanierh/boxology/issues/488) moves those checks plus workspace Clippy to explicit main-push `cargo xtask ci --no-budget`; pull requests retain workspace tests, two positive nested-Cargo generator capstones, fixture formatting and tests, key ordering, generation purity, and determinism. A fresh Mac runner measured the sequential Clippy/test compile profiles beyond ten minutes, while the slim Linux lane completed in 1m22s.

## 2026-08-03 — #335

- Classification: `semantic`
- Observation: V0 requires one native-macOS born-valid/install behavioral run instead of duplicating the same real-toolchain proof on Linux; the independent Linux/macOS byte-determinism matrix remains gating.
- Evidence: [#335](https://github.com/fontanierh/boxology/issues/335) and [S6 D5–D6](../specs/s6-installer-and-generated-project.md#d5--determinism).

## 2026-08-03 — #340

- Classification: `semantic`
- Observation: The synthetic S6 fixture-pair pre-proof is removed; the real `greet(name)` evolution in the clean S7 acceptance run is the sole additive-classification proof.
- Evidence: [#340](https://github.com/fontanierh/boxology/issues/340) and [S7 D3](../specs/s7-skill-acceptance-self-hosting.md#d3--the-acceptance-run-and-its-evidence-protocol).

## 2026-08-03 — #342

- Classification: `mechanical`
- Observation: Xtask registry absorption is deferred from the v0 gate to the first task immediately post-v0 because removing the temporary duplicate lists is automatable cleanup; root manifests and `boxology check` in repository CI preserve the box-boundary semantics during the bounded transition.
- Evidence: [#342](https://github.com/fontanierh/boxology/issues/342) and [S7 D5](../specs/s7-skill-acceptance-self-hosting.md#d5--the-absorption-immediately-post-v0).

## 2026-08-03 — #343

- Classification: `semantic`
- Observation: V0 completion requires closed stream evidence audits plus an explicit ordered ledger of already-excluded, deferred, or otherwise non-gating follow-ups, rather than literal zero-open tracker debt; every accepted criterion still needs evidence or a merged normative re-scope.
- Evidence: [#343](https://github.com/fontanierh/boxology/issues/343) and [S7 D7](../specs/s7-skill-acceptance-self-hosting.md#d7--s7-complete-is-the-v0-gate).

## 2026-08-03 — #497

- Classification: `mechanical`
- Observation: Twenty JIT slots per platform exceeded the MacBook's useful CPU and memory capacity, fragmented per-slot Cargo caches, and added contention without reducing the required-check critical path.
- Evidence: [#497](https://github.com/fontanierh/boxology/issues/497) bounds the repo-owned topology to one base plus three standard slots for each platform, eight runners total, and retires both ten-slot expansion LaunchAgents.

## 2026-08-03 — #503

- Classification: `mechanical`
- Observation: The independently recoverable process-reaper suite and `boxology-init` package test were serialized inside the canonical Mac pull-request job, leaving available native runner capacity idle while that job determined the required-check critical path.
- Evidence: [#503](https://github.com/fontanierh/boxology/issues/503) splits those suites into a required parallel native Mac capstone. Run 30828942886 measured `checks-macos` at 11m04s versus `checks-linux` at 4m22s; main-push deep CI retains the unchanged full-workspace test.
- Status (2026-08-03): Round two also delegates the generator, workspace, and xtask package tests, shallow fixture-project checks, and external-test integrity gates; both native Mac jobs drop Actions cache traffic, the workflow globally drops debuginfo, and main-push deep coverage remains unchanged.
- Status (2026-08-03): Round three isolates the born-valid integration test in a third required native Mac lane; the original capstone retains every other delegated test and main-push deep coverage remains unchanged.

## 2026-08-04 — #343

- Classification: `semantic`
- Observation: V0's required evidence corpus is fixed to the four landed fixtures (`hello`, `greeter`, `ping`, `ping-app`); the full-grammar authoring evidence — `kitchen-sink`, structured/container grammar, named payloads, Blob/Secret end-to-end coverage, and the capability name override — is deferred to named post-v0 residuals, narrowing what v0 proves about the box boundary model itself without weakening runtime behavior, classification taxonomy, determinism, born-valid, or clean-run evidence.
- Evidence: [V0 Streams residual section](../boxology-details/11-v0-streams.md#post-v0-residuals-recorded-at-the-corpus-decision); residual owners [#100](https://github.com/fontanierh/boxology/issues/100), [#102](https://github.com/fontanierh/boxology/issues/102), [#104](https://github.com/fontanierh/boxology/issues/104), [#480](https://github.com/fontanierh/boxology/issues/480), [#358](https://github.com/fontanierh/boxology/issues/358); ledger [#343](https://github.com/fontanierh/boxology/issues/343).

## 2026-08-04 — #522

- Classification: `semantic`
- Observation: Continuous multi-platform and deep PR validation are relaxed so V0 delivery is gated only by one native Apple-silicon Mac job; Linux/x86/cross-platform cadence and continuous determinism comparison move off the merge critical path.
- Evidence: [#522](https://github.com/fontanierh/boxology/issues/522) measured self-hosted Actions validation duration at 5m42s for a code change; [#518](https://github.com/fontanierh/boxology/pull/518) measured self-hosted Actions validation duration at 1m11s for a docs-only change. [#526](https://github.com/fontanierh/boxology/pull/526) then took 6m37s: 4m14s in the full workspace sweep, 44s in init units, 31s in ping-app, and 34s in the reaper suite. This reconciliation removes those unconditional sweeps from the PR path, retaining xtask invariants, directly changed-crate tests, explicit-base `boxology check`, and root build-graph checks while moving complete coverage to dispatch-only `deep-validation.yml`; [#527](https://github.com/fontanierh/boxology/pull/527) proved the replacement lane green in 2m19s (about 65% faster), and [#525](https://github.com/fontanierh/boxology/issues/525) owns post-V0 cross-platform restoration.

## 2026-08-04 — #327

- Classification: `mechanical`
- Observation: S5 makes fmt, Clippy, and workspace tests part of `boxology check`; keeping the complete product command in required PR CI would duplicate those expanding full-workspace steps after directly changed-crate tests and recreate the merge bottleneck.
- Evidence: [#327](https://github.com/fontanierh/boxology/issues/327) owns the real check-step execution; required CI retains hygiene, repository invariants, directly changed-crate tests, root build-graph checks, and scoped reaper coverage, while dispatch-only [`deep-validation.yml`](../.github/workflows/deep-validation.yml) retains both canonical xtask validation and the complete product check. [#530](https://github.com/fontanierh/boxology/pull/530) established the pre-expansion required lane at 3m10s.
