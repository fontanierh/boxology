# Boxology Mac-hosted CI runners

This is the authoritative runbook for S0-T8. It provisions twenty disposable
GitHub Actions JIT slots for each active label on the MacBook: Linux jobs run in the
native ARM64 Colima VM, and macOS jobs run on the native Apple-silicon host.
Every enabled workflow uses one of these labels; no enabled workflow targets a
GitHub-hosted runner.

Each label has one base supervisor plus nineteen slot supervisors, split between
the original nine-slot manager and a ten-slot expansion manager. A slot owns one
JIT runner, one disposable workspace, and one cache root, so independent PRs can
run concurrently without sharing checkout state. Native Mac slots keep a private
per-slot Cargo target directory between jobs and use `CARGO_BUILD_JOBS=4` and
`RUST_TEST_THREADS=4`; runner installations use APFS copy-on-write clones, and
extra Linux containers remain capped at one CPU and 2 GiB.

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

Copy `supervise-macos.sh` and
`com.fontanierh.boxology-ci-macos-runner.plist` outside the checkout, then load
the user LaunchAgent:

```sh
install -m 700 ops/ci-runner/supervise-macos.sh "$HOME/.crab/ci-runner/supervise-macos.sh"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-macos-runner.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner.plist"
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner.plist"
launchctl print "gui/$(id -u)/com.fontanierh.boxology-ci-macos-runner"
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

Activate the nineteen native Mac slots alongside the base supervisor:

```sh
install -m 700 ops/ci-runner/supervise-slots.sh "$HOME/.crab/ci-runner/supervise-slots.sh"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-macos-runner-slots.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots.plist"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots.plist"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-macos-runner-slots-extra.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots-extra.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots-extra.plist"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-macos-runner-slots-extra.plist"
```

## JIT lifecycle and smoke test

Install the reviewed `supervise.sh` outside this mutable checkout, make it
executable, and invoke it with non-secret settings such as
`REPOSITORY=fontanierh/boxology` and `CI_RUNNER_IMAGE=boxology-linux-arm64-pr:verified`.
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

Only after a successful Linux smoke run, replace every placeholder in the plist,
copy it and the reviewed supervisor to paths outside the checkout, then validate
and load it in the user launchd domain:

```sh
plutil -lint /PATH/TO/com.fontanierh.boxology-ci-runner.plist
launchctl bootstrap "gui/$(id -u)" /PATH/TO/com.fontanierh.boxology-ci-runner.plist
launchctl print "gui/$(id -u)/com.fontanierh.boxology-ci-runner"
```

Activate the nineteen Linux slots alongside the base supervisor:

```sh
install -m 700 ops/ci-runner/supervise-slots.sh "$HOME/.crab/ci-runner/supervise-slots.sh"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-runner-slots.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots.plist"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots.plist"
install -m 600 ops/ci-runner/com.fontanierh.boxology-ci-runner-slots-extra.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots-extra.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots-extra.plist"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-ci-runner-slots-extra.plist"
```

The Linux workflow is manual-only and remains the operator's end-to-end health check. Dispatch
[`self-hosted-runner-smoke.yml`](../../.github/workflows/self-hosted-runner-smoke.yml) after the supervisor is ready; it is read-only, contains no secrets,
and is safe to remain queued while no runner exists. It checks Linux, ARM64/aarch64, non-root identity,
image evidence, checkout credential hygiene, and the ARM host branch of the
determinism fixture. Dispatch [`macos-self-hosted-runner-smoke.yml`](../../.github/workflows/macos-self-hosted-runner-smoke.yml)
to verify native `macOS`/`ARM64`, `aarch64-apple-darwin`, host evidence, and the
native determinism fixture.

## CI activation

The activated `pr.yml` Linux lane assigns `checks-linux`, `deny`, both determinism
consumers, and `validation` to `[self-hosted, linux, ARM64, boxology-linux-arm64-pr]`.
The Linux evidence and determinism verification target is `aarch64-unknown-linux-gnu`;
the `checks-macos` native determinism gate and scheduled advisory workflow use
`[self-hosted, macOS, ARM64, boxology-macos-pr]`. The x86 audit workflow is
intentionally removed for this emergency migration; x86 coverage is a deferred
follow-up and is not part of the active CI contract. Consequently every enabled
workflow job runs through this MacBook and uses no GitHub-hosted Actions minutes.
The full workspace test suite runs once in `checks-linux`; the native Mac gate
reuses the per-slot Cargo target and checks platform-specific determinism without
rebuilding the entire workspace a second time.

## Health, cleanup, and rollback

Check `colima status --profile boxology-ci-arm64`, `DOCKER_CONTEXT=colima-boxology-ci-arm64`,
the image architecture/identity/SHA labels, all base, slot, and expansion launchd jobs, and the GitHub
runner labels. Inspect the current Linux container only for fixed state fields
and mounts; the sole mount must be the fresh named runner volume. Never collect
raw runner logs because job output can contain secrets.

To stop service, unload the installed plist(s), remove only the current Linux
container/volume and native macOS run directory, and stop the dedicated Colima
profile. A stale supervisor lock
may be removed only after confirming no supervisor process remains. To roll back the
active lane, revert the workflow routing while leaving both smoke workflows available.
Do not register a persistent runner or widen the
container's mounts, network, capabilities, or credentials as a rollback shortcut.

Safety boundaries: no repository checkout in the image, read-only root, bounded
CPU/memory, no host path mounts/socket/host networking, no privileged container,
no persistent runner registration, no broker PAT in repo/plist/env/command line,
GitHub API writes are limited to JIT registration/deletion lifecycle operations, and one
runner per slot. The Linux JIT argument's same-user
visibility is the documented residual; API, image, Keychain, lock, context, and
architecture failures stop or back off rather than weakening isolation.
