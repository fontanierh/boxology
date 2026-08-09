# S7 foundation acceptance runbook

> **Status:** the clean S7 gate already passed in the
> [2026-08-03 acceptance record](../records/2026-08-03-foundation-acceptance-clean.md), and V0
> completed on 2026-08-09 with [shipped evidence](../records/2026-08-09-v0-completion-evidence.md).
> This runbook remains the repeatable evidence protocol; it does not reopen either milestone.
> Current execution is separate in the [post-V0 roadmap](../boxology-details/12-post-v0-self-hosting-roadmap.md).

This runbook preserves the behavioral evidence protocol required by [S7 D2–D3](../specs/s7-skill-acceptance-self-hosting.md#d2--the-skills-acceptance-contract-is-behavioral) and the [foundation milestone](../boxology-details/07-product-contract.md#first-end-to-end-foundation-milestone). It is not extra Boxology guidance for the lead agent. Every replay, including a failed or intervened run, produces its own dated record under [`records/`](../records/README.md).

## Roles and success rule

- One coding agent able to load the checked-in skill is the **lead agent** for the entire run. Start one fresh session, disable delegation and subagents, and keep that same session through initialization and evolution.
- The maintainer is the **developer role**. The developer sends only the exact asks and permitted answers below.
- The operator may prepare the empty repository, create evidence commits, and rerun commands after the lead stops. Operator observations stay out of the lead's context.
- The checked-in [Boxology onboarding skill](../.agents/skills/boxology/SKILL.md) is the lead's only Boxology-specific guidance. Do not supply this repository's instructions, specs, tests, issues, history, or implementation as guidance, and do not edit the skill during the run.
- A replay is clean only when every success check in this document passes with no intervention. Replays with other agents are optional additional evidence and do not replace the original clean record.

## Pin the run

Use a clean Boxology source checkout at the exact candidate commit. Its S4 classifier, S5 `boxology check`, S6 installer, generated README, and generated composition must already be complete. Choose a target and evidence directory outside that checkout. The target must contain only `.git`; the evidence directory must not be inside the target.

Do not start merely because those components compile. The candidate commit must contain merged positive evidence that a second externally exposed capability is routed through the generated in-process and HTTP composition without an authored change to the application-composition package. A composition that selects only the first descriptor capability, or a generated manifest that binds only `ping.ping`, is a **NO-GO**: it would force the acceptance lead to change foreign package source. Likewise, `boxology check --base` must execute S4 classification and report findings; a build that skips base-revision classification is a NO-GO. Repair these product defects before starting a run, never during one.

Record the literal absolute paths in the run notes, then use these variable names in the operator shell:

```sh
RUN_SOURCE=/absolute/path/to/boxology-source
RUN_TARGET=/absolute/path/to/empty/hello-v0
RUN_EVIDENCE=/absolute/path/to/evidence
```

Prepare and capture the precondition before opening the lead session:

```sh
mkdir -p "$RUN_TARGET" "$RUN_EVIDENCE"
git -C "$RUN_TARGET" init -b main
git -C "$RUN_SOURCE" status --porcelain=v1 >"$RUN_EVIDENCE/source-status.before"
git -C "$RUN_SOURCE" rev-parse HEAD >"$RUN_EVIDENCE/source-commit"
shasum -a 256 "$RUN_SOURCE/.agents/skills/boxology/SKILL.md" >"$RUN_EVIDENCE/skill.before.sha256"
find "$RUN_TARGET" -mindepth 1 -maxdepth 1 -not -name .git -print >"$RUN_EVIDENCE/target-unexpected.before"
git -C "$RUN_TARGET" status --short --branch >"$RUN_EVIDENCE/target-status.before"
```

`source-status.before` and `target-unexpected.before` must be empty. Preserve the complete lead transcript using the host's ordinary export facility; note the host and export method in the record. Do not place transcript tooling or evidence files in the target repository.

The evidence map for the seven milestone steps is fixed:

1. the empty-target capture and first ask prove greenfield activation;
2. the initializer output and baseline tree prove installation;
3. the baseline Rust/HTTP test and baseline check prove both bindings and the first valid state;
4. the transcript must show the skill explaining the box model and identifying the coding agent as lead;
5. the second ask is the exact backward-compatible `greet(name)` request;
6. the changed-path audit and two `Hello, Ada!` transcripts prove the ownership and binding result; and
7. the final base-relative check proves the same visible validation path and additive classification.

## Exact developer-role asks

Replace the angle-bracket values only. Send the first ask as one message:

> Use the Boxology onboarding skill to initialize a greenfield managed project named `hello-v0` directly in `<RUN_TARGET>`, using `<RUN_SOURCE>` as the Boxology source checkout. The target contains only `.git`. Explain the Boxology model and your lead-agent role before making changes. The skill is your only Boxology-specific guidance: use the source checkout only for the skill-directed tool installation and as the initializer's dependency source; do not consult its repository instructions, documentation, specs, tests, issues, history, or implementation. Continue as this project's lead agent until the generated project's documented build, Rust-and-HTTP invocation, and `boxology check` all pass. Do not create commits. Tell me when that baseline is complete, including the commands you ran and their results.

After the lead reports the baseline complete, do not discuss its implementation. Perform the baseline evidence boundary below. Then send this second ask as one message:

> Add a backward-compatible `greet(name)` capability to the generated Hello box, whose package id is `ping`. Preserve the existing `ping` capability. Calling `greet("Ada")` must return `Hello, Ada!` through both the in-process Rust binding and HTTP. The resulting repository must change no foreign package source and may contain outside the `ping` box only permitted deterministic artifacts attributable to that box. Regenerate deterministic output and use the project's normal visible validation path. Do not create commits. Tell me when the change is complete, including the exact commands and outputs that demonstrate both `Hello, Ada!` results and the final check.

## Permitted answers

The developer may answer only questions anticipated by the onboarding flow. Use these literal forms with the pinned values substituted:

- `The project name is hello-v0.`
- `The target root is <RUN_TARGET>.`
- `The Boxology source checkout is <RUN_SOURCE>.`
- `Yes. The target contains only .git.`
- `Use the normal choice for this host. Keep one lead agent and do not create commits.`
- `I cannot provide implementation guidance. Proceed from the skill and the generated repository, or report that you are blocked.`

Approving an ordinary host permission prompt is permitted when it grants only the filesystem/process access already required by the ask. Record the prompt and approval. A request for a new external service, credential, destructive action, or broader authority ends the run as failed unless it can be declined without affecting the scenario.

## Prohibited coaching and intervention

An intervention is any developer utterance that supplies implementation *how*: a file or symbol to edit, a command or flag to run, a code shape, a procedure, a diagnosis, an interpretation of an error, or repository-internal knowledge. The following also make the run intervened:

- supplying any Boxology guidance beyond the pinned skill;
- editing or replacing the skill after the run starts;
- switching leads, delegating implementation, or adding a reviewer/repair agent;
- correcting the lead's tool installation, generation, binding, classification, or validation approach;
- telling the lead how to avoid a foreign-package change or how to obtain an additive classification;
- modifying target files for the lead.

If the lead asks for prohibited help, send the final permitted answer above once. If it remains blocked, stop. Do not rescue the run and later call it clean.

## Baseline evidence boundary

After the first ask finishes, close or pause the lead session while the operator captures evidence. The generated README's commands must be exactly reproducible; at the current contract they are:

```sh
set -o pipefail
(
  set -e
  cd "$RUN_TARGET"
  cargo build --workspace
  cargo test -p ping-app assembled_ping_answers_in_process_and_over_real_http
  boxology check
) 2>&1 | tee "$RUN_EVIDENCE/baseline-validation.log"
git -C "$RUN_TARGET" status --short >"$RUN_EVIDENCE/baseline-status.precommit"
git -C "$RUN_TARGET" ls-files --others --cached --exclude-standard | LC_ALL=C sort >"$RUN_EVIDENCE/baseline-files"
```

The build must materialize `Cargo.lock`, the test must exercise both bindings, and the check must pass. No repair may occur during this evidence step. Commit the exact baseline so S4 has an immutable base revision:

```sh
git -C "$RUN_TARGET" add --all
git -C "$RUN_TARGET" -c user.name='Boxology Acceptance' -c user.email='acceptance@boxology.invalid' -c commit.gpgsign=false commit -m 'Initialize Boxology acceptance project'
git -C "$RUN_TARGET" rev-parse HEAD >"$RUN_EVIDENCE/baseline-commit"
git -C "$RUN_TARGET" status --porcelain=v1 >"$RUN_EVIDENCE/baseline-status.after"
```

Resume the same lead session only after the baseline commit exists, then send the second ask.

## Evolution evidence boundary

When the lead reports completion, stop the session before verification. Copy its reported Rust and HTTP command lines into the run notes. Inspect each command as plain text, then run it directly; never execute agent-provided text through `eval` or an equivalent shell expansion. Capture the complete command lines and outputs as `greet-rust.log` and `greet-http.log`. Each must unambiguously contain `Hello, Ada!` from its named binding.

Run each inspected command from `RUN_TARGET` in a separate shell with pipeline failure propagation:

```sh
set -o pipefail
(
  cd "$RUN_TARGET"
  <RUST_GREET_COMMAND>
) 2>&1 | tee "$RUN_EVIDENCE/greet-rust.log"
(
  cd "$RUN_TARGET"
  <HTTP_GREET_COMMAND>
) 2>&1 | tee "$RUN_EVIDENCE/greet-http.log"
```

Replace exactly one placeholder per subshell with the corresponding command; do not put a shell wrapper, command substitution, or `eval` around it.

Commit the lead's exact result, then capture the state and final visible check against the baseline:

```sh
git -C "$RUN_TARGET" status --short >"$RUN_EVIDENCE/evolved-status.precommit"
git -C "$RUN_TARGET" add --all
git -C "$RUN_TARGET" -c user.name='Boxology Acceptance' -c user.email='acceptance@boxology.invalid' -c commit.gpgsign=false commit -m 'Add additive greet capability'
git -C "$RUN_TARGET" rev-parse HEAD >"$RUN_EVIDENCE/evolved-commit"
RUN_BASELINE=$(cat "$RUN_EVIDENCE/baseline-commit")
RUN_EVOLVED=$(cat "$RUN_EVIDENCE/evolved-commit")
git -C "$RUN_TARGET" diff --name-status "$RUN_BASELINE" "$RUN_EVOLVED" >"$RUN_EVIDENCE/evolved-paths"
git -C "$RUN_TARGET" diff --stat "$RUN_BASELINE" "$RUN_EVOLVED" >"$RUN_EVIDENCE/evolved-stat"
set -o pipefail
(
  cd "$RUN_TARGET"
  boxology check --base "$RUN_BASELINE"
) 2>&1 | tee "$RUN_EVIDENCE/evolved-check.log"
git -C "$RUN_TARGET" status --porcelain=v1 >"$RUN_EVIDENCE/evolved-status.after"
shasum -a 256 "$RUN_SOURCE/.agents/skills/boxology/SKILL.md" >"$RUN_EVIDENCE/skill.after.sha256"
git -C "$RUN_SOURCE" status --porcelain=v1 >"$RUN_EVIDENCE/source-status.after"
```

The final check must pass and report the real capability addition as `additive`. Classification is report-only, so exit code zero alone is insufficient. Compare the two skill hashes byte-for-byte. Both repositories must be clean after capture.

Classify every line of `evolved-paths` against the baseline manifests. A clean run has authored changes only in the accountable `ping` package; every changed path outside it is a declared deterministic output attributable to `ping`. A source/configuration change in the root platform package, application composition, or any other package is foreign and fails the run. Do not create an acceptance-only exception.

## Recording every outcome

Create one new `records/YYYY-MM-DD-foundation-acceptance-<outcome>.md` file in the Boxology source repository and add it to the records index. Use `clean`, `failed`, or `intervened` in the slug. Keep excerpts trimmed so the record PR stays under the hand-authored line budget, while retaining the complete evidence bundle outside the managed project.

The record must cite:

1. date, maintainer/developer role, lead host/model, transcript-export method, and whether delegation was disabled;
2. Boxology source commit and unchanged skill hash;
3. empty-target proof, baseline commit, evolved commit, and both clean status captures, or the exact stage at which any item was not reached;
4. the exact two developer asks, any permitted answers, and every permission prompt;
5. baseline build, Rust/HTTP invocation, and `boxology check` command with trimmed output;
6. final Rust and HTTP commands with separate `Hello, Ada!` excerpts;
7. final `boxology check --base` excerpt showing pass and `additive` classification;
8. the complete changed-path list, its package/derived classification, and the explicit foreign-source conclusion;
9. every failure, intervention, retry, or operator action, including the first point at which the run ceased to be clean.

Validate the record PR with:

```sh
cargo xtask links
cargo xtask records
cargo xtask records --base origin/main
```

A failed or intervened replay record is evidence and remains historical. Revise any defective product artifact in a separate PR, start a new target and fresh lead session, and create a new run record. The original milestone remains closed by its clean record; later replays add evidence without reopening it.
