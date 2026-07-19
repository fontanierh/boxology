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

`.gitattributes` (`* text=auto eol=lf` plus explicit binary exceptions) is the enforcement point for line endings — `.editorconfig` only guides cooperating editors, and source bytes feed generation, so Git-level normalization is the rule that matters. Linear history on `main` via squash merge remains **convention** documented here; repository settings currently permit other merge modes and cannot be restricted on the current plan (see D9).

### D2 — `xtask` is the automation home, with defined CI parity

Repository automation is a private `crates/xtask` binary invoked as `cargo xtask <command>` via the checked-in `.cargo/config.toml` alias (it is not a Cargo built-in). Checks are tested Rust code, not YAML logic — deliberate, since CI is control-plane material by this project's own philosophy.

Parity is defined precisely: **`cargo xtask ci` runs every host-local check** (fmt, clippy, test, doc, whitespace, links, budget, determinism-local). **CI-only orchestration** — cross-platform artifact upload and comparison, event metadata, caching — lives in the workflow, which invokes the same xtask commands per job and adds nothing semantic. Commands that need Git or event inputs take them as explicit arguments (`--base <sha>`); nothing reads GitHub context implicitly.

S0 commands: `ci`, `links`, `budget --base <sha>`, `determinism` (local protocol), `determinism-manifest --out <path>` (CI comparison input). Later streams add commands, not workflows.

### D3 — Toolchain and language posture

Exact stable pin in `rust-toolchain.toml` (latest stable at T1 time, recorded there), components `rustfmt` + `clippy`; edition 2024; no MSRV (nothing is published; the pin is the supported version). Toolchain bumps are dedicated PRs, never riders, running full validation including the determinism matrix — this is also what makes `clippy -D warnings` a stable gate. Committed `rustfmt.toml` pins "default" formatting explicitly. **Generated Rust is excluded from formatting by explicit package selection, not by `rustfmt.toml ignore`** — review established that `ignore` is nightly-only on the pinned stable toolchain. The formatting gate is `cargo fmt --check -p <pkg> …` over the owned hand-authored package list (bootstrap: maintained in xtask; from S7: derived from manifests, per D10), and an xtask test proves a generated-style crate in the workspace is not visited by the gate. Generated crates are printed deterministically by the generator's pinned printer (S2).

### D4 — PR validation workflow

`pr.yml` runs on pull requests and pushes to `main`:

| Job | Command | Runner |
| --- | --- | --- |
| checks-linux | `cargo xtask ci --base <event base SHA>` (fmt-by-selection, clippy, test, doc, whitespace, links, budget, deny, determinism local) + `cargo xtask determinism-manifest --out linux/` → upload `linux/` (manifest + bounded output trees) | `ubuntu-24.04` |
| checks-macos | `cargo xtask test` + `cargo xtask determinism` + `cargo xtask determinism-manifest --out macos/` → upload `macos/` | `macos-15` |
| deny | `cargo xtask deny` (pinned `cargo-deny check bans licenses sources`) | `ubuntu-24.04` |
| determinism-compare | download `linux/` + `macos/`; `cargo xtask determinism-compare linux/ macos/` | `ubuntu-24.04` |
| determinism-meta-cross | download meta-fixture artifacts; run the comparator on the deliberately platform-dependent fixture; **succeed only if the comparator fails with the expected mismatch diagnostic**; fail on comparator success or infrastructure error | `ubuntu-24.04` |
| **validation** | aggregator: `needs:` every job above **including determinism-meta-cross**, `if: always()`, fails unless all succeeded | `ubuntu-24.04` |

Command inventory (all defined in xtask): `ci`, `test`, `links`, `budget`, `deny`, `determinism`, `determinism-manifest --out <dir>`, `determinism-compare <a> <b>`. The producer → upload → download → compare data flow above is the executable contract; the compare job consumes exactly what the platform jobs upload. This table is the **end state after T7**; each task enables only the jobs whose commands exist, adding its job to the aggregator's `needs` in the same PR (staged, never dangling).

Rules:

- **`validation` is the single stable required-check name.** New jobs must be added to its `needs` list; because it runs `if: always()`, a skipped or failed dependency cannot silently produce a green aggregate. This exact check is the branch-protection target if/when protection is available (D9).
- **The `ci` interface is explicit and single:** `cargo xtask ci --base <sha>` runs everything including budget; `cargo xtask ci --no-budget` is the mode for `main` pushes (no meaningful base) and exploratory local runs. Local acceptance runs use `--base origin/main` explicitly — no command discovers Git or GitHub context implicitly. `ci` includes the pinned `deny` invocation, so **local/CI parity has no semantic exceptions**: `cargo xtask deny` verifies the pinned cargo-deny version is installed and errors with the install command otherwise.
- **Runner labels are fixed major-OS labels, not immutable pins.** GitHub updates `ubuntu-24.04`/`macos-15` images continuously; each job therefore records the runner image version and target triple into the job log and the determinism evidence. The supported triples are stated: `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` — the matrix is deliberately cross-architecture as well as cross-OS. If truly immutable environments are ever required, hosted runners cannot provide them; that would be a containerized-runner decision taken then.
- Actions pinned by full commit SHA; dependency caching; concurrency groups cancel superseded runs.
- **Wall-clock is a monitored target, not an acceptance invariant:** the measured quantity is job duration excluding queue time, on cache-hit runs, tracked as the median of the last ten `main` runs, with 10 minutes as the alarm threshold. A stream that pushes it over addresses it in that stream's spec.

### D5 — Markdown link and anchor checking

`cargo xtask links`: every tracked `.md` file's repository-relative links must resolve; intra- and cross-document `#anchors` must match real heading slugs (GitHub slugging). External URLs are not fetched. This mechanizes the claim currently hand-asserted in every documentation PR.

### D6 — Dependency policy

- `Cargo.lock` committed. A PR whose lock diff includes crates not required by its own manifest changes fails review by policy (mechanical enforcement arrives with S5).
- `cargo-deny` at an **exact pinned version**, installed via `cargo install cargo-deny --version <pinned> --locked` (cached); the version is recorded in the workflow and bumped only by dedicated PRs. `deny.toml`: permissive-license allowlist, source allowlist (crates.io only), bans. **Bans/licenses/sources gate PRs** via the `deny` job in D4's matrix. **Advisories never gate PRs**: a daily scheduled workflow runs `cargo-deny check advisories` and files/updates a tracker issue — a CVE published overnight must not fail an unrelated PR.

### D7 — Review-budget check

`cargo xtask budget --base <sha>` computes hand-authored added lines against the given base and **fails above 400 — absolutely, with no override and no exemption**, implementing AGENTS.md exactly as merged. Oversized work is split or its task re-scoped; that is the methodology, and the check is its mechanical form.

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

**Negative fixtures are fault-injection meta-tests, not registry members.** Deliberately nondeterministic fixture subjects (map-order, timestamp, absolute-path, CRLF, locale-format) live outside the normal registry; the harness's own test suite runs each under the protocol, **asserts the comparator reports the expected failure class, and itself exits successfully**. Registered-subject runs must always be green. Where a class is inherently probabilistic (natural `HashMap` ordering), the fixture forces the failure deterministically (seeded/explicit ordering difference) rather than sampling. The cross-platform analogue: one meta-fixture intentionally emits platform-dependent bytes; the workflow runs it in a dedicated non-gating job and asserts `determinism-compare` *fails* on it — an expected-failure lane, separate from the gating lane.

### D9 — Branch protection and plan reality

Fact, verified during review: on the current private-repository plan, branch-protection and rulesets APIs return `403` (Pro or public required). Therefore: protection is **operator guidance, not an S0 deliverable** — this spec records the exact recommended configuration (require the `validation` check; restrict non-squash merges if linear history is promoted from convention to invariant) to be applied if the repository is made public or the plan upgraded. That choice is the operator's, outside this spec. Until then, `main` hygiene is convention plus review.

### D10 — Bootstrap-to-canonical handoff

`cargo xtask ci` is **temporary bootstrap orchestration**. When S5 ships `boxology check` and S7 adopts manifests on this repository: platform validation (ownership, edges, regeneration, classification) is delegated to `boxology check` invoked by `xtask ci`; xtask retains only repository-specific checks (links, budget, determinism meta-tests) under clearly separate names; and every bootstrap registry duplicated here — the derived-output exclusion list, the rustfmt ignore list — is replaced by manifest-derived data, with the xtask copies deleted. Manifests are authoritative from S7 onward; S0 never becomes a second registry that survives.

## Acceptance criteria

1. `cargo xtask ci` passes locally on both supported triples and runs exactly the host-local checks CI runs.
2. A PR introducing a clippy warning, fmt violation, broken relative link, broken anchor, or trailing whitespace fails `validation`.
3. A PR adding >400 hand-authored lines fails `validation`; there is no bypass mechanism.
4. Every fault-injection meta-fixture yields its expected failure class under the local protocol, with the meta-test suite itself green; the platform-dependent meta-fixture makes `determinism-compare` fail in the expected-failure lane.
5. The `deny` job fails a PR introducing a disallowed license or non-crates.io source. The advisory path is proven two ways: a deterministic xtask integration test with a mocked advisory database exercising the find-by-title idempotent issue-upsert logic, and a `workflow_dispatch` input (`simulate_advisory=<id>`) on the scheduled workflow for a post-merge smoke run — review established scheduled workflows execute only from the default branch, so a throwaway-branch schedule cannot test this.
6. The `validation` aggregator fails when any needed job fails or is skipped (verified by a deliberate red run).
7. Runner image version and target triple appear in determinism evidence for both platforms.

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

T1 → T2 strictly; T3–T5 independent after T2; T6 → T7, parallel with T3–T5.

## Tracker notes

The #17 and #41 reconciliation comments record this spec's narrowings (candidate-writable bootstrap CI; product-repo dependency rules). The formatting mechanism here (fmt-by-selection) is the one referenced by S2 D7 and by the revised validation-baseline wording in `08-rust-build-topology.md` (edited in the S2 PR). Issue #85's S0 items (formatting mechanism, command inventory/data flow, ci interface, deny parity, gated negative control, observational labels, evidence separation, staged tasks, advisory proof) are resolved in this revision.

## Matters left open

*(None load-bearing; per review, load-bearing items may not hide here.)*

- Exact toolchain version, runner-image versions, and pinned cargo-deny version — resolved at implementation time and recorded in the task PRs.
- Whether macOS validation later narrows to merge-time only — deferred until CI cost is measured against the D4 target.
- Containerized runners for truly immutable environments — only if hosted-runner drift is ever observed to matter.
