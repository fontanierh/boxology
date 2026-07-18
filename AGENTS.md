# Repository Workflow

## Tracker reconciliation

Before merging any pull request or closing any issue:

1. Compare the proposed result with current `main`, every linked issue, and every relevant review thread.
2. Scan all other open issues for premises, questions, or scope changed by the same accepted decisions.
3. Update every affected issue before the merge or closure.
4. Close an issue only when it is resolved, or when it is explicitly superseded and every unresolved part is transferred to named open issues.
5. Keep reviewer recommendations and other proposals undecided unless the project explicitly accepted them and recorded the decision in merged documentation.

Record the reconciliation in the pull request or closing issue comment. The full design rule is documented in [Quality and Authority](boxology-details/06-quality-and-authority.md#tracker-reconciliation-gate).
