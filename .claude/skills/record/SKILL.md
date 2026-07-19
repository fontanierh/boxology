---
name: record
description: Create a dated operational record in records/ capturing the current conversation — a situation report, process review, retrospective, or calibration note. Use when the maintainer asks to record a discussion, decision, or state of play.
---

# Create an operational record

The convention — what a record is, how it is named and indexed, that it is append-only, and how its decisions bind — is defined in [records/README.md](../../../records/README.md) and the Operational records section of [AGENTS.md](../../../AGENTS.md). Read both first and follow them; do not restate or improvise the rules.

## Steps

1. Read `records/README.md` and the Operational records section of `AGENTS.md`.
2. Distill the conversation into a record: context (date, participants, what prompted it), substance (findings, evidence, analysis), and any decision with its rationale and revisit triggers. Write for a reader who was not present; existing records are the style reference.
3. Name the file per the convention with today's date and a short topic slug, place it in `records/`, and add it to the index in `records/README.md`.
4. If the record states a decision that changes a normative rule, make that normative edit (AGENTS.md, design document, or spec) in the same change — the record itself binds nothing.
5. Validate with `cargo xtask records` and `cargo xtask records --base origin/main`.
6. Land it through a pull request under the repository's PR rules in `AGENTS.md`; self-merge only when the maintainer has asked for that.
