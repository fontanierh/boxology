# Repository Workflow

## Tracker reconciliation

Before merging any pull request or closing any issue:

1. Compare the proposed result with current `main`, every linked issue, and every relevant review thread.
2. Scan all other open issues for premises, questions, or scope changed by the same accepted decisions.
3. Update every affected issue before the merge or closure.
4. Close an issue only when it is resolved, or when it is explicitly superseded and every unresolved part is transferred to named open issues.
5. Keep reviewer recommendations and other proposals undecided unless the project explicitly accepted them and recorded the decision in merged documentation.

Record the reconciliation in the pull request or closing issue comment. The full design rule is documented in [Quality and Authority](boxology-details/06-quality-and-authority.md#tracker-reconciliation-gate).

## Operational records

Dated situation reports, process reviews, retrospectives, and calibration notes live in [`records/`](records/README.md), one file per record, named `YYYY-MM-DD-topic.md`. Records are historical and never rewritten; corrections are new records citing the old. Decisions bind through the normative documents they cite or amend, not through the record itself. The naming, index, and append-only rules are enforced mechanically by `cargo xtask records` as part of repository validation. To create a record from a conversation, use the [`record` skill](.agents/skills/record/SKILL.md) — a portable Agent Skills-format skill in `.agents/skills/`, usable by any agent — which defers to the conventions in [`records/README.md`](records/README.md).

## Issue deliverable kinds

Every issue declares its deliverable kind at creation. Issues resolved by modifying or adding markdown carry the `design-docs` label. Issues resolved in code carry no deliverable label; code is the default in the implementation era. The `post-mvp` and `factory` labels remain the sequencing and product axes.

## V0 execution methodology

V0 implementation proceeds through four levels, each specified before the next begins:

1. **Streams.** The high-level v0 workstreams are defined in [V0 Streams](boxology-details/11-v0-streams.md). Streams partition the v0 scope; every task belongs to exactly one stream.
2. **Stream specs and task lists.** Each stream receives a spec in `specs/`, reviewed and merged before its implementation starts. The spec produces the stream's task list as tracker issues referencing the stream.
3. **Task specs.** Each task is specified — in the issue or an accompanying document — before its implementation starts. A task spec states scope, interfaces touched, and its acceptance checks.
4. **PR stacks.** Each task is implemented as a stack of pull requests, based sequentially and merged in order. **Every pull request adds at most 600 hand-authored lines, including tests.** Checked-in derived artifacts (generated contract crates, schemas, `Cargo.lock`) are excluded from the count but must satisfy the reproducibility rules; the budget measures what a human must review, and derived output is verified mechanically instead.

Each pull request keeps a single accountable owner under the ownership rules, passes the repository's validation, and goes through the tracker reconciliation above. Tests assert what a value **is**, not what it is not — an absence check passes vacuously whenever the value had no path there, and a fixture that already satisfies the property under test proves nothing about the code that enforces it. A change that cannot fit the budget is split further or its task re-scoped; the budget is a review-attention ceiling, not a stylistic preference.

## Delivery method

The contributor delivery loop used to execute this repository's tasks — independent specification, implementation, fresh review, repair, validation, tracker reconciliation, then merge — is committed as an Agent Skill at [`.agents/skills/repository-delivery-loop/SKILL.md`](.agents/skills/repository-delivery-loop/SKILL.md). Its repository-owned [model configuration](.agents/skills/repository-delivery-loop/models.toml) selects the harness, model, effort, and ordered fallbacks for each role. Workers from the active harness use its native sub-agent mechanism; workers from another harness run through that harness's CLI. The skill documents how this repository is developed; it is not a product deliverable. The portable Boxology onboarding skill remains a distinct S7 deliverable — defined in [V0 Streams](boxology-details/11-v0-streams.md) — in the established `.agents/skills/` location; its product guidance and acceptance contract are specified by the [S7 spec](specs/s7-skill-acceptance-self-hosting.md).
