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
