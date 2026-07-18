# Quality and Authority

[Back to the white paper](../boxology-whitepaper.md)

This document expands the quality ownership, CI, review, policy, and human-authority model discussed during the design interview.

## Separation of responsibility

Quality belongs to accountable packages and the software factory, not to the application runtime.

The runtime provides capability invocation, transport and authentication integration, and resolved provider bindings. It makes no promise that application behavior is correct or that a box's tests are sufficient.

Package owners and users are responsible for defining strong automated validation. The harness executes and enforces the configured validation before a change can merge.

## Package quality contract

Every box, provider, application composition, and platform package owns a quality contract appropriate to its behavior. The discussion did not prescribe one universal command or test framework, but it did establish that package CI should be good enough to check the accountable package automatically.

The contract can include:

- Compilation and static checks.
- Package-local tests.
- Contract and compatibility checks where applicable.
- Integration tests where applicable.
- Tests for provider-backed behavior.
- AI reviewers.
- Any specialized validation defined by the package owners.

The harness can add mandatory system-level checks beyond what an individual package declares.

Package kinds add different baselines. Boxes require contract compatibility and behavioral validation. Providers require conformance and isolation evidence. Compositions require assembly and integration validation. Platform packages require whole-workspace validation and stricter approval by default, subject to configured harness policy, because runtime, CI, build, generator, and enforcement changes can affect every package.

When a native box's declared contract-generation inputs change or its owner explicitly requests regeneration, deterministic platform validation regenerates its contract outputs with the current workspace generator and classifies the semantic contract diff. Untouched contract crates produced by older backward-compatible generator releases remain valid. The harness decides whether a compatible change, an evidenced contraction, or an explicit override may merge. The default factory should block an incompatible tightening or removal until its configured deprecation process is satisfied. A more permissive harness can change that policy, but it cannot suppress the underlying classification or claim that the change remained compatible.

The guarantee is procedural:

> The harness guarantees that the declared and mandatory evidence was produced successfully. It cannot guarantee that the evidence completely defines correctness.

This distinction avoids attributing application-quality guarantees to the runtime itself.

## AI review

AI reviewers can participate directly in CI. They can inspect implementation quality, contract compatibility, tests, architecture, and adherence to package rules.

The exact number, models, independence rules, and review rubrics were not selected in the interview. Those choices are harness policy rather than runtime behavior.

AI review does not replace deterministic checks. It is one category of evidence the merger can require.

## Integration evidence

Because agents work concurrently, CI must evaluate a candidate against the current integration state. A branch that passed when originally completed may fail after another pull request merges.

The merger therefore reruns the applicable checks against the latest main state. It returns failures to the durable task for repair.

An intervening change to the same package, an imported contract, or shared dependency resolution can also require area-lead reassessment even when the tests remain green. The quality process recognizes that automated checks may not capture every semantic change.

## Tracker reconciliation gate

Before any pull request merges, the merge process must reconcile its decisions with the issue tracker:

- Review every issue and review thread the pull request claims to address.
- Update any linked issue whose premise, questions, or remaining scope changed.
- Scan the other open issues for assumptions made stale by the same merged decisions and update or close them when the result is already settled.
- Close an issue only when all of its required work is resolved, or when it is explicitly superseded and every unresolved part has been transferred to named open issues. Otherwise, keep it open and record the exact remaining scope.

The same reconciliation is mandatory before closing an issue even when no pull request performs the closure. The recorded result must distinguish decisions established by merged documentation or implementation from reviewer proposals that remain undecided. Reconciliation must not silently promote an unaccepted suggestion into project policy.

## Provider conformance

Provider packages are trusted infrastructure components and need their own quality contracts; they are distinct from the platform package kind.

Provider conformance tests should attempt cross-binding access and validate the credentials, roles, and default privileges required by the provider's part of a claimed isolation profile. Composition and deployment validation must separately inspect process identity, network and resource policy, sandboxing, and egress controls where the selected profile requires them. These checks are evidence about configured controls, not proof against every query, process, or deployment. Providers can also validate generated configuration, local-development setup, migrations, connectivity, and health conventions.

The runtime does not sandbox a defective provider into correctness. A provider that violates its stated behavior is considered broken.

Runtime isolation profiles govern behavior of code that reaches a deployment. Agent authorship, CI, credential, and supply-chain risks are separate control-plane threats. Review and protected checks are pre-merge defenses, not substitutes for runtime isolation; the foundation boundary for those threats is defined below.

## Foundation lead-sandbox threat boundary

The foundation treats the complete lead sandbox like a coding-agent workstation. The sandbox is the sole hard containment boundary; there is no nested credential-free executor for repository-controlled builds, tests, scripts, dependencies, or generated code.

Inside that boundary:

- The managed repository, including its code, project instructions, issues, pull requests, reviews, and comments, is trusted and may intentionally steer the lead.
- Third-party dependency source, build output, tool output, and unrelated external material are data rather than authoritative project instructions. The coding agent is responsible for interpreting them accordingly; v1 adds no mechanical prompt-injection classifier.
- The lead, harness, Slack integration, and executed repository code share the sandbox filesystem, environment variables, processes, supplied credentials, and network access. Running `cargo test` or a build script therefore has the same ambient access as running it directly from the coding agent.
- Full outbound networking is available. V1 does not provide an egress allowlist or proxy.

Outside that boundary, the sandbox receives no host filesystem, unrelated volumes, container-control socket, or infrastructure-administration access unless the operator deliberately grants it. Granting any of those expands the deployment's trusted boundary.

The installer recommends repository- and channel-scoped credentials and may warn when it can identify broader access, but it accepts the credentials the operator supplies. It does not validate or reject the operator's security posture. V1 also provides no mechanical secret redaction, data-loss prevention, credential broker, or just-in-time credential substitution. Safe use of available credentials is the agent's responsibility.

Factory releases and images use ordinary GitHub and registry trust. The installed release or image digest is recorded for reproducibility, but v1 does not add a signing or attestation system.

Compromise response is human-operated: stop or destroy the sandbox, revoke its credentials, inspect GitHub and Slack for effects, and rebuild from a trusted release and repository state. The MVP makes no claim that it can contain a malicious build script or prevent code inside the sandbox from exfiltrating an available credential, so it does not add acceptance tests that pretend otherwise.

Mediated egress, just-in-time credentials, mechanical redaction, automated containment, and stronger release verification are explicitly deferred to [issue #65](https://github.com/fontanierh/boxology/issues/65). Multi-agent and per-role security remains part of the later coordination milestone in [issue #57](https://github.com/fontanierh/boxology/issues/57).

## Protecting quality policy

An implementation agent may need to change tests or CI, but allowing it to silently weaken the checks that judge its own work would undermine the merge guarantee.

The agreed default was:

> CI and quality-policy files are protected control-plane artifacts. Changing them requires human approval by default.

GitHub checks, branch protection, and review ownership can provide the initial enforcement mechanism. The harness can integrate with those systems instead of immediately rebuilding them.

Teams may deliberately choose a more permissive or highly autonomous policy. The platform should make the safer configuration easy and provide consistent defaults, but it should not impose one universal risk posture on every user.

Even in a permissive configuration, a policy downgrade should be explicit and auditable rather than an unnoticed side effect of an implementation task.

## Top-level authority

Humans interact with the factory through a top-level lead agent. Humans can provide authoritative guidance, reorganize areas, reprioritize work, resolve ambiguity, and approve sensitive decisions.

The lead can surface structured approval requests when analysis or implementation reaches a decision requiring human authority. The desired interface should make the requested action, evidence, consequences, and scope clear.

The top-level lead controls the harness. That does not automatically mean it can bypass merge checks, change external systems, or perform other sensitive actions without the approval required by policy.

Human guidance and approvals should be recorded so later agents can distinguish authoritative decisions from ordinary agent suggestions.

## Deprecation authority

Removing a deprecated interface is another policy-governed action. The deprecation agent collects dependency and monitoring evidence, while the harness applies the configured removal rules.

For public or risky endpoints, a human can provide the final authorization to remove the old interface. The interview did not define a universal threshold at which human approval becomes mandatory.

## Continuous quality and coherence

The perpetual quality agent complements pull-request CI by looking for problems across time and across box boundaries. It can surface aging deprecations, cycles, stalled migrations, and broader architectural drift.

Its findings create tasks or human questions. It does not silently rewrite unrelated boxes as part of one change.

## Matters not yet specified

The discussion did not settle:

- The required baseline quality checks shipped by the platform.
- The exact policy-file ownership and approval mechanism.
- Risk tiers for automated versus human-approved merges.
- How AI reviewer independence and disagreement are handled.
- How evidence and approvals are represented in merge records.
- Whether changing tests and implementation in one pull request needs special treatment beyond protected quality files.
- The exact capabilities and hard limits of the top-level lead.
