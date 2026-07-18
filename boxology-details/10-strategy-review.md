# Strategy Review: Thesis Risks and the Self-Hosting Gate

[Back to the white paper](../boxology-whitepaper.md)

This document records the strategic review held on 2026-07-18, immediately after the last MVP-spec blockers closed and before specification work began. The review was conducted with Fable 5 (Claude) and was explicitly requested as a brutally honest assessment: is the plan genuinely useful and novel, and does the philosophy hold? The maintainer accepted the assessment in full. This record preserves it so future decisions can be checked against it.

## What the review affirmed

- **The diagnosis is right.** Code is becoming cheap; safely coordinating many simultaneous changes is the bottleneck. Framing architecture as a context-budget and concurrency-control mechanism for agents — rather than an aesthetic preference — is the sharpest idea in the project, articulated before it was fashionable.
- **The design process is a genuine strength.** Falsifiable milestones, recorded anti-decisions, honest threat models, and willingness to de-scope when claims could not be backed are rare hygiene. Several artifacts are good engineering independent of the thesis: the contract-crate/implementation-crate lift, the generator as compatibility authority, the edge-policy table, `boxology check`.
- **The division of labor is plausible as an endgame:** humans own boundaries, contracts, types, and data models; agents implement and evolve what is inside.

## The risks the review recorded

**1. The de-scoping trajectory is evidence about the thesis.** Across the control-plane design, the sandbox-native foundation, the deployment recipe, and finally the skill-only pivot, the factory retreated every time it met reality. Each retreat was individually rational. Their sum is that what survived v0 is the part that was always buildable (a codegen platform) and the part that was always cheap (guidance documents). The project must treat this pattern as data, not coincidence, and must not let the later bootstrap rungs — factory-assisted changes, workers, a merger — keep dissolving.

**2. The novelty is concentrated in an untested thesis.** The components (declared package boundaries, contract diffing, merge queues, expand-migrate-contract) all exist elsewhere. The genuinely novel claim is that box discipline works as concurrency control for autonomous agents. A thesis is not a contribution until tested, and v0 does not test it. The safe-parallelism experiment is therefore the actual product; the platform is its instrumentation. Either experimental outcome is valuable — including the negative one.

**3. The v0 audience mismatch, and its resolution.** The safe-parallelism benefit appears only with many concurrent agents, while the v0 operator is a solo developer — the population that suffers the coordination problem least. The accepted resolution: v0 is not a market product. It is bootstrap instrumentation on the way to self-hosting, and its true user is this project. The long-term audience is the many-agent factory operator.

**4. The factory is probably the commodity; the substrate is the bet.** Every platform company will ship agent orchestration. What they will not ship is an opinion about how code must be structured to make parallel agents safe. Boxology's defensible asset is the box discipline plus its mechanical checks — if and only if evidence shows structured repositories outperform unstructured ones under the same orchestration.

**5. Justifications age differently, and positioning should anchor on the one that ages best.** "Agents need small blast radius to comprehend code" depreciates with every model generation. "Parallel writers need conflict control" is a systems property and ages well. Best of all: **human attention is the resource that does not scale.** Box-scoped pull requests with machine-classified contract diffs are a trust interface — they keep agent work reviewable and accountable per unit by the humans who remain responsible for it. That justification survives arbitrarily capable models because it is about oversight, not ability.

## Accepted implications

- **The many-agent gate must come uncomfortably early.** The commitment to build a real harness/gateway/factory system as boxes, dogfooded on this project's own work, before it feels justified, is recorded as a standing forcing function in [issue #74](https://github.com/fontanierh/boxology/issues/74). No calendar date; the trigger is sequencing pressure. Design remains owned by [issue #57](https://github.com/fontanierh/boxology/issues/57) and the experiment by [issue #34](https://github.com/fontanierh/boxology/issues/34).
- **The dogfooding pain discriminator is decided in advance.** During bootstrap, friction that is mechanical and automatable is the factory's future job — evidence for continuing. Friction that is semantic — fighting the box boundaries themselves — is thesis damage and must be recorded as such. Every relaxation of the discipline is data and is recorded with its category.
- **Positioning leads with oversight.** The white paper anchors on the human-attention justification alongside coordination.

## Self-hosting ladder

"Can Boxology self-host?" has a staged answer, and each stage is a real test:

1. **Process self-hosting (already practiced).** The tracker-reconciliation gate, single-owner documentation PRs, and recorded decisions apply the philosophy at the documentation layer. This is necessary but weak evidence: documents are cheap to keep consistent.
2. **Ownership and validation self-hosting (as soon as the tooling exists).** The product source repository adopts `boxology.toml` manifests, package kinds, and `boxology check` on itself. The platform's own crates are platform-kind packages rather than boxes, and the generator is validated against its own output using a pinned prior release — the standard compiler-bootstrap pattern. A broken generator must remain repairable without itself.
3. **Tool self-hosting (the stage that matters most).** Boxology's own tools become boxes: `boxology check` as a typed `check(workspace, base) -> Report` capability with the CLI as one binding and a future factory consuming the same handle; the contract generator as a box whose own contract crate is generated by the pinned previous release and revalidated by the current one — the compiler-bootstrap pattern, satisfying the repairable-without-itself rule; the installer as a small composition shipped as a standalone binary and the first real consumer of the generic CLI binding. Real tool APIs — paths, diagnostics, reports, mutating operations with side-effect metadata — are a far harsher stress test of the contract type subset than any Hello World. The box tax deliberately lands on the platform's own development first, producing the earliest mechanical-versus-semantic friction data under the discriminator above. The sequencing is honest: tools are built conventionally as platform packages for v0, because nothing can be boxed until a generator exists, then boxified as a deliberate early milestone — the first concrete rung of the issue #74 commitment.
4. **Application self-hosting.** Factory components — harness, gateway, coordination — are built as boxes with real contracts, providers, and compositions: the first test of the thesis on a non-toy application.
5. **Full self-hosting.** The factory manages the product source repository, including its own development, through the same box discipline it enforces for users.

One layer is definitionally excluded: the runtime core — `CallContext`, `ContractType`, handle and binding machinery — is the substrate boxes are made of and cannot sit behind a capability contract. It is the irreducible kernel every self-hosting system has, and it remains a platform-kind package. Everything above the runtime can be a box; the runtime is what "box" means.

Stages must not be skipped rhetorically: stages 3 through 5 produce the evidence, stage 3 produces it earliest and cheapest, and the pull toward lingering at stage 2 is exactly the failure mode issue #74 exists to prevent.
