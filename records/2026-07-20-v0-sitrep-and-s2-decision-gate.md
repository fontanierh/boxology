# V0 Situation Report and the S2 Crate-Root Decision Gate

Record of the review held on 2026-07-20 between the maintainer and Codex after
PR #187 merged. The maintainer asked for a complete account of repository
activity, progress, project health, and distance from v0, then asked that the
result be preserved and that the active S2 decision gate be explained.

This is a historical situation report, not a normative design document. It
records no new product decision. The accepted stream definitions and specs
remain authoritative, and the unresolved S2 decisions remain open in issue
[#102](https://github.com/fontanierh/boxology/issues/102).

## Executive assessment

Boxology is moving exceptionally quickly and the implementation process is
healthy. S0 is genuinely complete, S1 has delivered most of the contract and
invocation substrate plus the first transportless composition assembly, and S2
has begun with a deterministic pure-input parser/model seam.

It is nevertheless still an early foundation, not a usable end-to-end product.
Only one of eight v0 streams is formally complete. The generator does not emit
contracts, the HTTP binding does not exist, compatibility classification and
`boxology check` have not started, and there is no installer, generated Hello
project, acceptance run, or stage-2 self-hosting evidence.

A scope-weighted estimate at this checkpoint is approximately **20–25% of v0**,
with a wide error band because S4–S7 have not yet received stream specs and the
first completed stream showed that pre-execution PR estimates ran roughly three
times light. The exact, auditable statement is narrower: S0 is complete; S1 is
well advanced but incomplete; S2 is early; S3 is specified but unimplemented;
S4–S7 exist only as stream definitions.

## Repository activity at the checkpoint

The checkpoint was taken at 2026-07-20 16:26 CEST against `main` commit
`53d2df52fcbf3520c930f8fe1e716cd1afb4eacf`, the squash merge of
[#187](https://github.com/fontanierh/boxology/pull/187).

- The private repository was created on 2026-07-15.
- 88 pull requests existed: 83 merged, five closed unmerged, and none open.
- The five unmerged pull requests were deliberate red proofs showing that S0's
  gates reject bad inputs; they were not abandoned product changes.
- Merge activity was nine PRs on July 16, ten on July 18, 38 on July 19, and 26
  on July 20, using GitHub's recorded merge dates.
- 99 tracker issues existed: 42 closed and 57 open. Of the open issues, 38 were
  explicitly `post-mvp`; 18 were S1–S3 task or completion issues; the remaining
  issue was the early factory/dogfooding forcing function #74. S4–S7 had not yet
  been decomposed into task issues, so the open count did not represent all
  remaining v0 work.
- Local `main` was clean and exactly synchronized with `origin/main`.
- The repository contained 77 tracked files, about 14,450 Rust lines, and 213
  test cases in the exact-main validation run.
- No pull request was open, but clean local worktrees for the next S1-T6 and
  S2-T1 slices had been created. This was a boundary between merges, not an idle
  repository.

## Progress by stream

### S0 — product-repo bootstrap and CI: complete

The completion audit in [#93](https://github.com/fontanierh/boxology/issues/93)
closed on 2026-07-19. Merged S0 supplies the pinned Rust workspace, repository
automation, Linux/macOS validation, Markdown link and anchor checks, the
absolute 400-hand-authored-line review budget, dependency and advisory policy,
and the local and cross-platform determinism protocols with negative controls.

The process evidence and the decision to retain the absolute 400-line ceiling
are recorded in the earlier [S0 situation report](2026-07-19-s0-sitrep-and-pr-budget-review.md).

### S1 — runtime core and composition assembly: well advanced

Tasks T1–T5 were closed. Merged code included:

- the contract value, slot, presence, opacity, secret, and blob models;
- descriptor-guided conformance and typed conversion;
- contract, implementation, import, capability, and type descriptors;
- `CallContext`, child derivation, invocation/domain error separation, erased
  dispatch, and construction/poll panic containment;
- sealed runtime import handles and their pre-dispatch ordering;
- the complete current assembly-error vocabulary; and
- a public transportless `CompositionBuilder`, deterministic registration and
  import validation, factories receiving lazy imports, fallible start, sealing,
  and local provider selection.

Task T6 remained open. Conceptual slices 1 and 2 of its six-slice accepted plan
were complete, with slice 2 split into two PRs at a pre-approved semantic seam.
Slices 3–6 still owned assembled in-process acceptance evidence,
transport/exposure lifecycle, the stub transport and failure proofs, and the
shutdown/race state machine. T7's Hello, kitchen-sink, and greeter fixtures had
not begun and remained blocked by T6.

### S2 — contract generator: early

Four coherent T1 slices were merged:

1. a pure in-memory `GenerationRequest`/diagnostics/model foundation and the
   first real generator-model determinism subject;
2. deterministic validation of logical inputs and declared import inputs;
3. immutable `boxology.toml` identity parsing with diagnostics `BXG0007` through
   `BXG0013`; and
4. deterministic parsing of every declared `.rs` input as a complete `syn::File`,
   retaining path/AST association and aggregating syntax failures as `BXG0014`.

T1 still lacked crate-root selection, module traversal and reachability,
ancestor-`cfg` enforcement, declaration discovery, the normative grammar and
remaining identities, capability assembly, and imported-schema semantics. T2–T6
and T8 had not started, so there was no schema/fingerprint emission, contract
crate, typed handle, adapter, macro companion, hardened generator API, or golden
compile-and-run proof.

### S3–S7: ahead, not delivered

S3 had an accepted detailed HTTP spec and six task issues, but no implementation.
S4 compatibility classification, S5 manifest/ownership validation and CLI, S6
installer/generated project, and S7 skill/acceptance/self-hosting had only the
stream-level definitions in [V0 Streams](../boxology-details/11-v0-streams.md).

## Quality and delivery health

The exact-main [run 29750189100](https://github.com/fontanierh/boxology/actions/runs/29750189100)
passed all six jobs in approximately 82 seconds. Linux and macOS each ran 110
contract tests, 24 generator-model tests, 11 runtime tests, 66 xtask tests, and
two doctests. Formatting, clippy, documentation, dependency policy, local
determinism, artifact verification, cross-platform byte comparison, and the
expected-mismatch negative-control lane all passed.

Of the latest 100 workflow runs, 92 succeeded, six failed, and two were
cancelled. All six failures were intentional runs from the S0 cross-platform
red-proof PR. The cancellations were consistent with the configured
cancel-in-progress behavior for superseded runs.

Independent review also found observable defects before merge rather than
serving only as ceremony. PR #184's initial review found that deadline evidence
could not distinguish an expired deadline from any present deadline. PR #187's
initial review found that provider-selection evidence could not distinguish the
consumer from the intended provider. Both candidates were repaired and freshly
re-reviewed before merge.

The recent PRs respected the 400-line hand-authored budget. GitHub totals above
400 on some PRs included excluded, mechanically derived `Cargo.lock` changes;
the recorded hand-authored counts remained below the ceiling.

## Risks and interpretation

1. **The core thesis remains untested.** The current evidence supports disciplined
   agent implementation of a foundation. It does not yet demonstrate that the
   box discipline improves safe many-agent concurrency. The project's
   [strategy review](../boxology-details/10-strategy-review.md) already states
   that the safe-parallelism experiment, not v0 code generation by itself, is
   the real product test.
2. **Velocity can hide remaining scope.** Twenty-six merges in one day is strong
   throughput, but it is not product completion. The long-pole generator and
   all streams after S3 remain ahead.
3. **Estimation remains weak.** S0's roughly threefold PR-count miss is recurring
   in S1. Dozens, and plausibly more than one hundred, small PRs may remain
   through S7 even if raw wall-clock throughput stays high.
4. **Human attention remains the scarce resource.** The review ceiling protects
   attention per PR, but the aggregate review load at the observed merge rate
   is itself a watch condition because oversight is Boxology's durable
   justification.
5. **Server-side enforcement is incomplete.** Squash-only PR merging is enabled,
   but the current GitHub plan does not provide repository branch protection or
   rulesets. Required checks and direct-push prevention are therefore not
   server-enforced. The bootstrap CI is also candidate-writable until S5/S7 can
   evaluate policy against the base revision.
6. **Review evidence is not first-class GitHub review state.** Recent independent
   model reviews were summarized in PR bodies and reconciliation comments, while
   GitHub showed no submitted review objects. The reports affected delivery, but
   the audit surface is more prose-dependent than native review artifacts.
7. **Local integration hygiene is accumulating.** The checkout had 49 worktrees,
   76 local branches, and 50 branches with gone upstreams. All inspected
   worktrees were clean and `main` was unaffected, but eventual pruning is
   operational debt.

## The S2 crate-root decision gate

S2 D2 requires deterministic module resolution to start at **the crate root
within the declared inputs**, follow plain `mod x;` declarations, reject
`#[path]`, identify annotated items in unreachable files, and enforce `cfg` and
`cfg_attr` restrictions through each resolved ancestor chain.

After manifest identity parsing landed, an independent specification attempt
returned `NO-GO`: the accepted design named the starting concept but no accepted
input or rule identified the actual root file.

- `GenerationRequest` had no `crate_root` member.
- Manifest `[[crates]]` entries named package, path, and role, but not a root
  file.
- The generator's declared inputs intentionally excluded the implementation's
  `Cargo.toml`, so Cargo's optional `[lib].path` could not act as hidden
  authority.
- The S1 `authoring/` fixture layout did not yet define how an authoring root
  mapped to a manifest crate entry.

This is a product-contract decision, not a missing local variable. Guessing
`src/lib.rs`, choosing the first Rust file, or silently reading more filesystem
state would determine which declarations are authoritative, which files are
unreachable, which ancestor attributes apply, what custom Cargo layouts v0
supports, and whether the generator remains a pure function of its explicit
request. Once tests and generated artifacts depended on a guess, that guess
would become an accidental compatibility promise.

The principal options recorded on #102 were:

1. Have S2 parse exactly one `box-implementation` manifest entry and derive
   `<path>/src/lib.rs`.
2. Add a validated logical `crate_root` path to `GenerationRequest`; the caller,
   eventually S5, derives and supplies it.
3. Add an explicit root field to manifest schema 1.
4. Require the literal v0 layout `implementation/src/lib.rs`.

The independent specifier recommended option 2, but the recommendation was not
accepted as a decision. Its main advantage is alignment with S2 D1: callers own
filesystem/workspace resolution and the generator consumes a complete explicit
request. It avoids teaching the pure generator Cargo or workspace discovery and
keeps the root visible in determinism inputs. It still requires an explicit v0
policy for how S5 obtains that value and whether nonstandard Cargo roots are
unsupported, caller-configurable, or represented later in a manifest.

Option 1 keeps author configuration smaller but makes the generator interpret
more workspace topology and bakes in `src/lib.rs`. Option 3 is explicit and can
support custom roots but expands the user-facing manifest and risks duplicating
Cargo configuration. Option 4 is simplest for the greenfield v0 installer but
is the most rigid and must be documented honestly as a support restriction.

The gate blocks module traversal, reachability, ancestor-`cfg` enforcement, and
the authoritative declaration set. It did not block parsing every declared
Rust input, so that decision-invariant work landed in PR #186. Further work may
prepare similarly invariant structures, but it must not encode root selection
or any equivalent layout behavior before the project accepts one option and
reconciles S2, the fixtures, S5's future resolver, and #102.

The crate-root question is one of three load-bearing S2 decisions still named in
the spec. Self-import policy remains coordinated with S1 #99, and transitive
presence through `Secret` remains coordinated with S3 #112. None was decided by
this review.

## Bottom line

At this checkpoint the project was green, fast, and unusually disciplined. The
correct interpretation was **healthy execution of an early foundation**, not
near-completion or validation of the many-agent thesis. The S2 crate-root pause
was positive evidence for the methodology: the implementation stopped at an
authority gap, completed only decision-invariant work, and kept the missing
product decision explicit instead of converting an implementation convenience
into policy.
