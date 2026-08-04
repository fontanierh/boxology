//! S3 AC7: fixture contract crates must not depend on `boxology-http`.
//! Proves only S3 graph isolation; S5 remains global Cargo ownership authority.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const GREETER: &str = "crates/fixtures/greeter/generated/contract/Cargo.toml";
const HELLO: &str = "crates/fixtures/hello/generated/contract/Cargo.toml";
const PING: &str = "crates/fixtures/ping/generated/contract/Cargo.toml";
const HTTP: &str = "crates/boxology-http/Cargo.toml";
const CONTRACT: &str = "crates/boxology-contract/Cargo.toml";

#[test]
fn named_evidence_resolves_through_inventory() {
    boxology_http_conformance::assert_named_evidence_resolution(
        "workspace_isolation",
        &[
            (
                "fixture_contract_graphs_isolate_boxology_http",
                fixture_contract_graphs_isolate_boxology_http as *const (),
            ),
            (
                "injected_contract_to_http_edge_is_reported",
                injected_contract_to_http_edge_is_reported as *const (),
            ),
        ],
    );
}

#[test]
fn fixture_contract_graphs_isolate_boxology_http() {
    let root = repo_root();
    let rg = load(&root.join("Cargo.toml"), &root);
    let pg = load(&root.join("crates/fixtures/ping-app/Cargo.toml"), &root);
    let rc = contracts(&rg, &[GREETER, HELLO]);
    let pc = contracts(&pg, &[PING]);
    assert_eq!(names(&rg, &rc), set(["greeter-contract", "hello-contract"]));
    assert_eq!(names(&pg, &pc), set(["ping-contract"]));
    let mut union = names(&rg, &rc);
    union.extend(names(&pg, &pc));
    assert_eq!(
        union,
        set(["greeter-contract", "hello-contract", "ping-contract"])
    );
    assert!(validate(&rg, &rc).is_empty(), "root: http reachable");
    assert!(validate(&pg, &pc).is_empty(), "ping-app: http reachable");
}

#[test]
fn injected_contract_to_http_edge_is_reported() {
    let root = repo_root();
    let mut rg = load(&root.join("Cargo.toml"), &root);
    let mut pg = load(&root.join("crates/fixtures/ping-app/Cargo.toml"), &root);
    inject(&mut rg);
    inject(&mut pg);
    let rc = contracts(&rg, &[GREETER, HELLO]);
    let pc = contracts(&pg, &[PING]);
    check_injected(&rg, &rc, &validate(&rg, &rc));
    check_injected(&pg, &pc, &validate(&pg, &pc));
}

struct Graph {
    packages: BTreeMap<String, (String, String)>,
    adj: BTreeMap<String, Vec<String>>,
    http: String,
    contract: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name>")
        .to_path_buf()
}

#[rustfmt::skip]
fn load(manifest: &Path, root: &Path) -> Graph {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--locked", "--offline", "--format-version", "1", "--all-features", "--manifest-path"])
        .arg(manifest)
        .output()
        .expect("spawn cargo metadata");
    assert!(out.status.success(), "cargo metadata failed for {}: {}", manifest.display(), String::from_utf8_lossy(&out.stderr));
    parse(&serde_json::from_slice(&out.stdout).expect("metadata JSON"), root)
}

#[rustfmt::skip]
fn parse(doc: &Value, root: &Path) -> Graph {
    let mut packages = BTreeMap::new();
    for p in doc["packages"].as_array().expect("packages") {
        packages.insert(
            p["id"].as_str().expect("id").to_owned(),
            (p["name"].as_str().expect("name").to_owned(), rel(p["manifest_path"].as_str().expect("manifest_path"), root)),
        );
    }
    let mut adj = BTreeMap::new();
    for n in doc["resolve"]["nodes"].as_array().expect("nodes") {
        let deps = n.get("deps").and_then(Value::as_array).into_iter().flatten()
            .map(|d| d["pkg"].as_str().expect("deps.pkg").to_owned()).collect();
        adj.insert(n["id"].as_str().expect("node id").to_owned(), deps);
    }
    Graph { http: id_at(&packages, HTTP), contract: id_at(&packages, CONTRACT), packages, adj }
}

#[rustfmt::skip]
fn rel(manifest_path: &str, root: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = Path::new(manifest_path);
    let abs = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let abs = abs.canonicalize().unwrap_or(abs);
    match abs.strip_prefix(&root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => abs.to_string_lossy().replace('\\', "/"),
    }
}

#[rustfmt::skip]
fn id_at(packages: &BTreeMap<String, (String, String)>, manifest: &str) -> String {
    let ids: Vec<_> = packages.iter().filter(|(_, (_, m))| m == manifest).map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 1, "expected exactly one package at {manifest}");
    ids[0].clone()
}

fn contracts(g: &Graph, expected: &[&str]) -> Vec<String> {
    expected.iter().map(|m| id_at(&g.packages, m)).collect()
}

#[rustfmt::skip]
fn validate(g: &Graph, contracts: &[String]) -> Vec<Vec<String>> {
    assert!(g.packages.contains_key(&g.http), "boxology-http missing");
    let mut bad = Vec::new();
    for c in contracts {
        assert!(path(g, c, &g.contract).is_some(), "{} must reach contract", name(g, c));
        if let Some(p) = path(g, c, &g.http) { bad.push(p); }
    }
    bad
}

fn inject(g: &mut Graph) {
    g.adj
        .entry(g.contract.clone())
        .or_default()
        .push(g.http.clone());
}

#[rustfmt::skip]
fn check_injected(g: &Graph, contracts: &[String], failures: &[Vec<String>]) {
    assert_eq!(failures.len(), contracts.len(), "one failure/contract; {:?}", diag(g, failures));
    for c in contracts {
        let expect = vec![c.clone(), g.contract.clone(), g.http.clone()];
        assert!(
            failures.iter().any(|p| p == &expect),
            "missing {:?} ({:?}); have {:?}",
            expect,
            expect.iter().map(|id| name(g, id)).collect::<Vec<_>>(),
            diag(g, failures),
        );
    }
}

#[rustfmt::skip]
fn path(g: &Graph, start: &str, target: &str) -> Option<Vec<String>> {
    if start == target { return Some(vec![start.to_owned()]); }
    let mut q = VecDeque::from([start.to_owned()]);
    let mut prev = BTreeMap::new();
    let mut seen = BTreeSet::from([start.to_owned()]);
    while let Some(node) = q.pop_front() {
        for next in g.adj.get(&node).into_iter().flatten() {
            if !seen.insert(next.clone()) { continue; }
            prev.insert(next.clone(), node.clone());
            if next == target {
                let mut out = vec![target.to_owned()];
                let mut cur = target;
                while let Some(p) = prev.get(cur) { out.push(p.clone()); cur = p; }
                out.reverse();
                return Some(out);
            }
            q.push_back(next.clone());
        }
    }
    None
}

fn names(g: &Graph, ids: &[String]) -> BTreeSet<String> {
    ids.iter().map(|id| name(g, id).to_owned()).collect()
}

#[rustfmt::skip]
fn name<'a>(g: &'a Graph, id: &str) -> &'a str {
    &g.packages.get(id).unwrap_or_else(|| panic!("unknown {id}")).0
}

fn diag(g: &Graph, paths: &[Vec<String>]) -> Vec<Vec<String>> {
    paths
        .iter()
        .map(|p| p.iter().map(|id| name(g, id).to_owned()).collect())
        .collect()
}

fn set(names: impl IntoIterator<Item = &'static str>) -> BTreeSet<String> {
    names.into_iter().map(str::to_owned).collect()
}
