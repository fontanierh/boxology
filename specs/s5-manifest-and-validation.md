# S5 Spec — Manifest and Validation Tooling

[Stream definition](../boxology-details/11-v0-streams.md#s5--manifest-and-validation-tooling) ·
Status: **delivered**

S5 delivers strict `boxology.toml` parsing and discovery, ownership/path classification,
crate-role and Cargo-edge validation, lockfile checks, and the `boxology generate` / `boxology
check` commands. [Packages](../boxology-details/02-packages.md) and
[Rust Build Topology](../boxology-details/08-rust-build-topology.md) remain normative for the
format and validation baseline; this file records the implemented subset and live residuals.

## Boundary

V0 `check` reports policy facts; it is not a merger and does not replay a base package plus an
accountable diff, resolve minimal lockfile closure, or authorize an incompatible contract change.
Provider roles, isolation-profile enforcement, package-scoped validation, publication, and branch
protection are not delivered. The CLI surface is `boxology generate [--package <id>]` and
`boxology check [--base <revision>] [--format human|json]`.

## Delivered decisions

### D1 — Purity split

- `boxology-manifest` is the pure strict schema-1 parser.
- `boxology-workspace` is pure over supplied file, manifest, schema, and Cargo-metadata data. It
  owns discovery, classification, crate roles, and edge policy without filesystem/process access.
- `boxology-cli-core` owns the reusable effectful implementation: filesystem and git reads, Cargo
  commands, generation, validation, and report rendering. `boxology-cli` retains the installed
  `boxology` binary and compatibility re-export facade. It is also the governed composition for
  the `generate` classification path and consumes the generated classifier handle; `check`
  remains on the direct core path pending its named #575 slice. The `boxology` facade crate
  remains authoring-only.

Library diagnostics use stable `BXW####` codes and deterministic ordering.

### D2 — Manifest and classification

Schema 1 accepts `box`, `composition`, and `platform`; unknown keys, newer schemas, provider kind,
invalid workspace-relative globs, and impossible role combinations fail closed. Composition
bindings select an exact capability or a nonempty `<box>.*` expansion and use `in-process` or
`http` transport without exceeding declared exposure.

Each `[[crates]].path` is package-relative. Exact `.` names a Cargo crate at the declaring package
root; every other accepted value is one literal relative directory. Wildcards, absolute paths,
empty or dot segments, and traversal fail closed. A root entry matches only a Cargo member at that
package root; Cargo package name and normalized path must still match together.

A platform package may declare fixture-opaque owned subtrees. Nested manifests in those trees are
data, not discovered packages. Platform-only `protected` declarations identify control-plane
paths, but their V0 strength is reporting only.

Every tracked file classifies exactly once as one package's owned source or one declared derived
output. Ambiguous, overlapping, and unowned paths fail with sorted diagnostics. Cargo packages
match exactly one declared crate role. Edge policy reads declared normal, build, dev, renamed,
optional, feature, and target-specific dependencies from `cargo metadata`; source inclusion that
bypasses a Cargo edge remains outside that model.

### D3 — Generation

Generation candidates and inputs come from manifests, including declared imported schemas. The
CLI regenerates into temporary output, compares bytes, writes only changed packages through the
generator's confined per-file atomic publication, refreshes provenance, and attaches the S4
classification unmodified. Post-V0, that classification crosses the real generated classifier
handle assembled by the CLI composition; generation and publication remain ordinary core code.
`--package` selects one package; a byte-identical run writes nothing.
From a partially managed monorepo root, an explicit package selection enters that package's unique
descendant managed Cargo workspace. Unmanaged sibling trees remain outside generation and
validation. Zero or multiple matching managed roots retain the invocation root and fail closed
through ordinary discovery instead of selecting one arbitrarily.
For a `box-implementation` crate, path `.` projects to package-root `src/lib.rs`; a nested crate path
projects to `<path>/src/lib.rs`.
While any declared generated contract crate is absent, `generate` plans from the otherwise
validated manifest-owned source model. It may write one selected package without requiring an
unrelated absent derived workspace member to exist first. Ordinary Cargo-backed workspace
validation resumes as soon as every declared contract manifest exists. This bootstrap exception
does not apply to `check` or to a generation run whose complete declared contract graph exists.

V0 assumes the source-tree generator is one byte-stable version. A published generator that
changes representation for unchanged source must add the provenance-compatible historical skip
before claiming that release boundary.

### D4 — Check and base semantics

`check` is non-mutating, whole-workspace, and ordered. Validation defects exit `1`, invocation or
tooling failures exit `2`, and success exits `0`. Human and schema-versioned JSON reports carry the
same findings. The top-level `--help` path prints the stable command usage to stdout and exits `0`;
malformed invocations continue to print usage to stderr and exit `2`.

With `--base`, the CLI obtains base manifests and schemas from git. Contract changes are classified
against that revision and reported without suppression or policy authorization; successfully
classified findings retain a `passed` step status because they are facts for the harness, not
checker-policy failures. Each changed path uses the base revision's declarations when that exact
path exists in the base tree and the validated submitted declarations otherwise. Modified and
deleted paths therefore retain base authority, while introduced manifests and files may establish
submitted ownership. After those per-path classifications are combined, a nonempty diff needs at
least one non-derived owner; several owners are a valid coordinated change and every declared
package quality command still runs. Declared derived outputs remain byte-checked by regeneration;
every non-lock derived output must belong to one of those non-derived owners. The sole bootstrap
exception is a managed workspace root with no Git object at all in the base: its wholly introduced
tree is attributed exactly once under the submitted manifests. A lockfile diff without any
accountable package's manifest dependency-declaration change emits a coded scope finding; minimal-closure
replay remains deferred. This is reporting strength, not the
factory merger's replay/enforcement protocol. Without `--base`, local check uses the merge base
with `main`; where no usable revision exists, base-relative steps are explicitly reported skipped.
An explicit base comparison with zero changed paths is likewise reported skipped with a `0 changed
paths` reason rather than displayed as a vacuous ownership pass.

The baseline includes discovery/ownership, regeneration comparison, classification, lockfile
freshness, manifest-derived formatting, workspace Clippy/tests, and manifest quality commands.
Contract-classification findings are report-only and do not alone change the exit code; merger
policy remains outside the checker.

Three execution contexts are intentionally distinct:

- generated projects receive a golden-pinned `ubuntu-latest` workflow with explicit
  `boxology check --base <pull-request-base>`; V0 did not execute that source-provisioned workflow,
  and its first run is deferred to #525 at the first pinned external release;
- this repository's required PR job runs hygiene and conditional scoped tests, but does not invoke
  `boxology check`;
- local/deep `cargo xtask ci` owns exactly one complete product check.

### D5 — Determinism and CI absorption

Library results and report structure are sorted and independent of locale, root path, and time.
Captured third-party tool text is outside the byte-determinism claim. V0 proved the registered
workspace-report subject across controlled contexts on native macOS ARM64; #525 owns continuous
cross-platform evidence and first-release execution of the emitted Linux workflow.

PR #571 completed [#342](https://github.com/fontanierh/boxology/issues/342): each canonical
`cargo xtask ci` aggregate delegates platform validation to exactly one `boxology check`, retained
repository-only checks have separate names, and the duplicated derived-output and formatting
registries were deleted in favor of manifest data.

## Delivered acceptance evidence

The merged S5 suite covers malformed manifests, every Cargo edge kind, deterministic ownership,
green and tampered workspaces, no-op and selected generation, all exit codes and both renderings,
base-relative incompatible/reporting cases, lock freshness, two independent quality-command
fixtures, emitted-workflow goldens, and native-Mac repeated-root determinism. Completion evidence
is tracked by [#328](https://github.com/fontanierh/boxology/issues/328) and
[#329](https://github.com/fontanierh/boxology/issues/329).

## Live residuals

- The factory merger's base replay, minimal lock closure, and foreign-impact reassessment.
- The first-release provenance-compatible generator boundary and Linux/cross-platform proof (#525).
- Provider roles, isolation validation, package-scoped checks, identity lifecycle (#3), and
  new-package creation policy (#47).
- [#477](https://github.com/fontanierh/boxology/issues/477) items 2–6: cycle diagnostic locality,
  self-cycle/transitive-chain coverage, imported-path diagnostic invariants, the BXW0068 retirement
  comment, and one degraded mutant. Item 1's one-pass stale-import convergence is delivered.
