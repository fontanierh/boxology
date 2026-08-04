//! Development-only HTTP conformance tests for the generated Hello fixture.
//!
//! The library target exposes the evidence inventory PR-B will cite; behavior
//! lives in the integration tests.

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

/// Named conformance evidence PR-B will cite outside `RAW_CASES` / `ROWS`.
#[rustfmt::skip]
pub const NAMED_CONFORMANCE_EVIDENCE: &[NamedConformanceEvidence] = &[
    NamedConformanceEvidence { module: "raw_hello", name: "raw_hello_cases_are_canonical" },
    NamedConformanceEvidence { module: "raw_hello", name: "default_request_body_limit_boundary_is_one_mib" },
    NamedConformanceEvidence { module: "raw_hello", name: "malformed_request_line_is_bare_http_400" },
    NamedConformanceEvidence { module: "raw_hello", name: "request_head_over_default_16_kib_cap_is_bare_http_431" },
    NamedConformanceEvidence { module: "raw_server", name: "raw_server_client_cases_are_canonical" },
    NamedConformanceEvidence { module: "typed_hello", name: "typed_hello_round_trips_success_and_domain_error" },
    NamedConformanceEvidence { module: "typed_hello", name: "assertion_panic_survives_a_forced_shutdown_error" },
    NamedConformanceEvidence { module: "typed_hello", name: "stalled_assertions_are_aborted_before_shutdown" },
    NamedConformanceEvidence { module: "typed_hello", name: "typed_hello_preserves_keys_and_executes_each_serial_call" },
    NamedConformanceEvidence { module: "workspace_isolation", name: "fixture_contract_graphs_isolate_boxology_http" },
    NamedConformanceEvidence { module: "workspace_isolation", name: "injected_contract_to_http_edge_is_reported" },
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
