# 2026-08-11 — Telegram self-hosting closeout

## Outcome

Telegram's product entrypoints satisfy the repository's definition of self-hosting. The
implementation remains the governed `telegram` box, while the installed
`boxology-telegram` binary is owned by the governed `boxology-cli` composition. Every
substantive one-shot operation crosses a generated typed handle. The continuous listener is
composition-local orchestration over generated `listen_start`, `poll`, and local `status`
calls rather than a streaming capability invented for one consumer.

The code chain completed through:

- [PR #622](https://github.com/fontanierh/boxology/pull/622), which moved installed CLI and
  listener assembly behind generated handles;
- [PR #623](https://github.com/fontanierh/boxology/pull/623), which repaired a stale generator
  dependency inventory exposed by exact-main validation;
- [PR #624](https://github.com/fontanierh/boxology/pull/624), which selected the primary
  `boxology` binary in generated-project tests and refreshed integrity pins; and
- [PR #625](https://github.com/fontanierh/boxology/pull/625), which preserved legacy JSON byte
  order when workspace feature unification enables `serde_json/preserve_order`.

Merged `main` commit `a42664894ae3fd12bc7865dd51a37a74b9e448b7` is the recorded closeout
state. The exact pre-merge candidate `d503cb47d418f453cff11438b03cece490c0af18` passed the
complete governed-workspace check, and PR #625's required
[GitHub validation run](https://github.com/fontanierh/boxology/actions/runs/31481364664)
passed in 1 minute 52 seconds before merge.

## Exact validation

On the clean `a42664894ae3fd12bc7865dd51a37a74b9e448b7` worktree, supervised run
`telegram-record-cold-generate-main-20260811` executed:

```sh
cargo run -p boxology-cli --bin boxology -- generate --package telegram
```

It returned:

```text
generate telegram unchanged
generate result unchanged
```

Supervised run `telegram-record-generated-diff-main-20260811` then executed:

```sh
git diff --exit-code -- crates/boxology-telegram/generated
```

and passed with no generated diff.

On exact candidate `d503cb47d418f453cff11438b03cece490c0af18`, supervised run
`telegram-byte-parity-full-check-20260811` executed `boxology check`. Discovery,
regeneration, contract classification, diff ownership, Cargo graph, formatting, Clippy,
all workspace tests, all package quality commands, and the final result passed. This run
included deterministic generated-handle/fake-API coverage; no live Telegram operation was
performed.

## Command mapping

| Installed command | Governed path |
| --- | --- |
| `send` | generated `send` handle |
| `ask` | generated `ask` handle |
| `reply` | generated `reply` handle |
| `resolve-send` | generated `resolve_send` handle |
| `pair begin` | generated `pair_begin` handle |
| `pair complete` | generated `pair_complete` handle |
| `pair revoke` | generated `pair_revoke` handle |
| `poll` | generated `poll` handle |
| `ack` | generated `ack` handle |
| `status` | generated `status` handle |
| `listen` | composition-owned loop over generated `listen_start`, `poll`, and local `status` |

The original scalar `send_text` dogfood capability remains available as historical and
compatibility evidence, but the installed substantive CLI path uses the structured generated
operations above.

## Friction and repairs

Closeout validation found three integration defects rather than hiding them behind a relaxed
gate:

1. adding the installed Telegram binary made package-only `cargo run -p boxology-cli`
   ambiguous, so generated-project tests now name `--bin boxology` explicitly;
2. all-features feature unification changed JSON map insertion behavior, so construction order
   now preserves the existing exact legacy bytes under both feature sets; and
3. external-test integrity digests and one generator dependency inventory were stale after
   earlier merges and are now synchronized.

Boxology's one-accountable-package rule correctly rejected a combined platform-and-Telegram
repair. The repair was therefore split into #624 and #625. #624 merged on focused born-valid
and exact integrity evidence to break the temporary validation cycle; #625 then passed the
complete local and required GitHub gates. The ownership rule was not weakened.

## Explicit limitations

- [Issue #248](https://github.com/fontanierh/boxology/issues/248) continues to own live bot
  credentials and operational authorization. This closeout does not enable, pair, poll,
  listen, send, or otherwise contact live Telegram.
- Attachments, formatting, generic outbound backends, rich multi-user identity, batch
  acknowledgement, and a native streaming capability remain deferred because this consumer
  does not require them.
- Self-hosting applies to the Telegram product entrypoints, not to every supporting Rust crate.
  Runtime, contract, binding, fixture, generated-contract, and repository-operations packages
  retain their documented non-box roles.

With this record merged, [issue #573](https://github.com/fontanierh/boxology/issues/573) has no
remaining product or evidence work and may close. The broader post-V0 epic remains open for the
Boxology tool-entrypoint and minimum harness milestones.
