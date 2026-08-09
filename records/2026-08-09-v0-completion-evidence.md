# 2026-08-09 — V0 completion evidence

## Outcome

Exact `main` content commit `8ba0365707e5ca5757b2b67635cd04d3a379bdc6` is the V0
candidate. Replacement deep-validation
[run 31328401257](https://github.com/fontanierh/boxology/actions/runs/31328401257) records
`success` on `macos-arm64-host` (`macOS/ARM64`), host `aarch64-apple-darwin`, with Rust
`1.97.1`, after executing:

```sh
cargo xtask ci --no-budget
cargo run -q -p boxology-cli --bin boxology -- check
```

This record must not merge unless those placeholders are replaced by successful exact-run
evidence. Run A's log must contain `PASS` markers for both the `generator-model` and
`workspace-report` subjects in all six contexts — `baseline`, `repeat`, `path`, `time`, `locale`,
and `timezone` — plus `check result passed` from the complete default-base check above. It records
the candidate. The same run records `fixture-projects: PASS`, `generator-deep-tests: PASS`,
`generator-source-inventory: PASS`, and overall `summary: PASS`. This record does not declare V0.
After it merges, Run B must
pass on the resulting exact `main`, then
[#108](https://github.com/fontanierh/boxology/issues/108),
[#328](https://github.com/fontanierh/boxology/issues/328),
[#109](https://github.com/fontanierh/boxology/issues/109), and
[#329](https://github.com/fontanierh/boxology/issues/329) close in dependency order and
[#343](https://github.com/fontanierh/boxology/issues/343) closes to declare V0. Any intervening
content repair resets the evidence SHA.

## Truthful V0 boundary

- V0 is evidenced on native macOS ARM64. Continuous Linux, x86, and cross-platform comparison
  remain under [#525](https://github.com/fontanierh/boxology/issues/525) and are required before
  the first pinned external release.
- Generated output publication is atomic per changed file: all changed bytes stage before declared
  paths change, scan-complete ASCII-case alias rivals are refused, and pre-write inspection and
  prune enumeration fail closed. Each file is replaced by a same-directory atomic rename, and
  stale outputs are pruned per file. Best-effort staging cleanup may leave visible unmatched
  `.boxology-write-*` residue for operator cleanup without changing declared paths. V0 does not
  claim whole-tree rollback, journaling, or transactionality; a reported mixed tree converges on
  rerun.
  [#555](https://github.com/fontanierh/boxology/issues/555) owns the stronger boundary.

## Evidence chain

| Stream | Evidence status |
| --- | --- |
| S0 — repository and validation foundation | Completion audit [#93](https://github.com/fontanierh/boxology/issues/93) closed; current native-Mac topology reconciled through [#272](https://github.com/fontanierh/boxology/issues/272) |
| S1 — runtime core | Completion audit [#101](https://github.com/fontanierh/boxology/issues/101) closed |
| S2 — contract generator | [#108](https://github.com/fontanierh/boxology/issues/108) and completion audit [#109](https://github.com/fontanierh/boxology/issues/109) remain open; their required full reconciliation comments and tracker-only closure follow Run B, and they are not claimed closed here |
| S3 — HTTP binding | Completion audit [#116](https://github.com/fontanierh/boxology/issues/116) closed |
| S4 — classification | Completion audit [#321](https://github.com/fontanierh/boxology/issues/321) closed |
| S5 — manifest and validation tooling | [#328](https://github.com/fontanierh/boxology/issues/328) and completion audit [#329](https://github.com/fontanierh/boxology/issues/329) remain open; their required full reconciliation comments and tracker-only closure follow Run B, and they are not claimed closed here |
| S6 — installer and generated project | Completion audit [#336](https://github.com/fontanierh/boxology/issues/336) closed |

S7's portable skill, friction log, and stage-2 adoption closed in
[#338](https://github.com/fontanierh/boxology/issues/338),
[#339](https://github.com/fontanierh/boxology/issues/339), and
[#341](https://github.com/fontanierh/boxology/issues/341). The clean acceptance run closed
[#340](https://github.com/fontanierh/boxology/issues/340) and is preserved in the
[clean acceptance record](2026-08-03-foundation-acceptance-clean.md): Rust and HTTP both returned
`Hello, Ada!`, `ping.greet` was additive, and no foreign package source changed. The final S7 gate
remains #343 after Run B. The earlier
[classification-failure record](2026-08-03-foundation-acceptance-failed.md) and
[unavailable-skill record](2026-08-03-foundation-acceptance-skill-unavailable-failed.md) remain
preserved as failed-run evidence, not partial successes.

## Ordered post-V0 residuals

1. [#342](https://github.com/fontanierh/boxology/issues/342) starts first: absorb duplicated xtask
   platform checks into `boxology check` without dropping or double-running checks.
2. [#551](https://github.com/fontanierh/boxology/issues/551) may run beside it as the independent
   delivery-worker process-group reaper lane; [#481](https://github.com/fontanierh/boxology/issues/481)
   follows #342 and #551 for remaining test-scratch adoption and leak hardening.
3. [#102](https://github.com/fontanierh/boxology/issues/102) owns structured/container grammar,
   [#104](https://github.com/fontanierh/boxology/issues/104) owns named-field and `Blob`/`Secret`
   emission, and [#100](https://github.com/fontanierh/boxology/issues/100) owns the resulting full
   `kitchen-sink` fixture. Unsupported V0 paths remain fail-closed.
4. [#480](https://github.com/fontanierh/boxology/issues/480) owns capability wire-name override;
   [#477](https://github.com/fontanierh/boxology/issues/477) retains S5 diagnostic/test-quality
   items 2–6; [#358](https://github.com/fontanierh/boxology/issues/358) owns transitive dependency
   purity; and [#555](https://github.com/fontanierh/boxology/issues/555) owns whole-tree publication.
5. [#74](https://github.com/fontanierh/boxology/issues/74) forces stage-3 tool boxification
   uncomfortably early after #342 and its minimum grammar prerequisites. Its first-release boundary
   also owns the pinned-prior-release generator self-validation that cannot exist before V0 because
   V0 publishes no release.
6. [#525](https://github.com/fontanierh/boxology/issues/525) restores deliberate Linux/x86 and
   cross-platform determinism evidence before the first pinned external release.

## Recorded V0 exclusions

- Distribution, versioning, crates.io/GitHub-release publishing, and skill delivery wait for the
  first release intended for outside users; V0 is consumed from a source checkout and local skill.
- The generic development CLI binding arrives with #74's post-V0 tool-boxification rung.
- Human-facing getting-started documentation beyond the skill and generated-project README remains
  post-V0.
- Windows support remains post-V0.
- Authentication, providers, streaming, client-binding SDKs, and foreign-language boxes remain
  post-V0 under their accepted stream scopes.

## Separately authorized tooling

[#248](https://github.com/fontanierh/boxology/issues/248) is coordinator tooling, not a V0 product
residual. Live Telegram enabling, pairing, polling, listening, or sending still requires an explicit
human request; this record and the V0 declaration grant no such authority.
