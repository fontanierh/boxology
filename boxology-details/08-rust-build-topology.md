# Rust Build Topology

[Back to the white paper](../boxology-whitepaper.md)

This document defines how a native box maps onto Rust and Cargo. It separates the box's ownership boundary from its compilation units, establishes the generated contract boundary, and records the minimum build-graph enforcement required by the platform.

## One box, two compilation units

A box is one logical package and one pull-request owner. It owns two Cargo crates:

- A handwritten **implementation crate** containing its behavior and internal organization.
- A mechanically generated **contract crate** containing the Rust surface that consumers may compile against.

For example:

```text
boxes/customer/
  boxology.toml         # logical package and ownership manifest
  implementation/       # handwritten customer implementation crate
  generated/contract/   # generated customer contract crate
```

The exact directory and crate names are tooling choices. The semantic distinction is not: another box may compile against the contract crate but never against the implementation crate.

The author-facing `boxology` facade crate is distinct from the kernel crate, `boxology-contract`. `boxology-contract` owns the value/descriptor ABI. The delivered facade re-exports exactly the `contract` and `implementation` macros plus `boxology_contract::CallContext`; it does not re-export the wider kernel or runtime APIs.

Inside each implementation `Cargo.toml`, `boxology_generated_contract` is a fixed dependency **alias**, not a global package name. The generated package remains box-specific and workspace-unique:

```toml
[dependencies]
boxology = { workspace = true }
boxology_generated_contract = { package = "hello-contract", path = "../generated/contract" }
```

This does not weaken the single-owner rule. Both crates belong to the same box package, and a box-owned pull request may change its implementation inputs together with deterministic contract outputs attributable to them.

## Rust-first contract authoring

Developers and agents write the declaration-only contract block in
`implementation/src/contract.rs`. The implementation crate root contains the exact unconditional
items `mod contract;` and `pub use contract::*;`, followed by ordinary Rust implementation code.
This is an authoring convention, not generator path inference: generation still traverses from its
explicit request crate root across declared logical inputs. The block is deliberately a small
Rust-like grammar rather than arbitrary Rust:

```rust
// implementation/src/contract.rs
boxology::contract! {
    #[error]
    pub enum GreetError {
        EmptyName,
    }

    #[capability(exposure = external)]
    pub async fn greet(name: String) -> Result<String, GreetError>;
}
```

```rust
// implementation/src/lib.rs
mod contract;
pub use contract::*;

pub struct HelloService;

#[boxology::implementation]
impl HelloService {
    async fn greet(
        &self,
        context: boxology::CallContext,
        name: String,
    ) -> Result<String, GreetError> {
        // ordinary Rust body
        todo!()
    }
}
```

The contract signature and implementation signature intentionally repeat. The first is the language-neutral public contract and deterministic generation input. The second is ordinary executable Rust. Generated compile-time glue makes rustc prove they agree, so the repetition cannot silently drift; it avoids both a second handwritten interface file and a partial Rust type resolver.

The contract has one input value; multiple logical inputs use a named struct. Context is implicit there. The implementation receives `&self`, `boxology::CallContext`, then exactly that input and returns exactly the declared result. Its body, helpers, imports, aliases, macros, and private types are ordinary Rust. No exported service trait, internal service pattern, or organization beyond one inherent implementation is prescribed.

During normal compilation, `boxology::contract!` does not emit a second independent definition. It re-exports the sole compiled public type through the fixed `boxology_generated_contract` dependency alias and requires that crate's digest-keyed generated marker. Consumers, the implementation adapter, and bindings therefore share one type. A stale or mismatched generated crate fails compilation instead of creating two authorities.

## Deterministic generation before Cargo

Procedural macros cannot create a sibling contract crate in the same Cargo build. Boxology therefore generates before the normal Cargo build, but parses only the grammar it owns:

```text
declared logical Rust inputs
-> deterministic module traversal and direct-site discovery
-> shared controlled-contract parser
-> deterministic contract emitter
-> generated contract crate and schema
-> normal Cargo build
-> generated signature assertions
```

Each reachable box contains exactly one direct `boxology::contract!` invocation and one direct `#[boxology::implementation]` on a non-generic inherent impl for one concrete receiver. `cfg` or `cfg_attr` on either site or its module ancestry is rejected. The existing deterministic module traversal finds the sites. Generator and procedural macros use the same `boxology-contract-syntax` parser, so accepted syntax cannot drift between generation and compilation. Implementation bodies remain opaque.

Generation is a pure, pre-Cargo transformation of explicit bytes. It does not run Cargo, rustc, build scripts, user procedural macros, user code, runtime initialization, or implementation bodies. It publishes each changed generated file through one same-directory atomic replacement under the per-file staged-commit guarantee in [S2 D1 stage 4](../specs/s2-contract-generator.md#d1--controlled-declaration-plus-ordinary-implementation); whole-tree transactional publication is post-V0. The generator and later `contract!` expansion both call the shared parser's same semantic-digest function. That function domain-separates and SHA-256-hashes the ordered normalized contract model: declarations, names, semantic docs/deprecation, fields/variants/types, and capability signatures/metadata. It excludes spelling whitespace, non-documentation comments, spans, paths, implementation/private code, and unrelated source. This is not a raw-source hash or the separately specified public contract revision.

The generated crate exposes the marker keyed by that digest. The later normal Cargo build expands the contract facade and implementation attribute. Before generated adapter invocation, the implementation macro structurally rejects a generic impl, trait impl, non-concrete receiver, impl or method `where` clause, generic capability method, altered receiver, extra parameter, or `impl Trait`. Generated calls then make rustc prove alias-resolved exact nominal input/output/error identity, `Send + Sync + 'static` on the receiver, and `Send` on each future. Implementation imports, type aliases, qualified paths, macros, helpers, and private code otherwise remain ordinary Rust. Rustc mismatch prose is not a byte-stable Boxology diagnostic promise.

The generated outputs include the material required by the selected bindings, including:

- Contract-safe Rust input, output, and error types.
- Stable capability identities and metadata.
- Typed caller handles.
- An implementation-neutral server-side dispatch interface.
- A language-neutral schema used for compatibility analysis and non-Rust bindings.
- Programmable contract-level test bindings so consumers do not hand-maintain mocks of generated APIs.

The contract crate defines only the implementation-neutral dispatch side. It never imports, names, or reaches into the implementation crate. The generator separately emits adapter code inside the implementation crate that implements the generated dispatch interface and invokes the checked implementation methods. The composition connects that adapter to the typed handles. Whether the generated interface is internally represented by a trait, function table, or another erased mechanism remains an implementation detail.

The generated crate is checked into Git as a declared derived artifact so a clone remains an ordinary Cargo workspace with useful editor behavior and reviewable contract diffs. It is never hand-edited. The box's `boxology.toml` records the generator identity, semantic inputs, outputs, and regeneration command. CI uses the generator resolved by the protected workspace toolchain to recreate every submitted generated output byte-for-byte under the ownership rules in [Packages, Providers, and Compositions](02-packages.md#ownership-and-derived-artifact-enforcement).

Byte-for-byte stability is an acceptance requirement for the generator, not an assumed property. Generation must use stable ordering, omit timestamps, randomness, absolute host paths, and other machine-specific data, normalize paths and line endings, and use workspace-resolved formatting and tooling. The same source and workspace toolchain must produce identical output on every supported development and CI platform.

The same generator supplies programmable test bindings from the contract definition. They are emitted inside the generated contract crate behind a `test-support` feature (decided by the S1/S2 stream specs), and they are derived outputs rather than consumer-maintained mocks.

## Workspace generator lifecycle

A workspace has one current generator supplied by its platform toolchain. Boxes do not select or pin independent generator versions. The workspace resolves one exact tool version for a generation run—through its Cargo lockfile, platform tool manifest, or an equivalent deterministic installation mechanism—so local development and CI do not accidentally use different releases.

Every generated artifact records machine-readable generator provenance; generated Rust source can also carry a header such as:

```rust
// Generated by boxology-contract-generator 1.3.0.
// Do not edit manually.
```

Updating the workspace generator does not mass-regenerate every box. Existing generated crates remain valid and continue to compile. When a box's declared contract-generation inputs next change, or its owner explicitly requests regeneration, CI uses the current workspace generator and requires the checked-in output to match it. That lazy regeneration replaces the box's derived output and updates the provenance. A workspace can therefore contain artifacts produced by several historical generator releases while installing and running only the current generator.

Generator releases must be backward-compatible:

- Previously valid contract blocks remain accepted.
- Unchanged source produces a semantically equivalent contract even when the generated representation improves.
- Existing generated crates remain compatible with the current runtime and workspace tooling.

Compatibility analysis distinguishes a representational regeneration diff from a semantic contract change. If a new generator turns unchanged source into an incompatible contract or makes an older generated crate unusable, that release is defective and must not become the workspace generator. A breaking generator upgrade is not a normal box migration path.

## Contract evolution without mandatory public versions

The contract crate represents the complete contract surface the box currently supports. The platform does not require separate crates, namespaces, or traits named `v1`, `v2`, or similar.

Every capability needs a stable logical identity, and every generated contract state needs a revision or fingerprint so the platform can compare it with its base revision. These internal revision identities support analysis; they do not force a public versioning scheme.

The normal path is compatible expansion followed by managed contraction. For example:

```text
add an optional field
-> migrate managed consumers to populate it
-> verify adoption and applicable deployment evidence
-> make the field required
```

Or:

```text
mark a field deprecated
-> migrate managed consumers away from it
-> verify that applicable usage has drained
-> remove the field
```

The final tightening or removal can be incompatible with an old consumer. It becomes an authorized step only after the configured migration and deprecation policy is satisfied. The generator always identifies and describes the contract change; the harness decides whether its evidence is sufficient to merge.

Explicit parallel versions remain available when a box needs long-lived incompatible surfaces, especially for public or unmanaged consumers. A box can expose those surfaces from the same generated contract crate or choose a more elaborate packaging strategy. Parallel `v1` and `v2` artifacts are a product and harness choice, not a runtime invariant.

## Dependency and dispatch model

The common ownership manifest remains the authoritative dependency record. An implementation declares the exact foreign capabilities it imports. Generated typed handles expose only those imports.

The contract-owning box supplies the canonical handle type through its generated contract crate. Consumers do not generate independent copies of that public Rust API. A consumer may receive a generated `Imports` structure that contains only the handles declared by its manifest.

Box code is statically typed against those handles. The application composition owns binding: it selects implementations, creates the permitted handles, and connects each handle to an in-process target or a remote transport. Boxes do not perform ambient string-based lookup through a global service locator.

The box-facing API therefore remains stable across placement choices:

```text
typed capability handle
-> composition-selected in-process binding
or
-> composition-selected remote binding
```

The composition must reject missing, duplicate, or incompatible bindings before accepting traffic. The exact erased-dispatch mechanism remains runtime implementation work. Local and remote bindings share the asynchronous, fallible handle contract defined in [Canonical Capability Contract](09-capability-contract.md), while discovery, routing, placement, and overload remain separate topology work.

## Merge-blocking build-graph rules

Workspace CI classifies Cargo crates by their logical owner and role, using manifests rather than naming conventions. It compares the complete Cargo graph with declared package and contract dependencies.

The minimum edge policy is:

| From | To | Policy |
| --- | --- | --- |
| Box contract | Any box implementation | Forbidden |
| Box implementation | Its own generated contract | Allowed |
| Box implementation | Declared foreign contract | Allowed |
| Box implementation | Foreign implementation | Forbidden |
| Application composition | Selected box implementations | Allowed |
| Application composition | Selected provider crates | Allowed |
| Any box crate | Undeclared foreign contract | Forbidden |

The check covers normal, build, development, renamed, optional, feature-activated, and target-specific edges. A build script or generated source cannot be used to conceal a forbidden dependency.

Box-owned tests follow the same boundary and use the generator's programmable contract-level test bindings for foreign capabilities. Tests that deliberately link several real implementations belong to an application composition and its integration-quality contract.

Provider and platform crates have their own declared roles. A composition may depend on an
explicitly selected provider crate; unselected provider edges and other cross-kind provider edges
fail closed. Provider and platform crates cannot be used as passthroughs that expose a foreign box
implementation to ordinary box code.

## Build cycles and invocation cycles

Contract crates never depend on implementation crates. Contract-to-contract edges, when permitted by the declared contract model, must also remain acyclic because they are part of the Rust build graph.

If two contracts would otherwise refer to one another's types, the box designers must break the build cycle. They can use boundary-local data-transfer types and explicit translation, or extract a genuinely shared concept into a separately owned contract box whose own edges remain acyclic. The platform does not prescribe one universal choice, but it never disguises a contract cycle as a valid Cargo graph.

The split permits two implementations to import one another's contracts without creating a Cargo cycle:

```text
billing-implementation  -> customer-contract
customer-implementation -> billing-contract
composition             -> both implementations
```

This does not make the live-invocation cycle automatically desirable. The graph-specific policy in [Boxes](01-boxes.md#dependency-cycles) still blocks a newly introduced live cycle by default and requires an explicit architectural exception with runtime safeguards. The build topology merely keeps Cargo from conflating an approved runtime relationship with an impossible build relationship.

## Workspace operations and validation baseline

Boxology exposes two foundation operations:

```text
boxology generate [--package <id>]
boxology check [--base <git-revision>] [--format human|json]
```

`boxology generate` is the explicit mutating operation. Without selection it regenerates only packages whose declared generation inputs changed; `--package` also permits an owner to request regeneration of one package with the current workspace generator. It rewrites declared derived outputs, updates generator provenance, and reports the semantic contract classification. It never edits handwritten source.

When invoked from a partially managed monorepo, an explicit package selection resolves one unique
descendant managed Cargo workspace and performs discovery, generation, and validation at that
boundary. Unmanaged siblings are not adopted implicitly; ambiguous matches fail closed.

Generation treats an absent declared box-contract crate as an incomplete derived workspace, not
as a reason to make every other package unmaintainable. A selected package may regenerate from the
validated manifest-owned source model while another declared contract member is still absent;
Cargo-backed validation becomes mandatory again once all declared contract manifests exist.

`boxology check` is the canonical non-mutating validation command used by developers, the lead, and generated CI. V1 always validates the complete workspace; package-scoped and impact-selected validation are later optimizations. It regenerates into temporary output and compares byte-for-byte rather than modifying the checkout. If `--base` is supplied, contract and ownership changes are classified against that revision. Local use defaults to the merge base with the configured main branch; CI passes the pull request's base revision explicitly.

The command exits `0` when every check passes, `1` when repository validation fails, and `2` when invocation or configuration prevents validation from running. Human output is the default. `--format json` emits one JSON document whose top-level `schema` field identifies the diagnostic format version, followed by check identifiers, package identities, paths, diagnostics, contract classifications, and the final status.

The foundation `boxology check` baseline is:

1. Discover and validate every `boxology.toml`, ownership classification, package identity, Cargo crate role, declared import, and derived-output declaration.
2. Recreate required generated contracts and schemas byte-for-byte with the workspace generator while leaving untouched historical artifacts on their recorded compatible generator provenance.
3. Classify contract changes against the base revision and report incompatible tightening or removal even when harness policy later authorizes it.
4. Validate the complete Cargo graph, forbidden implementation edges, feature and target-specific edges, and shared-lockfile rules.
5. Run `cargo fmt --check` over the hand-authored packages by explicit selection; declared generated Rust is excluded because its pinned generator printer is authoritative. Generated contract items carry stable outer `#[rustfmt::skip]` attributes so an incidental workspace-wide format remains a byte-for-byte no-op rather than creating regeneration drift.
6. Run `cargo clippy --workspace --all-targets --all-features -- -D warnings` with the workspace's pinned Rust toolchain; changing that pin is a deliberate platform-package change.
7. Run `cargo test --workspace --all-features`.
8. Run each package's declared `[quality].commands`. The generated Hello project declares its in-process Rust and HTTP conformance tests there, including the accepted HTTP wire contract; the checker contains no Hello-specific branch.

An intentional contract change is authored in Rust and followed by `boxology generate`; its generated diff and semantic classification are reviewed together. An accidental stale or hand-edited artifact fails `boxology check` with the regeneration command needed to repair it. The checker never hides an incompatible change merely because generated files match.

The initializer emits golden-pinned repository-owned GitHub Actions workflow bytes for
`ubuntu-latest` and `boxology check --base <pull-request-base>`. This framework repository also
validates its own canonical command on hosted Ubuntu CI.
Developers and the lead can run `boxology check` through their ordinary workflow. The platform
does not create branch-protection or required-check settings; the operator decides whether to make
a visible check a merge requirement. There is no hidden factory-only validation layer.

Package quality commands are trusted repository code and run with the sandbox's full ambient access, as defined by the [foundation threat boundary](06-quality-and-authority.md#foundation-lead-sandbox-threat-boundary).

Hosted CI currently validates Linux, while the V0 evidence corpus was established on native macOS
ARM64. A chosen lead harness may run anywhere it supports, but Boxology makes no general
cross-platform equivalence claim. Generator outputs remain required to be platform-independent;
wider comparison is [#525](https://github.com/fontanierh/boxology/issues/525) scope.

## Foundation milestone

The generated Hello World project exercises the complete minimal topology:

1. One controlled contract block declares a unary capability and one checked inherent implementation supplies it.
2. The generator produces its contract crate, schema, typed handle, implementation-neutral dispatch interface, and implementation-local adapter.
3. The composition invokes the capability through an in-process Rust binding.
4. The same composition exposes the capability through HTTP from the same extracted contract.
5. `boxology check` runs deterministic regeneration, Cargo-edge policy, the Rust baseline, and Rust/HTTP behavior as visible repository validation.

This proves the boundary without requiring persistence, streaming, public version numbers, or a finalized remote execution model.

## Policy boundary

The platform always provides deterministic extraction, change classification, dependency evidence, and reproducible outputs. The harness owns the policy that turns those findings into warnings, reviews, approvals, or merge blocks.

The default factory should require backward-compatible changes or a completed, evidenced deprecation process. A team may configure a different risk posture, but a policy override or downgrade must be explicit and auditable. Disabling a gate removes the corresponding platform guarantee; it does not make an incompatible change compatible.

## Matters not yet specified

This topology does not settle:

- Detailed streaming, replay, and real-time behavior beyond the interaction shapes reserved by the contract.
- Discovery, placement, routing, lifecycle, and overload semantics.
- The internal erased-dispatch or routing implementation.
- Public artifact publication and support-window policy for unmanaged consumers.

Those questions remain focused work for the capability-contract, runtime-topology, compatibility, and registry designs.
