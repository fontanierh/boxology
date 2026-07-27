# 2026-07-27 — Opening the S4 lane, and a ledger of the decisions taken to do it

A situation report covering the night of 2026-07-26/27, and — its main purpose — a written home for
five decisions that were taken during it and recorded only in issue comments. One of them was being
cited from code as load-bearing justification while existing nowhere a reader could find it. That is
the failure this record exists to correct.

## What shipped

Eight pull requests merged: #373 through #380. Three issues closed on acceptance: #323 (S5-T1),
#324 (S5-T2), #361 (the test-integrity audit), plus #368 closed inside a feature PR.

The substantive movement was structural rather than volumetric:

- **S5-T1 and S5-T2 are complete.** `boxology-manifest` parses schema-1 manifests with a locked
  diagnostic surface; `boxology-workspace` classifies every tracked file exactly once, maps Cargo
  members to declared `[[crates]]` entries, and judges crate roles. Codes BXW0001–BXW0054, dense.
- **S5-T3 opened**, reading declared Cargo edges (#380).
- **A second delivery lane opened on S4-T1**, because S5-T4 and S5-T5 both need its codec and the
  spine would otherwise stall at T4 waiting on a stream nobody was building. `boxology-schema` now
  holds the format-1 document model and is the single authority for serialising `schema.json`.
- **The one thing gating S5-T4 from S2-T6 was found and closed** (#379). An audit established that
  everything `boxology generate` consumes already existed except one thing: nothing in the workspace
  wrote a `GeneratedTree`. That is now a sibling crate, which makes "no filesystem from the pure
  generator" structural rather than asserted.

## The decisions, and where they are binding

Per this directory's convention, a record does not bind; the normative documents it cites do. Each
entry below names the document that actually carries the rule.

### 1. The strict schema reader is fail-closed on exposure and idempotency

**Binding through `specs/s4-contract-change-classification.md` D1**, which requires the reader to
reject any value outside the format-1 vocabulary.

The reader rejects `max_exposure` values other than `external` and `idempotency` values other than
`none`. The reasoning is not a preference: the emitter provably cannot write the others, because
`boxology-contract-syntax` hardcodes `external`/`none` and rejects the rest at the grammar. A
document carrying `internal` therefore did not come from the one codec, and treating it as valid
would mean the read side accepts what the write side cannot produce.

Two consequences are worth stating plainly rather than discovering later.

**S4 D5's exposure-narrowing and idempotency-change rows are reserved** until schema emission grows
variable exposure. They are not exercisable against any document the emitter can produce, so
pinning them now would require hand-built fixtures asserting behaviour on documents that cannot
exist.

**Widening the emitted grammar must widen the reader in the same change**, or valid documents start
being rejected. This is a two-sided change and the S4 side will not anticipate it. Recorded on #103,
which owns schema-byte authority.

This decision was taken during the S4-T1 specification and recorded only as "ruling B1" in comments
on #316 and #103. Slice 3 of that stack then cited "ruling B1" from a doc comment as the
justification for two byte-frozen diagnostic rule-sources — a phrase appearing nowhere in `specs/`,
`records/`, `AGENTS.md`, or `boxology-details/`. Worse, the two codes were attributed to
`specs/s2-contract-generator.md` D3, which enumerates all three exposures and names `code_only` as
the *default* — so a reader following the citation found text contradicting the rule. Review caught
it before it merged. The attributions now point at S4 D1, and the reasoning lives here.

### 2. `boxology generate` prunes stale declared outputs

**Binding through `specs/s5-manifest-and-validation.md` D6 step 2**, which names
`boxology generate --package <id>` as the repair for a stale artifact.

A file under a declared `[[derived]].outputs` glob that the generated tree no longer declares is
deleted by the writer. Without this, D6 step 2's promise is false — running the named repair command
leaves the finding standing forever.

The earlier draft justified *never* deleting as determined by 02-packages' merger step 5. It is not:
S5 explicitly defers merger steps 5–6 out of v0 as a non-goal, and D3 classifies an orphan under a
declared glob cleanly rather than as a defect. The "determined by the text" framing was wrong; this
is a decision, and it is the maintainer's.

Pruning is **not yet implemented**, deliberately, and the ordering matters. It must land *on top of*
the writer's case-fidelity fix, not under it: `GlobPattern::matches` compares bytes, so a stale
`generated/SCHEMA.JSON` matches none of the declared globs. A prune walk would leave it while the
write kept landing live bytes into it. Worse, under commit-then-prune — the only ordering compatible
with staged commit — a case-only rename of a declared output would have the write land new bytes in
the old-cased file and prune then delete that file as an undeclared orphan, losing a declared output
outright.

### 3. Three edge-policy cells rest on inference, and are marked as such

**Binding through `specs/s5-manifest-and-validation.md` D4**, which licenses coded failures for
role-impossible and unmatched edges generically.

- A box contract crate depending on its own package's **sibling contract crate** is forbidden,
  fail-closed. No row in `08-rust-build-topology.md` covers it and no silence is recorded.
- A box contract depending on a **declared foreign contract** is allowed. Row 6 forbids only
  *undeclared* foreign-contract edges, `08:201` permits contract-to-contract edges "when permitted by
  the declared contract model", and `08:203`'s section on keeping such edges acyclic presupposes they
  exist.
- A path dependency onto a **non-workspace-member** is legal only from a platform crate. `s7:41`
  requires fixture crates to be reachable only from platform-owned test crates; without this rule,
  S7 D4's migration would make *leaving workspace membership* a general evasion of the entire edge
  table.

These are labelled inferred in the code, not determined. That distinction is not pedantry: S5-T2
shipped a twelve-cell role table documented as textually determined when two cells rested on a
sentence about material and responsibility, closed by a doc comment in another crate. The table was
right; the claim about it was not.

### 4. The `boxology-http` dev edges are restructured, not narrowed

**Binding through `boxology-details/08-rust-build-topology.md:195**: *"Tests that deliberately link
several real implementations belong to an application composition and its integration-quality
contract."*

`boxology-http` dev-depends on `hello-implementation` and `hello-contract`, which the edge table
forbids. This was flagged as a product call with three options. It was not one: the document had
already ruled, and the escalation happened because the sentence *before* the deciding one was quoted
and the paragraph was not read to its end. The three affected tests move to a composition-kind
package that already dev-depends on both.

Still open, and genuinely: `crates/fixtures/fixture-tests` carries no `boxology.toml` and is not
covered by the hello package's `owned` patterns. It needs one at stage-2 adoption.

### 5. The #361 gate findings are accepted risk

**Binding through nothing** — this is an allocation decision, not a rule.

Four mutation-proven defects in the repository's own CI gates are accepted rather than fixed: the
400-line budget can be disabled wholesale, the whitespace gate made a no-op, the editor gate
repointed, and malformed argv can exit 0. The gates have a single trusted author working under
independent review, so "a contributor could edit the exclusion constant" is not a live threat.

Two findings were exempted and fixed, because byte-identical cross-platform output is an explicit
acceptance criterion in S2, S4 D7, S5 D8 and S6 AC1: a determinism-manifest sort and a
diagnostic-ordering sort, both guarded by fixtures that already satisfied the property.

What makes this worth recording is what the attempted fix revealed. A slice fixing all six xtask
findings was implemented and reviewed; review returned BLOCK because **two of the six fixes defended
against the literal mutation in the issue rather than the property the issue named**. After the fix,
the whitespace gate was still no-op-able and the budget still disable-able. The cheap version of
that work was not cheap.

## The recurring defect, stated once

Four times this session, in three different crates, the same class of defect was found in the
*mechanism that exists to catch defects*.

`boxology-manifest`, `boxology-workspace` and `boxology-schema` each carry a "surface lock": a code
inventory, a byte-compared golden of each code's rule text, and a compile-time scan of the crate's
own source proving no code is emitted without being registered. Each lock was written by copying the
previous one. Each copy was found to have a hole the original also had, or a new one:

1. The `#[cfg(test)]` cut took everything before the *first* occurrence without asserting that
   occurrence was the test module. A stray marker above a new code truncates the scan silently.
2. `include_str!` of a single file goes blind the day the crate grows a second source file.
3. The cut scans everything *before* the test module, so production code appended *after* it — the
   most ordinary way code gets added — is never scanned at all.

All three produced a fully green suite. None was caught by the worker who wrote the lock; all three
were caught by adversarial review specifically instructed to attack the lock rather than the code.

Two further lessons attach to it. **The idiom is not portable**: `boxology-manifest`'s guarded cut,
applied verbatim to `boxology-schema`, panics — because that crate puts a `#[cfg(test)]` const inside
a macro two hundred lines above `mod tests`. It had been cited in four directives as
proven-and-reviewed; it is proven for the crate it was written in. And **locking early is not free**:
landing a lock before its reader means the whole production half is unreachable from non-test code,
which `-D warnings` denies. Neither earlier lock hit this because both landed beside live code.

## Where v0 stands

S5-T1 and S5-T2 closed. S5-T3 is specified as three PRs with its first merged and second in review.
S5-T4 and S5-T5 wait on S4-T1's classifier slice, which is placed as early as its dependency order
allows; S4-T1 is three of six slices in. S2-T6's writer is merged, leaving pruning and the purity
gates. S3, S6 and S7 are unstarted by deliberate allocation — S3 must resume before S6, but not
before S5 and S4 are past their blocking tasks.

The honest estimate correction: S5-T2's spec table said two PRs and it landed as seven. S4-T1's said
two and is specified as six. S5-T3's said one to two and is specified as three. Slice-level estimates
in this project have run 2–3× low with enough consistency that the pattern should be assumed rather
than rediscovered.
