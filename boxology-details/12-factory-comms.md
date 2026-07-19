# Factory Comms

[Back to the white paper](../boxology-whitepaper.md)

This document proposes the coordination layer for the [software factory](05-software-factory.md): how independent agents broadcast what they are doing and message one another using nothing but a GitHub repository. It is a design direction, not a v0 commitment.

**Scope note:** like the factory itself, this layer is not part of the Boxology platform. It is factory-application design. It answers one of the questions [issue #57](https://github.com/fontanierh/boxology/issues/57) leaves open — which coordination state can live visibly in GitHub rather than in a factory-owned store — for status and messaging only. Task claims, leases, fencing, and delivery guarantees stronger than those stated here remain deferred to that issue.

## Purpose

The factory does not run its agents. An agent is an independent coding-agent session — any harness, any machine, any operator — that plugs into the system. Such agents may sit behind NAT, inside ephemeral containers, or on laptops; nothing can push to them over the network, and no factory-owned service can supervise them. The only endpoint every participant is guaranteed to reach, outbound, is the GitHub repository itself.

Factory Comms therefore uses the repository as the entire coordination substrate. It provides three things:

1. A **status board**: every agent continuously publishes what it is currently doing.
2. **Messaging**: any agent can leave a durable message for another and reply to one it received.
3. A **doorbell**: an optional, agent-supplied way to be pinged when a message arrives, so that agents are not limited to polling.

Joining the factory is one `git push`. There is no registration service, no broker, no queue, and no factory-side configuration per agent.

## Design constraints

- **Pull-based correctness.** Any process that can `git fetch` is a fully correct participant. Notification only improves latency; an agent that ignores it entirely is slower, never wrong.
- **Harness-neutral.** The layer ships as a portable Agent Skills-format skill plus a tool. It must not depend on any harness-specific wake, persistence, or messaging feature.
- **Human-inspectable.** All state is readable on github.com with no tooling, in keeping with the oversight thesis: humans supervising a factory can audit its coordination the same way they audit its code.
- **No secrets in the datastore.** Anything requiring a credential stays on the agent operator's side of the GitHub boundary.
- **Cooperative, attributed, auditable.** Git cannot enforce per-path ownership inside one repository. Ownership rules below are conventions backed by attributed commit history, which makes violations detectable rather than impossible — the same posture as foundation-profile box isolation.

## The datastore: one ref per agent

Each agent owns one ref containing a small tree:

```text
factory/agents/<agent-id>
├── status.json        owner-writes-only: current activity, heartbeat, doorbell
├── inbox/
│   └── <ulid>.json    pending messages; any agent may add
└── archive/
    └── <ulid>.json    acknowledged messages, moved here by the owner
```

Git refs are the load-bearing choice because a ref update is a compare-and-swap: a non-forced push succeeds only if the remote tip is unchanged, which is the entire concurrency-control story. Every write is an attributed, timestamped commit, so the full history of status changes and message traffic is an audit log obtained as a side effect. One `git fetch` of the namespace retrieves the whole fleet's state in a single round trip, over Git transport rather than the rate-limited REST API.

The namespace is deliberately `refs/heads/factory/agents/*` — real branches — rather than a custom ref namespace, for three reasons: branches are browsable on github.com (any human can open an agent's inbox), the Contents API works only on branches, and GitHub Actions push triggers fire only for branches and tags. A push to a custom ref can trigger nothing, which would forfeit the doorbell mechanism below. Migrating to a custom namespace behind a dashboard is a possible later step once branch-list pollution outweighs raw inspectability.

### The status file

```json
{
  "protocol": 0,
  "agent": "worker-3",
  "state": "working",
  "task": "https://github.com/…/issues/123",
  "detail": "reworking submission after merger returned it",
  "updated_at": "2026-07-19T14:03:00Z",
  "heartbeat_at": "2026-07-19T14:03:00Z",
  "doorbell": { "kind": "repository_dispatch", "event": "factory-msg-worker-3" }
}
```

`state` is a small enum (`idle`, `working`, `blocked`, `offline`); `detail` is free text. The board view is one fetch plus one render of every agent's status file.

Because no supervisor observes agents, `heartbeat_at` is the only liveness signal that can exist. Agents refresh it at a declared cadence; the board flags staleness, and a sender messaging a silent agent can see that before escalating to a human. Heartbeats are also the raw material any future claim-expiry or fencing design in issue #57 will need.

## Write protocol

All writes go through the tool, which guarantees:

- **CAS only, never force.** Fetch the tip, build the new tree, commit, push fast-forward. On rejection: refetch, reapply, retry with capped backoff. Because concurrent writers always touch distinct files — status is owner-only, message filenames are unique — retries converge mechanically.
- **Sender-chosen ULID message ids, create-only.** Resending is idempotent, collisions are impossible, and lexicographic order approximates arrival order.
- **Schema validation before commit.** Malformed status or message payloads cannot enter the datastore. Every payload carries the `protocol` version; the tool refuses on a major mismatch, since independent agents arrive with whatever tool version they last bootstrapped.
- **No checkout.** Writes use Git plumbing (or the Git Data API), never the agent's working tree, so coordination traffic cannot collide with checked-out work.

## Messages, acks, and replies

```json
{
  "protocol": 0,
  "id": "01JD4XQ6…",
  "thread": "01JD4XQ6…",
  "reply_to": null,
  "from": "merger",
  "type": "rework-request",
  "body": "…",
  "sent_at": "2026-07-19T14:04:11Z"
}
```

`type` is a free-form convention, not an enum. `thread` is the root message's id (self-referential on the first message), so a conversation is reconstructible across both agents' refs and history even after pruning.

The lifecycle: a message lands in the recipient's `inbox/`; acknowledging it moves the file to `archive/`. "Pending" is exactly the contents of `inbox/`. The sender confirms delivery by observing the recipient's ref — the message left the inbox, therefore it was consumed. A janitor command prunes old archive entries; Git history retains everything regardless.

Replying is the hot path — woken, read, reply — so it is one operation: `reply <msg-id>` resolves the sender from the original, writes to *their* inbox with `reply_to` and `thread` set, and acknowledges the original in the same step (opt out with `--no-ack`). Reply-implies-ack keeps inboxes at zero without separate bookkeeping.

## Notification: the doorbell

Delivery is durable the moment the message commit lands; the doorbell is a latency optimization layered on top. Each agent *declares* in its status file how it wants to be pinged, if at all. After committing a message, the sender's tool reads the recipient's declaration and rings it best-effort.

A ping carries **no payload** — it means only "check your inbox" — so a lost, duplicated, or ignored ping is harmless; polling remains the backstop. Doorbell kinds name GitHub-native events that any sender can already fire with the repository credentials it holds:

- `poll` (default): no ping; the declaration states the polling cadence so senders know the expected latency.
- `repository_dispatch`: the sender fires the declared event type via the API.
- The branch push itself: an Actions workflow on `push` to `factory/agents/<id>` needs no declaration at all.

What *consumes* the event is entirely the operator's business, on their side of the boundary, and this is where each harness's actual capabilities slot in:

- **Live session** — the tool's `watch` command long-polls the namespace and emits one line per new message; a harness that can stream a background command into the conversation (Claude Code's Monitor tool) turns that into true push with zero infrastructure. A harness that cannot simply polls between turns.
- **Dormant session, agent machine reachable only outbound** — a **self-hosted Actions runner on the agent's own machine**: the message push triggers a workflow that runs on the agent's hardware and executes the harness's resume command (`claude --resume <id> …`, `codex exec resume --last …`). Self-hosted runners connect outbound-only, so GitHub's runner is the wake daemon and the operator writes no watcher code.
- **Cloud sessions** — a scheduled or event-fired remote session (for example a Claude Code Routine on an API or GitHub trigger). These start fresh sessions rather than resuming — which the design absorbs, because all coordination state lives in the ref: a fresh session that reads its own status file and inbox has reconstructed what it needs.

## The `factory-comms` skill

The layer ships as its own portable Agent Skills-format skill in `.agents/skills/`, referenced from the Boxology skill delivered by [stream S7](11-v0-streams.md) — with a strictly one-way dependency. The comms skill knows nothing about boxes or contracts; any skill-capable agent can adopt it alone, and it is testable in isolation. The Boxology skill adds only: when working in a factory, load `factory-comms` and join.

The split inside the skill: **discipline in the text, mechanics in the binary.**

The SKILL.md covers only behavior no tool can enforce — join on start; update status at meaningful transitions; on wake, read → act → reply → ack; refresh the heartbeat; never write another agent's status. Per-harness doorbell recipes live in referenced sub-files so the core stays harness-neutral. Everything mechanical belongs to the tool, whose `--help` carries the details, and whose `inbox` output prints each pending message with its ready-to-run reply command — for coding agents, the tool output is the affordance, so the skill text never teaches command syntax.

The skill ships a bootstrap script that resolves the binary: found on `PATH` → download the platform binary from a release pinned by version and checksum in the skill → fall back to `cargo install --locked` from source. Consistent with the [recorded v0 exclusions](11-v0-streams.md#recorded-v0-exclusions), release-based distribution is post-v0; until then the source-checkout path is the only one exercised.

## The tool

A single small CLI (working name `factory`), with the command surface:

```text
factory join                        create the agent ref; declare identity and doorbell
factory status set <state> [...]    update status.json; refreshes heartbeat
factory board                       fetch the namespace; render every agent's status
factory send <agent> [...]          commit message to recipient inbox; ring doorbell
factory inbox [--pending]           list messages, each with its ready-to-run reply
factory reply <msg-id> [...]        reply to sender; acks the original (--no-ack)
factory ack <msg-id>...             move messages to archive without replying
factory thread <msg-id>             reconstruct a conversation from history
factory watch [--exec <cmd>]        long-poll; emit per-message lines or run a hook
factory prune                       janitor: trim old archive entries
```

Per the tool-boxification rung of [issue #74](https://github.com/fontanierh/boxology/issues/74), the tool is built conventionally first and boxified later. It is shaped for that future decomposition from the start: a ref-store box (CAS Git plumbing), a coordination box (status, inbox, and ack lifecycle), and a doorbell capability contract with one provider per kind — a natural seam, since doorbell kinds are precisely interchangeable providers behind one interface. Once the S1 runtime and S2 generator exist, this tool is a candidate first consumer.

## Limits and non-goals

- **Latency** is bounded by the doorbell or, without one, the polling cadence. This suits status updates and task handoffs; it is not a transport for chatty mid-task RPC, which the factory's worker model already excludes.
- **Ownership is not enforced**, only attributed and auditable, as stated in the constraints.
- **Delivery is at-least-once** for the recipient (polling guarantees eventual delivery; pings may be lost or duplicated). There is no exactly-once processing promise; message handling should be idempotent.
- **This is not the task ledger.** Claims, leases, fencing, split-brain prevention, and coordinator redundancy remain the province of issue #57. One observation carries over: creating a ref is atomic, so a create-only push of `factory/claims/<task-id>` is a genuine compare-and-swap available with no additional infrastructure, should that design want it.
- **One repository per factory** is assumed; cross-repository factories are out of scope here.

## Matters not yet specified

- The formal JSON schemas, their versioning policy, and the compatibility rules between protocol revisions.
- Message size limits and whether attachments are permitted (as blobs in the sender's ref) or excluded.
- The archive retention and pruning policy, and who runs the janitor.
- Whether the board should aggregate into a rendered dashboard, and where it would live.
- Abuse handling if a factory ever spans mutually untrusting operators — the current design assumes the cooperative trust of a single factory.
- Naming: whether the agent-id space is flat, role-prefixed, or derived from the factory's governance configuration.
