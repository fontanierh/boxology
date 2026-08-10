#!/usr/bin/env bash
set -euo pipefail
umask 077

SLOT_COUNT="${SLOT_COUNT:-3}"
SLOT_START="${SLOT_START:-2}"
SUPERVISOR="${SUPERVISOR:-/Users/jim/.crab/ci-runner/supervise-macos.sh}"
SLOT_ROOT_PREFIX="${SLOT_ROOT_PREFIX:-/Users/jim/.crab/ci-runner/macos-runner-slot-}"
RUNTIME_ROOT_PREFIX="${RUNTIME_ROOT_PREFIX:-/tmp/boxology-ci-macos-runner-slot-}"
LOG_ROOT="${LOG_ROOT:-/Users/jim/.crab/ci-runner/logs/macos-slots}"
SLOT_MAX_RUNNERS="${SLOT_MAX_RUNNERS:-4}"
SLOT_CARGO_BUILD_JOBS="${SLOT_CARGO_BUILD_JOBS:-4}"
SLOT_RUST_TEST_THREADS="${SLOT_RUST_TEST_THREADS:-4}"
SLOT_CONTAINER_CPUS="${SLOT_CONTAINER_CPUS:-1}"
SLOT_CONTAINER_MEMORY="${SLOT_CONTAINER_MEMORY:-2g}"

[[ "$SLOT_COUNT" =~ ^[0-9]+$ && "$SLOT_COUNT" -ge 1 && "$SLOT_COUNT" -le 50 ]] || exit 64
[[ "$SLOT_START" =~ ^[0-9]+$ && "$SLOT_START" -ge 1 ]] || exit 64
[[ "$SLOT_MAX_RUNNERS" =~ ^[0-9]+$ && "$SLOT_MAX_RUNNERS" -ge 1 && "$SLOT_MAX_RUNNERS" -le 90 ]] || exit 64
[[ -x "$SUPERVISOR" ]] || exit 69
mkdir -p "$LOG_ROOT"
chmod 700 "$LOG_ROOT"

declare -a pids
spawn_slot() {
  local slot=$1 log_path
  log_path="$LOG_ROOT/slot-$slot.log"
  (
    export RUNNER_ROOT="${SLOT_ROOT_PREFIX}${slot}"
    export RUNTIME_DIR="${RUNTIME_ROOT_PREFIX}${slot}"
    export MAX_RUNNERS="$SLOT_MAX_RUNNERS"
    export CARGO_BUILD_JOBS="$SLOT_CARGO_BUILD_JOBS"
    export RUST_TEST_THREADS="$SLOT_RUST_TEST_THREADS"
    export CONTAINER_CPUS="$SLOT_CONTAINER_CPUS"
    export CONTAINER_MEMORY="$SLOT_CONTAINER_MEMORY"
    # The three capacity slots stay generic even if an operator shell happens
    # to carry the base service's required-PR affinity label.
    export RUNNER_EXTRA_LABEL=
    exec "$SUPERVISOR"
  ) >>"$log_path" 2>&1 &
  pids[$slot]=$!
}

stop_all() {
  local slot pid
  for ((slot = SLOT_START; slot < SLOT_START + SLOT_COUNT; slot++)); do
    pid="${pids[$slot]-}"
    [[ -n "$pid" ]] || continue
    kill "$pid" 2>/dev/null || true
  done
  for ((slot = SLOT_START; slot < SLOT_START + SLOT_COUNT; slot++)); do
    pid="${pids[$slot]-}"
    [[ -n "$pid" ]] || continue
    wait "$pid" 2>/dev/null || true
  done
}
shutdown() {
  trap - INT TERM
  exit 130
}
trap stop_all EXIT
trap shutdown INT TERM

for ((slot = SLOT_START; slot < SLOT_START + SLOT_COUNT; slot++)); do
  spawn_slot "$slot"
done

while :; do
  for ((slot = SLOT_START; slot < SLOT_START + SLOT_COUNT; slot++)); do
    pid="${pids[$slot]-}"
    if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
      spawn_slot "$slot"
    fi
  done
  sleep 5
done
