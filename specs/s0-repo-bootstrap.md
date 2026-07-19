# S0 Spec — Product-Repo Bootstrap and CI

[Stream definition](../boxology-details/11-v0-streams.md#s0--product-repo-bootstrap-and-ci) · Status: **proposed**

This is the stream specification for S0 under the [v0 execution methodology](../AGENTS.md#v0-execution-methodology). It defines the infrastructure the product repository needs before any platform code lands: the Rust workspace, the pinned toolchain, pull-request validation, mechanical enforcement of the repository's own working rules, and the cross-platform determinism harness. When this spec merges, its task list becomes tracker issues; each task is specified and implemented as a stack of pull requests under the 400-line review budget.

## Purpose

S0 exists so that every subsequent stream inherits, on day one:

1. A workspace whose toolchain, formatting, and lint posture cannot drift silently.
2. Pull-request validation that mechanically enforces what the repository currently enforces by assertion — including claims this project has been making by hand in every PR description ("all relative Markdown links resolve", "git diff --check") and the methodology's review budget.
3. A determinism harness that is **proven to catch every known class of nondeterminism before the contract generator exists**, so that when S2 registers the generator as a subject, a green harness means something.

The third point is the heart of S0. Cross-platform byte determinism is a normative platform guarantee ([Rust Build Topology](../boxology-details/08-rust-build-topology.md)); a harness built alongside the generator would co-evolve with its bugs. Building and validating the harness against synthetic subjects first inverts that risk.

## Non-goals

- No platform functionality: no runtime, generator, binding, manifest, or installer code.
- No `boxology.toml` on this repository — that is stage-2 self-hosting (S7), which requires S5's tooling to exist.
- No publishing or distribution (recorded v0 exclusion; v0 is consumed from source).
- No Windows (recorded v0 exclusion).
- No coverage tracking, benchmarking, or release automation — post-v0 unless a stream spec pulls one in with justification.

## Decisions

### D1 — Repository layout

```text
/Cargo.toml               # workspace root (virtual manifest)
/Cargo.lock               # committed
/rust-toolchain.toml      # pinned toolchain
/rustfmt.toml             # committed formatting config
/deny.toml                # dependency policy (D6)
/.editorconfig
/crates/                  # platform crates arrive here from S1 onward
/crates/xtask/            # repo automation (D2) — the workspace's first member
/specs/                   # stream and task specs
/boxology-details/        # design documents (unchanged)
/.github/workflows/       # PR validation and scheduled jobs
```

Design documents and specs stay in Markdown at their current locations. All Rust code lives under `crates/`; the `boxology-` crate-name prefix is reserved for platform crates, which arrive with their owning streams (S1+), not with S0.

### D2 — `xtask` is the automation home

Repository automation uses the cargo-xtask pattern: a private, unpublished `crates/xtask` binary invoked as `cargo xtask <command>`. Rationale: every check we add (link checking, budget checking, determinism) is thereby ordinary tested Rust code in the workspace rather than untested shell embedded in workflow YAML, runs identically on a developer laptop and in CI, and keeps workflow files down to "checkout, install toolchain, run xtask". This matters doubly here because CI configuration is protected control-plane material in this project's own philosophy — the less logic lives in YAML, the more of the control plane is reviewable, testable code.

S0 xtask commands: `cargo xtask ci` (everything PR validation runs), `cargo xtask links`, `cargo xtask budget`, `cargo xtask determinism`. Later streams add commands rather than workflows.

### D3 — Toolchain and language posture

- `rust-toolchain.toml` pins an **exact stable version** (the latest stable at implementation time), with `rustfmt` and `clippy` components. `rustup` makes the pin self-enforcing for every contributor and CI job.
- Workspace edition: 2024. No MSRV commitment in v0 — nothing is published, and the pin *is* the supported version.
- Toolchain bumps are deliberate, dedicated PRs (never riders), titled as such, running full validation including the determinism matrix — consistent with the merged rule that toolchain changes are platform-package changes with whole-workspace blast radius. This is also what makes `clippy -D warnings` a stable gate: new lints arrive only when a bump PR chooses to absorb them.
- Formatting is default rustfmt with the config committed (an empty-but-present `rustfmt.toml` plus edition), so "default" is pinned rather than ambient.

### D4 — PR validation workflow

One workflow, `pr.yml`, runs on every pull request and on `main` pushes:

| Check | Command | Platform |
| --- | --- | --- |
| Whitespace | `git diff --check` against merge base | Linux |
| Format | `cargo fmt --all --check` | Linux |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Linux |
| Test | `cargo test --workspace --all-features` | Linux + macOS |
| Docs | `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` | Linux |
| Markdown links | `cargo xtask links` (D5) | Linux |
| Review budget | `cargo xtask budget` (D7) | Linux |
| Determinism | `cargo xtask determinism` + cross-platform comparison (D8) | Linux + macOS |

Operational rules:

- **Runner images are pinned by name** (`ubuntu-24.04`, `macos-15`), never `-latest`. The CI environment is an input to determinism claims; it does not get to drift implicitly.
- **Actions are pinned by full commit SHA.** The repository's CI is its control plane, and unpinned third-party actions are the textbook supply-chain hole; this applies the project's own #24-adjacent posture to itself.
- Dependency caching (rust-cache or equivalent, SHA-pinned) and concurrency groups that cancel superseded runs on the same PR.
- Wall-clock budget: PR validation completes in **under 10 minutes** on a warm cache. If a later stream breaks this, that stream's spec must address it (test sharding, check tiering) rather than silently absorbing slow CI.
- After T2 lands, the operator enables branch protection on `main` requiring the validation check — an operator action, per the merged authority model, not something automation performs.

macOS runs the two checks where platform variance is load-bearing (tests, determinism) on every PR. This is deliberately conservative; if macOS minutes become a real cost, the fallback (macOS on merge-queue/`main` only) is a one-line change that a future PR can make with justification.

### D5 — Markdown link and anchor checking

`cargo xtask links` walks every tracked `.md` file and verifies that (a) every repository-relative link resolves to an existing file, and (b) every intra-document and cross-document `#anchor` matches a real heading slug (GitHub slugging rules). External URLs are **not** fetched — network-dependent CI is flaky CI; external-link rot is handled by humans.

This mechanizes a claim currently made by hand in every documentation PR, and this repository is document-heavy enough that the check pays for itself immediately (the design docs cross-reference each other and the tracker extensively).

### D6 — Dependency policy

- `Cargo.lock` is committed. Dependency additions and upgrades must be visible: a PR whose lock diff includes crates not required by its own manifest changes fails review by policy (mechanical enforcement of this arrives with S5's tooling; until then it is a review rule stated here).
- `cargo-deny` with a committed `deny.toml`: license allowlist (permissive licenses only for v0), duplicate-version warnings, and a source allowlist (crates.io only).
- **Advisory checking is a scheduled job, not a PR gate.** A CVE published overnight must not fail an unrelated PR; the scheduled job (daily) files/updates a tracker issue instead, which is triaged like any other work. Bans and license violations *do* gate PRs, because those only change when the PR itself changes dependencies.

### D7 — Review-budget check

`cargo xtask budget` computes hand-authored added lines against the PR's merge base and fails above **400**, implementing the methodology's cap mechanically.

- **Counted:** added lines in all files except exclusions.
- **Excluded:** `Cargo.lock`; files under any path declared as derived output (none exist yet; the declaration list lives in xtask and grows when generated trees appear); pure renames/moves as detected by git.
- **Override:** a `budget-override` label on the PR converts failure into a warning annotation. The label is applied by a human, is visible in history, and the PR description must say why — explicit and auditable, matching the house rule that policy downgrades are never silent side effects.
- **Proposed methodology clarification (flagged for review, not yet applied):** the budget applies to **code**; Markdown-only PRs are exempt. Rationale: every design and spec document merged to date — including this one — exceeds 400 lines, and the methodology's recorded intent is bounding *implementation review* attention; documentation review is governed by the tracker-reconciliation gate instead. If the reviewer accepts this, AGENTS.md gets the one-line clarification when this spec merges; if not, `budget` counts Markdown and doc PRs must be split.

### D8 — Determinism harness

The harness is S0's largest deliverable and is designed to be **complete and self-validating before the generator exists**.

**Model.** A *determinism subject* is a named, registered command (a function in xtask, later a generator invocation) that writes an output tree into a supplied directory and must produce byte-identical trees whenever inputs are unchanged, regardless of platform, path, time, or environment. Subjects are registered in xtask code; S2 registers the real generator later. Registration is code, not configuration, so adding a subject is a reviewed change.

**Local protocol** (`cargo xtask determinism`), per subject:

1. **Repeat run:** execute twice in fresh temp directories; compare trees byte-for-byte. Catches intra-platform nondeterminism — unordered map iteration, random seeds — the most common class, immediately, on one machine.
2. **Path variation:** the two runs use deliberately different, deliberately unusual absolute paths (differing lengths, a space, a non-ASCII segment). Catches absolute-path and path-length leakage into output.
3. **Environment variation:** runs differ in `TZ`, `LANG`/`LC_ALL`, and (where the subject reads it) `SOURCE_DATE_EPOCH` absence/presence. Catches timestamp and locale leakage.
4. The result is a **manifest**: a deterministic JSON document mapping each output path to its SHA-256 and size, with a schema-version field, sorted by path, itself byte-stable.

**Cross-platform protocol (CI):** the Linux and macOS jobs each run the local protocol and upload their manifests; a comparison job diffs the two manifests and fails with the differing paths and hashes. Comparison is on raw output bytes — no normalization at comparison time, because the normative requirement is that the *producer* normalizes (LF endings, sorted emission, no timestamps); the harness's job is to refuse to forgive.

**Self-validation.** The harness ships with fixture subjects that deliberately exhibit each failure class — map-iteration ordering, embedded timestamp, embedded absolute path, platform line-endings, locale-dependent formatting — each toggleable. The harness's own test suite asserts that every fixture failure class is *detected* (and that the clean fixture passes). A green harness is therefore a tested claim, not a hopeful one. The fixtures also serve as executable documentation of the determinism rules for S2's implementers.

**Failure UX.** A determinism failure names the subject, the varied dimension (repeat/path/env/platform), the first differing file, and a bounded hex diff around the first differing byte. Nondeterminism bugs are miserable to localize; the harness's diagnostics are part of its contract, not garnish.

### D9 — Repository hygiene

`.gitignore` (`/target`, editor droppings), `.editorconfig` (LF, final newline, UTF-8 — LF matters: line endings are a determinism dimension and the repo itself should model the rule). No CODEOWNERS in v0 (single accountable maintainer; roles arrive with the factory). Git history stays linear on `main` via squash merges, matching existing practice.

## Acceptance criteria

S0 is complete when all of the following are demonstrably true:

1. `cargo xtask ci` passes locally on Linux and macOS and is byte-identical in behavior to what PR validation runs.
2. A PR introducing a clippy warning, a formatting violation, a broken relative Markdown link, a broken anchor, or trailing whitespace fails validation.
3. A PR adding more than 400 hand-authored code lines fails the budget check; adding the `budget-override` label converts the failure to a visible warning.
4. Every deliberately nondeterministic fixture subject fails the harness with a diagnostic naming the correct failure class and file; the clean fixture passes on both platforms with identical manifests.
5. A synthetic cross-platform difference (a fixture that intentionally emits platform-dependent bytes) is caught by the CI comparison job.
6. PR validation completes in under 10 minutes on a warm cache.
7. Branch protection requiring validation is enabled on `main` (operator action, recorded in the closing issue comment).

## Task list

Derived tasks, each becoming a tracker issue with its own spec, implemented in PR stacks under the budget:

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Workspace scaffold: root manifest, toolchain pin, rustfmt config, editorconfig, gitignore, empty xtask skeleton with `ci` command | 1 |
| T2 | PR validation workflow: fmt/clippy/test/doc jobs, SHA-pinned actions, caching, concurrency, runner pins | 1–2 |
| T3 | `xtask links`: relative-link and anchor checker + CI wiring | 1 |
| T4 | `xtask budget`: merge-base diff accounting, exclusions, override label protocol + CI wiring | 1 |
| T5 | Dependency policy: `deny.toml`, PR-gating bans/licenses, scheduled advisory job filing tracker issues | 1 |
| T6 | Determinism harness core: subject model, repeat/path/env protocol, manifest format, diagnostics | 2 |
| T7 | Determinism fixtures and self-validation suite; cross-platform CI comparison job | 1–2 |

T1 → T2 sequence strictly; T3–T5 are independent after T2; T6 → T7 sequence and can proceed in parallel with T3–T5.

## Matters left open

- The exact pinned toolchain version and runner image tags — resolved at T1/T2 implementation time to current values, recorded in the task PRs.
- Whether macOS validation later narrows to merge-time only — deferred until CI cost is a measured problem.
- The budget check's Markdown exemption — explicitly awaiting review (D7); the check ships with whichever scope review decides.
- Coverage, benchmarking, and mutation testing — not S0; a later stream may propose them with justification.
- `SOURCE_DATE_EPOCH` semantics for the real generator — S2's spec decides whether the generator honors or ignores it; the harness only varies it.
