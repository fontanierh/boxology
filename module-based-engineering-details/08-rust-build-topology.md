# Rust Build Topology

[Back to the white paper](../module-based-engineering-whitepaper.md)

This document defines how a native module maps onto Rust and Cargo. It separates the module's ownership boundary from its compilation units, establishes the generated contract boundary, and records the minimum build-graph enforcement required by the platform.

## One module, two compilation units

A module is one logical package and one pull-request owner. It owns two Cargo crates:

- A handwritten **implementation crate** containing its behavior and internal organization.
- A mechanically generated **contract crate** containing the Rust surface that consumers may compile against.

For example:

```text
modules/customer/
  module manifest
  implementation/       # handwritten customer implementation crate
  generated/contract/   # generated customer contract crate
```

The exact directory and crate names are tooling choices. The semantic distinction is not: another module may compile against the contract crate but never against the implementation crate.

This does not weaken the single-owner rule. Both crates belong to the same module package, and a module-owned pull request may change its implementation inputs together with deterministic contract outputs attributable to them.

## Rust-first contract authoring

Developers and agents do not maintain a second API definition or an exported service trait. The authoring source is ordinary Rust implementation code. An annotation marks an implementation method as an exported capability and supplies the metadata that cannot be inferred from its Rust signature.

Conceptually:

```rust
impl CustomerService {
    #[capability(id = "customer.get")]
    #[exposure("internal")]
    pub async fn get_customer(
        &self,
        input: GetCustomer,
    ) -> Result<Customer, GetCustomerError> {
        // business logic
    }
}
```

The exact annotation syntax remains part of the capability-contract design. No trait, file layout, or internal service pattern is required merely because a method is exported. Unannotated methods remain ordinary internal Rust code.

The necessary restriction is at the boundary: inputs, outputs, errors, and other values crossing an annotated method must be representable by the contract type system. The generator rejects a boundary it cannot express faithfully. Internal implementation types and organization remain unconstrained.

## Generation before Cargo

Cargo determines its package graph before compiling a procedural macro, so annotation expansion alone cannot create a sibling contract crate in the same build. The platform therefore owns a deterministic generation step before Cargo runs:

```text
annotated implementation methods
-> platform contract generator
-> generated contract crate and schema
-> Cargo build
```

The generated outputs include the material required by the selected bindings, including:

- Contract-safe Rust input, output, and error types.
- Stable capability identities and metadata.
- Typed caller handles.
- The server-side dispatch surface and module-local adapter glue needed to reach the annotated implementation.
- A language-neutral schema used for compatibility analysis and non-Rust bindings.

The generated crate is checked into Git as a declared derived artifact so a clone remains an ordinary Cargo workspace with useful editor behavior and reviewable contract diffs. It is never hand-edited. The module manifest records its generator, inputs, pins, and outputs, and CI regenerates it byte-for-byte under the ownership rules in [Packages, Providers, and Compositions](02-packages.md#ownership-and-derived-artifact-enforcement).

The generator may also produce derived adapter code inside the implementation crate. All such output remains attributable to the same module and must satisfy the same reproducibility rule.

## Contract evolution without mandatory public versions

The contract crate represents the complete contract surface the module currently supports. The platform does not require separate crates, namespaces, or traits named `v1`, `v2`, or similar.

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

Explicit parallel versions remain available when a module needs long-lived incompatible surfaces, especially for public or unmanaged consumers. A module can expose those surfaces from the same generated contract crate or choose a more elaborate packaging strategy. Parallel `v1` and `v2` artifacts are a product and harness choice, not a runtime invariant.

## Dependency and dispatch model

The common ownership manifest remains the authoritative dependency record. An implementation declares the exact foreign capabilities it imports. Generated typed handles expose only those imports.

The contract-owning module supplies the canonical handle type through its generated contract crate. Consumers do not generate independent copies of that public Rust API. A consumer may receive a generated `Imports` structure that contains only the handles declared by its manifest.

Module code is statically typed against those handles. The application composition owns binding: it selects implementations, creates the permitted handles, and connects each handle to an in-process target or a remote transport. Modules do not perform ambient string-based lookup through a global service locator.

The module-facing API therefore remains stable across placement choices:

```text
typed capability handle
-> composition-selected in-process binding
or
-> composition-selected remote binding
```

The composition must reject missing, duplicate, or incompatible bindings before accepting traffic. The exact erased-dispatch mechanism and local-versus-remote failure semantics remain separate runtime implementation work.

## Merge-blocking build-graph rules

Workspace CI classifies Cargo crates by their logical owner and role, using manifests rather than naming conventions. It compares the complete Cargo graph with declared package and contract dependencies.

The minimum edge policy is:

| From | To | Policy |
| --- | --- | --- |
| Module contract | Any module implementation | Forbidden |
| Module implementation | Its own generated contract | Allowed |
| Module implementation | Declared foreign contract | Allowed |
| Module implementation | Foreign implementation | Forbidden |
| Application composition | Selected module implementations | Allowed |
| Any module crate | Undeclared foreign contract | Forbidden |

The check covers normal, build, development, renamed, optional, feature-activated, and target-specific edges. A build script or generated source cannot be used to conceal a forbidden dependency.

Module-owned tests follow the same boundary and use contract-level test bindings for foreign capabilities. Tests that deliberately link several real implementations belong to an application composition and its integration-quality contract.

Providers and platform crates have their own declared roles, but they cannot be used as passthroughs that expose a foreign module implementation to ordinary module code.

## Build cycles and invocation cycles

Contract crates never depend on implementation crates. Contract-to-contract edges, when permitted by the declared contract model, must also remain acyclic because they are part of the Rust build graph.

The split permits two implementations to import one another's contracts without creating a Cargo cycle:

```text
billing-implementation  -> customer-contract
customer-implementation -> billing-contract
composition             -> both implementations
```

This does not make the live-invocation cycle automatically desirable. The graph-specific policy in [Modules](01-modules.md#dependency-cycles) still blocks a newly introduced live cycle by default and requires an explicit architectural exception with runtime safeguards. The build topology merely keeps Cargo from conflating an approved runtime relationship with an impossible build relationship.

## Foundation milestone

The generated Hello World project exercises the complete minimal topology:

1. One implementation method is annotated as a unary capability.
2. The generator produces its contract crate, schema, typed handle, and dispatch glue.
3. The composition invokes the capability through an in-process Rust binding.
4. The same composition exposes the capability through HTTP from the same extracted contract.
5. Regeneration and Cargo-edge checks run as mandatory platform validation.

This proves the boundary without requiring persistence, streaming, public version numbers, or a finalized remote execution model.

## Policy boundary

The platform always provides deterministic extraction, change classification, dependency evidence, and reproducible outputs. The harness owns the policy that turns those findings into warnings, reviews, approvals, or merge blocks.

The default factory should require backward-compatible changes or a completed, evidenced deprecation process. A team may configure a different risk posture, but a policy override or downgrade must be explicit and auditable. Disabling a gate removes the corresponding platform guarantee; it does not make an incompatible change compatible.

## Matters not yet specified

This topology does not settle:

- The exact annotation syntax or schema-safe Rust type subset.
- How boundary types are lifted into the generated crate and referenced by the implementation.
- The exact language-neutral schema and compatibility taxonomy.
- Local and remote failure, deadline, cancellation, retry, discovery, and overload semantics.
- The internal erased-dispatch or routing implementation.
- Public artifact publication and support-window policy for unmanaged consumers.

Those questions remain focused work for the capability-contract, runtime-topology, compatibility, and registry designs.
