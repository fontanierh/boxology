# S3 Spec — HTTP Binding

[Stream definition](../boxology-details/11-v0-streams.md#s3--http-binding) · Status: **accepted at merge** (two review rounds addressed; cross-stream contract in issue #85)

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

The first draft demanded byte assertions without defining bytes. The canonical encoder is fully specified: UTF-8; no insignificant whitespace; struct keys in descriptor field order; envelope key sequences fixed exactly (`{"result":{"value":…}}`; `{"error":{"kind":…,"value":…}}`; `{"error":{"kind":…,"code":…,"message":…}}`); map keys sorted bytewise; escaping exactly `\"`, `\\`, `\b`, `\t`, `\n`, `\f`, `\r`, with remaining control characters as lowercase `\u00xx` and nothing else escaped; `f64` via Ryu-shortest and **`f32` via Ryu's f32 mode** (descriptor width selects; no widening-then-print), negative zero and boundaries pinned by golden vectors; integers per the canonical rules; Base64 standard alphabet with padding, **decoded strictly canonically** (non-canonical alphabet, wrong padding, or nonzero trailing pad bits rejected); no trailing newline. **Golden byte vectors for every scalar edge, control character, float boundary, map ordering, envelope, and Base64 edge are pinned in T2** as the cross-encoder authority. **Byte-identity claims apply to canonical encoder output only** — accepted non-canonical *request* bytes (extra whitespace) need not round-trip byte-for-byte, and the suite states each assertion's domain.

### D4 — Routing and identifier canonicality

Exact match on `POST /rpc/{box_id}/{capability_local_name}` (the qualified `box.capability` id is a schema/manifest spelling; the route uses the two components — the S2 identity decision). The first draft's decode-then-match created aliases (`h%65llo` ≡ `hello`); corrected: **no percent-escapes are accepted** — id grammars contain only unreserved characters, so any `%` in a segment is a non-identifier and yields `404`. No trailing-slash or case tolerance. A present query string is `400 invalid_request` (strict-input posture). Raw-path tests, not framework-extracted parameters. Unknown box vs. unknown capability: both `404`, distinguished by the now-**named** stable codes below.

### D5 — Stable wire error codes

Promised-but-unnamed codes are named; these are wire contract, in the call-error envelope's `code` field, and enter the conformance traceability table: `unknown_box`, `unknown_capability`, `invalid_request` (malformed syntax, contract violation, bad header grammar, query present, empty body, trailing bytes), `method_not_allowed` (`405`, with `Allow: POST`), `payload_too_large` (`413`), `unsupported_media_type` (`415`), `deadline_exceeded`, `unavailable`, `invalid_upstream_response`, `internal`. **Every service-generated invocation status carries the call-error envelope with its code** — including `405`/`413`/`415`; pre-service HTTP/1 framing failures such as malformed request-line `400` and over-cap-head `431` are bare per `03-runtime.md`. A handler-returned `Cancelled` while the client is still connected maps to `500 internal` (self-cancellation absent client cancellation is an internal condition). Typed-client classification of statuses a conforming client can still receive: `413` → `ContractViolation` (deployment limit lower than payload); `405`/`415` → `InvalidResponse` (impossible from a conforming client). The enumeration is normative in `03-runtime.md`, **edited in this PR's diff**.

### D6 — Header grammars, exactly

- `Boxology-Timeout-Ms`: grammar `0|[1-9][0-9]{0,9}` (≤ ~115.7 days), single occurrence. Duplicate header, sign, whitespace, non-digit, or overflow → `400 invalid_request` — a malformed deadline must not silently become no-deadline. Absent → composition default; a valid supplied value is silently capped at that default, which is also the binding's maximum accepted client deadline.
- `Idempotency-Key`: 1–256 bytes of visible ASCII, single occurrence; violations → `400`. Carried into `CallContext`; **transported, never honored** — the suite asserts a repeated key does not dedup.
- `traceparent`: W3C grammar; invalid or duplicated → **ignored**, fresh trace (observability must not break calls — the deliberate asymmetry with the timeout header, stated). `tracestate` without a valid parent → ignored.
- Client encoding of remaining budget: milliseconds, rounded **up** (a positive remaining budget never encodes as `0`). The grammar's ten digits bound the value at 9,999,999,999 ms ≈ **115.7 days** (the earlier ~11.5-day figure was arithmetic error, corrected per review).
- Client overflow policy is saturating: after rounding a positive remaining budget up to milliseconds, clamp it to exactly `9_999_999_999`; the encoded value is therefore never `0`.
- **Duplicate-occurrence rule for single-occurrence headers**: occurrences are counted by field lines; a comma-joined list within one line is one value that then fails its grammar (no grammar admits commas) → `400`.
- **`tracestate`**: W3C Trace Context Level 1 pinned; multiple `tracestate` field lines combine per the W3C list algorithm; combined length capped at 512 bytes; an invalid member or over-cap total drops `tracestate` while keeping a valid `traceparent`. `tracestate` without a valid parent is ignored (as before).
- **`Accept` is ignored** — responses are always `application/json`; no content negotiation in v0. Unlisted request headers are ignored (proxies add headers freely; only the contractual headers have grammar-enforced semantics).
- **`Idempotency-Key` is descriptor-independent transport pass-through**: accepted and carried into `CallContext` whether the capability declares `None` or `Inherent`; it is metadata for future dedup, never consulted in v0.
- Request `Content-Type`: `application/json` with optional `charset=utf-8` parameter (case-insensitive), single occurrence; anything else → `415`; duplicates → `400`. Responses: `Content-Type: application/json` only.
- Method/media table completed: non-POST on a valid path → `405` with `Allow: POST` (OPTIONS included — no CORS in v0); empty body → `400`; trailing bytes after the JSON document → `400`; maximum complete HTTP/1 request-head size (request line plus headers) configured explicitly (default 16 KiB), with an over-cap head returning bare `431`; values below Hyper's 8192-byte minimum use the 8192-byte floor — a header-read timeout alone is not a resource bound.

### D7 — Request lifecycle, deadline coverage, and task ownership

Resolving issue #85 item 8 and the S1 contradiction:

- **The request budget starts at head receipt** and covers body ingestion, decode, and dispatch. A trickled body exhausts the same deadline as a slow handler; pre-dispatch expiry → `504 deadline_exceeded` with **zero invocations**. (Split per review: expired-before-dispatch asserts zero invocations; a separate small-positive-budget case asserts the handler observes a shrinking budget and then expiry.)
- **Request-processing pipeline is normative and ordered**: (1) route match → `404`; (2) method → `405`; (3) media type / `Content-Encoding` → `415`; (4) header grammars → `400`; (5) size caps → `413`; (6) body read + UTF-8/syntax → `400`; (7) contract decode → `400`; (8) dispatch. Compound-invalid requests resolve to the earliest failing stage; representative compound cases (unknown-path+wrong-method, bad-media+expired, oversized+malformed) are conformance rows.
- **The binding implements S1 D11's transport contract**: it receives the composition-owned task tracker and its config (default deadline, byte limit, header cap, drain timeout) via `TransportRuntime` at fallible `start()`; socket bind failure fails composition start; `Composition::shutdown` drives its drain. Dispatch runs as an owned, spawned task registered in that tracker; the request future awaits the join handle.
- **Disconnect detection, stated at its true strength:** the drop-guard converts *service-future drop* into a token signal; the future is dropped when the pinned hyper/axum stack observes connection loss. The **guaranteed and conformance-tested case is peer full-close during dispatch** (complete request sent, connection closed while the handler runs). Half-close and other partial-shutdown patterns are recorded as not guaranteed in v0 — the contract claims exactly what the tested mechanism delivers, not incidental stack behavior. The spawned task continues regardless (advisory cancellation); a post-disconnect completion's response is discarded with no server-side error.
- **Timeout** races the join against the remaining budget with a **biased order: completion first, then deadline, then cancellation** — same-poll ties are deterministic by construction, tested with a paused clock at same-instant readiness. On expiry: `504`, token signalled, task stays tracked. **Panic ownership follows S1 D6**: catch-unwind at the dispatch boundary yields `Internal` (`500`); a `JoinError` occurs only for genuine post-drain aborts and also maps to `500`, tested as distinct cases.
- **Graceful shutdown**: stop accepting; await tracked tasks up to the drain timeout; then signal all tokens, grace-wait, abort. The abort rung atomically latches dispatch admission closed under the same mutex as task registration; a task racing admission after the latch remains owned and is aborted before handler entry, and every retained connection and dispatch handle is joined before shutdown returns. The suite itself uses clean start/stop per case.

### D8 — Client binding

Feature `client`: a remote `ImportTarget`/handle binding with base URL and the same codec. Sets context headers per D6; performs **no retries** (retry policy belongs to callers under declared idempotency; v0 declares none). **Resource limits**: response byte cap (default 8 MiB) and the codec depth guard apply before decode; bounded error-body reads. **Response classification table** (the first draft's "fails decode" was not a specification): every `(status, envelope, content-type)` combination is classified — `200`+`result` and `422`+`domain` decode strictly *at the envelope*, with the accepted tolerant rules applying inside result/domain payloads via `ConsumerOutput` role; call-error statuses require the matching `call` envelope; any other combination — wrong/missing content type, mismatched envelope, unknown call-error `code`, extra top-level fields, 1xx/204/3xx (redirects are not followed), truncated bodies — is `InvalidResponse` with the observed detail retained. Connect/DNS/refused → `Unavailable`; local deadline/cancellation → `Deadline`/`Cancelled`; races with response completion resolve first-to-complete. Unknown-`code` → `InvalidResponse` is recorded as a v0 posture (client and server ship from one source tree in bootstrap; forward-compat codes are a post-v0 concern).

Client base URLs are origins only: plaintext `http`, a DNS name, IPv4 address, or bracketed IPv6 address, and an optional explicit port. An absent path and `/` are equivalent; userinfo, non-root paths, queries, fragments, and other schemes are rejected. Deploying below an external proxy prefix therefore requires stripping that prefix before configuring the client. Requests use the exact absolute URI `http://{authority}/rpc/{box_id}/{capability_local_name}`; identifier segments are already canonical and are inserted directly without escaping or normalization.

### D9 — Conformance suite

Packaging corrected: a separate **`boxology-http-conformance`** dev-only crate (unpublished) — a crate cannot be its own reusable dev-dependency. It assembles fixture compositions (hello + kitchen-sink), attaches the server on an ephemeral port, and drives:

1. **Typed-client cases**: S1's presence-grid and round-trip tables replayed over the wire in both roles; envelopes; status mapping; header behaviors incl. rounding; disconnect-cancellation observability (fixture capability with a barrier + cancellation observer, deterministic); deadline cases per D7; no-dedup assertion.
2. **Raw-socket cases**, table-driven `(request bytes, expected status, expected code)`: malformed/duplicate-key JSON, wrong/duplicate media type, Content-Encoding, oversized body and header block, slow-trickled body vs. budget, invalid/duplicate timeout header, `%`-containing paths, query strings, non-POST/OPTIONS, empty body, trailing bytes, non-canonical integer strings, unknown fields/variants (role-checked), depth bombs.
3. **Adversarial raw-server cases** (client side): every classification row of D8's table, truncated bodies, oversized responses, redirect/204.

**Traceability is mandatory**: every rule in the normative wire text and every decision in this spec maps to at least one case; the T6 task spec carries the matrix and CI fails on unmapped rules. Same-poll race cases (paused clock), pipeline compound-invalid cases, and the strict-Base64 and float golden vectors are matrix rows like any other.

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

This spec decides parts of what #6 listed as open (foundation routing, server lifecycle, transport-boundary deadline enforcement); #6 retains discovery, placement, multi-box topology, and overload. #29's reconciliation notes v0 carries and validates W3C context without export. The axum/hyper/reqwest intake passes S0's deny gates. Normative `03-runtime.md` changes (named code enumeration incl. `405`/`413`/`415`, the no-percent-escape routing rule, HTTP/1.1-only) are **in this PR's diff**. S1's third revision supplies the presence/opacity ABI and transport lifecycle this spec consumes. Issue #85's S3 items (lifecycle API, disconnect strength, complete status/code table, race precedence, header holes, canonical-byte completeness, panic ownership, pipeline order, normative edits) are resolved in this revision.
