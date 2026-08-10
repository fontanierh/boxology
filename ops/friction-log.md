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
- Status (2026-08-04): the clause claiming retention of explicit-base product check is superseded; #531 kept the product check off the required path before #327 expanded it. The exact mechanical tradeoff is already recorded in the later #327 entry and is not restated.

## 2026-08-04 — #327

- Classification: `mechanical`
- Observation: S5 makes fmt, Clippy, and workspace tests part of `boxology check`; keeping the complete product command in required PR CI would duplicate those expanding full-workspace steps after directly changed-crate tests and recreate the merge bottleneck.
- Evidence: [#327](https://github.com/fontanierh/boxology/issues/327) owns the real check-step execution; required CI retains hygiene, repository invariants, directly changed-crate tests, root build-graph checks, and scoped reaper coverage, while dispatch-only [`deep-validation.yml`](../.github/workflows/deep-validation.yml) retains both canonical xtask validation and the complete product check. [#530](https://github.com/fontanierh/boxology/pull/530) established the pre-expansion required lane at 3m10s.

## 2026-08-04 — #525

- Classification: `mechanical`
- Observation: Four idle Linux JIT runners kept a dedicated Colima VM and roughly 388 MiB of live container memory resident even after required CI became native-Mac-only; their KeepAlive supervisors would recreate any container killed directly.
- Evidence: The two Linux launchd owners were disabled and unloaded only after GitHub reported all four runners `busy=false`; their supported EXIT cleanup removed every container and registration, and the dedicated Colima profile was stopped. The undispatchable Linux smoke workflow is removed here, while dormant provisioning source remains for explicit post-v0 cross-platform restoration under [#525](https://github.com/fontanierh/boxology/issues/525).

## 2026-08-04 — #328

- Classification: `semantic`
- Observation: S5 AC9/AC10 narrow V0 evidence to native-macOS ARM64 repeated-root determinism in the final exact-main deep run and golden-pinned emitted `ubuntu-latest` workflow bytes, transferring continuous cross-platform cadence and source-provisioned Linux workflow execution to the first-release boundary rather than claiming continuous multi-platform gating or live Linux execution before that release. #477 item 1 is V0-gating one-pass stale-import convergence; items 2–6 (cycle diagnostic locality, self-cycle/transitive-chain coverage, imported-path diagnostic invariant, BXW0068 retirement comment, and one degraded mutant) transfer as post-V0 diagnostic/test-quality residuals because they do not affect proven ordered-generation behavior or the current V0 corpus.
- Evidence: Existing decision chain [#522](https://github.com/fontanierh/boxology/issues/522)/[#525](https://github.com/fontanierh/boxology/issues/525) (native-Mac V0 evidence; post-V0 cross-platform restoration) and [#531](https://github.com/fontanierh/boxology/issues/531)/[#548](https://github.com/fontanierh/boxology/pull/548) (required PR lane runs no product `boxology check` command; dispatch-only deep validation owns the complete default-base check); normative edits land in [S5 D6/D7/D8/AC9/AC10/T6 and Matters left open](../specs/s5-manifest-and-validation.md).

## 2026-08-04 — #481

- Classification: `mechanical`
- Observation: Repo-wide residual scratch-root adoption and cleanup is automatable toil deferred post-V0. Accepted determinism-evidence helpers were already fixed in #483/#459; no accepted criterion or V0 behavioral/evidence claim is narrowed. Remaining failures are visible spurious rerun/temp-accumulation risk, not silent evidence falsification. Post-V0 implementation follows the namespaced `{name}-{pid}-{n}`, `create_dir`/AlreadyExists loop, and Drop pattern, after #107A-B settles overlapping files. Operational closure-run mitigation is a pre-dispatch sweep of stale `boxology-*` OS-temp directories, recorded as de-flaking.
- Evidence: [#481](https://github.com/fontanierh/boxology/issues/481); accepted helper fix [#483](https://github.com/fontanierh/boxology/pull/483)/[#459](https://github.com/fontanierh/boxology/issues/459); residual ledger [#343](https://github.com/fontanierh/boxology/issues/343) and [S7 D7](../specs/s7-skill-acceptance-self-hosting.md#d7--s7-complete-is-the-v0-gate); post-V0 sequencing after [#107](https://github.com/fontanierh/boxology/issues/107).

## 2026-08-04 — #107

- Classification: `semantic`
- Observation: S2's whole-tree claim narrows to implemented per-file staged commit (exclusive staging-name allocation), best-effort staging cleanup that preserves the original write error, scan-complete ASCII-case-rival refusal, fail-closed pre-write inspect and prune enumeration, ordered convergence, and re-run repair that prunes undeclared `.boxology-write-*` residue only when a declared output pattern matches it (otherwise the residue remains for operator cleanup); transactional whole-tree publication transfers to the named post-V0 issue.
- Evidence: Amended [S2 D1/T6](../specs/s2-contract-generator.md#d1--controlled-declaration-plus-ordinary-implementation), [build topology](../boxology-details/08-rust-build-topology.md), [S5](../specs/s5-manifest-and-validation.md)/[S6](../specs/s6-installer-and-generated-project.md) cross-references, and [#555](https://github.com/fontanierh/boxology/issues/555); writer tests pin exclusive staging/canary integrity, scan-complete rival refusal, propagated prune-walk failure with a live committed tree, staging failure with original-error preservation, truthful pre-write inspect diagnostics, repeated-write isolation, refusal-before-change, and prune-failure behavior, and the CLI surface lock anchors the sole writer call.

## 2026-08-09 — #560

- Classification: `mechanical`
- Observation: Trivial changes, including model/configuration edits, may bypass the full delivery loop, and advisory is reserved for critical decisions; planning or advisory that is used must optimize the pragmatic shortest honest shipping path. This removes mechanical ceremony without weakening product evidence and completes the final manual friction-entry audit.
- Evidence: [#560](https://github.com/fontanierh/boxology/pull/560) records the direct-change and advisor-sparing delivery policy; [#562](https://github.com/fontanierh/boxology/pull/562) records the pragmatic planning/advisory rule.

## 2026-08-09 — #342

- Classification: `mechanical`
- Observation: The planned return of full `boxology check` to required PR CI is relaxed because its indivisible baseline exceeds the required job's 20-minute timeout; required CI keeps fast root and changed-scope evidence while full regeneration, classification, edge, workspace-wide, and declared-quality enforcement remains dispatch-only/local.
- Evidence: [#342](https://github.com/fontanierh/boxology/issues/342) measured a cold explicit-base check at about 30 minutes; exact-main deep runs 31329847692 and 31328401257 measured 27:51 and 28:43. The [S0 amendment](../specs/s0-repo-bootstrap.md) and [S7 amendment](../specs/s7-skill-acceptance-self-hosting.md) record the lost pre-merge guarantee and forbid a policy-only workaround.

## 2026-08-10 — #593

- Classification: `mechanical`
- Observation: Five redundant mutation/end-to-end integration targets and two nested-Cargo generator unit tests move wholly off required PR CI rather than using a path selector that saved no time on the motivating PR. Their crates retain ordinary units, binaries, explicit doctests, and ordinary integration tests; dispatch-only product workspace tests execute all seven once while integrity-only source/body/list guards reject missing, renamed, or ignored exclusions without duplicate Cargo runs. This deliberately gives up relevant-change pre-merge lock/end-to-end evidence to remove a proven required-path bottleneck.
- Evidence: [#593](https://github.com/fontanierh/boxology/issues/593) records PR #592 GitHub timings of 74.59 seconds for the CLI end-to-end target, 5.64 seconds for the CLI surface target, and 45.56 seconds for the workspace surface mutation; the exact tradeoff and rollback are documented in the [runner runbook](ci-runner/README.md#current-ci-routing). Fresh pre-change baseline [PR #597](https://github.com/fontanierh/boxology/pull/597) completed required native-Mac validation in 5m53 in [Actions run 31350831322](https://github.com/fontanierh/boxology/actions/runs/31350831322). The trace reached multi-capability completion after about 52.5 seconds and sealed-import completion after about 81.4 additional seconds; 81.4 seconds is not a standalone sealed-test duration. The other 41 generator unit tests finished in about 1.3 seconds, directly supporting about 134 seconds of expected savings for that shape. This is baseline-only evidence; the revised CI PR supplies the first post-change GitHub timing.
- Status (2026-08-10): PR #601 passed on generic Mac slot 3 in 5m29; docs-only PR #602 then ran on the base runner in 5m41, with 5m03 in hygiene despite skipping Rust tests; warm-slot PR #603 still spent 2m20 in hygiene before entering all 158 xtask tests for a non-xtask product change. Phase A adds one base-only cache label and limits the xtask suite to xtask, workflow, runner-ops, or embedded Boxology-skill changes while keeping generic routing; rename detection is disabled so old authority paths remain visible. After the merged service is deployed and verified, phase B changes both the workflow route and its xtask integrity expectation to the primary label. Universal hygiene, changed-crate tests, and deep coverage remain; post-deployment Actions timing is the acceptance evidence.
