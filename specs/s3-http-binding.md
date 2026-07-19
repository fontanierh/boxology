# S3 Spec — HTTP Binding

[Stream definition](../boxology-details/11-v0-streams.md#s3--http-binding) · Status: **revised, awaiting re-review** (first review addressed; cross-stream contract in issue #85)

S3 implements the v1 HTTP transport against S1's assembly API. The wire contract — routing, JSON mapping, envelopes, status table, headers — is normative in the [Foundation HTTP binding section of Runtime](../boxology-details/03-runtime.md); this spec does not restate it. It records implementation decisions, resolves the details the normative text delegates, and defines the conformance suite that makes every choice observable.

## Purpose

The HTTP binding proves "defined once, invoked through Rust and HTTP" is a platform property. With S1's descriptor-guided, role-aware decoding, S3's job is a faithful syntax layer over `ContractValue`, the context/header envelope, an explicitly specified request/task lifecycle, and the suite that pins everything.

## Scope decisions

- **Server and client both ship.** Confirmed (the first review asked for the decision before tasks): the normative text specifies client-side `CallError` synthesis, the suite needs a typed driver, and the client is the executable proof that moving a capability behind HTTP changes no consumer types.
- **HTTP/1.1 only, stated and tested.** The first draft claimed HTTP/2 "as negotiated" while testing only 1.1 — an untested support claim, withdrawn. The server serves HTTP/1.1 (`http1`-only stack configuration); the client speaks HTTP/1.1. HTTP/2 (including any h2c posture) is post-v0 and arrives only with conformance coverage.
- **TLS out**: plaintext for local development and behind-proxy deployment; exactly a support statement, no Internet-facing claim implied.
- No auth (anonymous context constructed in-binding; identity headers untrusted), no streaming, no compression (`Content-Encoding` present → `415`), no CORS, no metrics export.

## Decisions

### D1 — Stack and features

`boxology-http`, one crate, **feature-isolated**: `server` (axum/hyper, default-on), `client` (reqwest, off by default), shared codec always. Exact versions and feature sets pinned at T1 and recorded there. Both sides use a deliberately thin slice: one route, raw body/headers, no middleware stack.

### D2 — Codec: descriptor-guided, role-aware, over a lossless syntax layer

Per issue #85 item 1, the codec does **not** parse JSON "directly into `ContractValue`". Layering:

1. **Syntax layer:** bytes → a lossless JSON syntax tree that *preserves duplicate keys and key order* (parsed with a depth guard, default 128, and the byte cap enforced before/while reading — not via `serde_json::Value`, which collapses duplicates).
2. **Semantic layer:** syntax tree + `TypeDescriptor` + `DecodeRole` → `ContractValue`, applying S1's role rules (strict `ProviderInput`, tolerant `ConsumerOutput`), resolving descriptor-directed representation (`"42"` as `u64` where the descriptor says integer-64, `{"base64":…}` as `Blob` where it says blob, enum envelopes where it says enum), and rejecting duplicate keys, non-canonical integer strings (`"007"`, signs on `u64`, whitespace), fractional/exponent integer syntax, and invalid UTF-8.

Encode is the reverse: `ContractValue` → **canonical bytes** (D3). Non-finite floats are unrepresentable in `ContractValue` (S1) — recorded so the wire never re-litigates it.

### D3 — Canonical response encoding (byte-assertable)

The first draft demanded byte assertions without defining bytes. The canonical encoder is fully specified: UTF-8; no insignificant whitespace; struct keys in descriptor field order, envelope keys in the exact order given by the normative envelope examples; map keys sorted bytewise; minimal string escaping (only JSON-mandatory escapes, no gratuitous `\uXXXX`); floats via shortest-round-trip (Ryu); integers per the canonical rules; standard padded Base64; no trailing newline. **Byte-identity claims apply to canonical encoder output only** — accepted non-canonical *request* bytes (extra whitespace) need not round-trip byte-for-byte, and the suite states each assertion's domain.

### D4 — Routing and identifier canonicality

Exact match on `POST /rpc/{box_id}/{capability_local_name}` (the qualified `box.capability` id is a schema/manifest spelling; the route uses the two components — the S2 identity decision). The first draft's decode-then-match created aliases (`h%65llo` ≡ `hello`); corrected: **no percent-escapes are accepted** — id grammars contain only unreserved characters, so any `%` in a segment is a non-identifier and yields `404`. No trailing-slash or case tolerance. A present query string is `400 invalid_request` (strict-input posture). Raw-path tests, not framework-extracted parameters. Unknown box vs. unknown capability: both `404`, distinguished by the now-**named** stable codes below.

### D5 — Stable wire error codes

Promised-but-unnamed codes are named; these are wire contract, in the call-error envelope's `code` field, and enter the conformance traceability table: `unknown_box`, `unknown_capability`, `invalid_request` (malformed syntax, contract violation, bad header grammar, query present, empty body, trailing bytes), `deadline_exceeded`, `unavailable`, `invalid_upstream_response`, `internal`. Status mapping stays the normative table's. Merge notes carry these codes into `03-runtime.md` as the normative enumeration.

### D6 — Header grammars, exactly

- `Boxology-Timeout-Ms`: grammar `0|[1-9][0-9]{0,9}` (≤ ~11.5 days), single occurrence. Duplicate header, sign, whitespace, non-digit, or overflow → `400 invalid_request` — a malformed deadline must not silently become no-deadline. Absent → composition default.
- `Idempotency-Key`: 1–256 bytes of visible ASCII, single occurrence; violations → `400`. Carried into `CallContext`; **transported, never honored** — the suite asserts a repeated key does not dedup.
- `traceparent`: W3C grammar; invalid or duplicated → **ignored**, fresh trace (observability must not break calls — the deliberate asymmetry with the timeout header, stated). `tracestate` without a valid parent → ignored.
- Client encoding of remaining budget: milliseconds, rounded **up** (a positive remaining budget never encodes as `0`).
- Request `Content-Type`: `application/json` with optional `charset=utf-8` parameter (case-insensitive), single occurrence; anything else → `415`; duplicates → `400`. Responses: `Content-Type: application/json` only.
- Method/media table completed: non-POST on a valid path → `405` with `Allow: POST` (OPTIONS included — no CORS in v0); empty body → `400`; trailing bytes after the JSON document → `400`; maximum header block size configured explicitly (default 16 KiB) — a header-read timeout alone is not a resource bound.

### D7 — Request lifecycle, deadline coverage, and task ownership

Resolving issue #85 item 8 and the S1 contradiction:

- **The request budget starts at head receipt** and covers body ingestion, decode, and dispatch. A trickled body exhausts the same deadline as a slow handler; pre-dispatch expiry → `504 deadline_exceeded` with **zero invocations**. (The first draft's AC — expired budget *and* handler-observed zero — was self-contradictory with S1's short-circuit; split per the review: expired-before-dispatch asserts zero invocations; a separate small-positive-budget case asserts the handler observes a shrinking budget and then expiry.)
- **Dispatch runs as an owned, spawned task** registered in a composition-held tracker; the request future awaits its join handle. This is the explicit mechanism (not incidental stack behavior) behind three guarantees: *disconnect* — a drop-guard in the request future signals the request's child `CancelToken` when hyper drops it, while the spawned task keeps running (advisory cancellation; work may complete; its result is discarded with no server-side error); *timeout* — the join is raced against the deadline; on expiry the response is `504`, the token is signalled, and the task remains tracked; *panic* — a `JoinError` maps to `500 internal`, never a reset. Completion-vs-timeout races resolve by first-to-complete at the race point, stated as such.
- **Graceful shutdown**: stop accepting; await tracked tasks up to the drain timeout; then signal all tokens, grace-wait, abort. The suite itself uses clean start/stop per case.

### D8 — Client binding

Feature `client`: a remote `ImportTarget`/handle binding with base URL and the same codec. Sets context headers per D6; performs **no retries** (retry policy belongs to callers under declared idempotency; v0 declares none). **Resource limits**: response byte cap (default 8 MiB) and the codec depth guard apply before decode; bounded error-body reads. **Response classification table** (the first draft's "fails decode" was not a specification): every `(status, envelope, content-type)` combination is classified — `200`+`result` and `422`+`domain` decode strictly *at the envelope*, with the accepted tolerant rules applying inside result/domain payloads via `ConsumerOutput` role; call-error statuses require the matching `call` envelope; any other combination — wrong/missing content type, mismatched envelope, unknown call-error `code`, extra top-level fields, 1xx/204/3xx (redirects are not followed), truncated bodies — is `InvalidResponse` with the observed detail retained. Connect/DNS/refused → `Unavailable`; local deadline/cancellation → `Deadline`/`Cancelled`; races with response completion resolve first-to-complete. Unknown-`code` → `InvalidResponse` is recorded as a v0 posture (client and server ship from one source tree in bootstrap; forward-compat codes are a post-v0 concern).

### D9 — Conformance suite

Packaging corrected: a separate **`boxology-http-conformance`** dev-only crate (unpublished) — a crate cannot be its own reusable dev-dependency. It assembles fixture compositions (hello + kitchen-sink), attaches the server on an ephemeral port, and drives:

1. **Typed-client cases**: S1's presence-grid and round-trip tables replayed over the wire in both roles; envelopes; status mapping; header behaviors incl. rounding; disconnect-cancellation observability (fixture capability with a barrier + cancellation observer, deterministic); deadline cases per D7; no-dedup assertion.
2. **Raw-socket cases**, table-driven `(request bytes, expected status, expected code)`: malformed/duplicate-key JSON, wrong/duplicate media type, Content-Encoding, oversized body and header block, slow-trickled body vs. budget, invalid/duplicate timeout header, `%`-containing paths, query strings, non-POST/OPTIONS, empty body, trailing bytes, non-canonical integer strings, unknown fields/variants (role-checked), depth bombs.
3. **Adversarial raw-server cases** (client side): every classification row of D8's table, truncated bodies, oversized responses, redirect/204.

**Traceability is mandatory**: every rule in the normative wire text and every decision in this spec maps to at least one case; the T6 task spec carries the matrix and CI fails on unmapped rules.

## Acceptance criteria

1. Conformance suite green on both platforms, including all raw-socket and raw-server tables and the named-code assertions for both `404` kinds.
2. `greet("Ada") → "Hello, Ada!"` over a real socket via typed client **and** via a raw request, with the response byte-asserted against the canonical encoder.
3. Disconnect: the observer fixture sees cancellation; the completing-after-disconnect case produces no server-side error and a discarded response; both proven via the D7 task-ownership mechanism (asserted on the tracker, not incidental behavior).
4. Repeated `Idempotency-Key` demonstrably executes twice.
5. Deadline: expired-before-dispatch → `504` + zero invocations; small-positive-budget → handler observes decreasing budget then `504`; trickled-body pre-dispatch expiry → `504`; `Boxology-Timeout-Ms: garbage` and duplicates → `400`.
6. Composition validation rejects a synthetic non-unary capability and a top-level-`Field` capability at `expose` time with the capability and feature named (the binding-level rejection, per S1 D10).
7. An S3-local `cargo metadata` test asserts no fixture contract crate depends on `boxology-http` (mechanical now; S5 owns the global rule later — replacing the first draft's non-demonstrable criterion).

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Syntax layer (lossless tree, guards) + descriptor-guided role-aware semantic codec | 2 |
| T2 | Canonical encoder + envelopes + status/code mapping | 1–2 |
| T3 | Server: routing/canonicality, header grammars, lifecycle/task-ownership, limits, shutdown | 2 |
| T4 | Client: headers, limits, classification table, `CallError` synthesis | 2 |
| T5 | `boxology-http-conformance` harness + typed-client cases | 1–2 |
| T6 | Raw-socket + raw-server tables + traceability matrix with unmapped-rule gate | 2 |

T1 → T2 → {T3, T4} → T5 → T6. Depends on S1 (descriptors, roles, assembly, fixtures); S2 only for regenerated fixtures late — hand-written S1 fixtures suffice to start.

## Matters left open

*(None load-bearing.)*

- Default drain and header-read timeouts — set at T3 with measured values, recorded in the task PR.
- Depth-guard default (128) and response cap (8 MiB) — revisit on evidence.
- Raw-case table graduating to a cross-binding conformance format — at the second remote binding.

## Tracker notes

This spec decides parts of what #6 listed as open (foundation routing, server lifecycle, transport-boundary deadline enforcement); #6 retains discovery, placement, multi-box topology, and overload. #29's reconciliation notes v0 carries and validates W3C context without export. The axum/hyper/reqwest intake passes S0's deny gates. Issue #85 items 1, 3 (server side), and 8 are resolved here jointly with the S1/S2 revisions.
