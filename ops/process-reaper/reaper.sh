#!/bin/bash
# Boxology review-orphan reaper. macOS Bash 3.2. Fail closed. See README.md.
set -euo pipefail
umask 077

PS_BIN="${REAPER_PS:-/bin/ps}"; LSOF_BIN="${REAPER_LSOF:-/usr/sbin/lsof}"
KILL_BIN="${REAPER_KILL:-/bin/kill}"; DATE_BIN="${REAPER_DATE:-/bin/date}"
REALPATH_BIN="${REAPER_REALPATH:-/bin/realpath}"
REVIEWS_ROOT="${REAPER_REVIEWS_ROOT:-/Users/jim/.codex/reviews}"
WORKTREES_ROOT="${REAPER_WORKTREES_ROOT:-/Users/jim/module-based-engineering/.git/worktrees}"
DENY_ROOTS="${REAPER_DENY_ROOTS:-/Users/jim/.codex/worktrees:/Users/jim/module-based-engineering:/Users/jim/.crab:/Users/jim/crab-source:/Users/jim/crab-bin:/tmp/boxology-ci:/tmp/boxology-ci-macos-runner}"
STATE_DIR="${REAPER_STATE_DIR:-/Users/jim/.codex/process-reaper/state}"
DRY_RUN="${REAPER_DRY_RUN:-1}"; MIN_AGE_S="${REAPER_MIN_AGE_S:-3600}"
STABLE_S="${REAPER_STABLE_S:-600}"; GRACE_S="${REAPER_GRACE_S:-60}"; MAX_STATE=512
SELF_PID=$$; SELF_UID="$(/usr/bin/id -u)"

die_cfg() { printf 'reaper: unsafe config\n' >&2; exit 64; }
now_epoch() { [[ -n "${REAPER_NOW_EPOCH:-}" ]] && printf '%s\n' "$REAPER_NOW_EPOCH" || "$DATE_BIN" '+%s'; }
canon() { "$REALPATH_BIN" "$1" 2>/dev/null; }
beneath() { [[ -n "$1" && -n "$2" && "$2" != / && "$1" == "$2"/* ]]; }
under() { [[ "$1" == "$2" || "$1" == "$2"/* ]]; }
emit() {
  local w="${3##*/}"; w="${w//[^A-Za-z0-9._-]/_}"; [[ -n "$w" ]] || w=-
  printf 'epoch=%s pid=%s age=%s worktree=%s action=%s reason=%s\n' "$(now_epoch)" "$1" "$2" "$w" "$4" "$5"
}
lstart_epoch() { "$DATE_BIN" -j -f '%a %b %e %T %Y' "$1" '+%s' 2>/dev/null; }

[[ -n "$REVIEWS_ROOT" && "$REVIEWS_ROOT" != / && -n "$WORKTREES_ROOT" && "$WORKTREES_ROOT" != / ]] || die_cfg
[[ -n "$STATE_DIR" && "$STATE_DIR" != / && -n "$DENY_ROOTS" ]] || die_cfg
[[ "$DRY_RUN" == 0 || "$DRY_RUN" == 1 ]] || die_cfg
[[ "$MIN_AGE_S" =~ ^[0-9]+$ && "$STABLE_S" =~ ^[0-9]+$ && "$GRACE_S" =~ ^[0-9]+$ ]] || die_cfg
REVIEWS_ROOT="$(canon "$REVIEWS_ROOT")" || die_cfg
WORKTREES_ROOT="$(canon "$WORKTREES_ROOT")" || die_cfg
COMMON_GIT="$(canon "$WORKTREES_ROOT/..")" || die_cfg
[[ -n "$COMMON_GIT" && "$COMMON_GIT" != / ]] || die_cfg
mkdir -p "$STATE_DIR" && chmod 700 "$STATE_DIR" || die_cfg
STATE_DIR="$(canon "$STATE_DIR")" || die_cfg
[[ "$STATE_DIR" != / ]] || die_cfg

DENY_CANON=; old_ifs=$IFS; IFS=:
for d in $DENY_ROOTS; do
  IFS=$old_ifs; [[ -n "$d" && "$d" != / ]] || die_cfg
  c="$(canon "$d" 2>/dev/null || printf '%s\n' "$d")"
  [[ -n "$c" && "$c" != / ]] || die_cfg
  DENY_CANON="${DENY_CANON}${DENY_CANON:+:}${c}"
done; IFS=$old_ifs

denied() {
  local p="$1" c old=$IFS; IFS=:
  for c in $DENY_CANON; do IFS=$old; under "$p" "$c" && return 0; done
  IFS=$old; return 1
}
sp() { printf '%s/%s\n' "$STATE_DIR" "$1"; }
scount() { local n=0 f; for f in "$STATE_DIR"/*; do [[ -f "$f" && "${f##*/}" =~ ^[0-9]+$ ]] && n=$((n+1)); done; echo "$n"; }
wstate() {
  local t; t="$(sp ".$1.tmp.$$")"
  printf 'lstart=%s\ncwd=%s\nfirst_seen=%s\nterm_sent=%s\n' "$2" "$3" "$4" "$5" >"$t"
  mv -f "$t" "$(sp "$1")"
}
cstate() { rm -f "$(sp "$1")"; }
rstate() {
  local f="$1" k v; S_LSTART=; S_CWD=; S_FIRST=; S_TERM=
  [[ -f "$f" ]] || return 1
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    k="${line%%=*}"; v="${line#*=}"
    case "$k" in lstart) S_LSTART=$v;; cwd) S_CWD=$v;; first_seen) S_FIRST=$v;; term_sent) S_TERM=$v;; *) return 1;; esac
  done <"$f"
  [[ -n "$S_LSTART" && -n "$S_CWD" && "$S_FIRST" =~ ^[0-9]+$ ]] || return 1
  [[ -z "$S_TERM" || "$S_TERM" =~ ^[0-9]+$ ]] || return 1
}

cwd_of() {
  local pid="$1" line path=
  while IFS= read -r line || [[ -n "$line" ]]; do case "$line" in n*) path="${line#n}";; esac
  done < <("$LSOF_BIN" -a -p "$pid" -d cwd -Fn 2>/dev/null) || true
  [[ -n "$path" ]] && printf '%s\n' "$path"
}
gitdir_of() {
  local dir="$1" gitf parent content raw_gpath raw_parent gpath back common name
  while beneath "$dir" "$REVIEWS_ROOT"; do
    gitf="$dir/.git"
    if [[ -e "$gitf" ]]; then
      [[ -f "$gitf" && ! -L "$gitf" ]] || return 1
      IFS= read -r content <"$gitf" || return 1
      case "$content" in 'gitdir: /'*) raw_gpath="${content#gitdir: }";; *) return 1;; esac
      name="${raw_gpath##*/}"; raw_parent="$(canon "$raw_gpath/..")" || return 1
      [[ "$raw_parent" == "$WORKTREES_ROOT" && -n "$name" && ! -L "$raw_gpath" && -d "$raw_gpath" ]] || return 1
      gpath="$(canon "$raw_gpath")" || return 1
      [[ -f "$gpath/gitdir" && ! -L "$gpath/gitdir" ]] || return 1
      [[ -f "$gpath/commondir" && ! -L "$gpath/commondir" ]] || return 1
      IFS= read -r back <"$gpath/gitdir" || return 1
      [[ -n "$back" ]] || return 1
      case "$back" in /*) ;; *) back="$gpath/$back";; esac
      back="$(canon "$back")" || return 1
      gitf="$(canon "$gitf")" || return 1
      [[ "$back" == "$gitf" ]] || return 1
      IFS= read -r common <"$gpath/commondir" || return 1
      [[ -n "$common" ]] || return 1
      case "$common" in /*) ;; *) common="$gpath/$common";; esac
      common="$(canon "$common")" || return 1
      [[ "$common" == "$COMMON_GIT" ]] || return 1
      printf '%s\n' "$dir"; return 0
    fi
    parent="$(canon "$dir/..")" || return 1
    [[ "$parent" != "$dir" ]] || return 1
    dir=$parent
  done
  return 1
}

# On success: cwd|wt|age|lstart
validate_pid() {
  local pid="$1" el="${2:-}" ec="${3:-}" ppid uid lstart cwd can age wt epoch_ls line
  [[ "$pid" =~ ^[1-9][0-9]*$ && "$pid" -ne "$SELF_PID" ]] || return 1
  line="$("$PS_BIN" -p "$pid" -o pid=,ppid=,uid=,lstart= 2>/dev/null)" || return 1
  line="${line#"${line%%[![:space:]]*}"}"; [[ -n "$line" ]] || return 1
  set -- $line; [[ $# -ge 8 ]] || return 1
  pid=$1; ppid=$2; uid=$3; shift 3; lstart="$*"
  [[ "$pid" =~ ^[0-9]+$ && "$ppid" == 1 && "$uid" == "$SELF_UID" ]] || return 1
  [[ "$pid" -ne 0 && "$pid" -ne 1 && "$pid" -ne "$SELF_PID" ]] || return 1
  epoch_ls="$(lstart_epoch "$lstart")" || return 1
  age=$(( $(now_epoch) - epoch_ls )); [[ "$age" -ge "$MIN_AGE_S" ]] || return 1
  cwd="$(cwd_of "$pid")" || return 1
  can="$(canon "$cwd")" || return 1
  beneath "$can" "$REVIEWS_ROOT" || return 1
  denied "$can" && return 1
  wt="$(gitdir_of "$can")" || return 1
  [[ -n "$el" && "$lstart" != "$el" ]] && return 1
  [[ -n "$ec" && "$can" != "$ec" ]] && return 1
  printf '%s|%s|%s|%s\n' "$can" "$wt" "$age" "$lstart"
}

do_signal() {
  local sig="$1" pid="$2"
  [[ "$DRY_RUN" == 1 ]] && return 0
  "$KILL_BIN" "-$sig" "$pid" 2>/dev/null
}

PS_FILE=$(mktemp "$STATE_DIR/.ps.XXXXXX")
SEEN_FILE=$(mktemp "$STATE_DIR/.seen.XXXXXX")
CAND_FILE=$(mktemp "$STATE_DIR/.cand.XXXXXX")
trap 'rm -f "$PS_FILE" "$SEEN_FILE" "$CAND_FILE"' EXIT
"$PS_BIN" -axo pid=,ppid=,uid=,lstart= >"$PS_FILE" 2>/dev/null || { printf 'reaper: ps failed\n' >&2; exit 1; }

while IFS= read -r line || [[ -n "$line" ]]; do
  line="${line#"${line%%[![:space:]]*}"}"; [[ -z "$line" ]] && continue
  set -- $line
  [[ $# -ge 8 ]] || { printf 'reaper: unparsable ps\n' >&2; exit 1; }
  pid=$1; ppid=$2; uid=$3; shift 3; lstart="$*"
  [[ "$pid" =~ ^[0-9]+$ && "$ppid" =~ ^[0-9]+$ && "$uid" =~ ^[0-9]+$ ]] || { printf 'reaper: unparsable ps\n' >&2; exit 1; }
  lstart_epoch "$lstart" >/dev/null || { printf 'reaper: unparsable ps\n' >&2; exit 1; }
  printf '%s\n' "$pid" >>"$CAND_FILE"
done <"$PS_FILE"

NOW=$(now_epoch)
while IFS= read -r pid || [[ -n "$pid" ]]; do
  [[ -n "$pid" ]] || continue
  info=$(validate_pid "$pid" || true); [[ -n "$info" ]] || continue
  cwd=${info%%|*}; rest=${info#*|}; wt=${rest%%|*}; rest=${rest#*|}; age=${rest%%|*}; lstart=${rest#*|}
  printf '%s\n' "$pid" >>"$SEEN_FILE"
  f=$(sp "$pid")
  if [[ -f "$f" ]] && ! rstate "$f"; then cstate "$pid"; emit "$pid" "$age" "$wt" skip corrupt_state; fi
  if [[ -f "$f" ]] && rstate "$f"; then
    if [[ "$S_LSTART" != "$lstart" || "$S_CWD" != "$cwd" ]]; then
      cstate "$pid"; emit "$pid" "$age" "$wt" skip fingerprint_changed
      if [[ $(scount) -lt $MAX_STATE ]]; then wstate "$pid" "$lstart" "$cwd" "$NOW" ""; emit "$pid" "$age" "$wt" record new
      else emit "$pid" "$age" "$wt" skip state_full; fi
      continue
    fi
    if [[ -z "$S_TERM" ]]; then
      if [[ $((NOW - S_FIRST)) -lt $STABLE_S ]]; then emit "$pid" "$age" "$wt" skip not_stable; continue; fi
      info2=$(validate_pid "$pid" "$lstart" "$cwd" || true)
      if [[ -z "$info2" ]]; then cstate "$pid"; emit "$pid" "$age" "$wt" skip vanished; continue; fi
      if [[ "$DRY_RUN" == 1 ]]; then
        emit "$pid" "$age" "$wt" would-term stable
        wstate "$pid" "$lstart" "$cwd" "$S_FIRST" "$NOW"
      elif do_signal TERM "$pid"; then
        emit "$pid" "$age" "$wt" term stable
        wstate "$pid" "$lstart" "$cwd" "$S_FIRST" "$NOW"
      else emit "$pid" "$age" "$wt" skip signal_failed; fi
      continue
    fi
    if [[ $((NOW - S_TERM)) -lt $GRACE_S ]]; then emit "$pid" "$age" "$wt" skip grace; continue; fi
    info2=$(validate_pid "$pid" "$lstart" "$cwd" || true)
    if [[ -z "$info2" ]]; then cstate "$pid"; emit "$pid" "$age" "$wt" skip vanished; continue; fi
    if [[ "$DRY_RUN" == 1 ]]; then
      emit "$pid" "$age" "$wt" would-kill grace; cstate "$pid"
    elif do_signal KILL "$pid"; then
      emit "$pid" "$age" "$wt" kill grace; cstate "$pid"
    else emit "$pid" "$age" "$wt" skip signal_failed; fi
    continue
  fi
  if [[ $(scount) -ge $MAX_STATE ]]; then emit "$pid" "$age" "$wt" skip state_full; continue; fi
  wstate "$pid" "$lstart" "$cwd" "$NOW" ""; emit "$pid" "$age" "$wt" record new
done <"$CAND_FILE"

for f in "$STATE_DIR"/*; do
  [[ -f "$f" ]] || continue; base=${f##*/}; [[ "$base" =~ ^[0-9]+$ ]] || continue
  grep -qx "$base" "$SEEN_FILE" 2>/dev/null && continue
  cstate "$base"; emit "$base" 0 - skip pruned
done
exit 0
