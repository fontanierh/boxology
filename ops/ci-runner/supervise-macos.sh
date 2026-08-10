#!/usr/bin/env bash
set -euo pipefail
umask 077

: "${REPOSITORY:?set REPOSITORY to owner/repository}"
RUNNER_BASE="${RUNNER_BASE:-/Users/jim/.crab/ci-runner/macos-runner-base}"
RUNNER_ROOT="${RUNNER_ROOT:-/Users/jim/.crab/ci-runner/macos-runner}"
KEYCHAIN_SERVICE="${KEYCHAIN_SERVICE:-com.fontanierh.boxology-ci-runner}"
KEYCHAIN_ACCOUNT="${KEYCHAIN_ACCOUNT:-$(/usr/bin/id -un)}"
RUNNER_GROUP_ID="${RUNNER_GROUP_ID:-1}"
MAX_RUNNERS="${MAX_RUNNERS:-4}"
RUNNER_VERSION=2.336.0
RUNNER_SHA256=8e8839c49b7060b6b2154f4931f815df330c27f167d53ef2239ee3dfce28b079
RUST_VERSION=1.97.1
CARGO_DENY_VERSION=0.20.2
IMAGE_ID=boxology-macos-arm64-pr
RUNNER_LABEL=boxology-macos-pr
RUNNER_EXTRA_LABEL="${RUNNER_EXTRA_LABEL:-}"
IMAGE_OS=macos-arm64-host
MACOS_VERSION="$(/usr/bin/sw_vers -productVersion)"
IMAGE_VERSION="macOS-${MACOS_VERSION}-arm64-runner-${RUNNER_VERSION}-rust-${RUST_VERSION}-deny-${CARGO_DENY_VERSION}"
RUNTIME_DIR="${RUNTIME_DIR:-/tmp/boxology-ci-macos-runner}"
LOCK="$RUNTIME_DIR/supervisor.lock"
CACHE_ROOT="$RUNNER_ROOT/cache"
TARGET_ROOT="$CACHE_ROOT/target"
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"
RUNNER_NAME= RUNNER_ID= runner_pid= run_dir= token=

[[ "$REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || exit 64
[[ "$RUNNER_GROUP_ID" =~ ^[0-9]+$ ]] || exit 64
[[ "$MAX_RUNNERS" =~ ^[0-9]+$ && "$MAX_RUNNERS" -ge 1 && "$MAX_RUNNERS" -le 90 ]] || exit 64
[[ -z "$RUNNER_EXTRA_LABEL" || ( "$RUNNER_EXTRA_LABEL" =~ ^[A-Za-z0-9_.-]+$ && "$RUNNER_EXTRA_LABEL" != "$RUNNER_LABEL" ) ]] || exit 64
[[ "$(uname -m)" = arm64 ]] || exit 69
for tool in curl jq security uuidgen shasum sw_vers; do command -v "$tool" >/dev/null || exit 69; done
[[ -x "$RUNNER_BASE/run.sh" && -x "$RUNNER_BASE/bin/Runner.Listener" ]] || exit 69
mkdir -p "$RUNTIME_DIR" "$RUNNER_ROOT/runs" "$CACHE_ROOT/home" "$TARGET_ROOT"
chmod 700 "$RUNTIME_DIR" "$RUNNER_ROOT" "$RUNNER_ROOT/runs" "$CACHE_ROOT" "$CACHE_ROOT/home" "$TARGET_ROOT"
# Single-owner mutex per RUNTIME_DIR. A supervisor killed uncleanly (OOM, SIGKILL,
# reboot) leaves the lock behind; reclaim it only when the recorded owner PID is gone,
# so a live supervisor is still refused but a dead one cannot brick the slot forever.
if ! mkdir "$LOCK" 2>/dev/null; then
  lock_owner="$(cat "$LOCK/pid" 2>/dev/null || true)"
  if [[ "$lock_owner" =~ ^[0-9]+$ ]] && kill -0 "$lock_owner" 2>/dev/null; then
    printf '%s\n' 'runner: macOS supervisor already running' >&2; exit 75
  fi
  rm -rf "$LOCK"
  mkdir "$LOCK" 2>/dev/null || { printf '%s\n' 'runner: macOS supervisor already running' >&2; exit 75; }
fi
printf '%s\n' "$$" >"$LOCK/pid"

validate_runner_list() {
  jq -e '
    (.total_count | type == "number" and . == floor and . >= 0 and . <= 100)
    and (.runners | type == "array")
    and ((.runners | length) == .total_count)
    and all(.runners[];
      (.id | type == "number" and . == floor and . > 0)
      and (.name | type == "string")
      and (.labels | type == "array")
      and all(.labels[];
        (type == "object")
        and (.name | if type == "string" then length > 0 else false end)))
  ' <<<"$1" >/dev/null
}
runner_identity() {
  [[ -n "$RUNNER_ID" && -n "$RUNNER_NAME" ]] || return 1
  jq -er --argjson id "$RUNNER_ID" --arg name "$RUNNER_NAME" '
    ([.runners[] | select(.id == $id)] | length) as $id_count
    | ([.runners[] | select(.name == $name)] | length) as $name_count
    | ([.runners[] | select(.id == $id and .name == $name)] | length) as $exact_count
    | if ($exact_count == 1 and $id_count == 1 and $name_count == 1) then "exact"
      elif ($id_count > 0 or $name_count > 0) then "mismatch"
      else "absent"
      end
  ' <<<"$1"
}
validate_jit_response() {
  jq -e --arg expected_name "$1" '
    (.runner | type == "object")
    and (.runner.id | type == "number" and . == floor and . > 0)
    and (.runner.name | type == "string" and . == $expected_name)
    and (.encoded_jit_config | type == "string" and length > 0)
  ' <<<"$2" >/dev/null
}
delete_jit_runner() {
  if [[ -z "$RUNNER_ID" ]]; then
    [[ -z "$RUNNER_NAME" ]] && return 0
    reconcile_jit_runner
    return
  fi
  [[ -n "$RUNNER_NAME" ]] || return 1
  local runners identity
  runners="$(api GET '/actions/runners?per_page=100')" || return 1
  validate_runner_list "$runners" || return 1
  identity="$(runner_identity "$runners")" || return 1
  case "$identity" in
    exact)
      api DELETE "/actions/runners/$RUNNER_ID" >/dev/null 2>&1 || return 1
      RUNNER_ID=; RUNNER_NAME=
      ;;
    absent)
      RUNNER_ID=; RUNNER_NAME=
      ;;
    mismatch|*) return 1 ;;
  esac
}
reconcile_jit_runner() {
  local runners identity name_count
  if [[ -z "$RUNNER_NAME" ]]; then
    [[ -z "$RUNNER_ID" ]] && return 0
    return 1
  fi
  runners="$(api GET '/actions/runners?per_page=100')" || return 1
  validate_runner_list "$runners" || return 1
  if [[ -n "$RUNNER_ID" ]]; then
    identity="$(runner_identity "$runners")" || return 1
    case "$identity" in
      exact) delete_jit_runner; return $? ;;
      absent) RUNNER_ID=; RUNNER_NAME=; return 0 ;;
      mismatch|*) return 1 ;;
    esac
  fi
  name_count="$(jq -r --arg name "$RUNNER_NAME" '[.runners[] | select(.name == $name)] | length' <<<"$runners")" || return 1
  case "$name_count" in
    0) RUNNER_NAME=; return 0 ;;
    1) RUNNER_ID="$(jq -er --arg name "$RUNNER_NAME" '.runners[] | select(.name == $name) | .id' <<<"$runners")" || return 1; delete_jit_runner ;;
    *) return 1 ;;
  esac
}
api() {
  (( $# >= 2 )) || return 64
  local method="$1" path="$2" payload="${3-}"
  local url="https://api.github.com/repos/$REPOSITORY$path"
  if [[ -n "$payload" ]]; then
    printf '%s' "$payload" | curl -q --silent --fail --proto '=https' --tlsv1.2 --connect-timeout 5 --max-time 30 \
      --config <(printf 'header = "Authorization: Bearer %s"\nheader = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\n' "$token") --request "$method" --header 'Content-Type: application/json' \
      --data-binary @- "$url" 2>/dev/null
  else
    curl -q --silent --fail --proto '=https' --tlsv1.2 --connect-timeout 5 --max-time 30 --config <(printf 'header = "Authorization: Bearer %s"\nheader = "Accept: application/vnd.github+json"\nheader = "X-GitHub-Api-Version: 2022-11-28"\n' "$token") \
      --request "$method" "$url" 2>/dev/null
  fi
}
check_repo() {
  local repo runners
  repo="$(api GET '')" || return 1
  jq -e '.private == true' <<<"$repo" >/dev/null || return 1
  runners="$(api GET '/actions/runners?per_page=100')" || return 1
  validate_runner_list "$runners" || return 1
  jq -e '(.total_count | numbers) < 100' <<<"$runners" >/dev/null || return 1
  jq -e --arg label "$RUNNER_LABEL" --argjson max "$MAX_RUNNERS" \
    '([.runners[] | select(.labels | any(.name == $label))] | length) < $max' \
    <<<"$runners" >/dev/null || return 1
  if [[ -n "$RUNNER_EXTRA_LABEL" ]]; then
    jq -e --arg label "$RUNNER_EXTRA_LABEL" \
      '([.runners[] | select(.labels | any(.name == $label))] | length) < 1' \
      <<<"$runners" >/dev/null || return 1
  fi
}
verify_runner_base() {
  [[ "$(cat "$RUNNER_BASE/.boxology-runner-sha256" 2>/dev/null)" = "$RUNNER_SHA256" ]] || return 1
  [[ "$(cat "$RUNNER_BASE/.boxology-runner-version" 2>/dev/null)" = "$RUNNER_VERSION" ]] || return 1
  [[ "$("$RUNNER_BASE/bin/Runner.Listener" --version 2>/dev/null)" = "$RUNNER_VERSION" ]] || return 1
}
wait_for_deregistration() {
  local runners registration_state
  for _ in 1 2 3 4 5; do
    runners="$(api GET '/actions/runners?per_page=100')" || { sleep 2; continue; }
    validate_runner_list "$runners" || { sleep 2; continue; }
    registration_state="$(runner_identity "$runners")" || { sleep 2; continue; }
    case "$registration_state" in
      absent) RUNNER_ID=; RUNNER_NAME=; return 0 ;;
      mismatch) return 1 ;;
      exact) sleep 2 ;;
      *) return 1 ;;
    esac
  done
  delete_jit_runner
}
remove_run_dir() {
  local candidate="${1-}"
  [[ -n "$candidate" && "$candidate" != / && "$candidate" == "$RUNNER_ROOT/runs/"* ]] || return 1
  rm -rf -- "$candidate"
}
stop_runner() {
  if [[ -n "$runner_pid" ]]; then
    kill "$runner_pid" 2>/dev/null || true
    wait "$runner_pid" 2>/dev/null || true
    runner_pid=
  fi
}
cleanup() {
  local cleanup_status=0
  stop_runner
  if [[ -n "$run_dir" ]]; then remove_run_dir "$run_dir" && run_dir= || cleanup_status=1; fi
  delete_jit_runner || cleanup_status=1
  unset token
  # `rm -rf` on the lock, not `rmdir`: it now holds the owner pid file, so `rmdir`
  # would fail and leak the lock on every clean exit.
  ((cleanup_status)) || { rm -rf "$LOCK" && rmdir "$RUNTIME_DIR"; } 2>/dev/null || cleanup_status=1
  return "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

token="$(security find-generic-password -a "$KEYCHAIN_ACCOUNT" -s "$KEYCHAIN_SERVICE" -w 2>/dev/null)" || exit 77
[[ "$token" =~ ^[A-Za-z0-9_]+$ ]] || exit 77

run_once() {
  local response jit status run_id payload
  [[ -z "$RUNNER_NAME$RUNNER_ID" ]] || reconcile_jit_runner || return 1
  check_repo && verify_runner_base || return 1
  run_id="$(uuidgen | tr '[:upper:]' '[:lower:]')" || return 1
  [[ "$run_id" =~ ^[0-9a-f-]{36}$ ]] || return 1
  RUNNER_NAME="$IMAGE_ID-$run_id"
  payload="$(jq -cn --arg name "$RUNNER_NAME" --argjson group "$RUNNER_GROUP_ID" --arg label "$RUNNER_LABEL" --arg extra "$RUNNER_EXTRA_LABEL" \
    '{name:$name,runner_group_id:$group,labels:(["self-hosted","macOS","ARM64",$label] + (if $extra == "" then [] else [$extra] end)),work_folder:"_work"}')" || return 1
  response="$(api POST /actions/runners/generate-jitconfig "$payload")" || { reconcile_jit_runner; return 1; }
  validate_jit_response "$RUNNER_NAME" "$response" || { reconcile_jit_runner; return 1; }
  RUNNER_ID="$(jq -er '.runner.id | numbers' <<<"$response")" || { reconcile_jit_runner; return 1; }
  jit="$(jq -er '.encoded_jit_config' <<<"$response")" || { reconcile_jit_runner; return 1; }
  run_dir="$(mktemp -d "$RUNNER_ROOT/runs/run.XXXXXX")" || { delete_jit_runner; return 1; }
  # The pinned runner base is about 435 MB. APFS clones keep each JIT runner
  # isolated while avoiding a byte-for-byte copy on every job.
  cp -ac "$RUNNER_BASE/." "$run_dir/"
  mkdir -p "$run_dir/_work/_temp" "$run_dir/_work/_tool" "$CACHE_ROOT/home/.cargo"
  status=0
  (
    cd "$run_dir"
    export HOME="$CACHE_ROOT/home" CARGO_HOME="$CACHE_ROOT/home/.cargo" CARGO_TARGET_DIR="$TARGET_ROOT" RUSTUP_HOME=/Users/jim/.rustup \
      RUNNER_TEMP="$run_dir/_work/_temp" TMPDIR="$run_dir/_work/_temp" RUNNER_TOOL_CACHE="$run_dir/_work/_tool" \
      ImageOS="$IMAGE_OS" ImageVersion="$IMAGE_VERSION" \
      CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" RUST_TEST_THREADS="$RUST_TEST_THREADS" \
      PATH="$CACHE_ROOT/home/.cargo/bin:/Users/jim/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
    unset GITHUB_TOKEN GH_TOKEN RUNNER_TOKEN ACTIONS_RUNTIME_TOKEN \
      AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AZURE_CLIENT_SECRET
    ./run.sh --disableupdate --jitconfig "$jit"
  ) >/dev/null 2>/dev/null &
  runner_pid=$!
  if wait "$runner_pid"; then status=0; else status=$?; fi
  runner_pid=
  if ((status != 0)); then delete_jit_runner || wait_for_deregistration || true; else wait_for_deregistration || status=1; fi
  printf 'runner: status=exited exit=%s\n' "$status"
  unset response payload jit
  remove_run_dir "$run_dir" || status=1
  run_dir=
  return "$status"
}

delay=5
while :; do
  if run_once; then
    delay=5
  elif ((delay < 300)); then
    delay=$((delay * 2))
    printf 'supervisor: macOS runner retry in %s seconds\n' "$delay" >&2
  fi
  sleep "$delay"
done
