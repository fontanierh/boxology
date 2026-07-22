#!/usr/bin/env bash
set -euo pipefail
umask 077
: "${REPOSITORY:?set REPOSITORY to owner/repository}"
: "${CI_RUNNER_IMAGE:?set CI_RUNNER_IMAGE to the verified image}"
KEYCHAIN_SERVICE="${KEYCHAIN_SERVICE:-com.fontanierh.boxology-ci-runner}"
KEYCHAIN_ACCOUNT="${KEYCHAIN_ACCOUNT:-$(/usr/bin/id -un)}"
RUNNER_GROUP_ID="${RUNNER_GROUP_ID:-1}"
DOCKER_CONTEXT=colima-boxology-ci-arm64
RUNTIME_DIR=/tmp/boxology-ci-runner
LOCK="$RUNTIME_DIR/supervisor.lock"
IMAGE_ID=boxology-linux-arm64-pr
IMAGE_VERSION=ubuntu-24.04-arm64-runner-2.336.0-rust-1.97.1-deny-0.20.2
BASE_DIGEST=sha256:7f622ca8766bccb22f04242ecb6f19f770b2f08827dc4b8c707de5e78a6da7ab
RUNNER_SHA256=58b758e420b87093fbd4bfddd368074960053e2f1388f01848c82624b90f27d1
RUNNER_NAME= RUNNER_ID=
[[ "$REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || exit 64
[[ "$RUNNER_GROUP_ID" =~ ^[0-9]+$ ]] || exit 64
for tool in curl docker jq security uuidgen; do command -v "$tool" >/dev/null || exit 69; done
export DOCKER_CONTEXT
[[ "$(docker context show 2>/dev/null)" = "$DOCKER_CONTEXT" ]] || exit 69
mkdir -p "$RUNTIME_DIR"; chmod 700 "$RUNTIME_DIR"
mkdir "$LOCK" 2>/dev/null || { printf '%s\n' 'runner: supervisor already running' >&2; exit 75; }

container= volume= token=
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
dispose_resource() {
  local kind=$1 name=$2 inspect_error
  if inspect_error="$(docker inspect "$name" 2>&1 >/dev/null)"; then
    case "$kind" in
      container) docker rm -f "$name" >/dev/null 2>&1 || return 1 ;;
      volume) docker volume rm "$name" >/dev/null 2>&1 || return 1 ;;
      *) return 1 ;;
    esac
  else
    case "$inspect_error" in
      *"No such object:"*|*"no such volume"*) docker info >/dev/null 2>&1 || return 1 ;;
      *) return 1 ;;
    esac
  fi
}
dispose_runner() {
  local status=0
  if [[ -n "$container" ]]; then
    dispose_resource container "$container" && container= || status=1
  fi
  if [[ -n "$volume" ]]; then
    dispose_resource volume "$volume" && volume= || status=1
  fi
  return "$status"
}
cleanup() {
  local status=0; dispose_runner || status=1; delete_jit_runner || status=1
  unset token
  ((status)) || rmdir "$LOCK" "$RUNTIME_DIR" 2>/dev/null || status=1
  return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

token="$(security find-generic-password -a "$KEYCHAIN_ACCOUNT" -s "$KEYCHAIN_SERVICE" -w 2>/dev/null)" || exit 77
[[ "$token" =~ ^[A-Za-z0-9_]+$ ]] || exit 77

api() {
  local method=$1 path=$2 payload=${3-}
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
  jq -e '[.runners[] | select(.labels | any(.name == "boxology-linux-arm64-pr"))] | length == 0' \
    <<<"$runners" >/dev/null
}
verify_image() {
  local arch image_id image_version base_digest runner_sha
  arch="$(docker image inspect --format '{{.Architecture}}' "$CI_RUNNER_IMAGE" 2>/dev/null)" || return 1
  image_id="$(docker image inspect --format '{{index .Config.Labels "org.boxology.ci.image-id"}}' "$CI_RUNNER_IMAGE" 2>/dev/null)" || return 1
  image_version="$(docker image inspect --format '{{index .Config.Labels "org.boxology.ci.image-version"}}' "$CI_RUNNER_IMAGE" 2>/dev/null)" || return 1
  base_digest="$(docker image inspect --format '{{index .Config.Labels "org.boxology.ci.base-digest"}}' "$CI_RUNNER_IMAGE" 2>/dev/null)" || return 1
  runner_sha="$(docker image inspect --format '{{index .Config.Labels "org.boxology.ci.runner-sha256"}}' "$CI_RUNNER_IMAGE" 2>/dev/null)" || return 1
  [[ "$arch" = arm64 && "$image_id" = "$IMAGE_ID" && "$image_version" = "$IMAGE_VERSION" && "$base_digest" = "$BASE_DIGEST" && "$runner_sha" = "$RUNNER_SHA256" ]]
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
run_once() {
  local iteration=${1} payload response jit status diagnostics run_id candidate_container candidate_volume
  [[ -z "$container$volume" ]] || dispose_runner || return 1
  [[ -z "$RUNNER_NAME$RUNNER_ID" ]] || reconcile_jit_runner || return 1
  check_repo && verify_image || return 1
  run_id="$(uuidgen | tr '[:upper:]' '[:lower:]')" || return 1
  [[ "$run_id" =~ ^[0-9a-f-]{36}$ ]] || return 1
  RUNNER_NAME="${IMAGE_ID}-${run_id}"
  payload="$(jq -cn --arg name "$RUNNER_NAME" --argjson group "$RUNNER_GROUP_ID" \
    '{name:$name,runner_group_id:$group,labels:["self-hosted","linux","ARM64","boxology-linux-arm64-pr"],work_folder:"_work"}')" || return 1
  response="$(api POST /actions/runners/generate-jitconfig "$payload")" || { reconcile_jit_runner; return 1; }
  validate_jit_response "$RUNNER_NAME" "$response" || { reconcile_jit_runner; return 1; }
  RUNNER_ID="$(jq -er '.runner.id | numbers' <<<"$response")" || { reconcile_jit_runner; return 1; }
  jit="$(jq -er '.encoded_jit_config' <<<"$response")" || { reconcile_jit_runner; return 1; }
  candidate_container="${IMAGE_ID}-${run_id}"; candidate_volume="${candidate_container}-work"
  if docker volume inspect "$candidate_volume" >/dev/null 2>&1 || docker container inspect "$candidate_container" >/dev/null 2>&1; then delete_jit_runner; return 1; fi
  volume="$candidate_volume"; if ! docker volume create "$volume" >/dev/null; then delete_jit_runner; return 1; fi; container="$candidate_container"
  status=0
  printf '%s' "$jit" | docker run --name "$container" --network bridge --user runner --init \
      --read-only --cpus 4 --memory 8g --memory-swap 8g --cap-drop=ALL --security-opt no-new-privileges:true --pids-limit 512 \
      --tmpfs /tmp:rw,noexec,nosuid,size=256m --tmpfs /run:rw,noexec,nosuid,size=16m \
      --mount "type=volume,source=$volume,target=/runner" \
      --env ImageOS=ubuntu24.04-arm64-colima \
      --env ImageVersion="$IMAGE_VERSION" --env HOME=/runner/home --env CARGO_HOME=/runner/_work/.cargo \
      --env RUNNER_TEMP=/runner/_work/_temp \
      "$CI_RUNNER_IMAGE" >/dev/null 2>/dev/null &
  docker_pid=$!
  if wait "$docker_pid"; then
    status=0
  else
    status=$?
  fi
  if ((status != 0)); then delete_jit_runner || wait_for_deregistration || true; else wait_for_deregistration || status=1; fi
  diagnostics="$(docker inspect --format 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}}' "$container" 2>/dev/null || printf '%s' 'state=unavailable')"
  printf 'runner: %s\n' "$diagnostics"
  unset response payload jit
  dispose_runner || status=1
  return "$status"
}

delay=5
iteration=0
while :; do
  iteration=$((iteration + 1))
  if run_once "$iteration"; then
    delay=5
  elif ((delay < 300)); then
    delay=$((delay * 2))
  fi
  sleep "$delay"
done
