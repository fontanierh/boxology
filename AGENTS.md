# Repository Workflow

## Source of truth

Read the relevant accepted file in `specs/` and `boxology-details/` before changing behavior. These
documents describe current normative truth and should be updated in place when the product changes.
Use GitHub issues for task scope and unresolved decisions; do not treat review suggestions as
accepted policy until the project records that decision.

## Tracker reconciliation

Before merging a pull request or closing an issue:

1. Compare the result with current `main`, linked issues, and review threads.
2. Scan other open issues for premises or scope changed by the same decision.
3. Update every affected issue before merge or closure.
4. Close an issue only when it is resolved, or when remaining scope is transferred to named open
   issues.

Record the reconciliation in the pull request or closing issue comment. The full rule is in
[Quality and Authority](boxology-details/06-quality-and-authority.md#tracker-reconciliation-gate).

## Changes and validation

Keep changes pragmatic, narrowly scoped, and proportionately tested. Every pull request adds at
most 600 hand-authored lines, including tests. Checked-in derived artifacts such as generated
contract crates, schemas, and `Cargo.lock` do not count, but must remain mechanically reproducible.

Each change has one accountable owner under the root and package `boxology.toml` manifests. Run
`cargo xtask ci --base <revision>` for normal acceptance, or `cargo xtask ci --no-budget` when no
meaningful base exists. Tests should use positive controls and mutation cases where a vacuous
absence assertion could otherwise pass.

The portable [Boxology onboarding skill](.agents/skills/boxology/SKILL.md) is product guidance for
greenfield managed projects. It does not govern development of Boxology itself.
