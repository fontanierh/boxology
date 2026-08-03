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
- Evidence: [#488](https://github.com/fontanierh/boxology/issues/488) moves those checks to explicit main-push `cargo xtask ci --no-budget`; pull requests retain workspace tests and clippy, two positive nested-Cargo generator capstones, fixture formatting and tests, key ordering, generation purity, and determinism.
