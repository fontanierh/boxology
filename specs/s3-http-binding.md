# S3 Spec — HTTP Binding

[Stream definition](../boxology-details/11-v0-streams.md#s3--http-binding) · Status: **delivered in V0**

S3 implements the v1 HTTP transport against S1's assembly API. The wire contract — routing, JSON mapping, envelopes, status table, headers — is normative in the [Foundation HTTP binding section of Runtime](../boxology-details/03-runtime.md); this spec does not restate it. It records implementation decisions, resolves the details the normative text delegates, and defines the conformance suite that makes every choice observable.

## Purpose

The HTTP binding proves "defined once, invoked through Rust and HTTP" is a platform property. With S1's descriptor-guided, role-aware decoding, S3's job is a faithful syntax layer over `ContractValue`, the context/header envelope, an explicitly specified request/task lifecycle, and the suite that pins everything.

## Scope decisions

- **Server and client both ship.** The normative text specifies client-side `CallError` synthesis, the suite uses a typed driver, and the client proves that moving a capability behind HTTP changes no consumer types.
- **HTTP/1.1 only, stated and tested.** The first draft claimed HTTP/2 "as negotiated" while testing only 1.1 — an untested support claim, withdrawn. The server serves HTTP/1.1 (`http1`-only stack configuration); the client speaks HTTP/1.1. HTTP/2 (including any h2c posture) is post-v0 and arrives only with conformance coverage.
- **TLS out**: plaintext for local development and behind-proxy deployment; exactly a support statement, no Internet-facing claim implied.
- No auth (anonymous context constructed in-binding; identity headers untrusted), no streaming, no compression (`Content-Encoding` present → `415`), no CORS, no metrics export.

## Decisions

### D1 — Stack and features

`boxology-http`, one crate, is **feature-isolated**: `server` (axum/hyper, default-on), `client` (reqwest, off by default), shared codec always. Exact versions and feature sets are locked. Both sides use a deliberately thin slice: one route, raw body/headers, no middleware stack.

### D2 — Codec: descriptor-guided, role-aware, over a lossless syntax layer

Per issue #85 item 1, the codec does **not** parse JSON "directly into `ContractValue`". Layering:

1. **Syntax layer:** bytes → a lossless JSON syntax tree that *preserves duplicate keys and key order* (parsed with a depth guard, default 128, and the byte cap enforced before/while reading — not via `serde_json::Value`, which collapses duplicates). A leading U+FEFF is not JSON whitespace and is rejected as `invalid_request` (`400`); Boxology declines RFC 8259 §8.1's MAY to ignore it.
2. **Semantic layer:** syntax tree + `TypeDescriptor` + `DecodeRole` → `ContractValue`, applying S1's role rules (strict `ProviderInput`, tolerant `ConsumerOutput`), resolving descriptor-directed representation (`"42"` as `u64` where the descriptor says integer-64, `{"base64":…}` as `Blob` where it says blob, enum envelopes where it says enum), and rejecting duplicate keys, non-canonical integer strings (`"007"`, signs on `u64`, whitespace), fractional/exponent integer syntax, and invalid UTF-8.

Both layers and the canonical bare-value encoder live in the published
`boxology_contract::json` module. HTTP consumes that shared projection and owns only its protocol
envelopes and status/header behavior.

Encode is the reverse: `ContractValue` → **canonical bytes** (D3). Non-finite floats are unrepresentable in `ContractValue` (S1) — recorded so the wire never re-litigates it.

### D3 — Canonical response encoding (byte-assertable)

The canonical encoder is fully specified: UTF-8; no insignificant whitespace; struct keys in descriptor field order; envelope key sequences fixed exactly (`{"result":{"value":…}}`; `{"error":{"kind":…,"value":…}}`; `{"error":{"kind":…,"code":…,"message":…}}`); map keys sorted bytewise; escaping exactly `\"`, `\\`, `\b`, `\t`, `\n`, `\f`, `\r`, with remaining control characters as lowercase `\u00xx` and nothing else escaped; `f64` via Ryu-shortest and **`f32` via Ryu's f32 mode** (descriptor width selects; no widening-then-print), negative zero and boundaries pinned by golden vectors; integers per the canonical rules; Base64 standard alphabet with padding, **decoded strictly canonically** (non-canonical alphabet, wrong padding, or nonzero trailing pad bits rejected); no trailing newline. Golden byte vectors for every scalar edge, control character, float boundary, map ordering, envelope, and Base64 edge are pinned as the cross-encoder authority. **Byte-identity claims apply to canonical encoder output only** — accepted non-canonical *request* bytes (extra whitespace) need not round-trip byte-for-byte, and the suite states each assertion's domain.

### D4 — Routing and identifier canonicality

Exact match on `POST /rpc/{box_id}/{capability_local_name}` (the qualified `box.capability` id is a schema/manifest spelling; the route uses the two components — the S2 identity decision). The first draft's decode-then-match created aliases (`h%65llo` ≡ `hello`); corrected: **no percent-escapes are accepted** — id grammars contain only unreserved characters, so any `%` in a segment is a non-identifier and yields `404`. No trailing-slash or case tolerance. A present query string is `400 invalid_request` (strict-input posture). Raw-path tests, not framework-extracted parameters. Unknown box vs. unknown capability: both `404`, distinguished by the now-**named** stable codes below.

### D5 — Stable wire error codes

These named codes are wire contract in the call-error envelope and enter the conformance traceability table: `unknown_box`, `unknown_capability`, `invalid_request` (malformed syntax, contract violation, bad header grammar, query present, empty body, trailing bytes), `method_not_allowed` (`405`, with `Allow: POST`), `payload_too_large` (`413`), `unsupported_media_type` (`415`), `deadline_exceeded`, `unavailable`, `invalid_upstream_response`, `internal`. **Every service-generated invocation status carries the call-error envelope with its code** — including `405`/`413`/`415`; pre-service HTTP/1 framing failures such as malformed request-line `400`, over-long request-target `414`, and parse-buffer-exhaustion `431` are bare per `03-runtime.md`. A handler-returned `Cancelled` while the client is still connected maps to `500 internal` (self-cancellation absent client cancellation is an internal condition). Typed-client classification of statuses a conforming client can still receive: `413` → `ContractViolation` (deployment limit lower than payload); `405`/`415` → `InvalidResponse` (impossible from a conforming client). The enumeration is normative in `03-runtime.md`.

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
- Method/media table completed: non-POST on a valid path → `405` with `Allow: POST` (OPTIONS included — no CORS in v0); empty body → `400`; trailing bytes after the JSON document → `400`; the HTTP/1 parse-buffer bound is configured explicitly (default 16 KiB), with an incomplete head that exhausts it returning bare `431`; values below Hyper's 8192-byte minimum use the 8192-byte floor; when that buffer admits the request line, Hyper independently bounds request-target length and an over-long target is bare `414` before dispatch; the bare pre-service table is exactly `400`/`414`/`431` — a header-read timeout alone is not a resource bound.

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

Packaging corrected: a separate **`boxology-http-conformance`** dev-only crate (unpublished) — a crate cannot be its own reusable dev-dependency. That crate currently provides typed `hello` and current-wire conformance and depends only on `hello`; it attaches the server on an ephemeral port and drives (the full-grammar `kitchen-sink` composition is a post-v0 residual, [#100](https://github.com/fontanierh/boxology/issues/100)). The separate S6/S7 `ping`/`ping-app` real-socket and generated-project proof jointly satisfies the [v0 evidence corpus](../boxology-details/11-v0-streams.md#the-v0-evidence-corpus) HTTP obligation for the scalar/unit-error `ping` surface; `boxology-http-conformance` itself does not assemble `ping`/`ping-app`.

1. **Typed-client cases**: typed `hello` cases plus synthetic-descriptor expose-time rejections (AC6); envelopes; status mapping; header behaviors incl. rounding; disconnect-cancellation observability (fixture capability with a barrier + cancellation observer, deterministic); deadline cases per D7; no-dedup assertion. S1's presence-grid and round-trip tables remain kernel-level evidence; full-grammar typed wire replay and `Blob`/`Secret` typed end-to-end cases are post-v0 ([#100](https://github.com/fontanierh/boxology/issues/100), [#104](https://github.com/fontanierh/boxology/issues/104)). Canonical scalar/Base64/`Blob` codec unit vectors and every raw protocol case remain gating — E2E corpus narrowing is not codec deletion.
2. **Raw-socket cases**, table-driven `(request bytes, expected status, expected code)`: malformed/duplicate-key JSON, wrong/duplicate media type, Content-Encoding, oversized body and header block, slow-trickled body vs. budget, invalid/duplicate timeout header, `%`-containing paths, query strings, non-POST/OPTIONS, empty body, trailing bytes, non-canonical integer strings, unknown fields/variants (role-checked), depth bombs.
3. **Adversarial raw-server cases** (client side): every classification row of D8's table, truncated bodies, oversized responses, redirect/204.

**Traceability is mandatory**: every rule in the normative wire text and every decision in this spec maps to at least one case; the checked-in matrix fails CI on unmapped rules ([#115](https://github.com/fontanierh/boxology/issues/115) current-wire zero-unmapped gate). Same-poll race cases (paused clock), pipeline compound-invalid cases, and the strict-Base64 and float golden vectors are matrix rows like any other.

## Acceptance criteria

1. Conformance suite, all raw-socket/raw-server tables, and both named `404` assertions are green in native macOS ARM64 V0 evidence; cross-platform re-proof is [#525](https://github.com/fontanierh/boxology/issues/525) scope.
2. `greet("Ada") → "Hello, Ada!"` over a real socket via typed client **and** via a raw request, with the response byte-asserted against the canonical encoder.
3. Disconnect: the observer fixture sees cancellation; the completing-after-disconnect case produces no server-side error and a discarded response; both are explicitly asserted against D7's composition-owned task tracking rather than inferred from incidental stack behavior.
4. Repeated `Idempotency-Key` demonstrably executes twice.
5. Deadline: expired-before-dispatch → `504` + zero invocations; small-positive-budget → handler observes decreasing budget then `504`; trickled-body pre-dispatch expiry → `504`; `Boxology-Timeout-Ms: garbage` and duplicates → `400`.
6. Composition validation rejects a synthetic non-unary capability and a top-level-`Field` capability at `expose` time with the capability and feature named (the binding-level rejection, per S1 D10).
7. Workspace-isolation tests assert that no fixture contract crate depends on `boxology-http`, including a negative fixture that injects and detects such an edge.

## Matters left open

*(None load-bearing for v0.)*

- Current drain, header-read, depth-guard, and response-cap defaults remain evidence-driven and may be revisited without widening the claimed protocol.
- Raw-case table graduating to a cross-binding conformance format — at the second remote binding.
- Extended `kitchen-sink` / structured-container typed E2E and `Blob`/`Secret` typed E2E suite — [#100](https://github.com/fontanierh/boxology/issues/100), [#104](https://github.com/fontanierh/boxology/issues/104).
