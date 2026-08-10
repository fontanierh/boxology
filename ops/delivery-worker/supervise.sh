#!/bin/bash
# Opt-in ownership for Boxology delivery workers. macOS / Bash 3.2.
set -u
umask 077

SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
UID_N=$(/usr/bin/id -u); SELF_PID=$$
PS=/bin/ps; LSOF=/usr/sbin/lsof; KILL=/bin/kill; SLEEP=/bin/sleep
REALPATH=/bin/realpath; STAT=/usr/bin/stat; DATE=/bin/date; LOCKF=/usr/bin/lockf
STATE=/Users/jim/.codex/boxology-delivery-worker/runs
MAIN=/Users/jim/module-based-engineering; COMMON="$MAIN/.git"
ALLOWED=/Users/jim/.codex/worktrees
DENIED="/Users/jim/.codex/reviews:/Users/jim/.codex/acceptance:/Users/jim/.crab:/Users/jim/crab-source:/Users/jim/crab-bin:/tmp/boxology-ci:/tmp/boxology-ci-macos-runner"
GRACE=10; TEST_MODE=${DELIVERY_WORKER_TEST_MODE:-0}
if [[ "$TEST_MODE" == 1 ]]; then
  PS=${DW_PS:-$PS}; LSOF=${DW_LSOF:-$LSOF}; KILL=${DW_KILL:-$KILL}
  SLEEP=${DW_SLEEP:-$SLEEP}; REALPATH=${DW_REALPATH:-$REALPATH}; DATE=${DW_DATE:-$DATE}
  STATE=${DW_STATE_DIR:-$STATE}; MAIN=${DW_MAIN:-$MAIN}; COMMON=${DW_COMMON_GIT:-$COMMON}
  ALLOWED=${DW_ALLOWED_ROOT:-$ALLOWED}; DENIED=${DW_DENIED_ROOTS:-$DENIED}; GRACE=${DW_GRACE_SECONDS:-$GRACE}
fi

die() { printf 'delivery-worker: action=refuse reason=%s\n' "$1" >&2; exit "${2:-64}"; }
emit() {
  local locks=${3:-unknown} suffix=owned_lock=unknown
  [[ "$locks" == released ]] && suffix=owned_lock=released
  [[ "$locks" == shared ]] && suffix='owned_lock=released shared_lock=held_by_other'
  [[ "$locks" == not_observed ]] && suffix=owned_lock=not_observed
  printf 'run=%s phase=%s harness=%s pid=%s pgid=%s action=%s reason=%s %s\n' \
    "$R_RUN" "$R_PHASE" "$R_HARNESS" "${R_PID:-0}" "${R_PGID:-0}" "$1" "$2" "$suffix"
}
canon() { "$REALPATH" "$1" 2>/dev/null; }
under() { [[ "$1" == "$2" || "$1" == "$2"/* ]]; }
safe() { [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]]; }
fid() { "$STAT" -f '%d:%i' "$1" 2>/dev/null; }
now() { [[ -n "${DW_NOW:-}" ]] && printf '%s\n' "$DW_NOW" || "$DATE" '+%s'; }

setup_state() {
  [[ "$STATE" == /* && "$STATE" != / && ! -L "$STATE" ]] || die unsafe_state
  /bin/mkdir -p "$STATE" || die state_create
  [[ -d "$STATE" && ! -L "$STATE" && "$("$STAT" -f '%u:%Lp' "$STATE" 2>/dev/null)" == "$UID_N:700" ]] || die state_mode
  local c; c=$(canon "$STATE") || die state_canonical; [[ "$c" == "$STATE" ]] || die state_alias
}
denied() { local p=$1 d old=$IFS; IFS=:; for d in $DENIED; do IFS=$old; under "$p" "$d" && return 0; done; IFS=$old; return 1; }

# Sets G_* and proves a reciprocal linked worktree belonging to this repository.
worktree() {
  local wt=$1 cwd=$2 gitf line meta back common d
  G_WT=$(canon "$wt") || return 1; G_CWD=$(canon "$cwd") || return 1
  [[ "$wt" == "$G_WT" && "$cwd" == "$G_CWD" ]] || return 1
  ALLOWED=$(canon "$ALLOWED") || return 1; MAIN=$(canon "$MAIN") || return 1; COMMON=$(canon "$COMMON") || return 1
  under "$G_WT" "$ALLOWED" && ! under "$G_WT" "$MAIN" && ! denied "$G_WT" || return 1
  under "$G_CWD" "$G_WT" || return 1
  gitf="$G_WT/.git"; [[ -f "$gitf" && ! -L "$gitf" ]] || return 1
  IFS= read -r line <"$gitf" || return 1; case "$line" in 'gitdir: /'*) meta=${line#gitdir: };; *) return 1;; esac
  [[ -d "$meta" && ! -L "$meta" ]] || return 1; d=$(canon "$meta/..") || return 1
  [[ "$d" == "$COMMON/worktrees" && -f "$meta/gitdir" && ! -L "$meta/gitdir" && -f "$meta/commondir" && ! -L "$meta/commondir" ]] || return 1
  IFS= read -r back <"$meta/gitdir" || return 1; case "$back" in /*) ;; *) back="$meta/$back";; esac
  [[ "$(canon "$back")" == "$(canon "$gitf")" ]] || return 1
  IFS= read -r common <"$meta/commondir" || return 1; case "$common" in /*) ;; *) common="$meta/$common";; esac
  [[ "$(canon "$common")" == "$COMMON" ]] || return 1
  G_META=$(canon "$meta") || return 1; G_WT_ID=$(fid "$G_WT") || return 1
  G_GITFILE_ID=$(fid "$gitf") || return 1; G_META_ID=$(fid "$G_META") || return 1
  G_HEAD=$(/usr/bin/git -C "$G_WT" rev-parse HEAD 2>/dev/null) || return 1
}

record_path() { printf '%s/%s.record\n' "$STATE" "$R_RUN"; }
write_record() {
  local f t m; f=$(record_path); t="$f.tmp.$$"
  { printf 'schema=%s\nrun=%s\nphase=%s\nharness=%s\npid=%s\npgid=%s\nsession=%s\nuid=%s\nlstart=%s\nobserved=%s\ncommand=%s\nparent=%s\nparent_session=%s\nparent_lstart=%s\nworktree=%s\ncwd=%s\ncommon=%s\nmeta=%s\nwt_id=%s\ngitfile_id=%s\nmeta_id=%s\nhead=%s\nstage=%s\ncreated=%s\nupdated=%s\nterm=%s\n' \
    "$R_SCHEMA" "$R_RUN" "$R_PHASE" "$R_HARNESS" "$R_PID" "$R_PGID" "$R_SESSION" "$R_UID" "$R_LSTART" "$R_OBS" "$R_CMD" "$R_PARENT" "$R_PARENT_SESSION" "$R_PARENT_LSTART" "$R_WT" "$R_CWD" "$R_COMMON" "$R_META" "$R_WT_ID" "$R_GITFILE_ID" "$R_META_ID" "$R_HEAD" "$R_STAGE" "$R_CREATED" "$R_UPDATED" "${R_TERM:-}"
    while IFS= read -r m; do if [[ -n "$m" ]]; then printf 'member=%s\n' "$m"; fi; done <<<"${R_MEMBERS:-}"
    while IFS= read -r m; do if [[ -n "$m" ]]; then printf 'owned_lock=%s\n' "$m"; fi; done <<<"${R_LOCKS:-}"
  } >"$t" || return 1
  if [[ "$TEST_MODE" == 1 && "${DW_FAIL_RECORD_WRITE:-0}" == 1 ]]; then return 1; fi
  /bin/chmod 600 "$t" && /bin/mv -f "$t" "$f"
}
read_record() {
  local f k v seen='|' line; f=$(record_path)
  [[ -f "$f" && ! -L "$f" && "$("$STAT" -f '%u:%Lp' "$f" 2>/dev/null)" == "$UID_N:600" ]] || return 1
  R_MEMBERS=; R_LOCKS=; R_SCHEMA=; R_PID=; R_PGID=; R_SESSION=; R_UID=; R_LSTART=; R_OBS=; R_CMD=; R_PARENT=; R_PARENT_SESSION=; R_PARENT_LSTART=; R_WT=; R_CWD=; R_COMMON=; R_META=; R_WT_ID=; R_GITFILE_ID=; R_META_ID=; R_HEAD=; R_STAGE=; R_CREATED=; R_UPDATED=; R_TERM=
  while IFS= read -r line || [[ -n "$line" ]]; do
    k=${line%%=*}; v=${line#*=}; [[ "$line" == *=* ]] || return 1
    if [[ "$k" == member ]]; then R_MEMBERS="${R_MEMBERS}${R_MEMBERS:+$'\n'}$v"; continue; fi
    if [[ "$k" == owned_lock ]]; then R_LOCKS="${R_LOCKS}${R_LOCKS:+$'\n'}$v"; continue; fi
    [[ "$seen" != *"|$k|"* ]] || return 1; seen="$seen$k|"
    case "$k" in schema) R_SCHEMA=$v;; run) [[ "$v" == "$R_RUN" ]] || return 1;; phase) R_PHASE=$v;; harness) R_HARNESS=$v;; pid) R_PID=$v;; pgid) R_PGID=$v;; session) R_SESSION=$v;; uid) R_UID=$v;; lstart) R_LSTART=$v;; observed) R_OBS=$v;; command) R_CMD=$v;; parent) R_PARENT=$v;; parent_session) R_PARENT_SESSION=$v;; parent_lstart) R_PARENT_LSTART=$v;; worktree) R_WT=$v;; cwd) R_CWD=$v;; common) R_COMMON=$v;; meta) R_META=$v;; wt_id) R_WT_ID=$v;; gitfile_id) R_GITFILE_ID=$v;; meta_id) R_META_ID=$v;; head) R_HEAD=$v;; stage) R_STAGE=$v;; created) R_CREATED=$v;; updated) R_UPDATED=$v;; term) R_TERM=$v;; *) return 1;; esac
  done <"$f"
  [[ "$R_SCHEMA" =~ ^[12]$ && "$R_PID" =~ ^[1-9][0-9]*$ && "$R_PID" -gt 1 && "$R_PID" == "$R_PGID" && "$R_UID" == "$UID_N" && "$R_SESSION" =~ ^[0-9]+$ && "$R_PARENT" =~ ^[1-9][0-9]*$ && "$R_PARENT" -gt 1 && "$R_CREATED" =~ ^[0-9]+$ && "$R_UPDATED" =~ ^[0-9]+$ ]] || return 1
  [[ "$R_SCHEMA" != 2 || "$R_SESSION" == "$R_PID" ]] || return 1
  safe "$R_RUN" && safe "$R_PHASE" && safe "$R_HARNESS" && safe "$R_CMD" || return 1
  case "$R_PHASE" in implement|repair|validation) ;; *) return 1;; esac
  case "$R_STAGE" in running|interrupted|exited|term_prepared|term_sent) ;; *) return 1;; esac
  [[ -z "$R_TERM" || "$R_TERM" =~ ^[0-9]+$ ]] || return 1
  local m mp; while IFS= read -r m; do [[ -z "$m" ]] && continue; mp=${m%%:*}; [[ "$m" =~ ^[1-9][0-9]*:[a-f0-9]{64}$ && "$mp" -gt 1 ]] || return 1; done <<<"$R_MEMBERS"
  [[ "$R_STAGE" != term_prepared || -n "$R_MEMBERS" ]] || return 1
  [[ "$R_STAGE" != term_sent || ( -n "$R_TERM" && -n "$R_MEMBERS" ) ]] || return 1
  local l owner rest dev ino access mode path
  while IFS= read -r l; do
    [[ -z "$l" ]] && continue; rest=${l#*|}; dev=${rest%%:*}; rest=${rest#*:}; ino=${rest%%|*}; rest=${rest#*|}
    owner=${l%%|*}; access=${rest%%|*}; rest=${rest#*|}; mode=${rest%%|*}; path=${rest#*|}
    [[ "$l" =~ ^[1-9][0-9]*\|[0-9]+:[0-9]+\|[rwu?]\|[-RWrwux?]\|/ && "$owner" -gt 1 && "$dev" =~ ^[0-9]+$ && "$ino" =~ ^[0-9]+$ && -n "$path" && "$path" != *'|'* ]] || return 1
    [[ $'\n'"$R_MEMBERS"$'\n' == *$'\n'"$owner:"* ]] || return 1
  done <<<"$R_LOCKS"
}

snapshot() { "$PS" -axo pid=,ppid=,pgid=,sess=,uid=,lstart=,comm= >"$1" 2>/dev/null && /usr/bin/awk 'NF < 11 || $1 !~ /^[0-9]+$/ || $2 !~ /^[0-9]+$/ || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/ { exit 1 }' "$1"; }
snapshot_pid() {
  local rc; "$PS" -p "$1" -o pid=,ppid=,pgid=,sess=,uid=,lstart=,comm= >"$2" 2>/dev/null; rc=$?
  [[ $rc -eq 0 || ( $rc -eq 1 && ! -s "$2" ) ]] || return 1
  /usr/bin/awk 'NF && (NF < 11 || $1 !~ /^[0-9]+$/ || $2 !~ /^[0-9]+$/ || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/) { exit 1 }' "$2"
}
session_leader() {
  local line want_uid=$P_UID want_lstart=$P_LSTART want_obs=$P_OBS pid pgid uid lstart state observed
  line=$("$PS" -p "$1" -o pid=,pgid=,uid=,lstart=,state=,comm= 2>/dev/null) || return 1; set -- $line
  [[ $# -ge 10 ]] || return 1; pid=$1; pgid=$2; uid=$3; lstart="$4 $5 $6 $7 $8"; state=$9; shift 9; observed="$*"
  [[ "$pid" == "$pgid" && "$pid" == "$R_PID" && "$uid" == "$want_uid" && "$lstart" == "$want_lstart" && "$state" == *s* && "$observed" == "$want_obs" ]]
}
row() {
  local want=$1 file=$2 line; while IFS= read -r line; do set -- $line; [[ $# -ge 11 ]] || return 1
    if [[ "$1" == "$want" ]]; then P_PID=$1; P_PPID=$2; P_PGID=$3; P_SESSION=$4; P_UID=$5; P_LSTART="$6 $7 $8 $9 ${10}"; shift 10; P_OBS="$*"; return 0; fi
  done <"$file"; return 1
}
cwd_of() { local n= line; while IFS= read -r line; do case "$line" in n*) n=${line#n};; esac; done < <("$LSOF" -n -P -a -p "$1" -d cwd -Fn 2>/dev/null); [[ -n "$n" ]] && canon "$n"; }
handle_of() { local n= line; while IFS= read -r line; do case "$line" in n*) n=${line#n};; esac; done < <("$LSOF" -n -P -a -p "$1" -d 9 -Fn 2>/dev/null); [[ -n "$n" ]] && canon "$n"; }
fingerprint() { printf '%s' "$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS|$1" | /usr/bin/shasum -a 256 | /usr/bin/awk '{print $1}'; }
descends() { local p=$1 file=$2 n=0; [[ "$p" == "$R_PID" ]] && return 0; while [[ $n -lt 256 ]] && row "$p" "$file"; do p=$P_PPID; [[ "$p" == "$R_PID" ]] && return 0; [[ "$p" -gt 1 ]] || return 1; n=$((n+1)); done; return 1; }

record_worktree() {
  worktree "$R_WT" "$R_CWD" || return 1
  [[ "$G_META" == "$R_META" && "$COMMON" == "$R_COMMON" && "$G_WT_ID" == "$R_WT_ID" && "$G_GITFILE_ID" == "$R_GITFILE_ID" && "$G_META_ID" == "$R_META_ID" && "$G_HEAD" == "$R_HEAD" ]]
}
guardian() {
  local file=$1 ph; record_worktree || return 1; row "$R_PID" "$file" || return 1
  [[ "$P_PID" == "$R_PGID" && "$P_PGID" == "$R_PGID" && "$R_SESSION" == "$R_PID" && "$P_UID" == "$R_UID" && "$P_LSTART" == "$R_LSTART" && "$P_OBS" == "$R_OBS" ]] || return 1
  session_leader "$R_PID" || return 1
  if row "$R_PARENT" "$file"; then [[ "$P_SESSION" == "$R_PARENT_SESSION" && "$P_LSTART" == "$R_PARENT_LSTART" ]] || return 1; fi
  row "$R_PID" "$file" || return 1; [[ "$P_PPID" == "$R_PARENT" || "$P_PPID" == 1 ]] || return 1
  ph=$(cwd_of "$R_PID") || return 1; [[ "$ph" == "$R_CWD" ]] || return 1
  ph=$(handle_of "$R_PID") || return 1; [[ "$ph" == "$STATE/$R_RUN.identity" ]]
}
launch_guardian() {
  local file=$1 ph; record_worktree || return 1; row "$R_PID" "$file" || return 1
  [[ "$P_PID" == "$R_PGID" && "$P_PGID" == "$R_PGID" && "$P_PPID" == "$R_PARENT" && "$P_UID" == "$R_UID" ]] || return 1
  session_leader "$R_PID" || return 1
  R_SESSION=$R_PID; R_LSTART=$P_LSTART; R_OBS=$P_OBS
  ph=$(cwd_of "$R_PID") || return 1; [[ "$ph" == "$R_CWD" ]] || return 1
  ph=$(handle_of "$R_PID") || return 1; [[ "$ph" == "$STATE/$R_RUN.identity" ]]
}
collect() {
  local file=$1 line c fp pid check expected; check="$file.member.$$"; C_MEMBERS=
  while IFS= read -r line; do set -- $line; [[ $# -ge 11 ]] || { /bin/rm -f "$check"; return 1; }; pid=$1
    [[ "$3" == "$R_PGID" ]] || continue; row "$pid" "$file" || { /bin/rm -f "$check"; return 1; }
    [[ "$P_UID" == "$R_UID" ]] || { /bin/rm -f "$check"; return 1; }; descends "$pid" "$file" || { /bin/rm -f "$check"; return 1; }
    row "$pid" "$file" || { /bin/rm -f "$check"; return 1; }; expected="$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS"
    if ! c=$(cwd_of "$pid"); then
      snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }
      row "$pid" "$check" && { /bin/rm -f "$check"; return 1; }; continue
    fi
    snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }
    if ! row "$pid" "$check"; then continue; fi
    [[ "$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS" == "$expected" ]] || { /bin/rm -f "$check"; return 1; }
    fp=$(fingerprint "$c") || { /bin/rm -f "$check"; return 1; }
    C_MEMBERS="${C_MEMBERS}${C_MEMBERS:+$'\n'}$pid:$fp"
  done <"$file"
  /bin/rm -f "$check"
  [[ -n "$C_MEMBERS" ]] || return 1
  /usr/bin/awk -v lead="$R_PID" -v grp="$R_PGID" '{pp[$1]=$2; pg[$1]=$3} END { for (p in pp) { q=p; for (n=0; n<256 && q>1; n++) { if (q==lead) { if (pg[p]!=grp) exit 1; break } q=pp[q] } } }' "$file"
}
same_members() { [[ "$1" == "$2" ]]; }
group_present() { /usr/bin/awk -v g="$R_PGID" '$3==g { found=1 } END { exit !found }' "$1"; }
recorded_survivors() {
  local file=$1 m pid fp c check; check="$file.survivor.$$"; C_MEMBERS=
  while IFS= read -r m; do [[ -n "$m" ]] || continue; pid=${m%%:*}; fp=${m#*:}
    if row "$pid" "$file"; then
      [[ "$P_PGID" == "$R_PGID" ]] || { /bin/rm -f "$check"; return 1; }
      if ! c=$(cwd_of "$pid"); then
        snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }
        row "$pid" "$check" && { /bin/rm -f "$check"; return 1; }; continue
      fi
      [[ "$(fingerprint "$c")" == "$fp" ]] || { /bin/rm -f "$check"; return 1; }
      snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }; row "$pid" "$check" || continue
      [[ "$(fingerprint "$c")" == "$fp" ]] || { /bin/rm -f "$check"; return 1; }; C_MEMBERS="${C_MEMBERS}${C_MEMBERS:+$'\n'}$m"
    fi
  done <<<"$R_MEMBERS"
  /bin/rm -f "$check"
}
survivors() {
  local file=$1 line
  recorded_survivors "$file" || return 1
  while IFS= read -r line; do set -- $line; [[ "$3" == "$R_PGID" ]] || continue
    [[ $'\n'"$R_MEMBERS"$'\n' == *$'\n'"$1:"* ]] || return 1
  done <"$file"; return 0
}
kill_evidence() { record_worktree || return 1; if row "$R_PID" "$1"; then guardian "$1" && collect "$1"; else survivors "$1"; fi; }

lock_name() { case "${1##*/}" in .package-cache|.cargo-build-lock|.cargo-artifact-lock|.cargo-lock) return 0;; *) return 1;; esac; }
append_lock() {
  local pid=$1 access=${2:-?} mode=${3:--} inode=$4 path=$5 id
  lock_name "$path" || return 0; [[ "$path" != *'|'* && "$path" != *$'\n'* ]] || return 1
  case "$access" in r|w|u) ;; *) access=?;; esac; case "$mode" in R|W|r|w|u|x) ;; *) mode=-;; esac
  path=$(canon "$path") || return 1; [[ -f "$path" && ! -L "$path" ]] || return 1
  id=$(fid "$path") || return 1; [[ "${id#*:}" == "$inode" ]] || return 1
  local item="$pid|$id|$access|$mode|$path"
  [[ $'\n'"$R_LOCKS"$'\n' == *$'\n'"$item"$'\n' ]] || R_LOCKS="${R_LOCKS}${R_LOCKS:+$'\n'}$item"
}
observe_locks() {
  local members=$1 m pid fp out="$STATE/.$R_RUN.lsof.$$" field fd= access=? mode=- inode= path= seen=0 c
  R_LOCKS=
  while IFS= read -r m; do
    [[ -n "$m" ]] || continue; pid=${m%%:*}; fp=${m#*:}
    snapshot_pid "$pid" "$out.ps" || { /bin/rm -f "$out" "$out.ps"; return 1; }
    row "$pid" "$out.ps" || { /bin/rm -f "$out" "$out.ps"; continue; }
    if ! c=$(cwd_of "$pid"); then
      snapshot_pid "$pid" "$out.ps" || { /bin/rm -f "$out" "$out.ps"; return 1; }
      row "$pid" "$out.ps" && { /bin/rm -f "$out" "$out.ps"; return 1; }
      /bin/rm -f "$out" "$out.ps"; continue
    fi
    [[ "$(fingerprint "$c")" == "$fp" ]] || { /bin/rm -f "$out" "$out.ps"; return 1; }
    if ! "$LSOF" -n -P -a -p "$pid" -F0pfailn >"$out" 2>/dev/null; then
      snapshot_pid "$pid" "$out.ps" || { /bin/rm -f "$out" "$out.ps"; return 1; }
      row "$pid" "$out.ps" && { /bin/rm -f "$out" "$out.ps"; return 1; }
      /bin/rm -f "$out" "$out.ps"; continue
    fi
    while IFS= read -r -d '' field; do
      [[ "$field" == $'\n'* ]] && field=${field#$'\n'}
      case "$field" in
        p*) [[ "${field#p}" == "$pid" ]] || { /bin/rm -f "$out" "$out.ps"; return 1; }; seen=1;;
        f*) [[ -n "$fd" && -n "$path" ]] && append_lock "$pid" "$access" "$mode" "$inode" "$path" || [[ -z "$fd" || -z "$path" ]] || { /bin/rm -f "$out" "$out.ps"; return 1; }; fd=${field#f}; access=?; mode=-; inode=; path=;;
        a*) access=${field#a};; l*) mode=${field#l}; [[ -n "$mode" ]] || mode=-;;
        i*) inode=${field#i};; n*) path=${field#n};; '') ;;
        *) /bin/rm -f "$out" "$out.ps"; return 1;;
      esac
    done <"$out"
    [[ -n "$fd" && -n "$path" ]] && append_lock "$pid" "$access" "$mode" "$inode" "$path" || [[ -z "$fd" || -z "$path" ]] || { /bin/rm -f "$out" "$out.ps"; return 1; }
    [[ $seen == 1 ]] || { /bin/rm -f "$out" "$out.ps"; return 1; }; /bin/rm -f "$out" "$out.ps"; fd=; access=?; mode=-; inode=; path=; seen=0
  done <<<"$members"
  /bin/rm -f "$out" "$out.ps"
}
classify_locks() {
  local l rest id path out="$STATE/.$R_RUN.lockcheck.$$" any=0 rc
  [[ -n "${R_LOCKS:-}" ]] || { printf 'not_observed\n'; return; }
  while IFS= read -r l; do
    path=${l#*|}; path=${path#*|}; path=${path#*|}; path=${path#*|}; id=${l#*|}; id=${id%%|*}
    [[ "$(fid "$path")" == "$id" ]] || { /bin/rm -f "$out"; printf 'unknown\n'; return; }
    "$LSOF" -n -P -F0pi -- "$path" >"$out" 2>/dev/null; rc=$?
    if [[ $rc -eq 0 ]]; then
      /usr/bin/tr '\0' '\n' <"$out" | /usr/bin/awk -v ino="${id#*:}" '/^p[0-9]+$/{p=1} /^i/{if ($0!="i" ino) bad=1} END{exit (!p || bad)}'
      rc=$?; [[ $rc -eq 0 ]] || { /bin/rm -f "$out"; printf 'unknown\n'; return; }; any=1
    elif [[ $rc -ne 1 || -s "$out" ]]; then /bin/rm -f "$out"; printf 'unknown\n'; return
    fi
  done <<<"$R_LOCKS"
  /bin/rm -f "$out"; [[ $any == 1 ]] && printf 'shared\n' || printf 'released\n'
}

locked() { exec 8>"$STATE/$R_RUN.lock" || return 1; "$LOCKF" -t 0 8 >/dev/null 2>&1; }
clear_record() { /bin/rm -f "$(record_path)" "$STATE/$R_RUN.identity"; }
abort_launch() { trap - INT TERM HUP; printf 'abort\n' >&7 2>/dev/null; exec 7>&-; wait "$guardian" 2>/dev/null; /bin/rm -f "$gate" "$snap" "$STATE/$R_RUN.identity" "$(record_path)" "$(record_path).tmp.$$"; }
guardian_cmd() {
  local cwd=${1:-} identity=${2:-} gate=${3:-} x; shift 3 || exit 125
  [[ "$cwd" == /* && "$identity" == "$STATE/"*.identity && "$gate" == "$STATE/"*.gate.* && "${1:-}" == -- ]] || exit 125
  shift; [[ $# -gt 0 ]] || exit 125
  cd "$cwd" || exit 125; exec 9<"$identity" || exit 125
  IFS= read -r x <"$gate" || exit 125; [[ "$x" == go ]] || exit 125
  "$@"; exit $?
}
run_cmd() {
  local phase= harness= wt= cwd= gate guardian rc parent_line cmd; local -a argv
  while [[ $# -gt 0 ]]; do case "$1" in --run-id) R_RUN=${2:-}; shift 2;; --phase) phase=${2:-}; shift 2;; --harness) harness=${2:-}; shift 2;; --worktree) wt=${2:-}; shift 2;; --cwd) cwd=${2:-}; shift 2;; --) shift; break;; *) die bad_argument;; esac; done
  safe "${R_RUN:-}" && safe "$harness" || die unsafe_label; case "$phase" in implement|repair|validation) ;; *) die unsafe_phase;; esac
  [[ $# -gt 0 ]] || die missing_command; cmd=${1##*/}; safe "$cmd" || die unsafe_command; argv=("$@")
  setup_state; locked || die run_locked 75; [[ ! -e "$(record_path)" ]] || die record_exists 75
  worktree "$wt" "$cwd" || die unsafe_worktree
  R_PHASE=$phase; R_HARNESS=$harness; R_CMD=$cmd; R_PARENT=$$; R_UID=$UID_N
  parent_line=$("$PS" -p $$ -o sess=,lstart= 2>/dev/null) || die parent_evidence; set -- $parent_line
  [[ $# -eq 6 ]] || die parent_evidence; R_PARENT_SESSION=$1; R_PARENT_LSTART="$2 $3 $4 $5 $6"
  R_WT=$G_WT; R_CWD=$G_CWD; R_COMMON=$COMMON; R_META=$G_META; R_WT_ID=$G_WT_ID; R_GITFILE_ID=$G_GITFILE_ID; R_META_ID=$G_META_ID; R_HEAD=$G_HEAD
  : >"$STATE/$R_RUN.identity"; /bin/chmod 600 "$STATE/$R_RUN.identity"; gate="$STATE/$R_RUN.gate.$$"; /usr/bin/mkfifo "$gate" || die gate_create
  exec 7<>"$gate" || { /bin/rm -f "$gate" "$STATE/$R_RUN.identity"; die gate_open; }
  set +m; ( exec 8>&- 7>&-; exec /usr/bin/perl -MPOSIX -e 'POSIX::setsid() >= 0 or exit 125; exec {$ARGV[0]} @ARGV; exit 125' -- "$SELF" guardian "$R_CWD" "$STATE/$R_RUN.identity" "$gate" -- "${argv[@]}" ) & guardian=$!
  R_SCHEMA=2; R_PID=$guardian; R_PGID=$guardian; R_MEMBERS=; R_LOCKS=; R_TERM=; R_CREATED=$(now); R_UPDATED=$R_CREATED; R_STAGE=running
  local i released=0 snap="$STATE/.$R_RUN.launch.$$"
  trap 'if [[ "$released" == 0 ]]; then abort_launch; else trap - INT TERM HUP; R_STAGE=interrupted; R_UPDATED=$(now); write_record 2>/dev/null; fi; exit 143' INT TERM HUP
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do snapshot "$snap" && launch_guardian "$snap" && break; "$SLEEP" .05; done
  if ! launch_guardian "$snap"; then abort_launch; die launch_evidence; fi
  snapshot "$snap" && guardian "$snap" || { abort_launch; die launch_changed; }
  write_record || { abort_launch; die record_write; }; released=1
  printf 'go\n' >&7 || { exec 7>&-; R_STAGE=interrupted; R_UPDATED=$(now); write_record; die gate_release; }; exec 7>&-
  /bin/rm -f "$gate" "$snap"; rc=0; wait "$guardian" || rc=$?
  trap - INT TERM HUP; R_UPDATED=$(now)
  if [[ $rc -ne 0 ]]; then R_STAGE=exited; write_record; emit retain abnormal; return "$rc"; fi
  snap="$STATE/.$R_RUN.exit.$$"; snapshot "$snap" || { /bin/rm -f "$snap"; R_STAGE=exited; write_record; die ps_failed; }
  if row "$R_PID" "$snap" || { while IFS= read -r parent_line; do set -- $parent_line; [[ "$3" == "$R_PGID" ]] && { /bin/rm -f "$snap"; R_STAGE=exited; write_record; die group_not_empty 70; }; done <"$snap"; false; }; then R_STAGE=exited; write_record; die group_not_empty 70; fi
  /bin/rm -f "$snap"; clear_record; emit clear normal; return 0
}

reap_cmd() {
  local dry=0 resume=0 prepared=0 remaining snap="$STATE/.$R_RUN.reap.$$" before lock_state=unknown
  [[ "${1:-}" == --dry-run ]] && { dry=1; shift; }; [[ $# -eq 0 ]] || die bad_argument
  setup_state; locked || die run_locked 75; read_record || die unsafe_record
  snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
  if [[ "$R_SCHEMA" != 2 ]] && group_present "$snap"; then /bin/rm -f "$snap"; emit refuse legacy_shared_session; return 70; fi
  if row "$R_PID" "$snap"; then
    guardian "$snap" || { /bin/rm -f "$snap"; emit refuse ambiguous; return 70; }
    if [[ "$R_STAGE" == term_sent && -n "$R_MEMBERS" ]]; then survivors "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_term; return 70; }; before=$C_MEMBERS; resume=1
    elif [[ "$R_STAGE" == term_prepared && -n "$R_MEMBERS" ]]; then
      recorded_survivors "$snap" && collect "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_prepare; return 70; }; before=$C_MEMBERS; prepared=1
    else collect "$snap" || { /bin/rm -f "$snap"; emit refuse ambiguous; return 70; }; before=$C_MEMBERS; fi
  elif [[ "$R_STAGE" == term_sent && -n "$R_MEMBERS" ]]; then
    survivors "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_term; return 70; }; before=$C_MEMBERS; resume=1
  elif [[ "$R_STAGE" == term_prepared && -n "$R_MEMBERS" ]]; then
    survivors "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_prepare; return 70; }; before=$C_MEMBERS; prepared=1
  elif group_present "$snap"; then /bin/rm -f "$snap"; emit refuse leader_missing; return 70
  else /bin/rm -f "$snap"; if [[ $dry == 1 ]]; then emit would-clear group_empty "$lock_state"; else clear_record; emit clear group_empty "$lock_state"; fi; return 0; fi
  if [[ $resume == 1 ]]; then
    if [[ -z "$before" ]]; then /bin/rm -f "$snap"; lock_state=$(classify_locks); if [[ $dry == 1 ]]; then emit would-clear term "$lock_state"; else clear_record; emit clear term "$lock_state"; fi; return 0; fi
    remaining=$((R_TERM + GRACE - $(now)))
    if [[ $dry == 1 ]]; then /bin/rm -f "$snap"; [[ $remaining -gt 0 ]] && emit would-kill grace || emit would-kill verified; return 0; fi
    [[ $remaining -gt 0 ]] && "$SLEEP" "$remaining"
    snapshot "$snap" && survivors "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_term; return 70; }; before=$C_MEMBERS
  fi
  if [[ $prepared == 1 && -z "$before" ]]; then
    /bin/rm -f "$snap"; lock_state=$(classify_locks); if [[ $dry == 1 ]]; then emit would-clear prepared "$lock_state"; else clear_record; emit clear prepared "$lock_state"; fi; return 0
  fi
  if [[ $resume == 0 && $prepared == 0 && $dry == 1 ]]; then observe_locks "$before" || { /bin/rm -f "$snap"; emit refuse lock_observation; return 70; }; fi
  [[ -n "$R_LOCKS" ]] && lock_state=unknown || lock_state=not_observed
  if [[ $dry == 1 ]]; then /bin/rm -f "$snap"; emit would-term verified "$lock_state"; return 0; fi
  if [[ $resume == 0 ]]; then
    # The exclusive session makes later arrivals owned descendants. Observe every member in the
    # exact refreshed roster, then re-prove the guardian before persisting that roster and TERMing.
    [[ $prepared == 1 ]] || { snapshot "$snap" && guardian "$snap" && collect "$snap" || { /bin/rm -f "$snap"; emit refuse changed_before_term; return 70; }; before=$C_MEMBERS; }
    observe_locks "$before" || { /bin/rm -f "$snap"; emit refuse lock_observation; return 70; }
    snapshot "$snap" && guardian "$snap" || { /bin/rm -f "$snap"; emit refuse changed_before_term; return 70; }
    R_MEMBERS=$before; R_STAGE=term_prepared; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }
    "$KILL" -TERM "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain term_failed; return 71; }
    R_STAGE=term_sent; R_TERM=$(now); R_UPDATED=$R_TERM; write_record || { /bin/rm -f "$snap"; die record_write; }
    "$SLEEP" "$GRACE"; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }; survivors "$snap" || { /bin/rm -f "$snap"; emit refuse changed_after_term; return 70; }
  fi
  [[ -n "$C_MEMBERS" ]] || { /bin/rm -f "$snap"; lock_state=$(classify_locks); clear_record; emit clear term "$lock_state"; return 0; }
  before=$C_MEMBERS; snapshot "$snap" && kill_evidence "$snap" && same_members "$before" "$C_MEMBERS" || { /bin/rm -f "$snap"; emit refuse changed_before_kill; return 70; }
  "$KILL" -KILL "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain kill_failed; return 71; }
  "$SLEEP" .1; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }; survivors "$snap" || { /bin/rm -f "$snap"; emit refuse post_kill_ambiguous; return 70; }
  [[ -z "$C_MEMBERS" ]] || { /bin/rm -f "$snap"; emit retain post_kill_survivor; return 70; }
  /bin/rm -f "$snap"; lock_state=$(classify_locks); clear_record; emit clear kill "$lock_state"
}

status_cmd() { setup_state; if [[ ! -e "$(record_path)" ]]; then printf 'run=%s state=inactive\n' "$R_RUN"; return; fi; read_record || die unsafe_record; emit status "$R_STAGE"; }
usage() { die usage; }
action=${1:-}; shift || true
case "$action" in
  guardian) guardian_cmd "$@";;
  run) R_RUN=; run_cmd "$@";;
  reap|status) R_RUN=; [[ "${1:-}" == --run-id && -n "${2:-}" ]] || usage; R_RUN=$2; safe "$R_RUN" || die unsafe_label; shift 2; if [[ "$action" == reap ]]; then reap_cmd "$@"; else [[ $# -eq 0 ]] || usage; status_cmd; fi;;
  *) usage;;
esac
