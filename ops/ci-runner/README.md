# Boxology Mac-hosted CI runners

This is the operational runbook for the current CI host. **Four native Apple-silicon Mac JIT
runners are active.** The checked-in Linux/Colima implementation is dormant and is documented
separately below; it is not part of current health, capacity, or support claims.

## Active topology

One base supervisor and one three-child slot supervisor expose four runners with the generic
`[self-hosted, macOS, ARM64, boxology-macos-pr]` capability. The checked-in base configuration
also assigns `boxology-macos-pr-primary`. The two-step rollout below makes the required PR
workflow select it for stable warm-cache affinity while slots 2–4 remain generic capacity for
deep, advisory, smoke, and overflow work. Each JIT runner receives a fresh APFS clone and
disposable checkout. Each slot keeps its own Cargo target/cache outside the checkout and uses
`CARGO_BUILD_JOBS=4` and `RUST_TEST_THREADS=4`. Four slots are the host's concurrency ceiling;
adding slots fragments caches and raises thermal contention.

The runner is actions/runner `2.336.0` for Apple silicon, pinned to archive SHA-256
`8e8839c49b7060b6b2154f4931f815df330c27f167d53ef2239ee3dfce28b079`. Rust is pinned by the
repository to `1.97.1`; cargo-deny is pinned to `0.20.2`. The host macOS version is recorded in
`ImageVersion` evidence because the native host is not an immutable image. Any pin change must be
a dedicated reviewed change that updates the URL/version/checksum together and verifies the
official source; never add an unverified or tag-only fallback.

Native execution is not container isolation. It is accepted only for this private repository and
trusted Henry/agent collaborators.

## Credential boundary

Create the dedicated Keychain item interactively; the value must never enter the checkout, plist,
environment, command line, or a persistent plaintext file:

```sh
security add-generic-password -U -a "$USER" \
  -s com.fontanierh.boxology-ci-runner -w
```

The credential needs repository read access and permission to create JIT runners
(`Administration: write` for a fine-grained repository token). Do not use `gh`'s credential store
for unattended provisioning. The supervisor reads the Keychain item, verifies repository privacy,
requests one bounded JIT configuration, and passes it only to the official runner. The broker token
does not enter the job environment; normal Actions checkout credentials may exist transiently.

The official runner consumes `run.sh --disableupdate --jitconfig`. The JIT argument is visible to
same-user job processes; this accepted residual is why the trusted-private-collaborator boundary is
mandatory. Runtime self-update is disabled and the archive pin is authoritative.

## Install and validate the native runner

The bootstrap refuses to overwrite an existing installation and verifies the archive before
extraction:

```sh
./ops/ci-runner/bootstrap-macos.sh
"$HOME/.crab/ci-runner/macos-runner-base/bin/Runner.Listener" --version
bash -n ops/ci-runner/supervise-macos.sh ops/ci-runner/supervise-slots.sh
plutil -lint ops/ci-runner/com.fontanierh.boxology-ci-macos-runner.plist
plutil -lint ops/ci-runner/com.fontanierh.boxology-ci-macos-runner-slots.plist
```

Install only reviewed script/plist bytes into the dedicated `~/.crab/ci-runner` topology. Never
overwrite a live installed topology without first draining jobs and taking a restorable,
checksummed snapshot. The base service is `com.fontanierh.boxology-ci-macos-runner`; the slot
service is `com.fontanierh.boxology-ci-macos-runner-slots` and launches slots 2–4.

The rollout is intentionally two-step because a workflow cannot dispatch on a label that is not
live yet:

1. Merge the supervisor/configuration/tests change while `pr.yml` still uses the generic label.
2. Drain current jobs, snapshot installed bytes and checksums, install the merged
   `supervise-macos.sh`, `supervise-slots.sh`, and base plist, then reload both owned services.
3. Verify exactly one online JIT registration has `boxology-macos-pr-primary`, exactly four have
   `boxology-macos-pr`, and dispatch the smoke workflow.
4. Merge the exact two-file Phase-B change: route `pr.yml` to the primary label and change its
   `REQUIRED_PR_RUNNER_LABEL` integrity expectation in `crates/xtask/src/main.rs`. That PR can
   self-validate on the now-live label.

Roll back by restoring the snapshot and reloading both services before reverting the workflow
label. Do not substitute a persistent runner or weaken the exact label during activation.

Each supervisor validates tools, paths, runner count, repository response, locks, and the pinned
runner base before provisioning. It APFS-clones the base into a fresh owned run directory, waits
for one job, emits state-only diagnostics, removes failed registrations, and deletes the run
directory. A slot has a distinct lock, runtime root, runner root, and cache root. Cleanup ambiguity
fails closed and backs off; never weaken path/identity checks to make cleanup succeed.

Dispatch
[`macos-self-hosted-runner-smoke.yml`](../../.github/workflows/macos-self-hosted-runner-smoke.yml)
after installation or a host/runner change. It verifies native `macOS`/`ARM64`,
`aarch64-apple-darwin`, host evidence, and the native determinism fixture.

## Current CI routing

Pull requests have one required `pr.yml` `validation` job. The support PR uses the generic native
label; phase B of the rollout above changes that route and its paired integrity expectation to
`boxology-macos-pr-primary`, which is the steady-state target. The job always runs
`cargo xtask ci-hygiene --base <event base SHA>`. Code PRs add directly changed-crate tests.
The complete xtask unit suite runs only when xtask, workflow, runner-ops, or the embedded Boxology
skill source changes. Its path inventory disables rename detection so moving an authority file
still exposes both its old and new paths. Deep validation owns that redundant pass for ordinary
product work. Five
expensive targets are deliberately dispatch-only even when
their crate changes: `boxology-cli`'s `cli` and `surface_lock`, `boxology-workspace` and
`boxology-classifier`'s `surface_lock`, and `boxology-generator-model`'s `purity_lock`. Those four
crates and the existing deep-only `boxology-init` born-valid case still run library/binary tests,
an explicit doctest command, and every other integration target. `boxology-generator` also skips
only `tests::generated_multi_capability_box_compiles_and_routes_both_capabilities` and
`tests::generated_import_adapter_sealed_import_routes_to_real_provider_end_to_end`; its other unit
tests, binaries, doctests, and integration targets remain required. Ordinary directly changed
crates retain an unfiltered `cargo test`.

This trades pre-merge mutation/end-to-end evidence for a shorter required path; it is not a claim
that relevant source changes no longer need those tests. Dispatch-only `cargo xtask ci --no-budget`
executes every non-ignored exclusion exactly once through its product `boxology check` workspace
test pass. Its separate registries are integrity-only: they pin the exact integration source, body,
and test inventories plus both exact live generator unit bodies, and reject missing, renamed, or
ignored targets without re-running them. The generator deep-test command runs only ignored tests.
PR #592's GitHub log measured 74.59 seconds for the 52-case CLI end-to-end target, 5.64 seconds for
the CLI surface target, and 45.56 seconds for the workspace surface mutation. A controlled local
run on the post-CLI-core tree measured test-body times of 76.03, 9.34, 41.71, 1.24, and 0.67 seconds
for the five targets respectively; those are local measurements, not GitHub before/after evidence.
The fresh pre-change baseline is [PR #597](https://github.com/fontanierh/boxology/pull/597): its
generator-plus-macros change completed required native-Mac validation in 5m53 in
[Actions run 31350831322](https://github.com/fontanierh/boxology/actions/runs/31350831322), with
hygiene and invariants green before changed-generator testing. The trace reached the multi-capability
completion about 52.5 seconds into the suite, then spent about 81.4 additional seconds reaching the
sealed-import completion; 81.4 seconds is not a standalone sealed-test duration. The other 41
generator unit tests finished in about 1.3 seconds, so the trace directly supports about 134 seconds
of expected savings for that shape. This is baseline evidence only; the revised CI PR supplies the
first post-change GitHub timing.

PR #601 passed on generic slot 3 in 5m29. Docs-only PR #602 then landed on the base runner in 5m41,
with 5m03 spent in hygiene despite skipping every Rust test step. Warm-slot PR #603 still spent
2m20 in hygiene and then entered the 158-test xtask suite for a non-xtask product change. These
runs show both cache-island variance and an avoidable unconditional repository-unit pass. The
exact xtask authority selector lands in phase A; primary routing follows only after the label is
verified live. Neither change claims a speedup before post-deployment Actions timing exists.

To roll back test selection, restore the unconditional xtask unit pass and remove the special-case
crate routing so directly changed crates again use the existing unfiltered `cargo test`; keep the
integrity guards unless their authority returns elsewhere. To roll back affinity, restore generic
PR routing before unloading the primary registration.
Root dependency/toolchain, opaque fixture/golden, and process-reaper work retain their respective
conditional scopes. Required PR CI runs zero product commands.

[`deep-validation.yml`](../../.github/workflows/deep-validation.yml) is manual, non-required, and
native-Mac-only. Its sole validation command is:

```sh
cargo xtask ci --no-budget
```

That aggregate owns exactly one full `boxology check`; do not add a separate product step. Avoid
dispatching deep validation during heavy local delivery on this shared host. Scheduled advisories
use the native label. Storage hygiene is the only GitHub-hosted Ubuntu job and performs no product
compilation or validation.

Monitor the required `validation` job's cache-hit duration excluding queue time across recent PR
runs. Four minutes is the alarm threshold; it triggers investigation, not a merge gate.

## Active health, cleanup, and rollback

Check both native launchd services, their four GitHub runner labels, the pinned base version, and
the absence of unexpected owned run directories after jobs. Inspect only fixed state fields and
sanitized supervisor output; raw runner logs can contain secrets and must not be collected.

Before topology work, disable dispatch, drain every queued/running workflow and busy runner, and
snapshot installed scripts/plists with checksums. Rollback restores that snapshot before reloading
services, then verifies registrations reconcile to exactly four. A stale lock may be removed only
after proving its supervisor process is gone. Never register a persistent runner, reuse a checkout,
widen credentials, or bypass JIT cleanup as a rollback shortcut.

To retire the active lane, drain it first, unload only the two owned native services, remove only
their verified run state, and confirm their JIT registrations disappear. Reverting workflow
routing is a separate reviewed repository change; local service state does not authorize it.

## Dormant Linux/Colima assets

`Dockerfile`, `entrypoint.sh`, `supervise.sh`, the non-macOS plists, and the Linux portions of
`migrate-topology.sh` are retained source for
[#525](https://github.com/fontanierh/boxology/issues/525). Linux launchd services must remain
disabled and unloaded, the dedicated Colima profile stopped, and Linux runner registrations absent
unless #525 deliberately reactivates or replaces the lane. There is no active Linux smoke or
cross-platform workflow.

The dormant image pins Ubuntu 24.04 ARM64 by literal OCI digest, actions/runner `2.336.0` by archive
SHA-256 `58b758e420b87093fbd4bfddd368074960053e2f1388f01848c82624b90f27d1`, Rust `1.97.1`, and
cargo-deny `0.20.2`. A future reactivation must update and verify digest, archive URL, checksum, and
identity labels together; no tag-only fallback is allowed.

The retained isolation contract is: read-only image root; no checkout baked into the image; no
host mounts, Docker socket, host networking, privileges, or extra capabilities; one unique runner
volume per JIT job; non-root execution; bounded CPU/memory/pids; and no broker credential in the
container or job. API, image, Keychain, lock, context, architecture, reused-volume, or cleanup
failures stop or back off rather than reducing isolation.

### Dormant cold-start procedure

Run this only after #525 explicitly authorizes Linux work; it prepares and smokes the dormant image
but does not load services or register a runner. Use reviewed, version-pinned Apple-silicon Colima,
Docker CLI/Buildx, Rustup, `curl`, `jq`, `uuidgen`, `security`, and `launchctl`:

```sh
set -euo pipefail
colima start --profile boxology-ci-arm64 --arch aarch64 --vm-type vz \
  --mount=none --network-address=false --cpu 4 --memory 8 --disk 30
docker context use colima-boxology-ci-arm64
test "$(docker context show)" = colima-boxology-ci-arm64
docker buildx build --platform linux/arm64 --load \
  -f ops/ci-runner/Dockerfile -t boxology-linux-arm64-pr:verified .

image=boxology-linux-arm64-pr:verified
docker run --rm --platform linux/arm64 --network none --read-only --user runner \
  --tmpfs /tmp:rw,exec,nosuid,size=512m,mode=1777 --entrypoint /bin/bash "$image" -ceu '
  test "$(uname -m)" = aarch64 && test "$(id -u)" -ne 0
  rustc --version | grep -F 1.97.1
  cargo deny --version | grep -F 0.20.2
  cp -a /opt/actions-runner/. /tmp/runner && /tmp/runner/run.sh --help >/dev/null
'
volume="boxology-local-smoke-$(uuidgen | tr '[:upper:]' '[:lower:]')"
docker volume create "$volume" >/dev/null
trap 'docker volume rm "$volume" >/dev/null 2>&1 || true' EXIT INT TERM
set +e
docker run --rm --platform linux/arm64 --network none --read-only --user runner \
  --mount "type=volume,source=$volume,target=/runner" \
  --tmpfs /tmp:rw,noexec,nosuid,size=256m,mode=1777 "$image" </dev/null
code=$?
set -e
test "$code" -eq 64
```

Before any authorized activation, separately verify the image's architecture, literal base digest,
runner checksum, and `org.boxology.ci.image-id`/`org.boxology.ci.image-version` labels.

`migrate-topology.sh` is a dual-platform migration/restore tool, not an active health command. If
#525 authorizes its use, review it against the chosen topology first. Its safety contract is to
disable workflows, drain jobs/runners, snapshot installed bytes and checksums, reconcile old JIT
registrations to zero, and keep dispatch disabled on mutation failure. Its narrow stale-run
acknowledgment is only for a freshly revalidated, older-than-24-hours, jobless orphaned PR record
off the default SHA with no live head ref. The printed backup must be retained until smoke and
production runs succeed; restore must verify checksums and reinstall both scripts and plists before
loading the previous topology.
