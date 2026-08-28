# Quality and Authority

[Back to the white paper](../boxology-whitepaper.md)

This document expands the quality ownership, CI, review, policy, and human-authority model discussed during the design interview.

Status: this document combines current principles with target mature-harness enforcement. Today,
declared package quality is reporting and guidance unless an existing repository gate explicitly
enforces it; later sections call out that boundary.

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

The foundation treats the environment in which the selected lead harness runs like an ordinary coding-agent workstation. Whatever isolation that harness or its operator supplies is the only hard containment boundary available; Boxology supplies no sandbox, nested credential-free executor, or additional containment for repository-controlled builds, tests, scripts, dependencies, or generated code.

Inside that boundary:

- The managed repository, including its code, project instructions, issues, pull requests, reviews, and comments, is trusted and may intentionally steer the lead. Boxology does not classify those instructions by author, require a private repository, align repository authors with a separate user allowlist, or mechanically distinguish an outside contributor. The lead interprets repository context using the same judgment it normally applies while working in code.
- Third-party dependency source, build output, tool output, and unrelated external material are data rather than authoritative project instructions. The coding agent is responsible for interpreting them accordingly; v1 adds no mechanical prompt-injection classifier.
- The lead, selected harness and gateway, and executed repository code share every filesystem path, environment variable, process, credential, and network capability the operator makes available. Running `cargo test`, a package quality command, or a build script therefore has the same ambient access as running it directly from that coding agent.
- Full outbound networking is available. V1 does not provide an egress allowlist or proxy.

Boxology neither grants nor withholds host files, unrelated volumes, container control, or infrastructure administration. If the chosen harness receives them, they are inside the effective trusted boundary.

The shipped guidance recommends least privilege but accepts the credentials the operator supplies and does not validate or reject the operator's security posture. For a GitHub-managed project, the practical recommended setup is single-repository access including contents, pull requests, and GitHub Actions workflow read/write so the lead can create, modify, and run repository CI. Operators may deliberately grant either narrower or broader authority. V1 also provides no mechanical secret redaction, data-loss prevention, credential broker, or just-in-time credential substitution. Safe use of available credentials is the agent's responsibility.

Boxology releases and dependencies use ordinary GitHub and package-registry trust. V1 does not add a signing or attestation system.

Compromise response is human-operated: stop the selected harness or destroy its environment, revoke supplied credentials, inspect affected external systems, and rebuild from trusted software and repository state. The MVP makes no claim that it can contain a malicious build script or prevent code in the lead environment from exfiltrating an available credential, so it does not add acceptance tests that pretend otherwise.

Mediated egress, just-in-time credentials, mechanical redaction, automated containment, stronger
release verification, and multi-agent role security are future concepts outside this framework's
current threat boundary.

## Protecting quality policy

An implementation agent may need to change tests or CI, but allowing it to silently weaken the checks that judge its own work would undermine the merge guarantee.

The agreed default guidance was:

> CI and quality-policy files are protected control-plane artifacts. Changing them requires human approval by default.

In the foundation, "protected" means that Boxology identifies these artifacts and the shipped
skill tells the lead to flag their changes for human review. With `--base`, `boxology check` reads
base and validated submitted manifests plus base schemas. Existing exact paths retain base
ownership authority; introduced paths use submitted ownership authority, and contract differences
remain classified against the base schema. This is reporting, not execution of immutable base
policy. The candidate owns the
checker and workflow, and Boxology ships no merger replay, branch protection, enforced approval,
or separation of duties. Operators can add required checks, branch protection, review ownership,
or harness policy when they want a stronger boundary. Semantic self-protection remains
[#17](https://github.com/fontanierh/boxology/issues/17). The lead is still permitted to create,
modify, and run GitHub Actions when its supplied credentials allow it.

Teams may deliberately choose a more permissive or highly autonomous policy. The platform should make the safer configuration easy and provide consistent defaults, but it should not impose one universal risk posture on every user.

Even in a permissive configuration, a policy downgrade should be explicit and auditable rather than an unnoticed side effect of an implementation task.

## Top-level authority

The coding agent using the Boxology skill is the lead agent. It receives authoritative user
guidance through whatever interface its selected harness or gateway provides. Boxology defines no
communication channel, user allowlist, role system, or human-identity protocol in v0; access
control and transport security belong to the harness and its operator. Any instruction that
harness presents as an authorized user message is authoritative.

The lead uses every capability supplied to it and follows its system prompt, project instructions, and agent judgment. The skill can ask it to explain sensitive actions and seek human review, but v0 has no structured approval protocol or platform-enforced authority ceiling.

## Deprecation authority

Removing a deprecated interface is another policy-governed action. The deprecation agent collects dependency and monitoring evidence, while the harness applies the configured removal rules.

For public or risky endpoints, a human can provide the final authorization to remove the old interface. The interview did not define a universal threshold at which human approval becomes mandatory.

## Continuous quality and coherence

The perpetual quality agent complements pull-request CI by looking for problems across time and across box boundaries. It can surface aging deprecations, cycles, stalled migrations, and broader architectural drift.

Its findings create tasks or human questions. It does not silently rewrite unrelated boxes as part of one change.

## Matters not yet specified

The discussion did not settle:

- The exact policy-file ownership and approval mechanism.
- Risk tiers for automated versus human-approved merges.
- How AI reviewer independence and disagreement are handled.
- How evidence and approvals are represented in merge records.
- Whether changing tests and implementation in one pull request needs special treatment beyond protected quality files.
- Formal roles, capabilities, and hard limits beyond the permissive skill-only foundation.
