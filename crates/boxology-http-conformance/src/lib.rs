//! Development-only HTTP conformance tests for the generated Hello fixture.
//!
//! Exposes the evidence inventory and compact S3-T6 normative traceability registry.

#![forbid(unsafe_code)]

/// Ordered raw-hello case IDs. Must match `tests/raw_hello.rs` `RAW_CASES` exactly.
#[rustfmt::skip]
pub const RAW_HELLO_CASE_IDS: &[&str] = &[
    "exact-success", "unknown-box", "unknown-capability", "percent-encoded-box", "percent-encoded-capability",
    "uppercase-prefix", "uppercase-box", "uppercase-capability", "trailing-slash", "query-string",
    "get-method", "options-method", "unknown-route-wrong-method", "put-method", "missing-content-type",
    "application-xml-media-type", "text-json-media-type", "json-suffix-media-type", "wrong-charset", "charset-utf8-accepted",
    "charset-utf8-case-accepted", "trailing-semicolon-media-type", "duplicate-content-type", "comma-joined-content-type", "content-encoding-identity",
    "content-encoding-gzip", "bad-media-expired", "timeout-non-digit", "timeout-leading-zero", "timeout-eleven-digits",
    "timeout-embedded-space", "timeout-duplicate", "timeout-max-valid-accepted", "idempotency-duplicate", "idempotency-empty",
    "idempotency-obs-text", "empty-body", "trailing-bytes", "bom-prefixed-body", "invalid-utf8-body",
    "depth-bomb", "malformed-json", "duplicate-key-object", "noncanonical-integer", "oversized-content-length",
    "oversized-chunked", "oversized-plus-malformed", "oversized-plus-bad-media", "trickled-body-vs-budget", "oversized-content-length-head-only",
];

/// Ordered raw-server case IDs. Must match `tests/raw_server.rs` `ROWS` exactly.
#[rustfmt::skip]
pub const RAW_SERVER_CASE_IDS: &[&str] = &[
    "ok-result", "domain-empty-name", "domain-unknown-variant-tolerant", "unknown-box", "unknown-capability",
    "invalid-request", "method-not-allowed", "payload-too-large", "unsupported-media-type", "deadline-exceeded",
    "unavailable", "invalid-upstream-response", "internal", "call-wrong-status", "call-wrong-message",
    "call-unknown-code", "call-wrong-kind", "call-extra-field", "result-on-error-status", "result-extra-top-field",
    "result-extra-inner-field", "domain-envelope-on-200", "domain-kind-not-domain", "domain-extra-field", "missing-content-type",
    "text-json-content-type", "charset-content-type", "redirect-302", "no-content-204", "bom-prefixed-response",
    "malformed-json", "truncated-body", "oversized-streamed", "connect-refused", "deadline-vs-stall",
];

/// Named conformance test cited outside the row tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamedConformanceEvidence {
    /// Integration-test binary stem (`raw_hello`, `raw_server`, …).
    pub module: &'static str,
    /// Exact `fn` name in that binary.
    pub name: &'static str,
}

/// Named conformance evidence cited outside `RAW_CASES` / `ROWS`.
#[rustfmt::skip]
pub const NAMED_CONFORMANCE_EVIDENCE: &[NamedConformanceEvidence] = &[
    NamedConformanceEvidence { module: "raw_hello", name: "raw_hello_cases_are_canonical" },
    NamedConformanceEvidence { module: "raw_hello", name: "default_request_body_limit_boundary_is_one_mib" },
    NamedConformanceEvidence { module: "raw_hello", name: "malformed_request_line_is_bare_http_400" },
    NamedConformanceEvidence { module: "raw_hello", name: "request_head_over_default_16_kib_cap_is_bare_http_431" },
    NamedConformanceEvidence { module: "raw_hello", name: "overlong_request_target_is_bare_http_414" },
    NamedConformanceEvidence { module: "raw_server", name: "raw_server_client_cases_are_canonical" },
    NamedConformanceEvidence { module: "typed_hello", name: "typed_hello_round_trips_success_and_domain_error" },
    NamedConformanceEvidence { module: "typed_hello", name: "assertion_panic_survives_a_forced_shutdown_error" },
    NamedConformanceEvidence { module: "typed_hello", name: "stalled_assertions_are_aborted_before_shutdown" },
    NamedConformanceEvidence { module: "typed_hello", name: "typed_hello_preserves_keys_and_executes_each_serial_call" },
    NamedConformanceEvidence { module: "workspace_isolation", name: "fixture_contract_graphs_isolate_boxology_http" },
    NamedConformanceEvidence { module: "workspace_isolation", name: "injected_contract_to_http_edge_is_reported" },
    NamedConformanceEvidence { module: "traceability", name: "normative_registry_gate_is_complete" },
    NamedConformanceEvidence { module: "traceability", name: "registry_mutants_fail_closed" },
];

/// Inventory names for one integration-test binary, declaration order.
pub fn named_evidence_names(module: &str) -> Vec<&'static str> {
    NAMED_CONFORMANCE_EVIDENCE
        .iter()
        .filter(|row| row.module == module)
        .map(|row| row.name)
        .collect()
}

/// Exact ordered equality between a live row table and the public inventory.
pub fn assert_ordered_case_ids(actual: &[&str], expected: &[&str], label: &str) {
    assert_eq!(
        actual, expected,
        "{label}: live case IDs drifted from the evidence inventory"
    );
}

/// Pair live `fn` pointers with inventory names; require exact name-set equality.
pub fn assert_named_evidence_resolution(module: &str, resolved: &[(&str, *const ())]) {
    let expected = named_evidence_names(module);
    let mut actual: Vec<&str> = resolved.iter().map(|(name, _)| *name).collect();
    let mut expected_sorted = expected.clone();
    actual.sort_unstable();
    expected_sorted.sort_unstable();
    assert_eq!(
        actual, expected_sorted,
        "{module}: named evidence set drifted"
    );
    assert_eq!(
        resolved.len(),
        expected.len(),
        "{module}: named evidence cardinality drifted"
    );
    for (name, ptr) in resolved {
        assert!(
            !ptr.is_null(),
            "{module}::{name} fn pointer must be non-null"
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFile {
    Runtime,
    Spec,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence<'a> {
    RawHello(&'a str),
    RawServer(&'a str),
    Named { module: &'a str, name: &'a str },
    Source { path: &'a str, function: &'a str },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Disposition<'a> {
    Wire(Vec<Evidence<'a>>),
    Kernel(&'a str),
    PostV0(u32),
    Meta(&'a str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormativeRule<'a> {
    pub id: &'a str,
    pub source: SourceFile,
    pub anchor: &'a str,
    pub disposition: Disposition<'a>,
}

/// `id :: source :: anchor :: wire|kernel:|post:|meta:` compact registry.
#[rustfmt::skip]
pub const REGISTRY_SRC: &str = r#"sc/server-and-client-ship :: spec :: Server and client both ship :: wire n:typed_hello/typed_hello_round_trips_success_and_domain_error n:raw_server/raw_server_client_cases_are_canonical
sc/http11-only-stated-and-tested :: spec :: HTTP/1.1 only, stated and tested :: wire n:raw_hello/raw_hello_cases_are_canonical f:crates/boxology-http/src/client.rs#executor_sends_prepared_http1_request_once_and_accepts_fragmented_exact_cap
sc/http2-post-v0 :: spec :: HTTP/2 (including any h2c posture) is post-v0 :: post:100
sc/tls-out-plaintext :: spec :: TLS out :: meta:support statement; no Internet-facing TLS claim in v0
sc/no-auth-anonymous-context :: spec :: No auth (anonymous context constructed in-binding :: wire f:crates/boxology-http/src/server.rs#context_uses_receipt_time_override_and_exact_metadata n:typed_hello/typed_hello_round_trips_success_and_domain_error
sc/no-streaming :: spec :: no streaming :: post:100
sc/no-compression-content-encoding :: spec :: Content-Encoding` present → `415` :: wire h:content-encoding-identity h:content-encoding-gzip
sc/no-cors-no-metrics :: spec :: no CORS, no metrics export :: meta:explicit non-goals
d1/feature-isolated-crate :: spec :: feature-isolated :: meta:crate packaging/features are architectural
d1/server-default-client-off :: spec :: client` (reqwest, off by default) :: meta:Cargo feature defaults
d1/thin-slice-one-route :: spec :: one route, raw body/headers, no middleware stack :: wire h:exact-success f:crates/boxology-http/src/server.rs#exact_route_returns_the_selected_exposure_and_runtime_seam_exists
d2/not-direct-serde-value :: spec :: directly into `ContractValue` :: meta:layering decision stated against serde_json::Value
d2/syntax-preserves-duplicates-and-order :: spec :: preserves duplicate keys and key order :: wire h:duplicate-key-object f:crates/boxology-http/src/semantic.rs#duplicate_map_keys_win_before_value_lowering_without_leaking f:crates/boxology-http/src/replay_tests.rs#blob_duplicate_keys_precede_payload_decoding
d2/depth-guard-default-128 :: spec :: depth guard, default 128 :: wire h:depth-bomb
d2/byte-cap-before-or-while-reading :: spec :: byte cap enforced before/while reading :: wire h:oversized-content-length h:oversized-chunked f:crates/boxology-http/src/server.rs#request_body_enforces_byte_limit_before_payload_inspection
d2/leading-bom-rejected :: spec :: leading U+FEFF is not JSON whitespace and is rejected :: wire h:bom-prefixed-body
d2/semantic-role-aware :: spec :: strict `ProviderInput`, tolerant `ConsumerOutput` :: wire f:crates/boxology-http/src/semantic.rs#supported_boundaries_decode_in_both_roles f:crates/boxology-http/src/replay_tests.rs#aggregate_bytes_preserve_order_roles_and_unknown_enum_opacity f:crates/boxology-http/src/client.rs#result_and_domain_payloads_are_consumer_tolerant
d2/descriptor-directed-representation :: spec :: resolving descriptor-directed representation :: wire f:crates/boxology-http/src/semantic.rs#wide_integer_strings_are_canonical_and_range_checked f:crates/boxology-http/src/replay_tests.rs#blob_bytes_replay_canonical_vectors_in_both_roles f:crates/boxology-http/src/semantic.rs#known_enums_decode_all_supported_payload_shapes_and_orders
d2/reject-duplicate-keys :: spec :: rejecting duplicate keys :: wire h:duplicate-key-object f:crates/boxology-http/src/semantic.rs#duplicate_map_keys_win_before_value_lowering_without_leaking f:crates/boxology-http/src/replay_tests.rs#strict_struct_bytes_preserve_duplicate_role_and_conformance_precedence
d2/reject-noncanonical-integers :: spec :: non-canonical integer strings :: wire h:noncanonical-integer f:crates/boxology-http/src/semantic.rs#integer_rejections_are_exact_and_payload_free f:crates/boxology-http/src/semantic.rs#wide_integer_strings_are_canonical_and_range_checked
d2/encode-is-canonical-reverse :: spec :: Encode is the reverse: `ContractValue` :: wire f:crates/boxology-http/src/encoder.rs#canonical_scalar_goldens_replay_byte_identically f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form
d2/nonfinite-floats-unrepresentable :: spec :: Non-finite floats are unrepresentable in `ContractValue` :: kernel:S1 ContractValue finite-only; wire must not re-litigate
d3/utf8-no-insignificant-whitespace :: spec :: UTF-8; no insignificant whitespace :: wire f:crates/boxology-http/src/encoder.rs#canonical_scalar_goldens_replay_byte_identically n:raw_hello/raw_hello_cases_are_canonical
d3/struct-keys-descriptor-order :: spec :: struct keys in descriptor field order :: wire f:crates/boxology-http/src/encoder.rs#aggregates_have_exact_canonical_bytes_and_replay f:crates/boxology-http/src/encoder.rs#top_level_and_struct_presence_are_exact
d3/envelope-key-sequences-fixed :: spec :: envelope key sequences fixed exactly :: wire f:crates/boxology-http/src/encoder.rs#blob_presence_and_hello_envelopes_are_exact f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body h:exact-success
d3/map-keys-sorted-bytewise :: spec :: map keys sorted bytewise :: wire f:crates/boxology-http/src/encoder.rs#aggregates_have_exact_canonical_bytes_and_replay
d3/escaping-exact :: spec :: escaping exactly :: wire f:crates/boxology-http/src/encoder.rs#strings_use_exact_d3_escaping
d3/f64-ryu-shortest :: spec :: `f64` via Ryu-shortest :: wire f:crates/boxology-http/src/encoder.rs#floats_use_descriptor_width_ryu_goldens
d3/f32-ryu-f32-mode :: spec :: `f32` via Ryu's f32 mode :: wire f:crates/boxology-http/src/encoder.rs#floats_use_descriptor_width_ryu_goldens
d3/base64-standard-padded-strict :: spec :: Base64 standard alphabet with padding :: wire f:crates/boxology-http/src/encoder.rs#blob_presence_and_hello_envelopes_are_exact f:crates/boxology-http/src/replay_tests.rs#blob_bytes_reject_noncanonical_representations_without_leakage
d3/no-trailing-newline :: spec :: no trailing newline :: wire f:crates/boxology-http/src/encoder.rs#canonical_scalar_goldens_replay_byte_identically h:exact-success
d3/golden-vectors-gating :: spec :: Golden byte vectors for every scalar edge :: wire f:crates/boxology-http/src/encoder.rs#canonical_scalar_goldens_replay_byte_identically f:crates/boxology-http/src/encoder.rs#floats_use_descriptor_width_ryu_goldens f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form f:crates/boxology-http/src/replay_tests.rs#blob_bytes_replay_canonical_vectors_in_both_roles
d3/byte-identity-encoder-output-only :: spec :: Byte-identity claims apply to canonical encoder output only :: wire f:crates/boxology-http/src/encoder.rs#requests_are_bare_canonical_values_and_replay_as_provider_input f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form
d4/exact-post-rpc-route :: spec :: POST /rpc/{box_id}/{capability_local_name}` :: wire h:exact-success h:trailing-slash h:uppercase-prefix f:crates/boxology-http/src/server.rs#exact_route_returns_the_selected_exposure_and_runtime_seam_exists
d4/no-percent-escapes :: spec :: no percent-escapes are accepted :: wire h:percent-encoded-box h:percent-encoded-capability f:crates/boxology-http/src/server.rs#malformed_and_unknown_routes_have_canonical_distinct_errors
d4/no-trailing-slash-or-case-tolerance :: spec :: No trailing-slash or case tolerance :: wire h:trailing-slash h:uppercase-box h:uppercase-capability
d4/query-string-invalid-request :: spec :: present query string is `400 invalid_request` :: wire h:query-string
d4/unknown-box-vs-capability-named :: spec :: Unknown box vs. unknown capability: both `404` :: wire h:unknown-box h:unknown-capability f:crates/boxology-http/src/server.rs#malformed_and_unknown_routes_have_canonical_distinct_errors
d5/stable-wire-codes-named :: spec :: unknown_box`, `unknown_capability`, `invalid_request` :: wire h:unknown-box h:unknown-capability h:malformed-json h:get-method h:oversized-content-length h:missing-content-type f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body
d5/service-statuses-carry-call-error-envelope :: spec :: Every service-generated invocation status carries the call-error :: wire h:get-method h:oversized-content-length h:application-xml-media-type f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body
d5/bare-framing-400-414-431 :: spec :: over-long request-target `414`, and parse-buffer-exhaustion `431` are bare :: wire n:raw_hello/malformed_request_line_is_bare_http_400 n:raw_hello/overlong_request_target_is_bare_http_414 n:raw_hello/request_head_over_default_16_kib_cap_is_bare_http_431
d5/handler-cancelled-maps-internal :: spec :: handler-returned `Cancelled` :: wire f:crates/boxology-http/src/encoder.rs#erased_call_errors_map_closed_and_never_expose_detail s:internal
d5/client-classifies-413-405-415 :: spec :: `413` → `ContractViolation` :: wire f:crates/boxology-http/src/client.rs#canonical_call_table_maps_every_typed_category s:payload-too-large s:method-not-allowed s:unsupported-media-type
d6/timeout-ms-grammar :: spec :: grammar `0|[1-9][0-9]{0,9}` :: wire h:timeout-non-digit h:timeout-leading-zero h:timeout-eleven-digits h:timeout-embedded-space h:timeout-duplicate h:timeout-max-valid-accepted f:crates/boxology-http/src/server.rs#timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers f:crates/boxology-http/src/server.rs#timeout_header_rejects_every_malformed_or_duplicate_form
d6/timeout-absent-uses-composition-default :: spec :: Absent → composition default :: wire f:crates/boxology-http/src/server.rs#context_uses_receipt_time_override_and_exact_metadata f:crates/boxology-http/src/server.rs#client_timeout_is_clamped_to_binding_policy_without_changing_wire_success
d6/timeout-capped-at-default-max :: spec :: silently capped at that default :: wire f:crates/boxology-http/src/server.rs#client_timeout_is_clamped_to_binding_policy_without_changing_wire_success
d6/idempotency-key-grammar-and-transport :: spec :: Idempotency-Key`: 1–256 bytes of visible ASCII :: wire h:idempotency-empty h:idempotency-obs-text h:idempotency-duplicate f:crates/boxology-http/src/server.rs#idempotency_header_accepts_boundaries_and_preserves_only_the_key f:crates/boxology-http/src/server.rs#idempotency_header_rejects_every_malformed_or_duplicate_form n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call
d6/idempotency-never-honored :: spec :: transported, never honored :: wire n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call h:idempotency-duplicate
d6/traceparent-invalid-ignored :: spec :: invalid or duplicated → **ignored** :: wire f:crates/boxology-http/src/server.rs#traceparent_accepts_level_one_and_future_prefixes_opaquely f:crates/boxology-http/src/server.rs#malformed_or_duplicate_traceparent_drops_the_entire_context
d6/tracestate-without-parent-ignored :: spec :: tracestate` without a valid parent → ignored :: wire f:crates/boxology-http/src/server.rs#invalid_tracestate_drops_only_state f:crates/boxology-http/src/client.rs#tracing_is_best_effort_and_parent_gates_state
d6/client-budget-rounds-up :: spec :: milliseconds, rounded **up** :: wire f:crates/boxology-http/src/client.rs#deadlines_are_ceiled_nonzero_and_clamped
d6/client-budget-saturates :: spec :: clamp it to exactly `9_999_999_999` :: wire f:crates/boxology-http/src/client.rs#deadlines_are_ceiled_nonzero_and_clamped
d6/duplicate-occurrence-by-field-lines :: spec :: occurrences are counted by field lines :: wire h:timeout-duplicate h:duplicate-content-type h:comma-joined-content-type f:crates/boxology-http/src/server.rs#timeout_header_rejects_every_malformed_or_duplicate_form
d6/tracestate-w3c-combine-and-cap :: spec :: combine per the W3C list algorithm :: wire f:crates/boxology-http/src/server.rs#tracestate_accepts_level_one_grammar_and_preserves_combined_bytes f:crates/boxology-http/src/server.rs#invalid_tracestate_drops_only_state
d6/accept-ignored :: spec :: Accept` is ignored :: wire f:crates/boxology-http/src/server.rs#timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers h:exact-success
d6/unlisted-headers-ignored :: spec :: Unlisted request headers are ignored :: wire f:crates/boxology-http/src/server.rs#timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers
d6/idempotency-descriptor-independent :: spec :: descriptor-independent transport pass-through :: wire f:crates/boxology-http/src/server.rs#idempotency_header_accepts_boundaries_and_preserves_only_the_key n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call
d6/content-type-json-charset :: spec :: Request `Content-Type`: `application/json` :: wire h:missing-content-type h:application-xml-media-type h:text-json-media-type h:json-suffix-media-type h:wrong-charset h:charset-utf8-accepted h:charset-utf8-case-accepted h:trailing-semicolon-media-type h:duplicate-content-type h:comma-joined-content-type f:crates/boxology-http/src/server.rs#admission_accepts_only_the_declared_json_media_forms
d6/response-content-type-json-only :: spec :: Responses: `Content-Type: application/json` only :: wire h:exact-success f:crates/boxology-http/src/encoder.rs#blob_presence_and_hello_envelopes_are_exact
d6/method-media-table :: spec :: non-POST on a valid path → `405` with `Allow: POST` :: wire h:get-method h:options-method h:put-method h:unknown-route-wrong-method h:empty-body h:trailing-bytes
d6/request-head-default-16kib :: spec :: default 16 KiB :: wire n:raw_hello/request_head_over_default_16_kib_cap_is_bare_http_431 f:crates/boxology-http/src/binding.rs#default_request_head_cap_accepts_16384_and_rejects_16385_bare f:crates/boxology-http/src/binding.rs#server_head_limits_have_explicit_defaults_and_hyper_floor
d6/request-head-over-cap-bare-431 :: spec :: incomplete head that exhausts it returning bare `431` :: wire n:raw_hello/request_head_over_default_16_kib_cap_is_bare_http_431 f:crates/boxology-http/src/binding.rs#default_request_head_cap_accepts_16384_and_rejects_16385_bare
d6/request-head-floor-8192 :: spec :: 8192-byte minimum use the 8192-byte floor :: wire f:crates/boxology-http/src/binding.rs#server_head_limits_have_explicit_defaults_and_hyper_floor f:crates/boxology-http/src/binding.rs#configured_one_byte_request_head_cap_uses_hyper_floor
d6/overlong-target-bare-414 :: spec :: over-long target is bare `414` before dispatch :: wire n:raw_hello/overlong_request_target_is_bare_http_414
d6/header-read-timeout-not-resource-bound :: spec :: a header-read timeout alone is not a resource bound :: wire f:crates/boxology-http/src/binding.rs#partial_request_head_closes_after_configured_timeout
d7/budget-starts-at-head-receipt :: spec :: The request budget starts at head receipt :: wire h:trickled-body-vs-budget f:crates/boxology-http/src/binding.rs#trickled_body_hits_the_head_deadline_without_dispatching f:crates/boxology-http/src/binding.rs#live_socket_deadline_budget_and_tracker_ownership_are_causal
d7/pipeline-order :: spec :: Request-processing pipeline is normative and ordered :: wire h:unknown-route-wrong-method h:bad-media-expired h:oversized-plus-malformed h:oversized-plus-bad-media f:crates/boxology-http/src/server.rs#admission_enforces_route_then_query_then_method_precedence f:crates/boxology-http/src/server.rs#admission_enforces_media_before_contractual_headers
d7/s1-d11-transport-contract :: spec :: The binding implements S1 D11's transport contract :: wire f:crates/boxology-http/src/binding.rs#live_socket_deadline_budget_and_tracker_ownership_are_causal f:crates/boxology-http/src/binding.rs#shutdown_stops_intake_and_refuses_new_connections f:crates/boxology-http/src/binding.rs#drain_timeout_aborts_and_joins_a_parked_connection
d7/disconnect-peer-full-close :: spec :: peer full-close during dispatch :: wire f:crates/boxology-http/src/binding.rs#peer_full_close_cancels_dispatch_and_keeps_it_composition_owned
d7/half-close-not-guaranteed :: spec :: partial-shutdown patterns are recorded as not guaranteed :: meta:v0 claims only the tested full-close mechanism
d7/timeout-race-biased-order :: spec :: biased order: completion first, then deadline, then cancellation :: wire f:crates/boxology-http/src/server.rs#dispatch_race_order_is_completion_then_deadline_then_cancellation f:crates/boxology-http/src/client.rs#race_preserves_results_and_polls_operation_first f:crates/boxology-http/src/client.rs#race_deadline_precedes_cancellation_and_newly_ready_operation_wins f:crates/boxology-http/src/client.rs#race_observes_later_deadline_and_cancellation_and_drops_operations
d7/panic-ownership-internal :: spec :: catch-unwind at the dispatch boundary yields `Internal` (`500`) :: wire f:crates/boxology-http/src/server.rs#timeout_and_request_cancellation_leave_dispatch_owned_until_completion f:crates/boxology-http/src/encoder.rs#erased_call_errors_map_closed_and_never_expose_detail s:internal
d7/graceful-shutdown :: spec :: Graceful shutdown :: wire f:crates/boxology-http/src/binding.rs#shutdown_stops_intake_and_refuses_new_connections f:crates/boxology-http/src/binding.rs#handle_abort_aborts_and_joins_owned_connection f:crates/boxology-http/src/binding.rs#drain_timeout_aborts_and_joins_a_parked_connection f:crates/boxology-http/src/server.rs#abort_racing_spawn_accounts_for_rejected_task_until_cleanup
d8/client-no-retries :: spec :: performs **no retries** :: wire f:crates/boxology-http/src/client.rs#executor_never_follows_redirects_or_retries_transport_failures
d8/response-byte-cap-default-8mib :: spec :: response byte cap (default 8 MiB) :: wire f:crates/boxology-http/src/client.rs#default_client_response_limits_are_exact f:crates/boxology-http/src/client.rs#byte_and_depth_limits_are_inclusive f:crates/boxology-http/src/client.rs#executor_enforces_declared_and_streamed_caps_for_every_status s:oversized-streamed
d8/classification-table :: spec :: every `(status, envelope, content-type)` combination is classified :: wire s:ok-result s:domain-empty-name s:domain-unknown-variant-tolerant s:unknown-box s:unknown-capability s:invalid-request s:method-not-allowed s:payload-too-large s:unsupported-media-type s:deadline-exceeded s:unavailable s:invalid-upstream-response s:internal s:call-wrong-status s:call-wrong-message s:call-unknown-code s:call-wrong-kind s:call-extra-field s:result-on-error-status s:result-extra-top-field s:result-extra-inner-field s:domain-envelope-on-200 s:domain-kind-not-domain s:domain-extra-field s:missing-content-type s:text-json-content-type s:charset-content-type s:redirect-302 s:no-content-204 s:bom-prefixed-response s:malformed-json s:truncated-body s:oversized-streamed s:connect-refused s:deadline-vs-stall f:crates/boxology-http/src/client.rs#canonical_call_table_maps_every_typed_category f:crates/boxology-http/src/client.rs#envelopes_are_strict_and_status_bound f:crates/boxology-http/src/client.rs#rejects_bad_headers_bodies_and_unrecognized_statuses_without_echoing n:raw_server/raw_server_client_cases_are_canonical
d8/connect-refused-unavailable :: spec :: Connect/DNS/refused → `Unavailable` :: wire s:connect-refused f:crates/boxology-http/src/client.rs#canonical_call_table_maps_every_typed_category
d8/local-deadline-cancellation :: spec :: local deadline/cancellation → `Deadline`/`Cancelled` :: wire s:deadline-vs-stall f:crates/boxology-http/src/client.rs#race_observes_later_deadline_and_cancellation_and_drops_operations f:crates/boxology-http/src/client.rs#stalled_real_response_is_cancelled_promptly_without_diagnostics
d8/unknown-code-invalid-response-v0 :: spec :: Unknown-`code` → `InvalidResponse` is recorded as a v0 posture :: wire s:call-unknown-code f:crates/boxology-http/src/client.rs#envelopes_are_strict_and_status_bound
d8/base-url-origins-only :: spec :: Client base URLs are origins only :: wire f:crates/boxology-http/src/client.rs#origins_accept_and_normalize_only_http_origins
d8/request-absolute-uri-canonical :: spec :: Requests use the exact absolute URI :: wire f:crates/boxology-http/src/client.rs#request_line_headers_and_body_are_exact f:crates/boxology-http/src/client.rs#executor_sends_prepared_http1_request_once_and_accepts_fragmented_exact_cap
d9/separate-conformance-crate :: spec :: boxology-http-conformance`** dev-only crate :: meta:packaging architecture; crate graph isolation is AC7
d9/kitchen-sink-post-v0 :: spec :: full-grammar `kitchen-sink` composition is a post-v0 residual :: post:100
d9/typed-client-cases :: spec :: Typed-client cases :: wire n:typed_hello/typed_hello_round_trips_success_and_domain_error n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call n:typed_hello/assertion_panic_survives_a_forced_shutdown_error n:typed_hello/stalled_assertions_are_aborted_before_shutdown f:crates/boxology-http/src/binding.rs#conform_and_prepare_reject_unroutable_exposures f:crates/boxology-http/src/server.rs#rejects_every_non_unary_shape_before_presence f:crates/boxology-http/src/server.rs#top_level_field_is_rejected_in_each_slot_with_stable_precedence
d9/s1-presence-grid-kernel :: spec :: presence-grid and round-trip tables remain kernel-level evidence :: kernel:S1 owns presence-grid/round-trip tables; not re-proven as HTTP suite rows
d9/full-grammar-typed-e2e-post-v0 :: spec :: full-grammar typed wire replay and `Blob`/`Secret` typed end-to-end :: post:100
d9/blob-secret-typed-e2e-post-v0 :: spec :: [#104](https://github.com/fontanierh/boxology/issues/104) :: post:104
d9/raw-socket-cases :: spec :: Raw-socket cases :: wire h:exact-success h:unknown-box h:unknown-capability h:percent-encoded-box h:percent-encoded-capability h:uppercase-prefix h:uppercase-box h:uppercase-capability h:trailing-slash h:query-string h:get-method h:options-method h:unknown-route-wrong-method h:put-method h:missing-content-type h:application-xml-media-type h:text-json-media-type h:json-suffix-media-type h:wrong-charset h:charset-utf8-accepted h:charset-utf8-case-accepted h:trailing-semicolon-media-type h:duplicate-content-type h:comma-joined-content-type h:content-encoding-identity h:content-encoding-gzip h:bad-media-expired h:timeout-non-digit h:timeout-leading-zero h:timeout-eleven-digits h:timeout-embedded-space h:timeout-duplicate h:timeout-max-valid-accepted h:idempotency-duplicate h:idempotency-empty h:idempotency-obs-text h:empty-body h:trailing-bytes h:bom-prefixed-body h:invalid-utf8-body h:depth-bomb h:malformed-json h:duplicate-key-object h:noncanonical-integer h:oversized-content-length h:oversized-chunked h:oversized-plus-malformed h:oversized-plus-bad-media h:trickled-body-vs-budget h:oversized-content-length-head-only n:raw_hello/raw_hello_cases_are_canonical
d9/raw-server-cases :: spec :: Adversarial raw-server cases :: wire s:ok-result s:domain-empty-name s:domain-unknown-variant-tolerant s:unknown-box s:unknown-capability s:invalid-request s:method-not-allowed s:payload-too-large s:unsupported-media-type s:deadline-exceeded s:unavailable s:invalid-upstream-response s:internal s:call-wrong-status s:call-wrong-message s:call-unknown-code s:call-wrong-kind s:call-extra-field s:result-on-error-status s:result-extra-top-field s:result-extra-inner-field s:domain-envelope-on-200 s:domain-kind-not-domain s:domain-extra-field s:missing-content-type s:text-json-content-type s:charset-content-type s:redirect-302 s:no-content-204 s:bom-prefixed-response s:malformed-json s:truncated-body s:oversized-streamed s:connect-refused s:deadline-vs-stall n:raw_server/raw_server_client_cases_are_canonical
d9/traceability-mandatory :: spec :: Traceability is mandatory :: wire n:traceability/normative_registry_gate_is_complete n:traceability/registry_mutants_fail_closed n:raw_hello/raw_hello_cases_are_canonical n:raw_server/raw_server_client_cases_are_canonical
ac1/suite-green-native :: spec :: green in native macOS ARM64 V0 evidence :: wire n:raw_hello/raw_hello_cases_are_canonical n:raw_server/raw_server_client_cases_are_canonical n:typed_hello/typed_hello_round_trips_success_and_domain_error
ac1/cross-platform-reproof-post-v0 :: spec :: cross-platform re-proof is [#525] :: post:525
ac2/greet-typed-and-raw :: spec :: greet("Ada") → "Hello, Ada!" :: wire n:typed_hello/typed_hello_round_trips_success_and_domain_error h:exact-success
ac3/disconnect-observer-and-discard :: spec :: Disconnect: the observer fixture sees cancellation :: wire f:crates/boxology-http/src/binding.rs#peer_full_close_cancels_dispatch_and_keeps_it_composition_owned
ac4/repeated-idempotency-executes-twice :: spec :: Repeated `Idempotency-Key` demonstrably executes twice :: wire n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call
ac5/deadline-cases :: spec :: expired-before-dispatch → `504` + zero invocations :: wire h:trickled-body-vs-budget h:timeout-non-digit h:timeout-duplicate f:crates/boxology-http/src/binding.rs#live_socket_deadline_budget_and_tracker_ownership_are_causal f:crates/boxology-http/src/binding.rs#trickled_body_hits_the_head_deadline_without_dispatching f:crates/boxology-http/src/server.rs#timeout_header_rejects_every_malformed_or_duplicate_form
ac6/expose-time-rejection :: spec :: Composition validation rejects a synthetic non-unary capability :: wire f:crates/boxology-http/src/binding.rs#conform_and_prepare_reject_unroutable_exposures f:crates/boxology-http/src/server.rs#rejects_every_non_unary_shape_before_presence f:crates/boxology-http/src/server.rs#top_level_field_is_rejected_in_each_slot_with_stable_precedence
ac7/fixture-graph-isolation :: spec :: no fixture contract crate depends on `boxology-http` :: wire n:workspace_isolation/fixture_contract_graphs_isolate_boxology_http n:workspace_isolation/injected_contract_to_http_edge_is_reported
rt/post-rpc-shape :: runtime :: POST /rpc/{box_id}/{capability_id} :: wire h:exact-success f:crates/boxology-http/src/server.rs#exact_route_returns_the_selected_exposure_and_runtime_seam_exists
rt/identifiers-from-canonical-contract :: runtime :: Both identifiers come from the canonical contract :: wire h:exact-success h:unknown-box h:unknown-capability
rt/unreserved-identifier-grammar :: runtime :: Identifier grammars contain only unreserved URI characters :: wire h:percent-encoded-box h:percent-encoded-capability
rt/no-percent-escapes-404 :: runtime :: accepts **no percent-escapes** :: wire h:percent-encoded-box h:percent-encoded-capability f:crates/boxology-http/src/server.rs#malformed_and_unknown_routes_have_canonical_distinct_errors
rt/http11-only-v1 :: runtime :: The binding serves HTTP/1.1 only in v1 :: wire n:raw_hello/raw_hello_cases_are_canonical f:crates/boxology-http/src/client.rs#executor_sends_prepared_http1_request_once_and_accepts_fragmented_exact_cap
rt/no-rest-verbs-or-resource-paths :: runtime :: no configurable REST verbs, resource paths :: meta:architectural non-goal for the foundation binding
rt/application-json-requests-responses :: runtime :: Requests and responses use `application/json` :: wire h:exact-success h:missing-content-type h:application-xml-media-type
rt/default-body-limit-1mib :: runtime :: composition default request-body limit is 1 MiB :: wire n:raw_hello/default_request_body_limit_boundary_is_one_mib
rt/body-limit-configurable :: runtime :: may be lowered or raised explicitly :: wire h:oversized-content-length h:oversized-chunked
rt/oversized-body-413 :: runtime :: An oversized body returns `413` :: wire h:oversized-content-length h:oversized-chunked h:oversized-content-length-head-only n:raw_hello/default_request_body_limit_boundary_is_one_mib
rt/unsupported-media-415 :: runtime :: an unsupported media type returns `415` :: wire h:application-xml-media-type h:content-encoding-gzip h:missing-content-type
rt/malformed-or-invalid-400-before-invocation :: runtime :: malformed JSON or a value rejected by contract validation returns `400` :: wire h:malformed-json h:invalid-utf8-body h:duplicate-key-object h:noncanonical-integer f:crates/boxology-http/src/server.rs#request_body_maps_syntax_failures_to_canonical_bad_request f:crates/boxology-http/src/server.rs#request_body_maps_provider_semantic_failures_to_canonical_bad_request
rt/strings-booleans-json :: runtime :: Strings and booleans use their JSON equivalents :: wire f:crates/boxology-http/src/semantic.rs#supported_boundaries_decode_in_both_roles f:crates/boxology-http/src/encoder.rs#canonical_scalar_goldens_replay_byte_identically h:exact-success
rt/integers-32-json-numbers :: runtime :: Integers through 32 bits and finite floating-point values :: wire f:crates/boxology-http/src/semantic.rs#supported_boundaries_decode_in_both_roles f:crates/boxology-http/src/semantic.rs#finite_floats_decode_at_the_declared_width_in_both_roles f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form
rt/nonfinite-floats-invalid :: runtime :: a non-finite request value is invalid input :: wire f:crates/boxology-http/src/semantic.rs#float_failures_are_exact_payload_free_and_role_symmetric f:crates/boxology-http/src/encoder.rs#floats_use_descriptor_width_ryu_goldens
rt/i64-u64-decimal-strings :: runtime :: `i64` and `u64` use decimal JSON strings :: wire f:crates/boxology-http/src/semantic.rs#wide_integer_strings_are_canonical_and_range_checked f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form
rt/string-keyed-maps-only :: runtime :: Other map-key types are not supported by this binding :: wire f:crates/boxology-http/src/semantic.rs#lists_and_maps_preserve_empty_order_nested_values_and_optional_nulls f:crates/boxology-http/src/semantic.rs#recursive_support_inventory_is_complete_before_payload_inspection
rt/binary-base64-object :: runtime :: Bounded binary values use `{"base64":"..."}` :: wire f:crates/boxology-http/src/replay_tests.rs#blob_bytes_replay_canonical_vectors_in_both_roles f:crates/boxology-http/src/encoder.rs#blob_presence_and_hello_envelopes_are_exact
rt/streaming-binary-outside-unary :: runtime :: Streaming binary values are outside the unary foundation binding :: post:100
rt/enum-tag-payload :: runtime :: Every enum uses `{"tag":"variant","payload":...}` :: wire f:crates/boxology-http/src/semantic.rs#known_enums_decode_all_supported_payload_shapes_and_orders f:crates/boxology-http/src/encoder.rs#known_enums_and_domain_errors_have_exact_recursive_bytes s:domain-empty-name
rt/object-required-rejects-null :: runtime :: `T` is required and rejects `null` :: wire f:crates/boxology-http/src/semantic.rs#structs_apply_presence_recursion_roles_and_input_order f:crates/boxology-http/src/replay_tests.rs#presence_bytes_replay_top_fields_children_and_nested_wrappers
rt/object-option-omission-none :: runtime :: `Option<T>` maps omission to `None` and rejects explicit `null` :: wire f:crates/boxology-http/src/semantic.rs#structs_apply_presence_recursion_roles_and_input_order f:crates/boxology-http/src/replay_tests.rs#presence_bytes_replay_top_fields_children_and_nested_wrappers
rt/object-field-three-states :: runtime :: `Field<T>` maps omission to `Missing`, `null` to `Null` :: wire f:crates/boxology-http/src/semantic.rs#structs_apply_presence_recursion_roles_and_input_order f:crates/boxology-http/src/replay_tests.rs#presence_bytes_replay_top_fields_children_and_nested_wrappers f:crates/boxology-http/src/encoder.rs#top_level_and_struct_presence_are_exact
rt/toplevel-option-null :: runtime :: At the top level, `Option<T>` uses JSON `null` for `None` :: wire f:crates/boxology-http/src/semantic.rs#top_level_presence_wrappers_preserve_null_and_value_without_missing f:crates/boxology-http/src/encoder.rs#top_level_and_struct_presence_are_exact
rt/toplevel-field-rejected :: runtime :: A top-level `Field<T>` is rejected during binding validation :: wire f:crates/boxology-http/src/server.rs#top_level_field_is_rejected_in_each_slot_with_stable_precedence f:crates/boxology-http/src/binding.rs#conform_and_prepare_reject_unroutable_exposures
rt/result-envelope :: runtime :: {"result":{"value":"..."}} :: wire h:exact-success s:ok-result f:crates/boxology-http/src/encoder.rs#blob_presence_and_hello_envelopes_are_exact
rt/domain-error-envelope :: runtime :: {"error":{"kind":"domain","value" :: wire s:domain-empty-name f:crates/boxology-http/src/encoder.rs#known_enums_and_domain_errors_have_exact_recursive_bytes n:typed_hello/typed_hello_round_trips_success_and_domain_error
rt/call-error-envelope :: runtime :: {"error":{"kind":"call","code":"invalid_request" :: wire h:malformed-json f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body
rt/domain-errors-422 :: runtime :: Declared domain errors always return `422` :: wire s:domain-empty-name n:typed_hello/typed_hello_round_trips_success_and_domain_error f:crates/boxology-http/src/encoder.rs#known_enums_and_domain_errors_have_exact_recursive_bytes
rt/status-400 :: runtime :: `400` | Malformed JSON or contract validation failure. :: wire h:malformed-json h:noncanonical-integer h:duplicate-key-object
rt/status-404 :: runtime :: `404` | Unknown box or capability identity :: wire h:unknown-box h:unknown-capability
rt/status-405 :: runtime :: Method other than POST on a valid route (`Allow: POST`) :: wire h:get-method h:options-method s:method-not-allowed
rt/status-413 :: runtime :: `413` | Request body over the configured limit. :: wire h:oversized-content-length s:payload-too-large
rt/status-415 :: runtime :: `415` | Unsupported media type or content coding. :: wire h:application-xml-media-type h:content-encoding-gzip s:unsupported-media-type
rt/status-504 :: runtime :: `504` | Deadline exceeded. :: wire h:trickled-body-vs-budget s:deadline-exceeded
rt/status-503 :: runtime :: `503` | Bound target unavailable. :: wire s:unavailable f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body
rt/status-502 :: runtime :: `502` | Bound target returned an invalid contract response. :: wire s:invalid-upstream-response f:crates/boxology-http/src/server.rs#invalid_handler_values_are_canonical_502_without_payload_leaks
rt/status-500 :: runtime :: `500` | Internal runtime failure. :: wire s:internal f:crates/boxology-http/src/encoder.rs#erased_call_errors_map_closed_and_never_expose_detail
rt/bare-400-malformed-request-line :: runtime :: Malformed HTTP/1 request line. :: wire n:raw_hello/malformed_request_line_is_bare_http_400
rt/bare-414-overlong-target :: runtime :: The configured parse buffer admits the request line, but :: wire n:raw_hello/overlong_request_target_is_bare_http_414
rt/bare-431-over-cap-head :: runtime :: An incomplete HTTP/1 request head exhausts the configured parse-buffer bound. :: wire n:raw_hello/request_head_over_default_16_kib_cap_is_bare_http_431
rt/client-callerror-no-invented-status :: runtime :: it has no invented HTTP status :: wire s:connect-refused s:deadline-vs-stall f:crates/boxology-http/src/client.rs#stalled_real_response_is_cancelled_promptly_without_diagnostics
rt/stable-codes-on-invocation-statuses :: runtime :: carry the call-error envelope with one of the stable codes :: wire h:unknown-box h:unknown-capability h:malformed-json h:get-method h:oversized-content-length h:missing-content-type h:trickled-body-vs-budget f:crates/boxology-http/src/encoder.rs#every_call_error_has_one_exact_status_code_message_and_body
rt/context-headers-carried :: runtime :: Boxology-Timeout-Ms`, W3C trace context in `traceparent` :: wire h:timeout-max-valid-accepted f:crates/boxology-http/src/server.rs#timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers f:crates/boxology-http/src/server.rs#traceparent_accepts_level_one_and_future_prefixes_opaquely f:crates/boxology-http/src/server.rs#tracestate_accepts_level_one_grammar_and_preserves_combined_bytes f:crates/boxology-http/src/server.rs#idempotency_header_accepts_boundaries_and_preserves_only_the_key
rt/missing-headers-mean-defaults :: runtime :: Missing headers mean composition-default deadline :: wire f:crates/boxology-http/src/server.rs#context_uses_receipt_time_override_and_exact_metadata f:crates/boxology-http/src/server.rs#malformed_or_duplicate_traceparent_drops_the_entire_context
rt/idempotency-transported-not-deduped :: runtime :: it does not provide a deduplication store :: wire n:typed_hello/typed_hello_preserves_keys_and_executes_each_serial_call
rt/disconnect-advisory-cancellation :: runtime :: Client disconnection requests advisory cancellation :: wire f:crates/boxology-http/src/binding.rs#peer_full_close_cancels_dispatch_and_keeps_it_composition_owned
rt/anonymous-context-untrusted-identity :: runtime :: v1 does not trust caller-supplied identity headers :: wire f:crates/boxology-http/src/server.rs#context_uses_receipt_time_override_and_exact_metadata n:typed_hello/typed_hello_round_trips_success_and_domain_error
rt/conformance-must-exercise-surface :: runtime :: Binding conformance tests must exercise routing :: wire n:raw_hello/raw_hello_cases_are_canonical n:raw_server/raw_server_client_cases_are_canonical n:typed_hello/typed_hello_round_trips_success_and_domain_error n:traceability/normative_registry_gate_is_complete n:traceability/registry_mutants_fail_closed f:crates/boxology-http/src/replay_tests.rs#presence_bytes_replay_top_fields_children_and_nested_wrappers f:crates/boxology-http/src/replay_tests.rs#scalar_bytes_replay_every_width_boundary_and_string_form
"#;

pub const RUNTIME_AUTHORITY_DIGEST: u64 = 0x26c3dfeebfe05084;
pub const SPEC_AUTHORITY_DIGEST: u64 = 0x0086f955afbe142a;
const MAX_ANCHOR_BYTES: usize = 80;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone, Copy, Debug)]
pub struct EvidenceInventories<'a> {
    pub raw_hello: &'a [&'a str],
    pub raw_server: &'a [&'a str],
    pub named: &'a [NamedConformanceEvidence],
}

#[derive(Clone, Copy, Debug)]
pub struct AuthorityBytes<'a> {
    pub runtime: &'a [u8],
    pub spec: &'a [u8],
    pub runtime_digest: u64,
    pub spec_digest: u64,
    pub source_files: &'a [(&'a str, &'a [u8])],
}

pub fn check_registry_src(
    src: &str,
    authority: &AuthorityBytes<'_>,
    inventories: &EvidenceInventories<'_>,
) -> Result<(), String> {
    registry_check(&parse_registry(src)?, authority, inventories)
}

// Fail-closed parser + pure gate kept dense; rustfmt would inflate past the PR budget.
#[rustfmt::skip]
mod gate {
    use super::*;

    pub fn parse_registry(src: &str) -> Result<Vec<NormativeRule<'_>>, String> {
        let mut rules = Vec::new();
        for (idx, line) in src.lines().enumerate() {
            let n = idx + 1;
            if line.is_empty() { return Err(format!("line {n}: blank line")); }
            let p: Vec<&str> = line.split(" :: ").collect();
            if p.len() != 4 { return Err(format!("line {n}: wrong field count {}", p.len())); }
            let (id, src_tok, anchor, disp_tok) = (p[0], p[1], p[2], p[3]);
            if id.is_empty() || src_tok.is_empty() || anchor.is_empty() || disp_tok.is_empty() {
                return Err(format!("line {n}: empty required component"));
            }
            let source = match src_tok {
                "spec" => SourceFile::Spec,
                "runtime" => SourceFile::Runtime,
                other => return Err(format!("line {n}: unknown source {other}")),
            };
            let disposition = parse_disposition(disp_tok).map_err(|e| format!("line {n}: {e}"))?;
            rules.push(NormativeRule { id, source, anchor, disposition });
        }
        if rules.is_empty() { return Err("registry is empty".into()); }
        Ok(rules)
    }

    fn parse_disposition(tok: &str) -> Result<Disposition<'_>, String> {
        if let Some(rest) = tok.strip_prefix("wire") {
            if !(rest.is_empty() || rest.starts_with(' ')) { return Err("unknown disposition tag".into()); }
            let body = rest.trim_start();
            if body.is_empty() { return Ok(Disposition::Wire(Vec::new())); }
            let mut evidence = Vec::new();
            for piece in body.split(' ') { evidence.push(parse_evidence(piece)?); }
            return Ok(Disposition::Wire(evidence));
        }
        if let Some(reason) = tok.strip_prefix("kernel:") { return Ok(Disposition::Kernel(reason)); }
        if let Some(reason) = tok.strip_prefix("meta:") { return Ok(Disposition::Meta(reason)); }
        if let Some(num) = tok.strip_prefix("post:") {
            if num.is_empty() { return Err("empty post issue".into()); }
            let issue: u32 = num.parse().map_err(|_| format!("invalid post issue {num}"))?;
            if issue == 0 { return Err("post:0".into()); }
            return Ok(Disposition::PostV0(issue));
        }
        Err("unknown disposition tag".into())
    }

    fn parse_evidence(tok: &str) -> Result<Evidence<'_>, String> {
        if let Some(id) = tok.strip_prefix("h:") {
            if id.is_empty() { return Err("invalid evidence token".into()); }
            return Ok(Evidence::RawHello(id));
        }
        if let Some(id) = tok.strip_prefix("s:") {
            if id.is_empty() { return Err("invalid evidence token".into()); }
            return Ok(Evidence::RawServer(id));
        }
        if let Some(rest) = tok.strip_prefix("n:") {
            let (module, name) = rest.split_once('/').ok_or("invalid evidence token")?;
            if module.is_empty() || name.is_empty() || name.contains('/') {
                return Err("invalid evidence token".into());
            }
            return Ok(Evidence::Named { module, name });
        }
        if let Some(rest) = tok.strip_prefix("f:") {
            let (path, function) = rest.split_once('#').ok_or("invalid evidence token")?;
            if path.is_empty() || function.is_empty() || function.contains('#') {
                return Err("invalid evidence token".into());
            }
            return Ok(Evidence::Source { path, function });
        }
        Err("invalid evidence token".into())
    }

    pub fn registry_check(
        rules: &[NormativeRule<'_>],
        authority: &AuthorityBytes<'_>,
        inventories: &EvidenceInventories<'_>,
    ) -> Result<(), String> {
        if fnv1a64(authority.runtime) != authority.runtime_digest {
            return Err("runtime authority digest drift".into());
        }
        if fnv1a64(authority.spec) != authority.spec_digest {
            return Err("spec authority digest drift".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut cited_hello = std::collections::BTreeSet::new();
        let mut cited_server = std::collections::BTreeSet::new();
        let mut cited_named = std::collections::BTreeSet::new();
        for rule in rules {
            if !ids.insert(rule.id) { return Err(format!("duplicate rule id {}", rule.id)); }
            if rule.anchor.is_empty() { return Err(format!("empty anchor for {}", rule.id)); }
            if rule.anchor.len() > MAX_ANCHOR_BYTES {
                return Err(format!("anchor exceeds 80 bytes for {}", rule.id));
            }
            let source_bytes = match rule.source {
                SourceFile::Runtime => authority.runtime,
                SourceFile::Spec => authority.spec,
            };
            if !find_bytes(source_bytes, rule.anchor.as_bytes()) {
                return Err(format!("anchor not found in authority for {}: {}", rule.id, rule.anchor));
            }
            match &rule.disposition {
                Disposition::Wire(evidence) => {
                    if evidence.is_empty() {
                        return Err(format!("Wire rule {} has empty evidence", rule.id));
                    }
                    for item in evidence {
                        match *item {
                            Evidence::RawHello(id) => {
                                if !inventories.raw_hello.contains(&id) {
                                    return Err(format!("dangling raw-hello id {id} on {}", rule.id));
                                }
                                cited_hello.insert(id);
                            }
                            Evidence::RawServer(id) => {
                                if !inventories.raw_server.contains(&id) {
                                    return Err(format!("dangling raw-server id {id} on {}", rule.id));
                                }
                                cited_server.insert(id);
                            }
                            Evidence::Named { module, name } => {
                                if !inventories.named.iter().any(|r| r.module == module && r.name == name) {
                                    return Err(format!("dangling named evidence {module}::{name} on {}", rule.id));
                                }
                                cited_named.insert((module, name));
                            }
                            Evidence::Source { path, function } => {
                                let Some((_, bytes)) = authority.source_files.iter().find(|(c, _)| *c == path) else {
                                    return Err(format!("missing source file {path} on {}", rule.id));
                                };
                                let needle = format!("fn {function}(");
                                if !find_bytes(bytes, needle.as_bytes()) {
                                    return Err(format!("missing source function {function} in {path} on {}", rule.id));
                                }
                            }
                        }
                    }
                }
                Disposition::Kernel(reason) | Disposition::Meta(reason) => {
                    if reason.is_empty() {
                        return Err(format!("empty Kernel/Meta reason for {}", rule.id));
                    }
                }
                Disposition::PostV0(issue) => {
                    if *issue == 0 {
                        return Err(format!("PostV0 issue must be nonzero for {}", rule.id));
                    }
                }
            }
        }
        for id in inventories.raw_hello {
            if !cited_hello.contains(id) { return Err(format!("uncited raw-hello id {id}")); }
        }
        for id in inventories.raw_server {
            if !cited_server.contains(id) { return Err(format!("uncited raw-server id {id}")); }
        }
        for row in inventories.named {
            if !cited_named.contains(&(row.module, row.name)) {
                return Err(format!("uncited named evidence {}::{}", row.module, row.name));
            }
        }
        Ok(())
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
    }
}

pub use gate::{parse_registry, registry_check};
