# Boxology Mac-hosted CI runners

This is the authoritative runbook for S0-T8. It provisions eight useful
GitHub Actions JIT runners on the MacBook: four Linux runners in the native
ARM64 Colima VM and four macOS runners on the native Apple-silicon host.
Product validation and compilation workflows use these labels. The cheap
non-compiling storage-hygiene job remains on GitHub-hosted Ubuntu; no product
validation or compilation job uses GitHub-hosted runners or minutes.

Each label has one base supervisor plus three slot supervisors. A slot owns one
JIT runner, one disposable workspace, and one cache root, so independent PRs can
run concurrently without sharing checkout state. Native Mac slots keep a private
per-slot Cargo target directory between jobs and use `CARGO_BUILD_JOBS=4` and
`RUST_TEST_THREADS=4`; runner installations use APFS copy-on-write clones, and
Linux slot containers remain capped at one CPU and 2 GiB. The four-per-platform
bound matches the host's useful CPU and memory capacity. More slots fragmented
the per-slot Cargo caches and increased contention without shortening the
required-check critical path.

## Pinned inputs

`Dockerfile` records literal (non-overridable) pins for the Ubuntu 24.04 ARM64 OCI digest, actions/runner `2.336.0`
archive SHA-256 (`58b758e420b87093fbd4bfddd368074960053e2f1388f01848c82624b90f27d1`), Rust `1.97.1`, and cargo-deny `0.20.2`. If either source pin is
changed, replace the base digest, archive URL, and checksum literals together, verify them
from the official source, and rebuild. Never add a tag-only fallback. The image labels
and `ImageOS`/`ImageVersion` values are the job-evidence identity; runtime self-update
is disabled, so this image pin is authoritative.

The native macOS runner uses actions/runner `2.336.0` for Apple silicon with
archive SHA-256
`8e8839c49b7060b6b2154f4931f815df330c27f167d53ef2239ee3dfce28b079`, the
repository Rust `1.97.1` toolchain, and cargo-deny `0.20.2`. The Mac OS version
is included in `ImageVersion` evidence because the host is not an immutable image.

## Prerequisites

Use an Apple-silicon Mac with approved, already-installed and version-pinned
Colima, Docker CLI/Buildx, Rustup, `curl`, `jq`, `uuidgen`, `security`, and
`launchctl`. Record the approved Colima and host macOS versions beside the
installation; this repository does not install host software. Use a private GitHub repository with trusted Henry/agent
collaborators; private forking is not changed or required for activation.

Create the dedicated Keychain item interactively; the token is never written to
the checkout, plist, environment, command line, or a persistent file:

```sh
security add-generic-password -U -a "$USER" \
  -s com.fontanierh.boxology-ci-runner -w
```

The item must contain an operator-approved token able to read the private
repository and create a JIT runner (`Administration: write` for a fine-grained
repository token). Access is limited to trusted Henry/agent collaborators. The
repository's private-forking setting is not changed and is not a PR-1 blocker.
Do not use `gh`'s credential store for the supervisor.

## VM and image

Create a profile with no host mounts and no host network address. Keep this
profile dedicated to this runner:

```sh
colima start --profile boxology-ci-arm64 --arch aarch64 --vm-type vz \
  --mount=none --network-address=false --cpu 4 --memory 8 --disk 30
docker context use colima-boxology-ci-arm64
test "$(docker context show)" = colima-boxology-ci-arm64
```

From the repository root, build and load the pinned ARM64 image. The Dockerfile
copies only its entrypoint; it never copies repository source into the image:

```sh
docker buildx build --platform linux/arm64 --load \
  -f ops/ci-runner/Dockerfile -t boxology-linux-arm64-pr:verified .
docker image inspect boxology-linux-arm64-pr:verified
```

Before provisioning, run this credential-free local ARM64 image smoke (no GitHub access). The first container stages the copied runner on executable tmpfs because `run.sh --help` writes `run-helper.sh`; the second uses a fresh named `/runner` volume, exercises the real entrypoint, and expects its fail-closed empty-input status:
```sh
set -euo pipefail
image=boxology-linux-arm64-pr:verified
docker run --rm --platform linux/arm64 --network none --read-only --user runner \
  --tmpfs /tmp:rw,exec,nosuid,size=512m,mode=1777 --entrypoint /bin/bash "$image" -ceu '
  test "$(uname -m)" = aarch64
  test "$(id -u)" -ne 0
  rustc --version | grep -F 1.97.1
  test "$RUSTUP_HOME" = /opt/rustup
  rustup component list --toolchain 1.97.1 | grep -F 'rustfmt-' | grep -F installed
  rustup component list --toolchain 1.97.1 | grep -F 'clippy-' | grep -F installed
  rustup component list --toolchain 1.97.1 | grep -F 'rust-analyzer-' | grep -F installed
  mkdir -p /tmp/toolchain-probe
  printf '[toolchain]\nchannel = "1.97.1"\ncomponents = ["rustfmt", "clippy", "rust-analyzer"]\nprofile = "minimal"\n' > /tmp/toolchain-probe/rust-toolchain.toml
  (cd /tmp/toolchain-probe && rustup toolchain install && rustup show active-toolchain)
  cargo deny --version | grep -F 0.20.2
  cp -a /opt/actions-runner/. /tmp/runner
  /tmp/runner/run.sh --help >/dev/null
'
volume="boxology-local-smoke-$(uuidgen | tr '[:upper:]' '[:lower:]')"
docker volume create "$volume" >/dev/null
cleanup() { docker volume rm "$volume" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
set +e
docker run --rm --platform linux/arm64 --network none --read-only --user runner \
  --mount "type=volume,source=$volume,target=/runner" --tmpfs /tmp:rw,noexec,nosuid,size=256m,mode=1777 \
  "$image" </dev/null
exit_code=$?
set -e
test "$exit_code" -eq 64
```

Verify architecture, the pinned base digest, the runner SHA-256 label, and
`org.boxology.ci.image-id`/`org.boxology.ci.image-version` labels before starting
the supervisor. A missing or changed identity is a fail-closed condition.
At runtime the image root is read-only; the unique named volume mounted at
`/runner` holds only the copied runner state, checkout, target, and Cargo home.
The pinned 1.97.1 toolchain — including `rustfmt`, `clippy`, and `rust-analyzer`
— is preinstalled into the read-only `/opt/rustup` image layer and shared by
every runner, so the workflow's `rustup toolchain install` finds the toolchain
and all repository-requested components already present and completes without
writing to `RUSTUP_HOME`. Because no toolchain is copied per job, the `/runner`
volume stays small and each job starts fast; the entrypoint still rejects any
`/runner` volume that is not empty as reused runner state.
base supervisor bounds its container to 4 CPUs and 8 GiB RAM without swap; slot
containers use the slot plist's 1-CPU/2-GiB bounds. All containers are capped at
512 pids.
It sets `TMPDIR` to the executable `_work/_temp` volume path so nested Cargo
tests can run temporary binaries while `/tmp` remains `noexec`.

## Native macOS runner

Install the pinned Apple-silicon runner into the dedicated host directory. The
bootstrap refuses to overwrite an existing install and verifies the archive
before extraction:

```sh
./ops/ci-runner/bootstrap-macos.sh
"$HOME/.crab/ci-runner/macos-runner-base/bin/Runner.Listener" --version
```

Validate the reviewed macOS supervisor and plist in the checkout. Do not copy
them over installed files: the migration procedure must snapshot the live
versions before it installs these repo-owned bytes.

```sh
bash -n ops/ci-runner/supervise-macos.sh
plutil -lint ops/ci-runner/com.fontanierh.boxology-ci-macos-runner.plist
```

The native supervisor uses the same Keychain item as the Linux supervisor,
provisions one JIT runner with `[self-hosted, macOS, ARM64, boxology-macos-pr]`,
APFS-clones the verified runner into a fresh per-job directory, and removes that
directory after completion. Its private per-slot Cargo target directory is kept
outside the checkout so Cargo can reuse fingerprints without sharing workspaces
between slots. The clone keeps jobs isolated without copying the 435 MB runner
base for every job. It emits only state diagnostics; never collect raw
runner logs because job output can contain secrets. The native runner is not
container-isolated: it is accepted only for this private repository and trusted
Henry/agent collaborators.

Stage the three native Mac slots alongside the base supervisor:

```sh
bash -n ops/ci-runner/supervise-slots.sh
plutil -lint ops/ci-runner/com.fontanierh.boxology-ci-macos-runner-slots.plist
```

## JIT lifecycle and smoke test

Copy the reviewed `supervise.sh` only to the dedicated topology staging
directory, then invoke that staged copy with a smoke-specific runtime directory
and non-secret settings. Never overwrite
`$HOME/.crab/ci-runner/supervise.sh` before `migrate-topology.sh activate` has
snapshotted the installed version.

```sh
mkdir -p "$HOME/.crab/ci-runner/topology-stage"
chmod 700 "$HOME/.crab/ci-runner/topology-stage"
install -m 700 ops/ci-runner/supervise.sh \
  "$HOME/.crab/ci-runner/topology-stage/supervise.sh"
REPOSITORY=fontanierh/boxology \
CI_RUNNER_IMAGE=boxology-linux-arm64-pr:verified \
RUNTIME_DIR=/tmp/boxology-ci-linux-stage-smoke \
  "$HOME/.crab/ci-runner/topology-stage/supervise.sh"
```

Set `RUNNER_GROUP_ID` when the repository's approved runner group is not the
default `1`.
It reads the Keychain item, verifies the pinned Docker context and repository privacy.
The operator, not the supervisor, confirms the trusted Henry/agent collaborator boundary;
the supervisor does not inspect collaborators. It makes bounded API requests for one
encoded JIT configuration over the GitHub API and passes it only on container stdin.
The broker PAT never enters the container or job environment; ordinary GitHub Actions
read/runtime credentials may be present for checkout.
The smoke workflow keeps `persist-credentials: false` and asserts only broker-PAT absence. The official runner transiently consumes `run.sh --disableupdate --jitconfig`; runtime self-update is disabled and the image pin is authoritative.
That argument is visible to same-user job processes by design. This residual is accepted only for trusted private collaborators; do not activate if that boundary changes.
Each slot supervisor waits for one job, emits sanitized state-only diagnostics, then removes failed JIT registrations, the container, and the unique volume. Failed cleanup retains owned handles/lock and backs off; a lock refuses a concurrent supervisor.

Only after a successful Linux smoke run, create a resolved Linux base plist in
the dedicated staging directory and replace every placeholder there. Never copy
it over the installed plist: the migration snapshots the installed version first,
then consumes this staged file only when no installed Linux base exists.

```sh
mkdir -p "$HOME/.crab/ci-runner/topology-stage"
chmod 700 "$HOME/.crab/ci-runner/topology-stage"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-runner.plist \
  "$HOME/.crab/ci-runner/topology-stage/com.fontanierh.boxology-ci-runner.plist"
# Resolve every placeholder in the staged copy, then:
plutil -lint "$HOME/.crab/ci-runner/topology-stage/com.fontanierh.boxology-ci-runner.plist"
! grep -Eq '/ABSOLUTE/PATH|OWNER/REPOSITORY|VERIFIED-IMAGE' \
  "$HOME/.crab/ci-runner/topology-stage/com.fontanierh.boxology-ci-runner.plist"
```

Stage the three Linux slots alongside the base supervisor:

```sh
bash -n ops/ci-runner/supervise.sh ops/ci-runner/supervise-slots.sh
plutil -lint ops/ci-runner/com.fontanierh.boxology-ci-runner-slots.plist
```

Linux provisioning is retained as dormant source material for post-v0 #525, but its launchd
services must remain disabled and unloaded during v0 delivery. There is no Linux smoke workflow:
keeping an undispatchable button after retiring its runners would be misleading. Dispatch
[`macos-self-hosted-runner-smoke.yml`](../../.github/workflows/macos-self-hosted-runner-smoke.yml)
to verify native `macOS`/`ARM64`, `aarch64-apple-darwin`, host evidence, and the native determinism
fixture.

## CI activation

Pull-request CI is one required native Mac job: `pr.yml` `validation` on
`[self-hosted, macOS, ARM64, boxology-macos-pr]`. It always runs
`cargo xtask ci-hygiene --base <event base SHA>` and, for non-Markdown diffs, the
xtask invariant suite and directly changed-crate tests. Root dependency/toolchain
changes also compile-check the whole workspace; the process-reaper fixture runs
only when its own implementation changes. Full-workspace, nested-workspace,
composition, Clippy, docs, deny, determinism, and complete `boxology check`
validation are deep-only. This avoids duplicating fmt, Clippy, and workspace tests
when S5 makes them part of the product check itself. Deep validation is
`deep-validation.yml`: `workflow_dispatch` only (no schedule, no required check),
same Mac label, running `cargo xtask ci --no-budget` and `boxology check`. Do not
dispatch deep validation during heavy local delivery on this MacBook.

The Linux ARM64 JIT launchd services are disabled and unloaded for v0; their dedicated Colima
profile is stopped and no Linux runner registrations remain. Scheduled advisories remain on the
native Mac label. Storage
hygiene remains a cheap, non-compiling GitHub-hosted Ubuntu job
(`ubuntu-latest`). Continuous Linux/x86/cross-platform validation and
determinism comparison are owned by
[#525](https://github.com/fontanierh/boxology/issues/525). No product validation
or compilation job uses GitHub-hosted runners or minutes. Native Mac slots reuse
the per-slot Cargo target with four Cargo jobs. Post-v0 #525 owns any deliberate
Linux/cross-platform reactivation.

The wall-clock monitor tracks the single required `validation` job's
cache-hit duration excluding queue time, with **4 minutes** as the alarm
threshold across recent PR runs.

## Health, cleanup, and rollback

Check `colima status --profile boxology-ci-arm64`, `DOCKER_CONTEXT=colima-boxology-ci-arm64`,
the image architecture/identity/SHA labels, the four base/slot launchd jobs, and the GitHub
runner labels. Inspect the current Linux container only for fixed state fields
and mounts; the sole mount must be the fresh named runner volume. Never collect
raw runner logs because job output can contain secrets.

Use the reviewed migration procedure rather than assembling launchctl commands
by hand. It first records and disables every active workflow, then waits for both
queued/running runs and busy runners to drain. It durably snapshots the installed
supervisor scripts and plists, unloads the old topology, waits for its JIT
registrations to disappear, and bootstraps Linux and macOS serially. Each
platform must reconcile from zero to exactly four registrations before dispatch
is restored. One base job plus one three-child slot manager is the only loaded
topology, and each child has a distinct lock/root, so there is no fifth process
that can race the `MAX_RUNNERS=4` check.

The normal drain blocks on every non-completed run from enabled or disabled
workflows and every busy runner. There is no automatic age exemption. An operator
may deliberately acknowledge one unterminalizable GitHub control-plane record
with `activate --ack-stale-run RUN_ID` (or the equivalent option after a restore
backup). The exact numeric run is freshly revalidated as older than 24 hours,
non-completed, jobless, an orphaned `pull_request`, off the current default SHA,
and without a live head ref. It is logged, and all API/schema changes fail closed.

```sh
./ops/ci-runner/migrate-topology.sh activate
```

The command prints its backup directory. Keep that directory until the new
topology has completed smoke and production runs. It fails closed with workflow
dispatch disabled after any topology mutation error and prints the exact restore command.
Restore is also dispatch-safe, drains work, verifies the backup checksums, and
reinstalls both the saved scripts and saved plists before loading the previous
topology:

```sh
./ops/ci-runner/migrate-topology.sh restore \
  /Users/jim/.crab/ci-runner/topology-backups/<backup>
```

To stop service, unload the installed plist(s), remove only the current Linux
container/volume and native macOS run directory, and stop the dedicated Colima
profile. A stale supervisor lock
may be removed only after confirming no supervisor process remains. To roll back
the runner-count migration, use the concrete `restore` command above.
To roll back the active lane, revert the workflow routing while leaving both
smoke workflows available.
Do not register a persistent runner or widen the
container's mounts, network, capabilities, or credentials as a rollback shortcut.

Safety boundaries: no repository checkout in the image, read-only root, bounded
CPU/memory, no host path mounts/socket/host networking, no privileged container,
no persistent runner registration, no broker PAT in repo/plist/env/command line,
GitHub API writes are limited to JIT registration/deletion lifecycle operations, and one
runner per slot. The Linux JIT argument's same-user
visibility is the documented residual; API, image, Keychain, lock, context, and
architecture failures stop or back off rather than weakening isolation.
