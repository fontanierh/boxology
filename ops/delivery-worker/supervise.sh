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
GRACE=10; TEST_MODE=${DELIVERY_WORKER_TEST_MODE:-0}; SID_FILTER=
if [[ "$TEST_MODE" == 1 ]]; then
  PS=${DW_PS:-$PS}; LSOF=${DW_LSOF:-$LSOF}; KILL=${DW_KILL:-$KILL}
  SLEEP=${DW_SLEEP:-$SLEEP}; REALPATH=${DW_REALPATH:-$REALPATH}; DATE=${DW_DATE:-$DATE}
  STATE=${DW_STATE_DIR:-$STATE}; MAIN=${DW_MAIN:-$MAIN}; COMMON=${DW_COMMON_GIT:-$COMMON}
  ALLOWED=${DW_ALLOWED_ROOT:-$ALLOWED}; DENIED=${DW_DENIED_ROOTS:-$DENIED}; GRACE=${DW_GRACE_SECONDS:-$GRACE}; SID_FILTER=${DW_SID_FILTER:-}
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
  { printf 'schema=%s\nrun=%s\nphase=%s\nharness=%s\npid=%s\npgid=%s\nsession=%s\nuid=%s\nlstart=%s\nobserved=%s\ncommand=%s\nparent=%s\nparent_session=%s\nparent_lstart=%s\nworktree=%s\ncwd=%s\ncommon=%s\nmeta=%s\nwt_id=%s\ngitfile_id=%s\nmeta_id=%s\nidentity_id=%s\ncontrol_id=%s\nstatus_id=%s\nhead=%s\nstage=%s\nlock_complete=%s\ncreated=%s\nupdated=%s\nterm=%s\n' \
    "$R_SCHEMA" "$R_RUN" "$R_PHASE" "$R_HARNESS" "$R_PID" "$R_PGID" "$R_SESSION" "$R_UID" "$R_LSTART" "$R_OBS" "$R_CMD" "$R_PARENT" "$R_PARENT_SESSION" "$R_PARENT_LSTART" "$R_WT" "$R_CWD" "$R_COMMON" "$R_META" "$R_WT_ID" "$R_GITFILE_ID" "$R_META_ID" "$R_IDENTITY_ID" "$R_CONTROL_ID" "$R_STATUS_ID" "$R_HEAD" "$R_STAGE" "$R_LOCK_COMPLETE" "$R_CREATED" "$R_UPDATED" "${R_TERM:-}"
    while IFS= read -r m; do if [[ -n "$m" ]]; then printf 'member=%s\n' "$m"; fi; done <<<"${R_MEMBERS:-}"
    while IFS= read -r m; do if [[ -n "$m" ]]; then printf 'owned_lock=%s\n' "$m"; fi; done <<<"${R_LOCKS:-}"
  } >"$t" || return 1
  if [[ "$TEST_MODE" == 1 && "${DW_FAIL_RECORD_WRITE:-0}" == 1 ]]; then return 1; fi
  /bin/chmod 600 "$t" && /bin/mv -f "$t" "$f"
}
read_record() {
  local f k v seen='|' line; f=$(record_path)
  [[ -f "$f" && ! -L "$f" && "$("$STAT" -f '%u:%Lp' "$f" 2>/dev/null)" == "$UID_N:600" ]] || return 1
  R_MEMBERS=; R_LOCKS=; R_SCHEMA=; R_PID=; R_PGID=; R_SESSION=; R_UID=; R_LSTART=; R_OBS=; R_CMD=; R_PARENT=; R_PARENT_SESSION=; R_PARENT_LSTART=; R_WT=; R_CWD=; R_COMMON=; R_META=; R_WT_ID=; R_GITFILE_ID=; R_META_ID=; R_IDENTITY_ID=; R_CONTROL_ID=; R_STATUS_ID=; R_HEAD=; R_STAGE=; R_LOCK_COMPLETE=; R_CREATED=; R_UPDATED=; R_TERM=
  while IFS= read -r line || [[ -n "$line" ]]; do
    k=${line%%=*}; v=${line#*=}; [[ "$line" == *=* ]] || return 1
    if [[ "$k" == member ]]; then R_MEMBERS="${R_MEMBERS}${R_MEMBERS:+$'\n'}$v"; continue; fi
    if [[ "$k" == owned_lock ]]; then R_LOCKS="${R_LOCKS}${R_LOCKS:+$'\n'}$v"; continue; fi
    [[ "$seen" != *"|$k|"* ]] || return 1; seen="$seen$k|"
    case "$k" in schema) R_SCHEMA=$v;; run) [[ "$v" == "$R_RUN" ]] || return 1;; phase) R_PHASE=$v;; harness) R_HARNESS=$v;; pid) R_PID=$v;; pgid) R_PGID=$v;; session) R_SESSION=$v;; uid) R_UID=$v;; lstart) R_LSTART=$v;; observed) R_OBS=$v;; command) R_CMD=$v;; parent) R_PARENT=$v;; parent_session) R_PARENT_SESSION=$v;; parent_lstart) R_PARENT_LSTART=$v;; worktree) R_WT=$v;; cwd) R_CWD=$v;; common) R_COMMON=$v;; meta) R_META=$v;; wt_id) R_WT_ID=$v;; gitfile_id) R_GITFILE_ID=$v;; meta_id) R_META_ID=$v;; identity_id) R_IDENTITY_ID=$v;; control_id) R_CONTROL_ID=$v;; status_id) R_STATUS_ID=$v;; head) R_HEAD=$v;; stage) R_STAGE=$v;; lock_complete) R_LOCK_COMPLETE=$v;; created) R_CREATED=$v;; updated) R_UPDATED=$v;; term) R_TERM=$v;; *) return 1;; esac
  done <"$f"
  [[ "$R_SCHEMA" =~ ^[12]$ && "$R_PID" =~ ^[1-9][0-9]*$ && "$R_PID" -gt 1 && "$R_PID" == "$R_PGID" && "$R_UID" == "$UID_N" && "$R_SESSION" =~ ^[0-9]+$ && "$R_PARENT" =~ ^[1-9][0-9]*$ && "$R_PARENT" -gt 1 && "$R_CREATED" =~ ^[0-9]+$ && "$R_UPDATED" =~ ^[0-9]+$ ]] || return 1
  [[ "$R_SCHEMA" != 2 || ( "$R_SESSION" == "$R_PID" && "$R_IDENTITY_ID" =~ ^[0-9]+:[0-9]+$ && "$R_CONTROL_ID" =~ ^[0-9]+:[0-9]+$ && "$R_STATUS_ID" =~ ^[0-9]+:[0-9]+$ && "$R_LOCK_COMPLETE" =~ ^[01]$ ) ]] || return 1
  safe "$R_RUN" && safe "$R_PHASE" && safe "$R_HARNESS" && safe "$R_CMD" || return 1
  case "$R_PHASE" in implement|repair|validation) ;; *) return 1;; esac
  case "$R_STAGE" in gated|running|interrupted|exited|term_prepared|term_sent|release_sent|kill_intent) ;; *) return 1;; esac
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

with_sid() { if [[ -n "$SID_FILTER" ]]; then "$SID_FILTER" "$1" "$2"; else /usr/bin/perl -ane 'next unless @F>=10 && $F[0]=~/^\d+$/; $s=syscall(310,0+$F[0]); next if $s<0; print "$F[0] $F[1] $F[2] $s $F[3] ",join(" ",@F[4..$#F]),"\n"' "$1" >"$2"; fi; }
snapshot() {
  local raw="$1.raw.$$"
  "$PS" -axo pid=,ppid=,pgid=,uid=,lstart=,comm= >"$raw" 2>/dev/null || { /bin/rm -f "$raw"; return 1; }
  with_sid "$raw" "$1" || { /bin/rm -f "$raw"; return 1; }; /bin/rm -f "$raw"
  /usr/bin/awk 'NF < 11 || $1 !~ /^[0-9]+$/ || $2 !~ /^[0-9]+$/ || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/ { exit 1 }' "$1"
}
snapshot_pid() {
  local rc raw="$2.raw.$$"; "$PS" -p "$1" -o pid=,ppid=,pgid=,uid=,lstart=,comm= >"$raw" 2>/dev/null; rc=$?
  [[ $rc -eq 0 || ( $rc -eq 1 && ! -s "$raw" ) ]] || { /bin/rm -f "$raw"; return 1; }
  with_sid "$raw" "$2" || { /bin/rm -f "$raw"; return 1; }; /bin/rm -f "$raw"
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
anchor_handles() {
  local fd= n= line; A_CWD=; A_IDENTITY_INO=; A_CONTROL_INO=; A_STATUS_INO=; A_IDENTITY_ACCESS=; A_CONTROL_ACCESS=; A_STATUS_ACCESS=
  while IFS= read -r line; do case "$line" in
    f*) fd=${line#f};;
    a*) case "$fd" in 9) A_IDENTITY_ACCESS=${line#a};; 10) A_CONTROL_ACCESS=${line#a};; 11) A_STATUS_ACCESS=${line#a};; esac;;
    i*) case "$fd" in 9) A_IDENTITY_INO=${line#i};; 10) A_CONTROL_INO=${line#i};; 11) A_STATUS_INO=${line#i};; esac;;
    n*) [[ "$fd" == cwd ]] && { n=$(canon "${line#n}") || return 1; A_CWD=$n; };;
  esac; done < <("$LSOF" -n -P -a -p "$1" -d cwd,9,10,11 -Ffain 2>/dev/null)
  [[ -n "$A_CWD" && "$A_IDENTITY_INO" == "${R_IDENTITY_ID#*:}" && "$A_IDENTITY_ACCESS" == r && "$A_CONTROL_INO" == "${R_CONTROL_ID#*:}" && "$A_CONTROL_ACCESS" == r ]]
}
fingerprint() { printf '%s' "$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS|$1" | /usr/bin/shasum -a 256 | /usr/bin/awk '{print $1}'; }

record_worktree() {
  worktree "$R_WT" "$R_CWD" || return 1
  [[ "$G_META" == "$R_META" && "$COMMON" == "$R_COMMON" && "$G_WT_ID" == "$R_WT_ID" && "$G_GITFILE_ID" == "$R_GITFILE_ID" && "$G_META_ID" == "$R_META_ID" && "$G_HEAD" == "$R_HEAD" ]]
}
guardian() {
  local file=$1; record_worktree || return 1; row "$R_PID" "$file" || return 1
  [[ "$P_PID" == "$R_PGID" && "$P_PGID" == "$R_PGID" && "$P_SESSION" == "$R_SESSION" && "$R_SESSION" == "$R_PID" && "$P_UID" == "$R_UID" && "$P_LSTART" == "$R_LSTART" && "$P_OBS" == "$R_OBS" ]] || return 1
  session_leader "$R_PID" || return 1
  if row "$R_PARENT" "$file"; then [[ "$P_SESSION" == "$R_PARENT_SESSION" && "$P_LSTART" == "$R_PARENT_LSTART" ]] || return 1; fi
  row "$R_PID" "$file" || return 1; [[ "$P_PPID" == "$R_PARENT" || "$P_PPID" == 1 ]] || return 1
  anchor_handles "$R_PID" || return 1
  [[ "$A_CWD" == "$R_CWD" ]]
}
launch_guardian() {
  local file=$1; record_worktree || return 1; row "$R_PID" "$file" || return 1
  [[ "$P_PID" == "$R_PGID" && "$P_PGID" == "$R_PGID" && "$P_SESSION" == "$R_PID" && "$P_PPID" == "$R_PARENT" && "$P_UID" == "$R_UID" ]] || return 1
  session_leader "$R_PID" || return 1
  R_SESSION=$R_PID; R_LSTART=$P_LSTART; R_OBS=$P_OBS
  anchor_handles "$R_PID" || return 1
  [[ "$A_CWD" == "$R_CWD" && "$A_STATUS_INO" == "${R_STATUS_ID#*:}" && "$A_STATUS_ACCESS" == w ]]
}
group_present() { /usr/bin/awk -v g="$R_PGID" '$3==g { found=1 } END { exit !found }' "$1"; }
session_members() {
  local file=$1 line c fp pid check expected; check="$file.session.$$"; C_MEMBERS=; S_ESCAPED=0
  [[ "$R_SCHEMA" == 2 ]] && guardian "$file" || { /bin/rm -f "$check"; return 1; }
  # The session is the primary ownership boundary, but a direct child may create a new one.
  # Inspect the bounded live ancestry graph while the exact guardian still anchors provenance.
  /usr/bin/awk -v lead="$R_PID" -v sid="$R_SESSION" '{pp[$1]=$2; ss[$1]=$4} END {for(p in pp) if(p!=lead && ss[p]!=sid) {q=p; for(n=0;n<256 && q>1;n++) {q=pp[q]; if(q==lead) exit 1}}}' "$file" || { S_ESCAPED=1; /bin/rm -f "$check"; return 2; }
  while IFS= read -r line; do set -- $line; [[ $# -ge 11 ]] || { /bin/rm -f "$check"; return 1; }; pid=$1
    [[ "$4" == "$R_SESSION" ]] || continue; row "$pid" "$file" || { /bin/rm -f "$check"; return 1; }
    [[ "$P_UID" == "$R_UID" ]] || { /bin/rm -f "$check"; return 1; }
    [[ "$P_PGID" == "$R_PGID" ]] || { S_ESCAPED=1; /bin/rm -f "$check"; return 2; }
    expected="$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS"
    if [[ "$pid" == "$R_PID" ]]; then c=$R_CWD
    else
    if ! c=$(cwd_of "$pid"); then
      snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }
      row "$pid" "$check" && { /bin/rm -f "$check"; return 1; }; continue
    fi
    fi
    snapshot_pid "$pid" "$check" || { /bin/rm -f "$check"; return 1; }
    row "$pid" "$check" || continue
    [[ "$P_PID|$P_PGID|$P_SESSION|$P_UID|$P_LSTART|$P_OBS" == "$expected" ]] || { /bin/rm -f "$check"; return 1; }
    fp=$(fingerprint "$c") || { /bin/rm -f "$check"; return 1; }
    C_MEMBERS="${C_MEMBERS}${C_MEMBERS:+$'\n'}$pid:$fp"
  done <"$file"
  /bin/rm -f "$check"
  [[ $'\n'"$C_MEMBERS"$'\n' == *$'\n'"$R_PID:"* ]] || return 1
}
session_present() { /usr/bin/awk -v s="$R_SESSION" '$4==s { found=1 } END { exit !found }' "$1"; }
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
  [[ "${R_LOCK_COMPLETE:-0}" == 1 ]] || { printf 'unknown\n'; return; }
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
clear_handles() { /bin/rm -f "$STATE/$R_RUN.identity" "$STATE/$R_RUN.control" "$STATE/$R_RUN.status" "$STATE/$R_RUN.gate.${R_PARENT:-0}" "$STATE/.$R_RUN.launch.${R_PARENT:-0}" "$STATE/.$R_RUN.exit.${R_PARENT:-0}" "$STATE/$R_RUN.test-arm" "$STATE/$R_RUN.test-trigger" "$STATE/$R_RUN.test-ready" "$STATE/$R_RUN.test-unstable" "$STATE/$R_RUN.test-unstable-trigger" "$STATE/$R_RUN.test-unstable-ready" "$STATE/$R_RUN.test-gated-arm" "$STATE/$R_RUN.test-gated-trigger" "$STATE/$R_RUN.test-start-arm" "$STATE/$R_RUN.test-start-trigger" "$STATE/$R_RUN.test-release-arm" "$STATE/$R_RUN.test-release-trigger"; }
clear_record() { /bin/rm -f "$(record_path)"; clear_handles; }
test_crash_barrier() {
  local stage=$1 arm="$STATE/$R_RUN.test-$1-arm" trigger="$STATE/$R_RUN.test-$1-trigger"
  [[ "$TEST_MODE" == 1 && -e "$arm" ]] || return 0
  [[ -f "$arm" && ! -L "$arm" && "$("$STAT" -f '%u:%Lp' "$arm" 2>/dev/null)" == "$UID_N:600" ]] || return 1
  : >"$trigger" && /bin/chmod 600 "$trigger" || return 1
  while [[ -e "$arm" ]]; do "$SLEEP" .01; done
}
test_term_barrier() {
  local arm="$STATE/$R_RUN.test-arm" trigger="$STATE/$R_RUN.test-trigger" ready="$STATE/$R_RUN.test-ready" proof="$STATE/.$R_RUN.test-exec.$$" i x line
  [[ "$TEST_MODE" == 1 && -e "$arm" ]] || return 0
  [[ -f "$arm" && ! -L "$arm" && "$("$STAT" -f '%u:%Lp' "$arm" 2>/dev/null)" == "$UID_N:600" ]] || return 1
  : >"$trigger" || { /bin/rm -f "$arm" "$trigger" "$ready"; return 1; }
  /bin/chmod 600 "$trigger" || { /bin/rm -f "$arm" "$trigger" "$ready"; return 1; }
  for i in $(jot 200); do
    if [[ -f "$ready" && ! -L "$ready" && "$("$STAT" -f '%u:%Lp' "$ready" 2>/dev/null)" == "$UID_N:600" ]]; then IFS= read -r x <"$ready" || x=; [[ "$x" == /bin/sleep ]] && break; fi
    "$SLEEP" .01
  done
  [[ "$x" == /bin/sleep ]] || { /bin/rm -f "$arm" "$trigger" "$ready" "$proof"; return 1; }
  for i in $(jot 200); do
    if snapshot "$proof"; then while IFS= read -r line; do set -- $line; [[ "$1" != "$R_PID" && "$4" == "$R_SESSION" && "$3" == "$R_PGID" ]] || continue; shift 10; [[ "$*" == "$x" ]] && { /bin/rm -f "$arm" "$trigger" "$ready" "$proof"; return 0; }; done <"$proof"; fi
    "$SLEEP" .01
  done
  /bin/rm -f "$arm" "$trigger" "$ready" "$proof"; return 1
}
test_roster_churn() {
  local attempt=$1 arm="$STATE/$R_RUN.test-unstable" trigger="$STATE/$R_RUN.test-unstable-trigger" ready="$STATE/$R_RUN.test-unstable-ready" i x= t
  t="$trigger.tmp.$$"
  [[ "$TEST_MODE" == 1 && -e "$arm" ]] || return 0
  [[ -f "$arm" && ! -L "$arm" && "$("$STAT" -f '%u:%Lp' "$arm" 2>/dev/null)" == "$UID_N:600" ]] || return 1
  printf '%s\n' "$attempt" >"$t" && /bin/chmod 600 "$t" && /bin/mv -f "$t" "$trigger" || { /bin/rm -f "$t" "$trigger" "$ready"; return 1; }
  for i in $(jot 200); do if [[ -f "$ready" && ! -L "$ready" && "$("$STAT" -f '%u:%Lp' "$ready" 2>/dev/null)" == "$UID_N:600" ]]; then IFS= read -r x <"$ready" || x=; [[ "$x" == "$attempt" ]] && { /bin/rm -f "$trigger" "$ready"; return 0; }; fi; "$SLEEP" .01; done
  /bin/rm -f "$t" "$trigger" "$ready"; return 1
}
abort_launch() { trap - INT TERM HUP; printf 'abort\n' >&7 2>/dev/null; exec 3>&- 4>&- 5>&- 6>&- 7>&-; wait "$guardian" 2>/dev/null; /bin/rm -f "$gate" "$snap" "$(record_path)" "$(record_path).tmp.$$"; clear_handles; }
guardian_cmd() {
  local run=${1:-} cwd=${2:-} identity=${3:-} control=${4:-} status=${5:-} gate=${6:-} x child rc=0; shift 6 || exit 125
  safe "$run" && [[ "$cwd" == /* && "$identity" == "$STATE/$run.identity" && "$control" == "$STATE/$run.control" && "$status" == "$STATE/$run.status" && "$gate" == "$STATE/$run.gate."* && "${1:-}" == -- ]] || exit 125
  shift; [[ $# -gt 0 ]] || exit 125
  cd "$cwd" || exit 125; exec 9<"$identity" 10<"$control" 11>"$status" || exit 125
  IFS= read -r x <"$gate" || exit 125; [[ "$x" == go ]] || exit 125
  trap '' INT TERM HUP
  ( exec 9>&- 10>&- 11>&-; trap - INT TERM HUP; exec "$@" ) & child=$!
  wait "$child" || rc=$?; trap '' PIPE
  printf '%s:%s\n' "$$" "$rc" >&11 2>/dev/null || true; exec 11>&-
  while IFS= read -r x <&10; do [[ "$x" == "release:$run" ]] && exit 0; done
  while :; do /bin/kill -STOP $$; done
}
run_cmd() {
  local phase= harness= wt= cwd= gate guardian rc parent_line cmd report extra control status; local -a argv
  while [[ $# -gt 0 ]]; do case "$1" in --run-id) R_RUN=${2:-}; shift 2;; --phase) phase=${2:-}; shift 2;; --harness) harness=${2:-}; shift 2;; --worktree) wt=${2:-}; shift 2;; --cwd) cwd=${2:-}; shift 2;; --) shift; break;; *) die bad_argument;; esac; done
  safe "${R_RUN:-}" && safe "$harness" || die unsafe_label; case "$phase" in implement|repair|validation) ;; *) die unsafe_phase;; esac
  [[ $# -gt 0 ]] || die missing_command; cmd=${1##*/}; safe "$cmd" || die unsafe_command; argv=("$@")
  setup_state; locked || die run_locked 75; [[ ! -e "$(record_path)" ]] || die record_exists 75
  worktree "$wt" "$cwd" || die unsafe_worktree
  R_PHASE=$phase; R_HARNESS=$harness; R_CMD=$cmd; R_PARENT=$$; R_UID=$UID_N
  parent_line=$("$PS" -p $$ -o lstart= 2>/dev/null) || die parent_evidence; set -- $parent_line
  [[ $# -eq 5 ]] || die parent_evidence; R_PARENT_SESSION=$(/usr/bin/perl -e '$x=syscall(310,0+$ARGV[0]); $x>=0 or exit 1; print $x' $$) || die parent_evidence; R_PARENT_LSTART="$1 $2 $3 $4 $5"
  R_WT=$G_WT; R_CWD=$G_CWD; R_COMMON=$COMMON; R_META=$G_META; R_WT_ID=$G_WT_ID; R_GITFILE_ID=$G_GITFILE_ID; R_META_ID=$G_META_ID; R_HEAD=$G_HEAD
  control="$STATE/$R_RUN.control"; status="$STATE/$R_RUN.status"; gate="$STATE/$R_RUN.gate.$$"
  : >"$STATE/$R_RUN.identity"; /bin/chmod 600 "$STATE/$R_RUN.identity"
  /usr/bin/mkfifo "$control" "$status" "$gate" || { clear_handles; /bin/rm -f "$gate"; die fifo_create; }
  /bin/chmod 600 "$control" "$status" "$gate" || { clear_handles; /bin/rm -f "$gate"; die fifo_mode; }
  R_IDENTITY_ID=$(fid "$STATE/$R_RUN.identity") || die fifo_identity; R_CONTROL_ID=$(fid "$control") || die fifo_identity; R_STATUS_ID=$(fid "$status") || die fifo_identity
  exec 5<>"$control" 6<>"$status" 7<>"$gate" || { /bin/rm -f "$gate"; clear_handles; die gate_open; }
  set +m; ( exec 8>&- 3>&- 4>&- 5>&- 6>&- 7>&-; exec /usr/bin/perl -MPOSIX -e 'POSIX::setsid() >= 0 or exit 125; exec {$ARGV[0]} @ARGV; exit 125' -- "$SELF" guardian "$R_RUN" "$R_CWD" "$STATE/$R_RUN.identity" "$control" "$status" "$gate" -- "${argv[@]}" ) & guardian=$!
  R_SCHEMA=2; R_PID=$guardian; R_PGID=$guardian; R_MEMBERS=; R_LOCKS=; R_LOCK_COMPLETE=0; R_TERM=; R_CREATED=$(now); R_UPDATED=$R_CREATED; R_STAGE=gated
  local i released=0 snap="$STATE/.$R_RUN.launch.$$"
  trap 'if [[ "$released" == 0 ]]; then abort_launch; else trap - INT TERM HUP; [[ "$R_STAGE" == release_sent ]] || R_STAGE=interrupted; R_UPDATED=$(now); write_record 2>/dev/null; { printf "go\n" >&7; } 2>/dev/null; /bin/rm -f "$gate" "$snap"; exec 3>&- 4>&- 5>&- 6>&- 7>&-; fi; exit 143' INT TERM HUP
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do snapshot "$snap" && launch_guardian "$snap" && break; "$SLEEP" .05; done
  if ! launch_guardian "$snap"; then abort_launch; die launch_evidence; fi
  exec 4>"$control" 3<"$status" || { abort_launch; die protocol_direction; }; exec 5>&- 6>&-
  /bin/rm -f "$STATE/$R_RUN.identity" "$control" "$status"
  snapshot "$snap" && guardian "$snap" || { abort_launch; die launch_changed; }
  write_record || { abort_launch; die record_write; }; released=1
  test_crash_barrier gated || { abort_launch; die test_barrier_failed; }
  printf 'go\n' >&7 || { exec 7>&-; R_STAGE=interrupted; R_UPDATED=$(now); write_record; die gate_release; }; exec 7>&-
  test_crash_barrier start || die test_barrier_failed
  R_STAGE=running; R_UPDATED=$(now); write_record || die record_write
  /bin/rm -f "$gate" "$snap"
  IFS= read -r report <&3 || { R_STAGE=interrupted; R_UPDATED=$(now); write_record; die status_read; }
  if IFS= read -r extra <&3; then R_STAGE=interrupted; R_UPDATED=$(now); write_record; die status_invalid; fi; exec 3>&-
  [[ "$report" =~ ^$R_PID:([0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])$ ]] || { R_STAGE=interrupted; R_UPDATED=$(now); write_record; die status_invalid; }
  rc=${report#*:}; snap="$STATE/.$R_RUN.exit.$$"; snapshot "$snap" || { /bin/rm -f "$snap"; R_STAGE=interrupted; R_UPDATED=$(now); write_record; die ps_failed; }
  if ! session_members "$snap" || [[ "$C_MEMBERS" != "$R_PID:"* || "$C_MEMBERS" == *$'\n'* ]]; then /bin/rm -f "$snap"; R_STAGE=exited; R_UPDATED=$(now); write_record; emit retain group_not_empty; return 70; fi
  R_STAGE=release_sent; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }
  test_crash_barrier release || { /bin/rm -f "$snap"; emit retain test_barrier_failed; return 70; }
  snapshot "$snap" && session_members "$snap" && [[ "$C_MEMBERS" != *$'\n'* ]] || { /bin/rm -f "$snap"; emit retain changed_before_release; return 70; }
  printf 'release:%s\n' "$R_RUN" >&4 2>/dev/null || { /bin/rm -f "$snap"; emit retain release_failed; return 71; }; exec 4>&-
  wait "$guardian" || { /bin/rm -f "$snap"; emit retain release_failed; return 71; }
  trap - INT TERM HUP; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
  if session_present "$snap"; then /bin/rm -f "$snap"; emit retain post_release_survivor; return 70; fi
  /bin/rm -f "$snap"
  if [[ $rc -ne 0 ]]; then R_STAGE=exited; R_UPDATED=$(now); write_record; clear_handles; emit retain abnormal; return "$rc"; fi
  clear_record; emit clear normal; return 0
}

reap_cmd() {
  local dry=0 resume=0 remaining snap="$STATE/.$R_RUN.reap.$$" before observed after lock_state=unknown rc attempt stable
  [[ "${1:-}" == --dry-run ]] && { dry=1; shift; }; [[ $# -eq 0 ]] || die bad_argument
  setup_state; locked || die run_locked 75; read_record || die unsafe_record
  snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
  if [[ "$R_SCHEMA" != 2 ]]; then
    if group_present "$snap"; then /bin/rm -f "$snap"; emit refuse legacy_shared_session; return 70; fi
    /bin/rm -f "$snap"; [[ $dry == 1 ]] && emit would-clear group_empty || { clear_record; emit clear group_empty; }; return 0
  fi
  if [[ "$R_STAGE" == kill_intent ]]; then
    if ! session_present "$snap"; then /bin/rm -f "$snap"; lock_state=$(classify_locks); [[ $dry == 1 ]] && emit would-clear kill "$lock_state" || { clear_record; emit clear kill "$lock_state"; }; return 0; fi
    session_members "$snap"; rc=$?
    if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
    [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit retain kill_anchor_missing; return 70; }
    if [[ $dry == 1 ]]; then /bin/rm -f "$snap"; emit would-kill resumed; return 0; fi
    "$KILL" -KILL "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain kill_failed; return 71; }
    "$SLEEP" .1; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
    if session_present "$snap"; then /bin/rm -f "$snap"; emit retain post_kill_survivor; return 70; fi
    /bin/rm -f "$snap"; lock_state=$(classify_locks); clear_record; emit clear kill "$lock_state"; return 0
  fi
  if [[ "$R_STAGE" == release_sent ]]; then
    if ! session_present "$snap"; then /bin/rm -f "$snap"; lock_state=$(classify_locks); [[ $dry == 1 ]] && emit would-clear release "$lock_state" || { clear_record; emit clear release "$lock_state"; }; return 0; fi
    session_members "$snap"; rc=$?
    if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
    [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit retain release_anchor_missing; return 70; }
    if [[ $dry == 1 ]]; then /bin/rm -f "$snap"; emit would-kill release; return 0; fi
    R_STAGE=kill_intent; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }
    snapshot "$snap"; rc=$?; [[ $rc -eq 0 ]] && session_members "$snap"; rc=$?
    if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
    [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit retain changed_before_kill; return 70; }
    "$KILL" -KILL "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain kill_failed; return 71; }
    "$SLEEP" .1; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
    if session_present "$snap"; then /bin/rm -f "$snap"; emit retain post_kill_survivor; return 70; fi
    /bin/rm -f "$snap"; lock_state=$(classify_locks); clear_record; emit clear kill "$lock_state"; return 0
  fi
  if ! session_present "$snap"; then
    /bin/rm -f "$snap"
    if [[ "$R_STAGE" == gated ]]; then [[ $dry == 1 ]] && emit would-clear pre_start || { clear_record; emit clear pre_start; }; return 0; fi
    if [[ "$R_STAGE" != exited ]]; then emit refuse leader_missing; return 70; fi
    [[ $dry == 1 ]] && emit would-clear group_empty || { clear_record; emit clear group_empty; }; return 0
  fi
  session_members "$snap"; rc=$?
  if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
  [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit refuse ambiguous; return 70; }
  before=$C_MEMBERS
  if [[ "$R_STAGE" == term_sent ]]; then
    remaining=$((R_TERM + GRACE - $(now))); resume=1
    if [[ $dry == 1 ]]; then /bin/rm -f "$snap"; [[ $remaining -gt 0 ]] && emit would-kill grace || emit would-kill verified; return 0; fi
    [[ $remaining -gt 0 ]] && "$SLEEP" "$remaining"
  elif [[ $dry == 1 ]]; then
    observe_locks "$before" || { /bin/rm -f "$snap"; emit refuse lock_observation; return 70; }
    [[ -n "$R_LOCKS" ]] && lock_state=unknown || lock_state=not_observed
    /bin/rm -f "$snap"; emit would-term verified "$lock_state"; return 0
  fi
  if [[ $resume == 0 ]]; then
    test_term_barrier || { /bin/rm -f "$snap"; emit retain test_barrier_failed; return 70; }
    stable=0
    for attempt in 1 2 3; do
      snapshot "$snap"; rc=$?; [[ $rc -eq 0 ]] && session_members "$snap"; rc=$?
      if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
      [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit refuse changed_before_term; return 70; }; observed=$C_MEMBERS
      observe_locks "$observed" || { /bin/rm -f "$snap"; emit refuse lock_observation; return 70; }
      test_roster_churn "$attempt" || { /bin/rm -f "$snap"; emit retain test_churn_failed; return 70; }
      snapshot "$snap"; rc=$?; [[ $rc -eq 0 ]] && session_members "$snap"; rc=$?
      if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
      [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit refuse changed_before_term; return 70; }; after=$C_MEMBERS
      if [[ "$observed" == "$after" ]]; then stable=1; break; fi
    done
    /bin/rm -f "$STATE/$R_RUN.test-unstable" "$STATE/$R_RUN.test-unstable-trigger" "$STATE/$R_RUN.test-unstable-ready"
    R_MEMBERS=$observed; R_LOCK_COMPLETE=$stable; [[ $stable == 1 ]] || R_LOCKS=
    R_STAGE=term_prepared; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }
    snapshot "$snap"; rc=$?; [[ $rc -eq 0 ]] && session_members "$snap"; rc=$?
    if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
    [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit refuse changed_before_term; return 70; }
    if [[ "$R_LOCK_COMPLETE" == 1 && "$C_MEMBERS" != "$R_MEMBERS" ]]; then R_LOCK_COMPLETE=0; R_LOCKS=; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }; fi
    "$KILL" -TERM "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain term_failed; return 71; }
    R_STAGE=term_sent; R_TERM=$(now); R_UPDATED=$R_TERM; write_record || { /bin/rm -f "$snap"; die record_write; }
    "$SLEEP" "$GRACE"
  fi
  snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }; session_members "$snap"; rc=$?
  if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
  [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit refuse changed_after_term; return 70; }
  # Persist intent before KILL. A resumed call may repeat KILL only while this exact anchor still
  # proves the recorded SID/PGID; an absent anchor never authorizes a numeric group signal.
  R_STAGE=kill_intent; R_UPDATED=$(now); write_record || { /bin/rm -f "$snap"; die record_write; }
  snapshot "$snap"; rc=$?; [[ $rc -eq 0 ]] && session_members "$snap"; rc=$?
  if [[ $rc -eq 2 ]]; then /bin/rm -f "$snap"; emit refuse session_escape; return 70; fi
  [[ $rc -eq 0 ]] || { /bin/rm -f "$snap"; emit retain changed_before_kill; return 70; }
  "$KILL" -KILL "-$R_PGID" 2>/dev/null || { /bin/rm -f "$snap"; emit retain kill_failed; return 71; }
  "$SLEEP" .1; snapshot "$snap" || { /bin/rm -f "$snap"; die ps_failed; }
  if session_present "$snap"; then /bin/rm -f "$snap"; emit retain post_kill_survivor; return 70; fi
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
