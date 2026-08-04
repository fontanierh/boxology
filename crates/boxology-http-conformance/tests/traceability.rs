//! Atomic normative-registry gate and active mutation proofs for S3-T6.

use std::{fs, path::PathBuf};

use boxology_http_conformance::{
    AuthorityBytes, EvidenceInventories, NAMED_CONFORMANCE_EVIDENCE, NamedConformanceEvidence,
    RAW_HELLO_CASE_IDS, RAW_SERVER_CASE_IDS, REGISTRY_SRC, RUNTIME_AUTHORITY_DIGEST,
    SPEC_AUTHORITY_DIGEST, assert_named_evidence_resolution, check_registry_src, parse_registry,
};

#[rustfmt::skip]
mod body {
use super::*;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("root")
}
fn read_repo(path: &str) -> Vec<u8> {
    fs::read(repo_root().join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

struct Ctx {
    runtime: Vec<u8>,
    spec: Vec<u8>,
    owned: Vec<(&'static str, Vec<u8>)>,
}
impl Ctx {
    fn load() -> Self {
        const PATHS: &[&str] = &[
            "crates/boxology-http/src/binding.rs", "crates/boxology-http/src/client.rs",
            "crates/boxology-http/src/encoder.rs", "crates/boxology-http/src/replay_tests.rs",
            "crates/boxology-http/src/semantic.rs", "crates/boxology-http/src/server.rs",
        ];
        Self {
            runtime: read_repo("boxology-details/03-runtime.md"),
            spec: read_repo("specs/s3-http-binding.md"),
            owned: PATHS.iter().map(|p| (*p, read_repo(p))).collect(),
        }
    }
    fn inv() -> EvidenceInventories<'static> {
        EvidenceInventories { raw_hello: RAW_HELLO_CASE_IDS, raw_server: RAW_SERVER_CASE_IDS, named: NAMED_CONFORMANCE_EVIDENCE }
    }
    fn check(&self, src: &str, rt: u64, sp: u64, inv: &EvidenceInventories<'_>) -> Result<(), String> {
        let sources: Vec<_> = self.owned.iter().map(|(p, b)| (*p, b.as_slice())).collect();
        check_registry_src(src, &AuthorityBytes {
            runtime: &self.runtime, spec: &self.spec, runtime_digest: rt, spec_digest: sp, source_files: &sources,
        }, inv)
    }
    fn err(&self, src: &str, inv: &EvidenceInventories<'_>, rt: u64, sp: u64, needle: &str) {
        let err = self.check(src, rt, sp, inv).expect_err("mutant must fail");
        assert!(err.contains(needle), "expected {needle:?} in {err:?}");
    }
}

#[test]
fn named_evidence_resolves_through_inventory() {
    assert_named_evidence_resolution("traceability", &[
        ("normative_registry_gate_is_complete", normative_registry_gate_is_complete as *const ()),
        ("registry_mutants_fail_closed", registry_mutants_fail_closed as *const ()),
    ]);
}

#[test]
fn normative_registry_gate_is_complete() {
    let ctx = Ctx::load();
    parse_registry(REGISTRY_SRC).expect("parse");
    ctx.check(REGISTRY_SRC, RUNTIME_AUTHORITY_DIGEST, SPEC_AUTHORITY_DIGEST, &Ctx::inv())
        .unwrap_or_else(|e| panic!("registry gate failed: {e}"));
}

#[test]
fn registry_mutants_fail_closed() {
    let ctx = Ctx::load();
    let inv = Ctx::inv();
    let (rt, sp) = (RUNTIME_AUTHORITY_DIGEST, SPEC_AUTHORITY_DIGEST);
    ctx.check(REGISTRY_SRC, rt, sp, &inv).expect("baseline must pass");
    let a = "Traceability is mandatory";
    let w = |ev: &str| if ev.is_empty() { format!("m :: spec :: {a} :: wire") } else { format!("m :: spec :: {a} :: wire {ev}") };
    let d = |disp: &str| format!("m :: spec :: {a} :: {disp}");
    let rows = [
        (w(""), "empty evidence"),
        (w("h:no-such-hello"), "dangling raw-hello"),
        (w("s:no-such-server"), "dangling raw-server"),
        (w("n:raw_hello/no_such_named"), "dangling named"),
        (w("f:crates/boxology-http/src/nope.rs#missing"), "missing source file"),
        (w("f:crates/boxology-http/src/binding.rs#this_function_does_not_exist"), "missing source function"),
        ("m :: spec ::  :: meta:x".into(), "empty required component"),
        ("m :: spec :: this-anchor-is-not-in-the-authority-source :: meta:x".into(), "anchor not found"),
        (d("kernel:"), "empty Kernel/Meta reason"),
        (d("meta:"), "empty Kernel/Meta reason"),
        (format!("m :: spec :: {a} :: meta:x\nm :: spec :: {a} :: meta:y"), "duplicate rule id"),
        ("only-one-field".into(), "wrong field count"),
        (format!("m :: spec :: {a} :: meta:x :: unexpected"), "wrong field count"),
        (d("noodle:nope"), "unknown disposition tag"),
        (w("z:bad"), "invalid evidence token"),
        (d("post:0"), "post:0"),
    ];
    for (src, needle) in &rows { ctx.err(src, &inv, rt, sp, needle); }
    ctx.err(REGISTRY_SRC, &inv, rt ^ 1, sp, "runtime authority digest drift");
    ctx.err(REGISTRY_SRC, &inv, rt, sp ^ 1, "spec authority digest drift");
    let hello = EvidenceInventories { raw_hello: &["exact-success", "never-cited-hello-row"], raw_server: &[], named: &[] };
    ctx.err(&w("h:exact-success"), &hello, rt, sp, "uncited raw-hello");
    let server = EvidenceInventories { raw_hello: &[], raw_server: &["ok-result", "never-cited-server-row"], named: &[] };
    ctx.err(&w("s:ok-result"), &server, rt, sp, "uncited raw-server");
    let named = EvidenceInventories {
        raw_hello: &[], raw_server: &[],
        named: &[
            NamedConformanceEvidence { module: "raw_hello", name: "raw_hello_cases_are_canonical" },
            NamedConformanceEvidence { module: "raw_hello", name: "never_cited_named" },
        ],
    };
    ctx.err(&w("n:raw_hello/raw_hello_cases_are_canonical"), &named, rt, sp, "uncited named");
}
} // mod body
