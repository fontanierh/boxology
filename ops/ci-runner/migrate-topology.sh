#!/bin/bash
set -euo pipefail
umask 077

REPOSITORY=fontanierh/boxology
INSTALL_ROOT=/Users/jim/.crab/ci-runner
LAUNCH_AGENTS=/Users/jim/Library/LaunchAgents
BACKUP_ROOT="$INSTALL_ROOT/topology-backups"
STAGE_ROOT="$INSTALL_ROOT/topology-stage"
LOCK="$INSTALL_ROOT/topology-migration.lock"
SCRIPT_ROOT="$(cd "$(dirname "$0")" && pwd -P)"
TIMEOUT_SECONDS=1800
STALE_RUN_AGE_SECONDS=86400
mutated=0
backup=
lock_acquired=0
verified_backup=
linux_base_source=
ack_stale_run=

base_labels='com.fontanierh.boxology-ci-runner
com.fontanierh.boxology-ci-macos-runner'
slot_labels='com.fontanierh.boxology-ci-runner-slots
com.fontanierh.boxology-ci-macos-runner-slots'
extra_labels='com.fontanierh.boxology-ci-runner-slots-extra
com.fontanierh.boxology-ci-macos-runner-slots-extra'
script_names='supervise.sh
supervise-macos.sh
supervise-slots.sh'

die() { printf 'runner migration: %s\n' "$*" >&2; exit 1; }

cleanup() {
  status=$?
  if ((lock_acquired == 1)); then rmdir "$LOCK"; fi
  if ((status != 0 && mutated == 1)); then
    printf 'runner migration: FAILED SAFE; workflow dispatch was not restored\n' >&2
    [[ -z "$backup" ]] || printf 'runner migration: restore with %s restore %s%s\n' "$0" "$backup" "${ack_stale_run:+ --ack-stale-run $ack_stale_run}" >&2
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

require_tools() {
  for command in gh jq launchctl plutil shasum; do
    command -v "$command" >/dev/null || die "missing command: $command"
  done
  [[ -x /usr/libexec/PlistBuddy ]] || die 'missing PlistBuddy'
  mkdir -p "$BACKUP_ROOT"
  chmod 700 "$BACKUP_ROOT"
  BACKUP_ROOT="$(cd "$BACKUP_ROOT" && pwd -P)"
  mkdir "$LOCK" 2>/dev/null || die "migration already active: $LOCK"
  lock_acquired=1
}

runner_count() {
  label=$1
  response="$(gh api "repos/$REPOSITORY/actions/runners?per_page=100")" || return 2
  jq -e '(.total_count | type == "number" and . == floor and . >= 0) and (.runners | type == "array") and (.total_count == (.runners | length)) and all(.runners[]; (.id | type == "number" and . == floor and . > 0) and (.name | type == "string") and (.status | type == "string") and (.busy | type == "boolean") and (.labels | type == "array") and all(.labels[]; .name | type == "string"))' <<< "$response" >/dev/null || return 2
  jq -er --arg label "$label" '[.runners[] | select(.labels | any(.name == $label))] | length' <<< "$response" || return 2
}

strict_epoch() {
  parsed="$(date -j -u -f '%Y-%m-%dT%H:%M:%SZ' "$1" +%s 2>/dev/null)" || return 2
  [[ "$(date -u -r "$parsed" '+%Y-%m-%dT%H:%M:%SZ')" = "$1" ]] || return 2
  printf '%s\n' "$parsed"
}

validate_stale_ack() {
  id=$1; now="$(date -u +%s)" || return 2
  run="$(gh api "repos/$REPOSITORY/actions/runs/$id")" || return 2
  jobs="$(gh api "repos/$REPOSITORY/actions/runs/$id/jobs?per_page=100")" || return 2
  repo="$(gh api "repos/$REPOSITORY")" || return 2
  jq -e --argjson id "$id" '(.id == $id) and (.status | type == "string" and . != "completed") and (.updated_at | type == "string") and (.event == "pull_request") and (.pull_requests == []) and (.head_branch | type == "string" and length > 0) and (.head_sha | type == "string" and test("^[0-9a-f]{40}$"))' <<< "$run" >/dev/null || return 2
  jq -e '(.total_count == 0) and (.jobs | type == "array" and length == 0)' <<< "$jobs" >/dev/null || return 2
  updated="$(jq -er .updated_at <<< "$run")" || return 2; epoch="$(strict_epoch "$updated")" || return 2
  age=$((now - epoch)); ((age > STALE_RUN_AGE_SECONDS)) || return 2
  default_branch="$(jq -er '.default_branch | strings | select(length > 0)' <<< "$repo")" || return 2
  encoded_default="$(jq -rn --arg value "$default_branch" '$value | @uri')" || return 2
  default_ref="$(gh api "repos/$REPOSITORY/git/ref/heads/$encoded_default")" || return 2
  default_sha="$(jq -er '.object.sha | strings | select(test("^[0-9a-f]{40}$"))' <<< "$default_ref")" || return 2
  head_sha="$(jq -er .head_sha <<< "$run")" || return 2; [[ "$head_sha" != "$default_sha" ]] || return 2
  head_branch="$(jq -er .head_branch <<< "$run")" || return 2; encoded_head="$(jq -rn --arg value "$head_branch" '$value | @uri')" || return 2
  if ref_result="$(gh api --include "repos/$REPOSITORY/git/ref/heads/$encoded_head" 2>&1)"; then return 2; fi
  grep -Eq '^HTTP/[0-9.]+ 404 ' <<< "$ref_result" || return 2
  printf 'runner migration: acknowledged stale control-plane run id=%s age_seconds=%s\n' "$id" "$age" >&2
}

active_run_count() {
  runs="$(gh run list --repo "$REPOSITORY" --all --limit 1000 --json databaseId,status)" || return 2
  jq -e 'type == "array" and all(.[]; (.databaseId | type == "number") and (.status | type == "string"))' <<< "$runs" >/dev/null || return 2
  [[ -z "$ack_stale_run" ]] || validate_stale_ack "$ack_stale_run" || return 2
  jq -er --argjson id "${ack_stale_run:-0}" '[.[] | select(.status != "completed" and .databaseId != $id)] | length' <<< "$runs" || return 2
}

busy_runner_count() {
  response="$(gh api "repos/$REPOSITORY/actions/runners?per_page=100")" || return 2
  jq -e '(.total_count | type == "number" and . == floor and . >= 0) and (.runners | type == "array") and (.total_count == (.runners | length)) and all(.runners[]; (.id | type == "number" and . == floor and . > 0) and (.name | type == "string") and (.status | type == "string") and (.busy | type == "boolean") and (.labels | type == "array") and all(.labels[]; .name | type == "string"))' <<< "$response" >/dev/null || return 2
  jq -er '[.runners[] | select(.busy)] | length' <<< "$response" || return 2
}

wait_until() {
  description=$1
  shift
  deadline=$((SECONDS + TIMEOUT_SECONDS))
  while true; do
    if "$@"; then return 0; else status=$?; fi
    [[ "$status" = 1 ]] || die "$description check failed"
    ((SECONDS < deadline)) || die "timed out waiting for $description"
    sleep 5
  done
}

drained() { active="$(active_run_count)" || return $?; busy="$(busy_runner_count)" || return $?; [[ "$active" = 0 && "$busy" = 0 ]]; }
label_is_zero() { count="$(runner_count "$1")" || return $?; [[ "$count" = 0 ]]; }
label_is_count() { count="$(runner_count "$1")" || return $?; [[ "$count" = "$2" ]]; }

save_active_workflows() {
  destination=$1
  gh workflow list --repo "$REPOSITORY" --all --limit 1000 --json id,state |
    jq -r '.[] | select(.state == "active") | .id' > "$destination"
}

workflows_disabled() {
  states="$(gh workflow list --repo "$REPOSITORY" --all --limit 1000 --json state --jq '.[] | select(.state == "active") | .state')" || return 2
  [[ -z "$states" ]]
}

disable_dispatch() {
  mutated=1
  while IFS= read -r workflow; do
    [[ -z "$workflow" ]] && continue
    [[ "$workflow" =~ ^[0-9]+$ ]] || die 'saved workflow ID is malformed'
    gh workflow disable "$workflow" --repo "$REPOSITORY"
  done < "$backup/active-workflows"
  wait_until 'workflow dispatch to be disabled' workflows_disabled
  wait_until 'queued and running Actions work to drain' drained
  workflows_disabled || die 'workflow dispatch changed during drain'
}

restore_dispatch() {
  source_file=$1
  while IFS= read -r workflow; do
    [[ -z "$workflow" ]] && continue
    [[ "$workflow" =~ ^[0-9]+$ ]] || die 'saved workflow ID is malformed'
    gh workflow enable "$workflow" --repo "$REPOSITORY"
  done < "$source_file"
}

snapshot_installation() {
  mkdir -p "$BACKUP_ROOT"
  backup="$(mktemp -d "$BACKUP_ROOT/$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")"
  mkdir "$backup/installed"
  : > "$backup/absent-files"
  : > "$backup/loaded-labels"
  save_active_workflows "$backup/active-workflows"
  for name in $script_names $base_labels $slot_labels $extra_labels; do
    case "$name" in
      *.sh) source_path="$INSTALL_ROOT/$name" ;;
      *) source_path="$LAUNCH_AGENTS/$name.plist" ;;
    esac
    [[ ! -L "$source_path" ]] || die "refusing to back up symlink: $source_path"
    if [[ -f "$source_path" ]]; then
      cp -p "$source_path" "$backup/installed/$(basename "$source_path")"
    else
      printf '%s\n' "$source_path" >> "$backup/absent-files"
    fi
  done
  for label in $base_labels $slot_labels $extra_labels; do
    if launchctl print "gui/$(id -u)/$label" >/dev/null 2>&1; then printf '%s\n' "$label" >> "$backup/loaded-labels"; fi
  done
  (
    cd "$backup"
    shasum -a 256 active-workflows absent-files loaded-labels
    for saved in installed/*; do [[ ! -e "$saved" ]] || shasum -a 256 "$saved"; done
  ) > "$backup/SHA256SUMS"
  chmod 400 "$backup/active-workflows" "$backup/absent-files" "$backup/loaded-labels" "$backup/SHA256SUMS"
  for saved in "$backup"/installed/*; do [[ ! -e "$saved" ]] || chmod 400 "$saved"; done
  chmod 500 "$backup/installed" "$backup"
  printf '%s\n' "$backup"
}

verify_backup() {
  candidate=$1
  [[ -d "$candidate" && ! -L "$candidate" ]] || die 'backup is not a real directory'
  candidate="$(cd "$candidate" && pwd -P)"
  [[ "$candidate" = "$BACKUP_ROOT"/* && -d "$candidate/installed" && ! -L "$candidate/installed" ]] ||
    die 'backup is outside the backup root'
  [[ -f "$candidate/SHA256SUMS" && -f "$candidate/active-workflows" && -f "$candidate/loaded-labels" ]] ||
    die 'backup is incomplete'
  (cd "$candidate" && shasum -a 256 -c SHA256SUMS >/dev/null)
  for saved in "$candidate"/installed/*; do
    [[ ! -e "$saved" ]] && continue
    [[ ! -L "$saved" && -f "$saved" ]] || die 'backup contains a non-regular installed file'
  done
  while IFS= read -r workflow; do
    [[ -z "$workflow" || "$workflow" =~ ^[0-9]+$ ]] || die 'backup has a malformed workflow ID'
  done < "$candidate/active-workflows"
  while IFS= read -r label; do
    [[ -z "$label" ]] && continue
    case "$label" in
      com.fontanierh.boxology-ci-runner|com.fontanierh.boxology-ci-macos-runner|com.fontanierh.boxology-ci-runner-slots|com.fontanierh.boxology-ci-macos-runner-slots|com.fontanierh.boxology-ci-runner-slots-extra|com.fontanierh.boxology-ci-macos-runner-slots-extra) ;;
      *) die 'backup has an unknown loaded label' ;;
    esac
    [[ -f "$candidate/installed/$label.plist" ]] || die "backup lacks loaded plist: $label"
  done < "$candidate/loaded-labels"
  verified_backup=$candidate
}

preflight_activate() {
  for name in $script_names; do
    [[ -f "$SCRIPT_ROOT/$name" ]] || die "reviewed script is missing: $name"
  done
  for label in $slot_labels com.fontanierh.boxology-ci-macos-runner; do
    [[ -f "$SCRIPT_ROOT/$label.plist" ]] || die "reviewed plist is missing: $label"
    plutil -lint "$SCRIPT_ROOT/$label.plist" >/dev/null
  done
  for label in $base_labels $slot_labels $extra_labels; do
    installed="$LAUNCH_AGENTS/$label.plist"
    [[ ! -L "$installed" ]] || die "installed plist is a symlink: $label"
    [[ ! -f "$installed" ]] || plutil -lint "$installed" >/dev/null
  done
  installed_linux="$LAUNCH_AGENTS/com.fontanierh.boxology-ci-runner.plist"
  staged_linux="$STAGE_ROOT/com.fontanierh.boxology-ci-runner.plist"
  if [[ -f "$installed_linux" ]]; then
    linux_base_source=$installed_linux
  else
    [[ -f "$staged_linux" && ! -L "$staged_linux" ]] || die "resolved Linux base plist is missing: $staged_linux"
    plutil -lint "$staged_linux" >/dev/null
    linux_base_source=$staged_linux
  fi
  if grep -Eq '/ABSOLUTE/PATH|OWNER/REPOSITORY|VERIFIED-IMAGE' "$linux_base_source"; then
    die 'selected Linux base plist contains repository placeholders'
  fi
}

bootout_if_loaded() {
  label=$1
  if launchctl print "gui/$(id -u)/$label" >/dev/null 2>&1; then
    launchctl bootout "gui/$(id -u)/$label"
  fi
}

stop_all_supervisors() {
  drained || die 'drain changed immediately before supervisor bootout'
  for label in $extra_labels $slot_labels $base_labels; do bootout_if_loaded "$label"; done
  wait_until 'Linux JIT registrations to reconcile to zero' label_is_zero boxology-linux-arm64-pr
  wait_until 'macOS JIT registrations to reconcile to zero' label_is_zero boxology-macos-pr
}

install_bounded_files() {
  install -m 700 "$SCRIPT_ROOT/supervise.sh" "$INSTALL_ROOT/supervise.sh"
  install -m 700 "$SCRIPT_ROOT/supervise-macos.sh" "$INSTALL_ROOT/supervise-macos.sh"
  install -m 700 "$SCRIPT_ROOT/supervise-slots.sh" "$INSTALL_ROOT/supervise-slots.sh"
  for label in $slot_labels com.fontanierh.boxology-ci-macos-runner; do
    install -m 600 "$SCRIPT_ROOT/$label.plist" "$LAUNCH_AGENTS/$label.plist"
  done
  linux_base="$LAUNCH_AGENTS/com.fontanierh.boxology-ci-runner.plist"
  if [[ "$linux_base_source" != "$linux_base" ]]; then install -m 600 "$linux_base_source" "$linux_base"; fi
  if grep -Eq '/ABSOLUTE/PATH|OWNER/REPOSITORY|VERIFIED-IMAGE' "$linux_base"; then
    die 'Linux base plist still contains repository placeholders'
  fi
  if /usr/libexec/PlistBuddy -c 'Print :EnvironmentVariables:MAX_RUNNERS' "$linux_base" >/dev/null 2>&1; then
    /usr/libexec/PlistBuddy -c 'Delete :EnvironmentVariables:MAX_RUNNERS' "$linux_base"
  fi
  /usr/libexec/PlistBuddy -c 'Add :EnvironmentVariables:MAX_RUNNERS string 4' "$linux_base"
  for label in $extra_labels; do rm -f "$LAUNCH_AGENTS/$label.plist"; done
  for label in $base_labels $slot_labels; do plutil -lint "$LAUNCH_AGENTS/$label.plist" >/dev/null; done
}

bootstrap_and_expect() {
  label=$1
  runner_label=$2
  expected=$3
  launchctl bootstrap "gui/$(id -u)" "$LAUNCH_AGENTS/$label.plist"
  wait_until "$runner_label registrations to reach $expected" label_is_count "$runner_label" "$expected"
}

start_bounded_topology() {
  bootstrap_and_expect com.fontanierh.boxology-ci-runner boxology-linux-arm64-pr 1
  bootstrap_and_expect com.fontanierh.boxology-ci-runner-slots boxology-linux-arm64-pr 4
  bootstrap_and_expect com.fontanierh.boxology-ci-macos-runner boxology-macos-pr 1
  bootstrap_and_expect com.fontanierh.boxology-ci-macos-runner-slots boxology-macos-pr 4
}

restore_files() {
  source_backup=$1
  for name in $script_names; do
    saved="$source_backup/installed/$name"
    destination="$INSTALL_ROOT/$name"
    if [[ -f "$saved" ]]; then cp -p "$saved" "$destination"; else rm -f "$destination"; fi
  done
  for label in $base_labels $slot_labels $extra_labels; do
    saved="$source_backup/installed/$label.plist"
    destination="$LAUNCH_AGENTS/$label.plist"
    if [[ -f "$saved" ]]; then cp -p "$saved" "$destination"; else rm -f "$destination"; fi
  done
}

start_restored_topology() {
  source_backup=$1
  expected=0
  for label in com.fontanierh.boxology-ci-runner com.fontanierh.boxology-ci-runner-slots com.fontanierh.boxology-ci-runner-slots-extra; do
    if grep -Fxq "$label" "$source_backup/loaded-labels"; then loaded=1; else status=$?; [[ "$status" = 1 ]] || die 'could not read loaded-labels'; loaded=0; fi
    [[ "$loaded" = 1 ]] || continue
    plist="$LAUNCH_AGENTS/$label.plist"
    [[ -f "$plist" ]] || continue
    plutil -lint "$plist" >/dev/null
    case "$label" in
      com.fontanierh.boxology-ci-runner) added=1 ;;
      *) added="$(/usr/libexec/PlistBuddy -c 'Print :EnvironmentVariables:SLOT_COUNT' "$plist")" ;;
    esac
    expected=$((expected + added))
    launchctl bootstrap "gui/$(id -u)" "$plist"
    wait_until "restored Linux registrations to reach $expected" label_is_count boxology-linux-arm64-pr "$expected"
  done
  expected=0
  for label in com.fontanierh.boxology-ci-macos-runner com.fontanierh.boxology-ci-macos-runner-slots com.fontanierh.boxology-ci-macos-runner-slots-extra; do
    if grep -Fxq "$label" "$source_backup/loaded-labels"; then loaded=1; else status=$?; [[ "$status" = 1 ]] || die 'could not read loaded-labels'; loaded=0; fi
    [[ "$loaded" = 1 ]] || continue
    plist="$LAUNCH_AGENTS/$label.plist"
    [[ -f "$plist" ]] || continue
    plutil -lint "$plist" >/dev/null
    case "$label" in
      com.fontanierh.boxology-ci-macos-runner) added=1 ;;
      *) added="$(/usr/libexec/PlistBuddy -c 'Print :EnvironmentVariables:SLOT_COUNT' "$plist")" ;;
    esac
    expected=$((expected + added))
    launchctl bootstrap "gui/$(id -u)" "$plist"
    wait_until "restored macOS registrations to reach $expected" label_is_count boxology-macos-pr "$expected"
  done
}

activate() {
  preflight_activate
  snapshot_installation
  disable_dispatch
  stop_all_supervisors
  install_bounded_files
  start_bounded_topology
  restore_dispatch "$backup/active-workflows"
  mutated=0
  printf 'runner migration: active; backup=%s\n' "$backup"
}

restore() {
  target=$1
  verify_backup "$target"
  target=$verified_backup
  snapshot_installation
  current_backup=$backup
  disable_dispatch
  stop_all_supervisors
  restore_files "$target"
  start_restored_topology "$target"
  restore_dispatch "$target/active-workflows"
  mutated=0
  printf 'runner migration: restored=%s recovery-backup=%s\n' "$target" "$current_backup"
}

require_tools
case "${1:-}" in
  activate) [[ $# = 1 || ($# = 3 && "$2" = --ack-stale-run && "$3" =~ ^[0-9]+$) ]] || die 'usage: migrate-topology.sh activate [--ack-stale-run RUN_ID]'; [[ $# = 1 ]] || ack_stale_run=$3; activate ;;
  restore) [[ $# = 2 || ($# = 4 && "$3" = --ack-stale-run && "$4" =~ ^[0-9]+$) ]] || die 'usage: migrate-topology.sh restore BACKUP [--ack-stale-run RUN_ID]'; [[ $# = 2 ]] || ack_stale_run=$4; restore "$2" ;;
  *) die 'usage: migrate-topology.sh activate [--ack-stale-run RUN_ID] | restore BACKUP [--ack-stale-run RUN_ID]' ;;
esac
