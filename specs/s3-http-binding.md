# S3 Spec — HTTP Binding

[Stream definition](../boxology-details/11-v0-streams.md#s3--http-binding) · Status: **proposed**

S3 implements the v1 HTTP transport against S1's assembly API. The wire contract itself — routing, JSON mapping, envelopes, status table, headers — is already normative in the [Foundation HTTP binding section of Runtime](../boxology-details/03-runtime.md); **this spec does not restate it.** It records the implementation decisions, resolves the wire-level details the normative text delegates, and defines the conformance suite that makes every choice observable.

## Purpose

The HTTP binding is the proof that "defined once, invoked through Rust and HTTP" is a property of the platform rather than two implementations that happen to agree. Because S1 centralized semantics in `ContractValue`, S3's essential job is narrow: a faithful syntax mapping (`ContractValue ↔ HTTP/JSON`), the context/header envelope, and the lifecycle behaviors (limits, cancellation, deadlines) — plus the test suite that pins all of it.

## Scope decision: server and client

S3 ships **both directions**:

- The **server binding**: a `TransportBinding` implementation exposing composition-selected capabilities over HTTP.
- The **client binding**: a typed handle bound to a remote base URL, synthesizing client-side `CallError`s per the normative rules.

The client is in scope for three reasons: the normative text already specifies client-side `CallError` synthesis (someone must implement it); the conformance suite needs a driver, and a typed client exercising the same `ContractValue` layer *is* the strongest driver; and the client is the executable form of the claim that moving a capability behind HTTP changes no consumer types. The acceptance milestone itself only requires the server plus any HTTP caller — flagged so review can strike the client if leanness wins.

## Non-goals

- No TLS: v0 serves plaintext HTTP for local development and behind-proxy deployment; stated plainly rather than implied. TLS termination is the operator's proxy's job until a future slice says otherwise.
- No authentication: the anonymous caller context is constructed inside the binding per the normative text; no identity headers are read (they are explicitly untrusted).
- No streaming shapes, no compression negotiation, no CORS, no metrics/tracing export (trace headers are carried into `CallContext` and no further).
- No REST ergonomics: `POST /rpc/{box_id}/{capability_id}` only; handwritten REST adapters remain a later, behind-the-contract pattern.
- No connection-level tuning surface beyond the few knobs in D6.

## Decisions

### D1 — Stack: axum on tokio, pinned

`axum` (with `hyper`/`tokio` underneath) is the dominant, maintained, tower-compatible choice; the binding uses a deliberately thin slice of it — one route, no extractors beyond raw body/headers, no middleware stack in v0 — so that a later swap to raw `hyper` is plausible if wanted. The client binding uses `reqwest` with the same posture. Both pinned in the workspace lockfile like everything else. HTTP/1.1 and HTTP/2 are both accepted as whatever the stack negotiates; the wire contract is version-agnostic and the conformance suite runs over HTTP/1.1.

### D2 — Crate layout

One crate, `boxology-http`, with `server` and `client` modules (a single crate because both sides share the wire codec, and splitting would either duplicate it or force a third micro-crate before any need exists). It depends on `boxology-runtime` + `boxology-contract`; generated contract crates never depend on it — the edge discipline again.

### D3 — The wire codec is a pure module over `ContractValue`

`ContractValue ↔ JSON bytes` lives in one pure module used identically by server decode/encode and client encode/decode. Wire-level rules the normative text fixes are implemented here (u64/i64 as decimal strings, adjacent enum tagging, `{"base64": …}` blobs, tri-state field presence, strict unknown-input rejection). Rules the normative text delegates, resolved now:

- **Duplicate JSON keys are rejected** (contract violation → `400`). Strict-input posture; last-wins would be a silent semantic lottery. This requires parsing with duplicate detection — the codec parses to `ContractValue` directly rather than through `serde_json::Value` (which collapses duplicates), which the IR design anticipated.
- **Non-finite floats**: unrepresentable in `ContractValue` by construction (S1); on decode, JSON has no NaN/Inf literal so the case is moot inbound; a handler can never emit one outbound because the IR rejects it at construction. Recorded so nobody re-litigates it at the wire.
- **Number syntax**: integers reject fractional parts and exponents (`1e2` is not a valid `u32` on this wire); floats accept standard JSON number syntax. Leading zeros and `-0` follow strict JSON.
- **String-encoded 64-bit integers** reject signs on `u64`, whitespace, and non-canonical forms (`"007"` is invalid) — canonical decimal only, so encode∘decode is identity on bytes, which classification and caching downstream may rely on.
- **UTF-8 is enforced** at body decode; invalid UTF-8 is malformed input (`400`), not lossy-replaced.
- **Depth and size guards in the codec itself**: a nesting-depth limit (default 128) and the composition's byte limit enforced pre-parse via `Content-Length` and streamed-body cap — malformed-input handling must not be an allocation amplifier.

### D4 — Routing and identifier edge cases

Exact-match routing: no trailing-slash tolerance, no case folding — ids are already lowercase by manifest grammar, and tolerance creates aliases that classification can never see. Percent-decoding per segment before matching; an id that decodes to something outside the id grammar is a `404` (unknown), not a `400` — the path namespace is closed. Unknown box vs. unknown capability both `404` per the normative table, with **distinct machine-readable `detail` values** in the call-error envelope so the conformance suite (and agents) can tell them apart.

### D5 — Context headers, precisely

- `Boxology-Timeout-Ms`: non-negative integer, no units, no float. Invalid value → `400` contract violation (silently ignoring a malformed deadline turns a caller's safety mechanism into a no-op). Absent → composition-default deadline. The resulting deadline is absolute at request start; the handler's remaining-budget view flows through S1 semantics. Response includes no deadline echo.
- `traceparent`/`tracestate`: validated structurally; an invalid `traceparent` is **ignored** (fresh trace context) rather than rejected — tracing is observability, not semantics, and a broken tracing proxy must not break calls. Carried, not interpreted, per S1.
- `Idempotency-Key`: opaque string ≤ 256 bytes (over-length → `400`); carried into `CallContext`; *transported, never honored* — no dedup exists in v0 and the conformance suite asserts a repeated key does **not** dedup, pinning honesty as behavior.
- Responses carry `Content-Type: application/json` and nothing else contractual. No server version header (fingerprinting surface with no consumer).

### D6 — Server lifecycle

The server binding attaches to a validated composition (S1's `TransportBinding` hook — binding conformance runs there: any non-unary shape or unsupported feature is rejected at composition validation, before traffic). Configuration surface, deliberately small: bind address, request-byte limit (default 1 MiB per normative text), default deadline, header read timeout, and graceful-shutdown drain timeout. Behaviors:

- **Deadline enforcement**: dispatch is wrapped in a timeout at the remaining-budget boundary → `504` with the deadline call-error; the handler simultaneously observes the same budget via context. In-process trust defers to the callee (S1 D9); at a transport boundary the transport enforces — this is the intended asymmetry, stated.
- **Client disconnect** triggers advisory cancellation on the request's `CancelToken`; work may complete anyway; nothing is rolled back. If the handler finishes after disconnect, the response is discarded — with no observable server-side error.
- **Graceful shutdown**: stop accepting, drain in-flight up to the drain timeout, then cancel-and-close. Needed by the conformance suite itself (clean start/stop per test) — correctness tooling first, production nicety second.
- **Panic in a handler** surfaces as `500 internal` (S1 already converts at dispatch; the transport maps it), never a connection reset.

### D7 — Client binding

A remote binding for a typed handle: base URL + the same codec. `CallError` synthesis per the normative table: connect/DNS/refused → `Unavailable`; local deadline expiry or cancellation before/mid-flight → `Deadline`/`Cancelled`; a response that fails envelope or contract decode → `InvalidResponse`; transport failure with no usable response → the corresponding client-side class, never an invented status. The client sets the context headers from `CallContext` (the propagation direction the server reads). It performs **no retries** — retry policy belongs to callers under declared idempotency metadata, and v0 ships none; the client doing "helpful" retries would violate the transported-never-honored honesty.

### D8 — The conformance suite is the deliverable

Structure: an in-process harness that assembles a fixture composition (S1's kitchen-sink + hello), attaches the server binding on an ephemeral port, and drives it two ways:

1. **Typed-client cases** — the presence grid and type round-trips replayed *over the wire* (S1's table cases, reused by construction), envelope selection, status mapping, header behaviors, disconnect-cancellation, deadline expiry.
2. **Raw-socket cases** — everything a correct typed client can never send: malformed JSON, duplicate keys, wrong media type, oversized bodies, invalid timeout header, bad percent-encoding, non-canonical integer strings, unknown input fields/variants, depth-bomb payloads. Raw cases are table-driven `(request bytes, expected status, expected error code)` so adding a case is data, not code.

The suite is structured as a reusable library (`boxology-http` dev-dependency now; future bindings will want the value-layer half), and it is the executable form of the normative section: **every rule in the normative wire text and every decision in this spec must map to at least one conformance case** — the task specs carry the traceability table.

## Acceptance criteria

1. Conformance suite green on both platforms, including every raw-socket case and the distinct-`404`-details assertion.
2. The hello fixture answers `greet("Ada") → "Hello, Ada!"` over a real socket via both the typed client and a raw `curl`-equivalent request, byte-asserted envelope.
3. Disconnect-cancellation is observable: a fixture capability that records cancellation observes it when the test client disconnects mid-call; a completing handler after disconnect produces no server error.
4. A repeated `Idempotency-Key` demonstrably does not dedup (two executions observed).
5. Deadline behavior: an expired budget produces `504` and the handler observed a non-positive budget; `Boxology-Timeout-Ms: garbage` produces `400`.
6. Binding conformance rejection: a synthetic non-unary capability in a fixture schema is rejected at composition validation with the capability and feature named.
7. No dependency edge from any generated contract crate to `boxology-http` (checked mechanically once S5's edge checker exists; asserted by review until then).

## Task list

| Task | Content | Est. PRs |
| --- | --- | --- |
| T1 | Wire codec: `ContractValue ↔ JSON` with strictness rules, duplicate/depth/UTF-8 guards | 2 |
| T2 | Server binding: routing, envelopes, status mapping, `TransportBinding` integration | 2 |
| T3 | Context headers, deadline enforcement, disconnect cancellation, lifecycle/shutdown | 1–2 |
| T4 | Client binding: header propagation, `CallError` synthesis, no-retry posture | 1–2 |
| T5 | Conformance harness + typed-client cases (reusing S1 grid data) | 1–2 |
| T6 | Raw-socket case table + traceability table to the normative text | 1–2 |

T1 first; T2/T4 fan out; T3 completes the server; T5–T6 close. Depends on S1 (assembly, IR, fixtures) and S2 only for regenerated fixtures late in the stream — hand-written S1 fixtures suffice to start.

## Matters left open

- Whether the client binding survives review or is cut to a test-only crate (scope flag above).
- Default header-read and drain timeouts — set at T3 with measured values, recorded in the task PR.
- The nesting-depth default (128) — revisit if real schemas approach it.
- HTTP/2-specific conformance cases — deferred until a consumer negotiates HTTP/2 in practice.
- Whether the raw-case table format graduates into a cross-binding conformance format — decided when a second remote binding exists.
