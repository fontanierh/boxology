#!/usr/bin/env bash
set -euo pipefail
umask 077

RUNNER_VERSION=2.336.0
RUNNER_SHA256=8e8839c49b7060b6b2154f4931f815df330c27f167d53ef2239ee3dfce28b079
RUNNER_ROOT="${RUNNER_ROOT:-/Users/jim/.crab/ci-runner/macos-runner-base}"
DOWNLOAD_ROOT="$(mktemp -d /Users/jim/.crab/ci-runner/macos-download.XXXXXX)"
ARCHIVE="$DOWNLOAD_ROOT/actions-runner-osx-arm64-$RUNNER_VERSION.tar.gz"
STAGING="$DOWNLOAD_ROOT/staging"
cleanup() { rm -rf -- "$DOWNLOAD_ROOT"; }
trap cleanup EXIT INT TERM

[[ "$(uname -m)" = arm64 ]] || { printf '%s\n' 'runner: this bootstrap requires Apple silicon' >&2; exit 64; }
if [[ -e "$RUNNER_ROOT" ]]; then
  printf '%s\n' "runner: refusing to overwrite existing runner root: $RUNNER_ROOT" >&2
  exit 75
fi
mkdir -p "$STAGING"
curl --fail --location --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 180 \
  -o "$ARCHIVE" \
  "https://github.com/actions/runner/releases/download/v$RUNNER_VERSION/actions-runner-osx-arm64-$RUNNER_VERSION.tar.gz"
printf '%s  %s\n' "$RUNNER_SHA256" "$ARCHIVE" | shasum -a 256 -c -
tar -xzf "$ARCHIVE" -C "$STAGING"
mkdir -p "$(dirname "$RUNNER_ROOT")"
mv "$STAGING" "$RUNNER_ROOT"
printf '%s\n' "$RUNNER_SHA256" > "$RUNNER_ROOT/.boxology-runner-sha256"
printf '%s\n' "$RUNNER_VERSION" > "$RUNNER_ROOT/.boxology-runner-version"
chmod 700 "$RUNNER_ROOT" "$RUNNER_ROOT/run.sh"
printf '%s\n' "runner: installed $RUNNER_VERSION at $RUNNER_ROOT"
