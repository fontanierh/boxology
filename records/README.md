# Records

Dated operational records: situation reports, process reviews, retrospectives, and calibration notes produced while executing the project. One file per record, named `YYYY-MM-DD-topic.md`.

The convention is mechanically enforced by `cargo xtask records`, which runs inside `cargo xtask ci`: every file here is either this index or a well-named record, every record is linked from the index below, and — checked against the pull request's merge base — a record, once merged, is never modified, renamed, or deleted. Only this index changes after the fact.

Records differ from [`boxology-details/`](../boxology-details/) in kind: design documents are normative and are amended in place when decisions change; records are historical and are never rewritten. A record that later proves wrong is corrected by a new record that cites it, so the reasoning trail — including its errors — stays inspectable. This is the documentation-layer form of the project's own review discipline.

A record may state a decision (for example, a process parameter held or changed after review). The decision is binding through the normative documents it cites or amends — `AGENTS.md`, a design document, a spec — not through the record itself. If a record's decision requires a normative change, that change lands in the same pull request.

The strategy review of 2026-07-18 predates this directory and remains at [`boxology-details/10-strategy-review.md`](../boxology-details/10-strategy-review.md); records of its kind land here from now on.

## Index

- [2026-07-23 — S2 architecture proof and v0 reassessment](2026-07-23-s2-arch-proof.md)
- [2026-07-22 — Generated-box progress and tracker-integrity situation report](2026-07-22-generated-box-and-tracker-sitrep.md)
- [2026-07-22 — Morning v0 situation report and coordinator course correction](2026-07-22-morning-coordinator-course-correction.md)
- [2026-07-22 — Late-night v0 situation report](2026-07-22-late-night-v0-sitrep.md)
- [2026-07-21 — Generated-box critical-path review](2026-07-21-generated-box-critical-path.md)
- [2026-07-21 — Overnight v0 situation report](2026-07-21-overnight-v0-sitrep.md)
- [2026-07-20 — V0 situation report and the S2 crate-root decision gate](2026-07-20-v0-sitrep-and-s2-decision-gate.md)
- [2026-07-19 — S0 situation report and review of the 400-line PR budget](2026-07-19-s0-sitrep-and-pr-budget-review.md)
