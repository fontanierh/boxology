# S0 Spec — Product-Repo Bootstrap and CI

[Stream definition](../boxology-details/11-v0-streams.md#s0--product-repo-bootstrap-and-ci) · Status: **accepted at merge** (two review rounds addressed; cross-stream contract in issue #85)

This is the stream specification for S0 under the [v0 execution methodology](../AGENTS.md#v0-execution-methodology). It defines the infrastructure the product repository needs before any platform code lands: the Rust workspace, the pinned toolchain, pull-request validation, mechanical enforcement of the repository's working rules, and the cross-platform determinism harness — proven against synthetic subjects before the generator exists.

## Purpose

S0 exists so every subsequent stream inherits, on day one: a workspace whose toolchain, formatting, and lint posture cannot drift silently; pull-request validation that mechanically enforces what is currently enforced by assertion; and a determinism harness whose detection power is itself tested, so a green harness is a tested claim when S2 registers the real generator.

## Non-goals

- No platform functionality: no runtime, generator, binding, manifest, or installer code.
- No `boxology.toml` on this repository — stage-2 self-hosting (S7) requires S5's tooling.
- No publishing, no Windows, no coverage/benchmarking/release automation (recorded v0 exclusions).
- **No semantic self-protection.** S0's CI definitions (`pr.yml`, `crates/xtask`) are candidate-writable: a pull request can modify the very checks that judge it, and the same-named check would pass. Until S5/S7 provide base-revision policy evaluation, this bootstrap CI is protected only by human review of changes to those paths. This is stated honestly rather than implied away; issue #17 is reconciled with this position.

## Decisions

### D1 — Repository layout and hygiene

```text
/Cargo.toml               # workspace root (virtual manifest)
/Cargo.lock               # committed
/rust-toolchain.toml      # pinned toolchain
/rustfmt.toml             # committed formatting config (hand-authored sources only; see D3)
/deny.toml                # dependency policy (D6)
/.cargo/config.toml       # [alias] xtask = "run -p xtask --"
/.gitattributes           # LF normalization + binary exceptions (authoritative for bytes)
/.editorconfig            # editor guidance only
/crates/                  # platform crates arrive from S1 onward
/crates/xtask/            # repo automation — the workspace's first member
/specs/                   # stream and task specs
/boxology-details/        # design documents (unchanged)
/.github/workflows/       # pr.yml, scheduled advisory job
```

`.gitattributes` (`* text=auto eol=lf` plus explicit binary exceptions) is the enforcement point for line endings — `.editorconfig` only guides cooperating editors, and source bytes feed generation, so Git-level normalization is the rule that matters. Linear history on `main` via squash merge is the accepted policy (#144), and repository merge-method settings enforce it for pull-request merges: only squash merging is enabled (#154). That setting governs the PR merge method only — branch protection remains unavailable, so direct pushes to `main` are not prevented (see D9).

### D2 — `xtask` is the automation home, with defined CI parity

Repository automation is a private `crates/xtask` binary invoked as `cargo xtask <command>` via the checked-in `.cargo/config.toml` alias (it is not a Cargo built-in). Checks are tested Rust code, not YAML logic — deliberate, since CI is control-plane material by this project's own philosophy.

Parity is defined per named tier: **`cargo xtask ci --base <sha>` plus parallel `cargo xtask ci-capstone` and `cargo xtask ci-born-valid` run every merge-critical host-local check**, while **`cargo xtask ci --no-budget` retains the complete deep suite** (including editor, clippy, docs, and compiler matrices). The PR command delegates the `boxology-init`, `boxology-generator`, `boxology-workspace`, and `xtask` package tests, shallow fixture-project checks, and all three external-test integrity gates to the two capstone commands; `ci-born-valid` owns the exact born-valid integration test while `ci-capstone` owns the rest. The primary workspace-test argv excludes those four packages. **CI-only orchestration** — cross-platform artifact upload and comparison and event metadata — lives in the workflow, which invokes the same xtask commands per job and adds nothing semantic. Commands that need Git or event inputs take them as explicit arguments; nothing reads GitHub context implicitly.

S0 commands: `ci`, `ci-capstone`, `ci-born-valid`, `test`, `links`, `budget --base <sha>`, `deny`, `advisories --repo <owner/repo> [--simulate <RUSTSEC-id>]`, `determinism` (local protocol), `determinism-manifest --out <path>` (CI comparison input), `determinism-manifest --out <path> --meta-cross` (platform negative-control input), `determinism-compare <a> <b>`, `determinism-meta-cross <linux> <macos>`, `determinism-verify <dir> --target <triple>` (with CI-only strictness flag `--require-image`), and internal `subject-run <name> --out <path>` (the determinism child helper). Later streams add commands, not workflows.

### D3 — Toolchain and language posture

Exact stable pin in `rust-toolchain.toml` (latest stable at T1 time, recorded there), components `rustfmt` + `clippy` + `rust-analyzer`; edition 2024; no MSRV (nothing is published; the pin is the supported version). Toolchain bumps are dedicated PRs, never riders, running full validation including the determinism matrix — this is also what makes `clippy -D warnings` a stable gate. Committed `rustfmt.toml` pins "default" formatting explicitly. **Generated Rust is excluded from formatting by explicit package selection, not by `rustfmt.toml ignore`** — review established that `ignore` is nightly-only on the pinned stable toolchain. The formatting gate is `cargo fmt --check -p <pkg> …` over the owned hand-authored package list (bootstrap: maintained in xtask; from S7: derived from manifests, per D10), and an xtask test proves a generated-style crate in the workspace is not visited by the gate. Generated crates are printed deterministically by the generator's pinned printer (S2). Main-push `cargo xtask ci --no-budget` also runs a quiet, fixed-flag `rust-analyzer analysis-stats` batch probe against the controlled Hello implementation with build scripts and proc macros disabled; it proves the pinned analyzer can load the editor project, not that the current workspace has zero diagnostics. Generated Rust remains outside the rustfmt package selection.

### D4 — PR validation workflow

`pr.yml` runs on pull requests and pushes to `main`. Every enabled workflow job
runs on the MacBook: each active label exposes four disposable runners. Linux jobs use the disposable
`[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` runner supplied by S0-T8,
while native macOS jobs use `[self-hosted, macOS, ARM64, boxology-macos-pr]`.
Each lane has one base supervisor plus three slot supervisors, with bounded
per-slot build/test parallelism, APFS-cloned native Mac runner directories,
private native Mac target caches, and Linux container resources. The x86 audit workflow is removed for
this emergency migration and is a deferred follow-up.

| Job | Command | Runner |
| --- | --- | --- |
| checks-linux | fail-fast `boxology check`; `cargo xtask determinism` (local protocol); produce normal/meta roots with `determinism-manifest --out linux/` and `--out linux-meta/ --meta-cross`, pack each complete root as an uncompressed tar, upload both plus the Linux `xtask` binary; evidence target `aarch64-unknown-linux-gnu` | `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` |
| checks-macos | fail-fast `boxology check`; PR `cargo xtask ci --base <event base SHA>` (fmt-by-selection, workspace tests excluding `boxology-init`, `boxology-generator`, `boxology-workspace`, and `xtask`, then key-order, whitespace, links, records, budget, deny, and determinism local); main-push `cargo xtask ci --no-budget` retains the full workspace and fixture suites and adds workspace clippy, editor/rust-analyzer, ignored generator deep tests serially, workspace docs, and fixture clippy/docs; produce normal/meta roots with `determinism-manifest --out macos/` and `--out macos-meta/ --meta-cross`, pack each complete root as an uncompressed tar, and upload both; evidence target `aarch64-apple-darwin` | `[self-hosted, macOS, ARM64, boxology-macos-pr]` |
| macos-capstone | unconditional process-reaper fixture suite; PR-only `cargo xtask ci-capstone`, which checks the pinned toolchain, runs every delegated package's full test surface while skipping exactly the separately gated born-valid function, shallow fixture-project checks, and all three external-test integrity gates with timed verdicts and one summary; main-push deep CI already covers those tests; shallow checkout, no Actions cache, `boxology check`, generated artifacts, or cross-platform producer role | `[self-hosted, macOS, ARM64, boxology-macos-pr]` |
| macos-born-valid | PR-only `cargo xtask ci-born-valid`, which checks the pinned toolchain and runs the integrity-pinned `boxology-init` born-valid integration test; main-push deep CI already covers it; shallow checkout, no Actions cache or cross-platform producer role | `[self-hosted, macOS, ARM64, boxology-macos-pr]` |
| deny | `cargo xtask deny` (pinned `cargo-deny check bans licenses sources`) | `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` |
| determinism-compare | download/unpack normal roots; verify `linux/` as `aarch64-unknown-linux-gnu` and `macos/` as `aarch64-apple-darwin` with `determinism-verify --require-image`; `cargo xtask determinism-compare linux/ macos/` | `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` |
| determinism-meta-cross | download/unpack meta roots; verify the same exact targets with `determinism-verify --require-image`; `cargo xtask determinism-meta-cross linux-meta/ macos-meta/` succeeds only for the exact expected comparator finding | `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` |
| **validation** | aggregator: `needs:` every job above **including determinism-meta-cross**, `if: always()`, fails unless all succeeded | `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]` |

Native macOS is the **canonical behavioral validation** platform: PRs split the merge-critical suite between `checks-macos`, `macos-capstone`, and `macos-born-valid`, while main pushes add the slower compiler matrix, documentation, and editor checks. The two positive nested-Cargo generator capstones remain PR-blocking; eight redundant or negative-matrix nested-Cargo tests are `#[ignore]` in the ordinary workspace run and execute serially in main-push `--no-budget` CI. Linux is the **determinism and check-evidence** producer for the cross-platform consumers; it does not re-run the full behavioral suite. The union of PR and main-push tiers still covers every former check: tiered `ci` (including local determinism and deny) plus the process-reaper, package capstone, and born-valid lane on macOS, standalone `deny`, Linux local determinism plus manifests, and both cross-platform compare/meta-cross jobs.

**2026-08-03 amendment (#503), round two.** Run 30828942886 measured `checks-macos` at 11m04s while `checks-linux` completed in 4m22s. Round one moved the independently recoverable process-reaper suite and `boxology-init` package test to `macos-capstone`. Round two also delegates the full `boxology-generator`, `boxology-workspace`, and `xtask` package tests, shallow fixture-project checks, and the three external-test integrity gates to the same required parallel lane. The primary PR workspace argv excludes all four delegated packages; main-push `--no-budget` retains the unchanged full-workspace and deep fixture argv, so this is parallel scheduling rather than a coverage deletion. The workflow globally disables dev/test debuginfo, and the Mac jobs rely on the supervisor-owned native target cache instead of Actions cache archive traffic. `macos-capstone` remains a required dependency of `validation`.

**2026-08-03 amendment (#503), round three.** The born-valid integration test moves from `macos-capstone` to its own required `macos-born-valid` lane. `ci-capstone` keeps the original full `boxology-init` package command and skips exactly the separately gated born-valid function, preserving its lib, bin, integration, example, and doctest surface; `ci-born-valid` pins and executes the exact born-valid test through the external-test integrity harness. Main-push `--no-budget` remains unchanged and complete. Both lanes use the same native Mac labels, zero-debuginfo environment, private target caches, and 15-minute timeout without Actions cache traffic.

The former `linux-x86.yml` audit workflow is intentionally absent. Its
`x86_64-unknown-linux-gnu` coverage is deferred until a Mac-hosted x86-compatible
execution lane is deliberately designed; it must not reintroduce hosted minutes.

The `pr.yml` command surface (including commands reached through `ci`) is: `ci`, `ci-capstone`, `ci-born-valid`, `test`, `links`, `budget`, `deny`, `determinism`, `determinism-manifest --out <dir>`, `determinism-manifest --out <dir> --meta-cross`, `determinism-compare <a> <b>`, `determinism-meta-cross <linux> <macos>`, and `determinism-verify <dir> --target <triple>` (`--require-image` requires CI runner image evidence). D6's `advisories --repo <owner/repo> [--simulate <RUSTSEC-id>]` command and the internal `subject-run <name> --out <dir>` determinism child helper complete the S0 xtask command set; `advisories` is not part of `pr.yml`; `subject-run` appears there only as the child process determinism spawns and is never invoked by the workflow itself. The producer → tar/upload → download/unpack → verify → compare/gate data flow above is the executable contract; the consumer jobs inspect exactly what the platform jobs upload. This table is the **end state after T8**; each task enables only the jobs whose commands exist, adding its job to the aggregator's `needs` in the same PR (staged, never dangling).

Rules:

- **`validation` is the single stable required-check name.** New jobs must be added to its `needs` list; because it runs `if: always()`, a skipped or failed dependency cannot silently produce a green aggregate. This exact check is the branch-protection target if/when protection is available (D9).
- Each complete `MANIFEST` / `trees/` / `evidence/` root is transported as one uncompressed tar so empty required directories are preserved. Tar is transport-only: consumers unpack it into a fresh root before `determinism-verify`, and no tar archive is ever compared.
- **The `ci` interface is explicit and tiered:** `cargo xtask ci --base <sha>`, `cargo xtask ci-capstone`, and `cargo xtask ci-born-valid` form the parallel merge-critical suite including budget; `cargo xtask ci --no-budget` is the main-push/deep mode (no meaningful base), retaining the complete test surface while adding workspace clippy, editor, documentation, fixture clippy/docs, and ignored generator compiler-matrix tests. Local acceptance runs use all three PR-tier commands, with `--base origin/main` explicit; local full validation uses `--no-budget` — no command discovers Git or GitHub context implicitly. The primary and deep modes include pinned `deny`, so local/CI parity is exact within each named tier: `cargo xtask deny` verifies the pinned cargo-deny version is installed and errors with the install command otherwise.
- **Runner labels are fixed capability labels, not immutable pins.** The stable PR matrix uses `aarch64-unknown-linux-gnu` in a disposable Linux container and `aarch64-apple-darwin` on the native Mac host. Both jobs record runner image/host version and target triple into evidence. The native macOS runner is trusted host execution, not container isolation; it is limited to this private repository and trusted collaborators.
- Actions pinned by full commit SHA; dependency caching; concurrency groups cancel superseded runs.
- **Wall-clock is a monitored target, not an acceptance invariant:** the measured quantity is the **cache-hit required-check critical path** excluding queue time — the longest dependency path ending at `validation`, not the sum of parallel job durations — tracked as the median of the last ten `main` runs, with **8 minutes** as the alarm threshold. A stream that pushes it over addresses it in that stream's spec. The threshold is operational monitoring, not a merge gate that can be claimed green without a measured Actions run.

### D5 — Markdown link and anchor checking

`cargo xtask links`: every tracked `.md` file's repository-relative links must resolve; intra- and cross-document `#anchors` must match real heading slugs (GitHub slugging). External URLs are not fetched. This mechanizes the claim currently hand-asserted in every documentation PR.

### D6 — Dependency policy

- `Cargo.lock` committed. A PR whose lock diff includes crates not required by its own manifest changes fails review by policy (mechanical enforcement arrives with S5).
- `cargo-deny` at an **exact pinned version**, installed via `cargo install cargo-deny --version <pinned> --locked` (cached); the version is recorded in the workflow and bumped only by dedicated PRs. `deny.toml`: permissive-license allowlist, source allowlist (crates.io only), bans. **Bans/licenses/sources gate PRs** via the `deny` job in D4's matrix. **Advisories never gate PRs**: a daily scheduled workflow runs `cargo-deny check advisories` and files/updates a tracker issue — a CVE published overnight must not fail an unrelated PR.

### D7 — Review-budget check

`cargo xtask budget --base <sha>` computes hand-authored added lines against the given base and **fails above 600 — absolutely, with no override and no exemption**, implementing AGENTS.md exactly as merged. Oversized work is split or its task re-scoped; that is the methodology, and the check is its mechanical form.

- Counted: added lines in all hand-authored files, Markdown included.
- Excluded: `Cargo.lock`, paths declared as derived outputs (bootstrap list in xtask, per D10), and pure renames as detected by Git.
- Correction from the first draft, recorded for honesty: the earlier proposal claimed merged documentation PRs routinely exceeded 400 added lines; review checked, and the largest merged PR to date is +370. The factual basis for a Markdown exemption was wrong, and the absolute rule stands. A future spec that genuinely cannot fit is evidence to bring to a methodology amendment, not routed around via labels — which the v0 authority model could not verify as human-applied anyway.

### D8 — Determinism harness

A *determinism subject* is a registered command producing an output tree that must be byte-identical whenever inputs are unchanged. Subjects are registered in xtask code; S2 registers the real generator.

**Experimental protocol — one controlled perturbation per experiment.** The first draft's protocol varied several dimensions at once and could not attribute failures; corrected:

1. **Repeat experiment:** two runs, *identical controlled* conditions — same canonical path; subprocesses spawned with a **scrubbed environment** (cleared, then a fixed allowlist: `TZ=UTC`, `LC_ALL=C`, fixed `SOURCE_DATE_EPOCH`, minimal `PATH`). PID, wall clock, and temp-state ambience cannot be frozen, so **real-subject findings carry observational labels** (`repeat mismatch`, `path-context mismatch`, `env-context mismatch`), not causal classes; causal class assertions (map-order, timestamp, path leak…) are reserved for the deterministic fault-injection meta-fixtures, which force each failure explicitly.
2. **Path experiment:** baseline vs. a run whose only change is a different, deliberately unusual absolute path (different length, a space, a non-ASCII segment). Differences attribute to path leakage.
3. **Time experiment:** baseline vs. changed `SOURCE_DATE_EPOCH` and system-time-visible env. Under the accepted platform guarantee, **generated bytes may never vary with time** — the harness varies the inputs precisely to prove invariance; S2 may not reopen this (it may ignore or sanitize internally, but output bytes are invariant, full stop).
4. **Locale/timezone experiments:** baseline vs. changed `LC_ALL`, then vs. changed `TZ`, separately.

Each experiment reports its observational label (real subjects) or asserted causal class (injection fixtures). Output is the platform-neutral manifest described below.

**Cross-platform protocol:** Linux and macOS each run the local protocol and upload a **platform-neutral output manifest** (sorted path → SHA-256 + size, versioned schema — nothing platform-identifying) plus the bounded output trees; `determinism-compare` diffs manifests and produces bounded byte-level diffs from the retained artifacts. **Run evidence (runner image version, target triple, tool versions) lives in a separate evidence envelope that is uploaded but never byte-compared** — review caught that evidence inside the compared manifest would guarantee a cross-platform mismatch by construction.

**Negative fixtures are fault-injection meta-tests, not registry members.** Deliberately nondeterministic fixture subjects (map-order, timestamp, absolute-path, CRLF, locale-format) live outside the normal registry; the harness's own test suite runs each under the protocol, **asserts the comparator reports the expected failure class, and itself exits successfully**. Registered-subject runs must always be green. Where a class is inherently probabilistic (natural `HashMap` ordering), the fixture forces the failure deterministically (seeded/explicit ordering difference) rather than sampling. The cross-platform analogue intentionally emits platform-dependent bytes in a **required expected-failure job** whose Rust wrapper succeeds only for the exact expected comparator finding, separate from the normal gating comparison lane.

### D9 — Branch protection and plan reality

Fact, verified during review: on the current private-repository plan, branch-protection and rulesets APIs return `403` (Pro or public required). Therefore: protection is **operator guidance, not an S0 deliverable** — this spec records the exact recommended configuration (require the `validation` check; require linear history so direct pushes cannot introduce merge commits) to be applied if the repository is made public or the plan upgraded. That choice is the operator's, outside this spec. Separately from branch protection, repository merge-method settings — a control that is available on the current plan — now allow only squash merges for pull requests (#144, #154). That setting enforces D1's merge method for PR merges, but it does not require the `validation` check, does not block merging on failing checks, and does not prevent direct pushes to `main`. All other `main` hygiene remains convention plus review.

### D10 — Bootstrap-to-canonical handoff

`cargo xtask ci` is **temporary bootstrap orchestration**. S7 adopts manifests and gates repository CI with `boxology check`; immediately after v0, S7-T5/#342 completes the absorption: platform validation (ownership, edges, regeneration, classification) is delegated to `boxology check` invoked by `xtask ci`; xtask retains only repository-specific checks (links, budget, determinism meta-tests) under clearly separate names; and every bootstrap registry duplicated here — the derived-output exclusion list and the hand-authored formatting package-selection lists (owned and excluded, per D3) — is replaced by manifest-derived data, with the xtask copies deleted. Manifests are authoritative for platform policy from S7 onward; the duplicated bootstrap registries are an explicitly bounded transition and do not survive #342.

### D11 — S0-T8 Mac-hosted ARM64 runner contract

S0-T8 is accepted as an emergency Mac-hosted migration. PR 1 added the Linux
image, host supervisor, operational runbook, launchd template, smoke workflow,
and ARM64 host-fixture selection. PR 2 activated Linux routing. This follow-up
adds the native macOS JIT runner, routes every enabled workflow through the Mac,
and removes the hosted x86 audit until a Mac-hosted replacement exists.

The accepted replacement has two one-job JIT lanes on the Mac: a native ARM64
Colima VM running the disposable Linux container, and a native Apple-silicon
macOS runner copied into a fresh per-job directory. The Linux image contains no
repository source, runs the runner as non-root, and is built from the official
Ubuntu 24.04 ARM64 base pinned by OCI digest. Both lanes pin actions/runner
v2.336.0, the repository Rust toolchain, and cargo-deny 0.20.2; runtime
self-update is disabled. Linux preinstalls the pinned toolchain and its
repository-required components (`rustfmt`, `clippy`, `rust-analyzer`) into the
read-only `/opt/rustup` image layer, so per-job `rustup toolchain install`
completes without writing and no toolchain is copied into the per-runner volume.
Linux has read-only-root, volume, CPU, memory, pids,
capability, and privilege bounds. Native macOS is trusted host execution and
has no container boundary; its archive checksum, host OS version, architecture,
and target triple are required evidence.

Activation gates are: a private repository with trusted Henry/agent collaborators; an operator-provided dedicated GitHub credential stored only in a macOS Keychain item; verified Linux image and native runner archive; clean Linux and macOS smoke dispatches; and health checks showing four configured runners per lane. The supervisors fail closed when a prerequisite, API response, identity, or lock is invalid and never use `gh` credentials for unattended provisioning. Rollback is to unload the base and slot supervisors, remove only their owned run state, stop the dedicated Colima profile, and revert workflow routing. The hosted x86 lane is not restored by rollback.

The broker PAT never enters either runner job environment. Ordinary GitHub Actions
read/runtime credentials may be present for checkout; smoke workflows keep
`persist-credentials: false` and assert only broker-PAT absence. Each official
runner transiently consumes the one-use JIT config through
`run.sh --disableupdate --jitconfig`, so the encoded argument is visible to
same-user job processes by design. This residual and native host execution are
accepted only under the private trusted-collaborator assumption.

The authoritative operator procedure is [`ops/ci-runner/README.md`](../ops/ci-runner/README.md). This contract does not alter historical records.

## Acceptance criteria

1. `cargo xtask ci` passes locally on both supported triples and runs exactly the host-local checks CI runs.
2. A PR introducing a compile/test failure, fmt violation, broken relative link, broken anchor, or trailing whitespace fails `validation`; a Clippy warning fails the next main-push deep run.
3. Normal PRs adding >600 hand-authored lines fail `validation`. The Mac-hosted
   runner migration is the explicitly authorized emergency exception to that
   normal budget for this PR.
4. Every fault-injection meta-fixture yields its expected failure class under the local protocol, with the meta-test suite itself green; the platform-dependent meta-fixture makes `determinism-compare` fail in the expected-failure lane.
5. The `deny` job fails a PR introducing a disallowed license or non-crates.io source. The advisory path is proven two ways: a deterministic xtask integration test with a mocked advisory database exercising the find-by-title idempotent issue-upsert logic, and a `workflow_dispatch` input (`simulate_advisory=<id>`) on the scheduled workflow for a post-merge smoke run — review established scheduled workflows execute only from the default branch, so a throwaway-branch schedule cannot test this.
6. The `validation` aggregator fails when any needed job fails or is skipped (verified by a deliberate red run).
7. Runner image version and target triple appear in determinism evidence for both platforms.
8. S0-T8 passes its runbook, image, native-runner, supervisor, isolation,
   broker-PAT-environment, and queued-smoke checks under the private/trusted-
   collaborator assumption; private forking is neither changed nor a hard
   activation gate, and the same-user JIT-config/native-host residuals are documented.

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Workspace scaffold: manifests, toolchain pin, rustfmt config, `.cargo` alias, `.gitattributes`, `.editorconfig`, xtask skeleton with `ci` | 1 |
| T2 | `pr.yml`: job matrix, SHA-pinned actions, caching, concurrency, `validation` aggregator, evidence recording | 1–2 |
| T3 | `xtask links` + wiring | 1 |
| T4 | `xtask budget` (absolute rule, exclusions, base-SHA input) + wiring | 1 |
| T5 | `deny.toml`, pinned cargo-deny gate job, scheduled advisory workflow | 1 |
| T6 | Determinism harness core: subject model, per-experiment protocol, manifest format, diagnostics | 2 |
| T7 | Meta-fixtures + expected-failure lanes (local and cross-platform) + artifact retention and compare job | 2 |
| T8 | Mac-hosted ARM64 CI: disposable Linux Colima runner, native macOS JIT runner, activation in all workflows, smoke coverage, and deferred x86 lane | 3 |

T1 → T2 strictly; T3–T5 independent after T2; T6 → T7, parallel with T3–T5.

## Tracker notes

The #17 and #41 reconciliation comments record this spec's narrowings (candidate-writable bootstrap CI; product-repo dependency rules). The formatting mechanism here (fmt-by-selection) is the one referenced by S2 D7 and by the revised validation-baseline wording in `08-rust-build-topology.md` (edited in the S2 PR). Issue #85's S0 items (formatting mechanism, command inventory/data flow, ci interface, deny parity, gated negative control, observational labels, evidence separation, staged tasks, advisory proof) are resolved in this revision.

## Matters left open

*(None load-bearing; per review, load-bearing items may not hide here.)*

- Exact toolchain version, runner-image versions, and pinned cargo-deny version — resolved at implementation time and recorded in the task PRs.
- Whether macOS validation later narrows to merge-time only — superseded by the native Mac-hosted lane.
- Hosted Linux/macOS runner cost now justifies the two Mac-hosted JIT lanes in D11; x86 coverage remains a deferred follow-up.

**Amendment of 2026-08-03** (maintainer acceleration decision): D10's registry absorption moves to S7-T5/#342 as the first task immediately post-v0; S7's root manifests and `boxology check` gate remain in force during the bounded transition.

**Amendment of 2026-08-04 (#522 reconciliation).** PR #522 collapsed the multi-job PR matrix to one native Apple-silicon Mac job; this amendment supersedes D2/D4's prior PR-tier table, the #503 parallel-lane rounds for pull-request validation, and D3's V0 toolchain-bump matrix clause (toolchain bumps require the dispatched native-Mac deep tier for V0; the cross-platform re-proof is [#525](https://github.com/fontanierh/boxology/issues/525) and is required before the first pinned external release). All other D3 language remains intact. The amendment records the accepted V0 speed tradeoff.

- **PR validation.** `pr.yml` has one required job, `validation`, on `[self-hosted, macOS, ARM64, boxology-macos-pr]`. It always runs `cargo xtask ci-hygiene --base <event base SHA>` (audit, fmt, key-order, tracked whitespace, links, records, budget). Non-Markdown diffs then run the fast xtask invariant suite, the complete tests for each directly changed crate (with `boxology-init` limited to lib/bin tests), and `boxology check --base <event base SHA>`. Root dependency or toolchain changes additionally compile-check the complete workspace. The process-reaper fixture runs only when its own implementation changes. Markdown-only diffs skip the Rust/product steps while hygiene still runs. Full-workspace tests, born-valid nested-workspace acceptance, ping-app composition, Clippy, docs, deny, and determinism remain in dispatch-only deep validation. There is no PR platform matrix, no Linux PR lane, no Actions cache, and no aggregator of sibling jobs. This deliberately trades immediate dependent-crate behavioral coverage for a short merge path; deep validation catches that class before V0 closure.
- **Deep validation.** `deep-validation.yml` is `workflow_dispatch` only — no schedule and no required check. One `deep` job on the same Mac label runs `cargo xtask ci --no-budget` and `boxology check` after installing the pinned toolchain and exact cargo-deny. Operators must dispatch it for toolchain-pin bumps and before V0 closure; it must not be dispatched during heavy local delivery on the shared MacBook.
- **macOS-only V0 gate.** Continuous V0 gating evidence is native `aarch64-apple-darwin` only. Cross-platform Linux/x86 validation, determinism compare/meta-cross, and continuous generator conformance across platforms are residual ownership of [#525](https://github.com/fontanierh/boxology/issues/525). D11's Mac-hosted Linux JIT infrastructure remains installed but dormant except for manual smoke workflows.
- **Command surface.** S0 commands gain `ci-hygiene --base <revision>`. Local cheap acceptance is `cargo xtask ci-hygiene --base origin/main`; deep local acceptance remains `cargo xtask ci --no-budget`. The `ci` / `ci-capstone` / `ci-born-valid` command bodies remain available for deep and local use but are not the PR workflow shape.
- **Acceptance criteria reconciled.**
  - AC1: local `ci-hygiene --base` matches the PR hygiene tier; local/deep `ci --no-budget` matches the dispatched deep job's host-local aggregate on macOS ARM64.
  - AC2: directly changed-crate test failures, root build-graph compile failures, and fmt/link/whitespace failures fail the single `validation` job; dependent-crate behavior, Clippy, editor, docs, composition, and other deep suites fail a dispatched deep run, not every PR.
  - AC4: local fault-injection meta-fixtures remain green under the local determinism protocol; continuous cross-platform expected-failure compare is owned by #525.
  - AC5: bans/licenses/sources gate via deep `deny` (and local `cargo xtask deny`); they are not a separate PR job. Advisory proof stays on the scheduled/dispatch advisories workflow.
  - AC6: the required check name is the single `validation` job itself; there is no multi-job aggregator.
  - AC7: runner image/host version and target triple remain recorded on the Mac PR and deep jobs; dual-platform determinism evidence envelopes are #525.
