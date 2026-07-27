# 2026-07-27 — S4 ordinary Rust identifiers and the reserved `Unknown` variant

This decision record captures the approved prerequisite for the S4 identifier and reserved-variant
work. It is historical: **this record itself does not bind**. The binding normative change is
`specs/s2-contract-generator.md` D3 and D4. `specs/s1-runtime-core.md` D4 and
`specs/s4-contract-change-classification.md` D1 are the cited existing authorities; neither is
amended by this record.

## Decision

The contract boundary uses one shared, public `boxology-contract` predicate for ordinary
non-raw Rust 2024 identifiers. It follows the Rust identifier profile based on Unicode
`XID_Start`/`XID_Continue`, rejects `_` alone, strict and reserved keywords, raw spellings, and
the Rust-disallowed zero-width joiner/non-joiner characters, and accepts weak-keyword spellings
where Rust treats them as identifiers. `unicode-ident` is a direct exact-pinned dependency of
`boxology-contract`.

The error variant name `Unknown` remains reserved. S1 D4 requires that generated typed errors use
that name to preserve an unknown domain-error tag and its opaque payload during tolerant decoding.
The S2 parser therefore reports the dedicated reserved-variant diagnostic. This is a semantic
runtime-opacity reservation, not an accidental consequence of the generic identifier check. S4
D1's one-codec and strict fail-closed rules mean that the reader and emitter share this vocabulary;
the reader cannot silently accept an `Unknown`-colliding or otherwise non-emittable format.

## Evidence and scope

The existing `syn` parser supplies Unicode-aware tokenization but its checked-in keyword table does
not cover the Rust 2024 `gen` reservation. The shared validator is consequently the final
acceptance gate in `boxology-contract-syntax`, with table-driven tests for the complete strict,
reserved, and weak keyword categories, Unicode boundaries, raw spellings, and the exact `Unknown`
diagnostic. The syntax crate depends on `boxology-contract`; the contract crate does not depend on
the syntax crate, so this does not introduce a cycle.

The preserved S4 draft and `boxology-schema` are outside this prerequisite and are intentionally
unchanged. No commit, push, issue edit, pull request, or deployment is part of this record.
