# S0 Spec — Product-Repo Bootstrap and CI

[Stream definition](../boxology-details/11-v0-streams.md#s0--product-repo-bootstrap-and-ci) · Status: **revised, awaiting re-review** (first review addressed; cross-stream contract in issue #85)

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
/rustfmt.toml             # committed formatting config, including ignore list for derived Rust
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

Exact stable pin in `rust-toolchain.toml` (latest stable at T1 time, recorded there), components `rustfmt` + `clippy`; edition 2024; no MSRV (nothing is published; the pin is the supported version). Toolchain bumps are dedicated PRs, never riders, running full validation including the determinism matrix — this is also what makes `clippy -D warnings` a stable gate. Committed `rustfmt.toml` pins "default" formatting explicitly and carries the **ignore list for declared derived Rust** (generated crates are not formatted by the toolchain's rustfmt; they are printed deterministically by the generator's pinned printer — see the S2 spec). This ignore list is **bootstrap-only state**: when S5's manifests become authoritative for derived outputs, the list is derived from manifests, not maintained by hand (D10).

### D4 — PR validation workflow

`pr.yml` runs on pull requests and pushes to `main`:

| Job | Command | Runner |
| --- | --- | --- |
| checks-linux | `cargo xtask ci --base <event base>` | `ubuntu-24.04` |
| checks-macos | `cargo xtask test` + `cargo xtask determinism` | `macos-15` |
| deny | pinned `cargo-deny check bans licenses sources` | `ubuntu-24.04` |
| determinism-compare | download both manifests + bounded artifacts, compare | `ubuntu-24.04` |
| **validation** | aggregator: `needs:` every job above, `if: always()`, fails unless all succeeded | `ubuntu-24.04` |

Rules:

- **`validation` is the single stable required-check name.** New jobs must be added to its `needs` list; because it runs `if: always()`, a skipped or failed dependency cannot silently produce a green aggregate. This exact check is the branch-protection target if/when protection is available (D9).
- **Budget inputs are event-defined:** on pull requests, `--base` is the PR base SHA from the event payload; on `main` pushes the budget check is skipped (the PR already enforced it; a push has no meaningful base).
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

1. **Repeat experiment:** two runs, *identical* conditions — same canonical path, frozen environment (`TZ=UTC`, `LC_ALL=C`, fixed `SOURCE_DATE_EPOCH`). Any difference is intra-run nondeterminism (map ordering, randomness), attributable as such.
2. **Path experiment:** baseline vs. a run whose only change is a different, deliberately unusual absolute path (different length, a space, a non-ASCII segment). Differences attribute to path leakage.
3. **Time experiment:** baseline vs. changed `SOURCE_DATE_EPOCH` and system-time-visible env. Under the accepted platform guarantee, **generated bytes may never vary with time** — the harness varies the inputs precisely to prove invariance; S2 may not reopen this (it may ignore or sanitize internally, but output bytes are invariant, full stop).
4. **Locale/timezone experiments:** baseline vs. changed `LC_ALL`, then vs. changed `TZ`, separately.

Each experiment reports its own attributed failure class. Output is a deterministic manifest (sorted path → SHA-256 + size, versioned schema, byte-stable).

**Cross-platform protocol:** Linux and macOS each run the local protocol, upload manifests **plus the output trees themselves (bounded: subjects are size-capped so artifacts stay small)**; `determinism-compare` diffs manifests and, on mismatch, produces the bounded byte-level diff from the retained artifacts — the first draft promised diffs from hashes alone, which is impossible.

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
5. The `deny` job fails a PR introducing a disallowed license or non-crates.io source; the scheduled advisory job files an issue for a known advisory (verified once with a pinned historical advisory in a throwaway branch).
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

## Matters left open

*(None load-bearing; per review, load-bearing items may not hide here.)*

- Exact toolchain version, runner-image versions, and pinned cargo-deny version — resolved at implementation time and recorded in the task PRs.
- Whether macOS validation later narrows to merge-time only — deferred until CI cost is measured against the D4 target.
- Containerized runners for truly immutable environments — only if hosted-runner drift is ever observed to matter.
