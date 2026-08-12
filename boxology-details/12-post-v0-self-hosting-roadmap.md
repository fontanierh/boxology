# Post-V0 self-hosting roadmap

## Purpose and operating rule

This is the living execution map after V0. It turns the self-hosting ladder into
small product proofs: Telegram first, Boxology's useful tool entrypoints second,
and a minimum coding-agent harness third. The tracking epic is
[#572](https://github.com/fontanierh/boxology/issues/572).

Be pragmatic: take the shortest honest path to a working consumer, prefer small
reversible changes, and distinguish current support from follow-up work. Add a
platform feature only when the next real box needs it. Do not box internal crates
for symmetry, clone another harness speculatively, or turn evidence into ceremony.
No shortcut may fabricate support or hide a trade-off.

## What “self-hosted” means

A product use-case entrypoint is self-hosted only when all of these are true:

1. governed `boxology.toml` manifests declare the box and its consuming
   composition in their appropriate packages;
2. Boxology generates the contract, implementation-local adapter, and typed handle;
3. the composition consumes that generated handle rather than calling the service
   implementation directly;
4. cold regeneration from a clean copy is byte-stable; and
5. `boxology check` is green for the governed workspace.

Passing a request through a handwritten JSON CLI is useful application behavior,
but is not this proof. Conversely, self-hosting applies to use-case entrypoints,
not every Cargo crate. Internal crates retain the appropriate governed non-box
role whenever boxing them would add indirection without testing a product claim.

## Kernel and substrate that stay outside the target

The following are not migration targets merely to increase a box count:

- `boxology-contract` and `boxology-runtime`, which supply the contract and call
  substrate from which generated boxes are built;
- `boxology`, `boxology-macros`, and `boxology-contract-syntax`, which expose or
  construct that substrate;
- `boxology-http`, the binding infrastructure, and
  `boxology-http-conformance`, its conformance evidence;
- fixture, generated-contract, and test-only packages whose purpose is evidence;
  and
- `xtask`, CI workflows, and `ops/`, which operate this repository rather than
  expose a reusable product use case.

These packages retain their appropriate governed platform, `box-contract`, or
fixture ownership and must pass repository checks. Exclusion from boxification is
not exclusion from ownership or quality.

## Proven baseline

V0 completed on 2026-08-09. The immutable
[V0 completion record](../records/2026-08-09-v0-completion-evidence.md) preserves
its candidate and validation chain. The post-V0 absorption in
[#342](https://github.com/fontanierh/boxology/issues/342) then merged through
[PR #571](https://github.com/fontanierh/boxology/pull/571): `cargo xtask ci` owns
one complete product check, while the required PR path remains deliberately lean.
That merged state, not the record's then-future residual list, is current truth.

The repository is already governed by Boxology manifests and `boxology check`.
`crates/boxology-telegram` is a tested service whose installed CLI is now owned by
the governed `boxology-cli` composition. T0
dogfood landed through [PR #583](https://github.com/fontanierh/boxology/pull/583):
it now has a generated scalar contract, implementation-local adapter, and real
assembled handle/runtime proof. The installed CLI is now owned by `boxology-cli`,
assembles every Telegram exposure, and drives substantive behavior only through
the generated handle. Its typed, inherently idempotent `send` and
structured `ask` capabilities cover the first substantive command slice; typed
`reply` and `resolve_send` preserve its outbound recovery lifecycle, and typed
pairing, polling, acknowledgement, local/probed status, and listener startup now
cross generated handles. The listener-start handle retains the exclusive consumer
lease across same-service polling. The backend-neutral one-shot projection and
composition-owned listener use generated request/outcome types without direct
implementation state or network access.
The immutable
[Telegram self-hosting closeout record](../records/2026-08-11-telegram-self-hosting-closeout.md)
pins the merged command map, cold-generation evidence, complete check, and operational
limitations. Telegram product self-hosting is complete; live Telegram authorization remains
separate under #248.

## Dependency order and milestones

| Milestone | Scope | Acceptance boundary |
| --- | --- | --- |
| T0 — scalar Telegram dogfood | [#573](https://github.com/fontanierh/boxology/issues/573): one code-only, non-idempotent `send_text(String) -> Result<i64, SendTextError>` capability over the existing service and fake API | An assembled in-process runtime/test seam connects a real generated handle to the existing implementation; the existing CLI is unchanged; fake-API success and error paths pass; cold generation and `boxology check` pass. This is partial dogfood, not full self-hosting |
| T1 — Telegram-forced structured subset | [#574](https://github.com/fontanierh/boxology/issues/574): structs, unit enums, acyclic local references, `Option`, and `Vec`, with their required generated named-field forms | Positive nested/container fixtures and fail-closed unsupported-form fixtures pass; cold output is byte-stable; the subset is sufficient for Telegram command payloads |
| T1a — typed send and ask | Continue [#573](https://github.com/fontanierh/boxology/issues/573) with inherently idempotent `send` and structured `ask` over the existing production seams | Generated handles preserve send replay without a second write, ask alternatives and durable lifecycle state, and structured disabled-state failures; the scalar handle and JSON CLI remain unchanged |
| T1b — typed reply and send resolution | Continue [#573](https://github.com/fontanierh/boxology/issues/573) across the shared outbound ambiguity/recovery state machine | Generated handles preserve reply correlation and replay, safe failures, non-retrying ambiguity, both explicit resolution paths, and independent disabled gates; the JSON CLI remains unchanged |
| T2a — typed pairing lifecycle | Continue [#573](https://github.com/fontanierh/boxology/issues/573) with pairing begin, complete, and revoke over the existing production seams | Generated handles preserve private matching, durable offset and pending state, ambiguity, exclusive consumer ownership, disabled gates, and local sensitive-state revocation; the JSON CLI remains unchanged |
| T2 — governed Telegram composition | Deliver the composition slice for [#573](https://github.com/fontanierh/boxology/issues/573) with governed CLI/listen assembly over the delivered typed operations, listener lease, and pure JSON projection | The implementation is a box, the CLI is a binding/composition, and substantive operations cross generated handles; `listen` orchestrates typed startup, polling, and local status; parity and deterministic fake evidence pass |
| T3 — useful Boxology tools | [#575](https://github.com/fontanierh/boxology/issues/575): classifier, `check`, generator, and installer use-case entrypoints | Each selected entrypoint has a real typed contract and composition consumer; #575 records the checked-in generator bootstrap boundary without claiming prior-release regeneration. The first pinned release later supplies that proof |
| H0 — minimum Pi-like harness | [#576](https://github.com/fontanierh/boxology/issues/576): `model-completion` application box, tool runner, session store, agent loop, and stdio JSON/RPC composition | A deterministic fake-model turn, a small live-model task in an isolated worktree, resume plus compaction, and generated-handle traversal all pass |
| H1 — Prime-like durability | Only capabilities demanded by an operating consumer | Each accepted capability is an application box or composition with its own recovery evidence; there is no blanket platform expansion |

Do not start by cloning Pi or Prime. T0 landed as one small PR. For T1, syntax/model,
schema writing/reading, raw reachability, and role-specific classifier mapping are
delivered. Generated structured types/codecs are also delivered under #574; descriptors
and complete checker/dispatch/handle/fake/adapter wiring are now delivered as well.
Typed send/ask/reply/resolve-send, pairing, poll, acknowledgement, local/probed
status, and listener startup consume that boundary. The backend-neutral one-shot CLI
projection and governed installed CLI/listen composition are delivered.
The E3 generated-handle proof unlocked tool self-hosting under #575, and the first
T3 slice now governs the classifier as a box:
`boxology generate` now classifies regenerated schemas through its generated typed
handle while `check`, generator, and installer remain ordinary code pending their
named slices. Classifier parse and pairing failures now cross that handle as typed
stage-tagged outcome data rather than an erased domain-error payload. Structured
foreign imports now hydrate the classifier's provider-owned request/outcome graph and
emit typed consumer methods without duplicating those types, so the governed `check`
box can consume this boundary losslessly in its next slice. The classifier's first checked-in derived tree is seeded by the exact
pre-box `314dcab` executable; current-version regeneration is required byte-stable,
but this is neither generator self-hosting nor prior-release reproduction evidence.
Every PR remains at or below 600 hand-authored lines.

## Telegram migration matrix

The current-support column describes the existing service and CLI behavior on this
baseline. Generated-handle progress is called out separately; “first dogfood
evidence” does not claim that command parity already exists.

The installed JSON binding maps generated request and outcome types behind the
backend-neutral seam. Its composition-owned listener orchestrates generated
`listen_start`, `poll`, and local `status` calls.

| Product/feature | Current support | Minimum missing | First dogfood evidence | Deferred |
| --- | --- | --- | --- | --- |
| Pairing: begin | Governed CLI and typed generated handle create a bounded pending private-pair request in durable local state | None for Telegram self-hosting | Generated-handle fake API/state lifecycle observes the digest, salt, expiry, and bot fingerprint but not the nonce | Rich authentication/backend policy |
| Pairing: complete | Governed CLI and typed generated handle poll for the matching private user/chat and confirm it | None for Telegram self-hosting | Generated-handle lifecycle rejects ineligible updates, advances the durable offset, persists the exact receipt, and preserves ambiguous confirmation | General identity or multi-user model |
| Pairing: revoke | Governed CLI and typed generated handle explicitly clear local pairing and sensitive collections | None for Telegram self-hosting | Generated handle revokes pairing, pending state, inbox, asks, and outbound state without a Telegram call while retaining bot/offset state | Remote credential revocation protocol |
| Send | Governed idempotent command sends text through the typed generated handle and tracks ambiguity | None for Telegram self-hosting | Inherent-idempotency request/outcome crosses a generated handle; replay returns the durable receipt without a second API write | Attachments, formatting, `Blob`, `Secret`, and generic outbound backends |
| Reply | Governed command resolves an event and replies through the typed generated handle | None for Telegram self-hosting | Generated-handle reply preserves event correlation, handling state, safe failures, and replay without a second write | Cross-backend reply abstraction |
| Poll | Governed CLI imports authorized updates through the typed generated handle | None for Telegram self-hosting | Fake updates cross the handle with stable ordering, offset/durability receipts, callback warnings, and restart replay | Streaming transport |
| Ack | Governed CLI marks one inbox event handled through the typed generated handle | None for Telegram self-hosting | Typed ack changes exactly the selected durable event and correlated ask state without a Telegram call | Batch ack |
| Ask | Governed CLI sends a recommendation and alternatives through the structured generated handle | None for Telegram self-hosting | Typed fake-API ask preserves alternatives, the delivery receipt, and durable lifecycle state | General interactive-form framework |
| Resolve-send | Governed CLI resolves an ambiguous outbound record through the typed generated handle | None for Telegram self-hosting | Typed delivered/not-delivered recovery updates only the selected record, while invalid tuples leave state byte-unchanged | Generic distributed transaction semantics |
| Local status | Governed CLI reports local state through the typed generated handle without a Telegram call | None for Telegram self-hosting | Disabled generated-handle fixture proves every public field, exact legacy JSON bytes, consumer-lock evidence, zero API calls, and byte-unchanged state | Observability platform |
| Probed status | Governed CLI calls Telegram probes through the typed generated handle only under explicit enablement | None for Telegram self-hosting | Fake API proves matching and mismatching bots, both webhook branches, disabled authorization before token/state/network work, and redacted retryable failure without state mutation | General health-check framework |
| Listen | Governed composition owns the bounded loop, lease, heartbeat, and event output over generated `listen_start`, `poll`, and local `status` calls | None for Telegram self-hosting | Deterministic generated-fake evidence preserves listener ordering, retries, deduplication, heartbeat, fatal stop, and redaction | Native streaming capability |

T0's code-only scalar seam remains first and stable, while T1a/T1b/T2a and the poll/ack slice add the current
CLI's idempotent `send`, structured `ask`, `reply`, and `resolve-send` semantics as
generated capabilities plus its private pairing and inbound lifecycle. T2 makes
the service implementation a governed box and the installed CLI a composition/binding;
every substantive CLI operation crosses a generated handle.

Live bot credentials and real pairing, polling, listening, or sending remain a
separate operational authorization under
[#248](https://github.com/fontanierh/boxology/issues/248). Product self-hosting
does not enable Telegram or grant permission to contact it.

## Crate and category disposition

| Crate/category | Disposition | First useful proof |
| --- | --- | --- |
| `boxology-telegram` | Self-hosted use-case entrypoints with the working implementation retained behind a governed binding | T0 scalar send; T1a typed send/ask; T1b typed reply/resolve-send; T2a typed pairing; typed poll/ack/status/listener lease; pure CLI projection; governed CLI/listen assembly |
| `boxology-classifier` | Box the classify use case, not every parsing helper | Typed old/new schema input to findings report under #575 |
| `boxology-cli` | Keep as a binding; route substantive self-hosted commands through generated handles | `check` and installer compositions under #575 |
| `boxology-generator-model`, `boxology-generator-writer`, `boxology-generator` | Box the generation entrypoint and keep model/writer internals ordinary; #575 records the current checked-in bootstrap boundary, while the first pinned release later proves prior-release regeneration | Typed generation plan/result under #575 |
| `boxology-init` | Treat as installer composition, not an independent box-for-symmetry target | Standalone composition consumes the generator handle, plus the check handle only if that real composition needs validation |
| `boxology-workspace`, `boxology-manifest`, `boxology-schema` | Keep parsing and schema substrate ordinary; expose only consumer-demanded workspace/check operations | `check(workspace, base) -> report` seam under #575 |
| `boxology-contract`, `boxology-runtime` | Irreducible contract/call substrate; do not box | Continuous governed-package validation |
| `boxology`, macros, contract syntax | Facade and construction substrate; do not box for counts | Existing generated-project and compiler evidence |
| HTTP binding and conformance | Binding infrastructure and evidence; leave ordinary unless a distinct HTTP application use case appears | Existing conformance suite |
| Generated contracts, conformance packages, fixtures | Evidence artifacts, not product boxes | Cold generation, golden comparison, and check |
| `xtask`, workflows, `ops/` | Repository operations, outside product self-hosting | Repository validation and process-safety evidence |

## Minimum Pi-like harness

The comparison target is Pi's official
[repository](https://github.com/earendil-works/pi), especially its
[agent core](https://github.com/earendil-works/pi/tree/main/packages/agent) and
[coding-agent package](https://github.com/earendil-works/pi/tree/main/packages/coding-agent).
These sources show the useful minimum: a stateful model/tool loop, coding tools,
sessions and compaction, and programmatic modes. They are a product reference,
not a compatibility contract.

The minimum Boxology architecture is five application boundaries:

```text
model-completion.complete
tool-runner.execute
session-store.load / session-store.append
agent-loop.run_turn
stdio JSON/RPC composition
```

`model-completion` is a normal application capability in a `kind = "box"`
package; Boxology has no provider package kind. `agent-loop.run_turn` consumes the
other generated handles. The first version can compile in one model implementation
and four tools (`read`, `write`, `edit`, and `bash`); it does not need runtime
plugin discovery. H0 now has the governed `model-completion.complete` contract,
configured xAI implementation, typed read/write/edit/bash tools, and a governed
linear session event store with durable append, replay, restart, and torn-tail recovery.
Bash may intentionally escape its root-confined initial cwd through
shell behavior, absolute paths, or a new process session.
The agent loop and deterministic persisted compaction are governed and exercised
through generated handles. Harness item 8 is complete: the four boxes assemble locally
and strict bounded JSONL exposes correlated `run_turn` and `compact` through only the
generated agent-loop handle, with process-lifetime IDs, request limits, and SIGINT
cancellation/stop behavior. Authorized live dogfood remains; no sandbox is claimed.

| Product/feature | Current support | Minimum missing | First dogfood evidence | Deferred |
| --- | --- | --- | --- | --- |
| Model completion/agent loop | Governed loop plus complete four-box local harness with correlated `run_turn` and `compact` | Static checked-in context loading before live dogfood (8c) | Generated fakes drive exact sequential calls; production binary assembly is exercised | Multiple calls, provider marketplace, multimodal or streaming protocol |
| Tool execution | Governed `tool-runner.execute` with root-confined read/write/edit and bounded unsandboxed bash starting in the selected root-confined cwd; explicit environment/process cleanup | Nothing for fixed H0 tools | Fake-model turn edits an isolated fixture only through the generated tool handle | Dynamic tool plugins and a general permission framework |
| Sessions/resume | Governed linear JSONL events through `session-store.load/append`, integrated with the agent loop for replay, restart, and torn-tail recovery | Nothing for H0 | Stop and restart, load the same session, and complete the next turn deterministically | Session trees, list/delete, and distributed service |
| Compaction | Governed `agent-loop.compact` persists a bounded caller-supplied summary and reconstructs from the latest valid checkpoint | Nothing for H0 | Fresh compositions continue after two separated checkpoints without pre-checkpoint context or old tool re-execution | Automatic summary generation and multiple compaction strategies |
| Interactive/print/protocol modes | Strict bounded sequential LF protocol with duplicate/request limits, deadlines, and active/idle SIGINT control | Nothing for H0 | Generated-fake result/failure, framing, lifecycle, cancellation, output, and CLI boundaries are locked | JSON-RPC 2.0, batching, notifications, network/daemon service, streaming, and UI |
| Skills/prompt templates | Checked-in Agent Skills and repository instructions already exist | H0 statically loads selected checked-in skills and prompts | Deterministic turn includes the exact selected skill/prompt content in model context | Executable packages, hot-loading, and a marketplace |
| Extensions/packages/themes | No dynamic harness extension ecosystem | Nothing for H0; statically compose required boxes | One checked-in composition selects its model implementation and tool set without dynamic loading | Package registry, themes, and hot-loading |
| Security/sandbox | Boxology is not a sandbox and has no generic permission engine | Isolated worktree and explicit process/environment boundaries in acceptance | Live-model task changes only its assigned worktree and records commands/results | Built-in sandbox or permission framework |

H0 acceptance is one deterministic fake-model loop, one authorized live-model
small real task in an isolated worktree, session resume plus compaction, and proof
that every application boundary uses generated handles. No result may claim that
Boxology or the harness supplies a built-in sandbox.

## Prime-like durability, only when demanded

Prime Agent's official
[repository and documentation](https://github.com/PrimeIntellect-ai/prime-agent)
show a broader operating agent: persistent Python execution, recursive child
agents, continual harness state, daemons, messaging, and scheduled autonomy.
These are candidate application boxes and compositions after H0, not features to
push automatically into the Boxology kernel.

| Product/feature | Current support | Minimum missing | First dogfood evidence | Deferred |
| --- | --- | --- | --- | --- |
| Persistent Python kernel | None | A stateful Python-session application box only if a consumer needs it | Restart/reattach preserves one explicit kernel state fixture | General notebook platform and arbitrary kernel types |
| Recursive child agents / RLM | No agent runtime | Child-run composition using model, tool, and session handles, with bounded depth/budget | Parent delegates one deterministic subproblem and records the returned evidence | Unbounded recursion or automatic swarm policy |
| Versioned continual-harness state, refinement, rollback | No harness memory/refinement product | Versioned application state and explicit accept/rollback operations | Failed refinement rolls back to the last accepted version byte-for-byte | Self-modifying platform policy |
| Daemon and reattach | No harness daemon | Recoverable supervisor/session composition | Kill and restart a fixture daemon, then reattach without duplicating the turn | General distributed scheduler |
| Direct agent messaging | No application messaging fabric | Addressed mailbox box when two real agents require it | Two fixture sessions exchange one deduplicated message | Global event bus |
| Persistent goals | No goal application | Versioned goal state attached to a session | Resume preserves active goal and completion evidence | Portfolio planning engine |
| Heartbeats | No agent heartbeat application | Lease/heartbeat composition for a real long-lived consumer | Stale fixture lease is detected and safely reclaimed | Universal liveness substrate |
| Schedules | No agent scheduler application | Durable trigger box with explicit ownership and replay rules | Restart fires one due fixture once | Cron replacement or broad automation platform |
| Bounded autonomy | No autonomous harness | Composition-level limits for turns, time, cost, and owned resources | Fixture stops at each bound and preserves resumable state | Open-ended autonomy |
| Security | No sandbox or generic permissions | Per-application authority, secret, filesystem, and process boundaries | Negative tests deny an out-of-scope fixture operation | Claiming Prime or Boxology is a security sandbox |

[#57](https://github.com/fontanierh/boxology/issues/57) remains the coordination
home for distributed needs. A Prime-like feature should move from this table into
an executable issue only when a named consumer and recovery test make it necessary.

## Platform features versus application boxes

The Telegram-forced platform-language subset in
[#574](https://github.com/fontanierh/boxology/issues/574):

- structs with named fields;
- unit enums;
- acyclic local type references;
- `Option<T>`; and
- `Vec<T>`.

That delivered slice extends parsing/modeling, schema, generation, and evidence only
as far as Telegram needs. It is coordinated with the broader grammar and emission homes
[#102](https://github.com/fontanierh/boxology/issues/102) and
[#104](https://github.com/fontanierh/boxology/issues/104); it does not silently
close their remaining breadth. The resulting fixture breadth stays coordinated
with [#100](https://github.com/fontanierh/boxology/issues/100).

Explicitly deferred until a real consumer forces them are maps, `Field`, `Secret`,
`Blob`, recursive types, forward references, named error payloads, capability
wire-name overrides, streaming, dynamic plugins or model backends, a generic
permission framework, and a sandbox. Model-completion boxes, tools, sessions,
compaction, Python kernels, subagents, goals, heartbeats, schedules, and autonomy
are application boxes or compositions, not reasons to enlarge the kernel by default.

## Dogfood and evidence protocol

Every bounded delivery slice leaves exact replayable evidence in its PR and linked
issue:

1. deterministic fake-model or fake-API tests cover success and meaningful
   failure/ambiguity paths;
2. tests consume real generated handles, not handwritten substitutes with the
   same shape; from T2 onward the governed composition consumes those handles;
3. deterministic generation records the exact classification and a repeated run
   is byte-unchanged when that slice changes generated surfaces; and
4. the epic, task, related legacy issues, PR reviews, and current `main` are
   reconciled before merge.

Full dogfood-milestone or issue closeout additionally requires cold generation
from a clean copy, `boxology check` on the governed workspace, and a dated
append-only record of the exact commit, commands, outcomes, and limitations.
Record mechanical or semantic friction in
[`ops/friction-log.md`](../ops/friction-log.md) when the work actually encounters
friction or relaxes a discipline. A delivered bounded slice such as T1a is real
progress, but it does not by itself claim full Telegram self-hosting or closeout.

A live-model or live-Telegram check complements deterministic evidence; it
never replaces it. Secrets stay outside the repository, and external contact
still requires its own authorization.

## Tracker and dependency map

| Issue | Role and dependency |
| --- | --- |
| [#572](https://github.com/fontanierh/boxology/issues/572) | Epic and current-roadmap owner; closes only after its accepted child scope is completed or transferred explicitly |
| [#573](https://github.com/fontanierh/boxology/issues/573) | Delivered Telegram product self-hosting: scalar and structured capabilities, governed CLI/listen composition, cold generation, complete check, and the dated closeout record are merged |
| [#574](https://github.com/fontanierh/boxology/issues/574) | Delivered minimum structured Telegram boundary; bounded slices from #102/#104 with fixture coordination under #100 |
| [#575](https://github.com/fontanierh/boxology/issues/575) | Classifier/check/generator/installer use-case entrypoints; classifier and its structured-import prerequisites are delivered, typed `check` is the current critical path, and generator/installer follow by concrete dependency; advances #74 |
| [#576](https://github.com/fontanierh/boxology/issues/576) | Minimum Pi-like harness; follows generated-handle dogfood and models completion as an application box rather than inventing a provider package kind or new kernel feature |
| [#74](https://github.com/fontanierh/boxology/issues/74) | Existing stage-3 tool-self-hosting forcing function; #575 is its executable rung, not a duplicate closure claim |
| [#100](https://github.com/fontanierh/boxology/issues/100) | Broader fixture/type-vocabulary evidence; #574 adds only the Telegram-forced cases |
| [#102](https://github.com/fontanierh/boxology/issues/102) | Broader parser/model grammar; #574 is the accepted minimum local subset |
| [#104](https://github.com/fontanierh/boxology/issues/104) | Broader named-field/type emission; #574 takes only the subset required for Telegram |
| [#57](https://github.com/fontanierh/boxology/issues/57) | Distributed coordination; relevant to Prime-like messaging/agents only after a real consumer exists |
| [#248](https://github.com/fontanierh/boxology/issues/248) | Separately authorized live Telegram credentials and operations; never implied by product self-hosting |

## Anti-goals and explicit deferrals

- Do not box the runtime, contract machinery, facade, syntax, bindings, evidence
  packages, or repository operations merely for symmetry.
- Do not rewrite the working Telegram service merely for boxification; extend it
  through existing production seams and replace its CLI only with a governed composition.
- Do not conflate generated Telegram code with authorization to use live Telegram.
- Do not clone Pi's extension ecosystem or Prime's durable-agent surface before
  the minimum Telegram and harness consumers expose a concrete need.
- Do not add a platform matrix, factory policy, permission system, sandbox,
  streaming protocol, or dynamic model/plugin system to satisfy a roadmap box.
- Do not make calendar promises. Sequence by dependency, keep each PR reviewable,
  and update this document when evidence changes the shortest honest path.
