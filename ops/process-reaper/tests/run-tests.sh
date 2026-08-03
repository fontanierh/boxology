#!/bin/bash
# Fixture-only suite for ops/process-reaper/reaper.sh. Never calls real ps/lsof/kill.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REAPER="$ROOT/reaper.sh"
PLIST="$ROOT/com.fontanierh.boxology-review-reaper.plist"
UID_N="$(/usr/bin/id -u)"
PASS=0; FAIL=0
TMP_ROOTS=
LSTART='Mon Aug  3 10:00:00 2026'
LSTART_EP=$(/bin/date -j -f '%a %b %e %T %Y' "$LSTART" '+%s')

cleanup() { local d; for d in $TMP_ROOTS; do case "$d" in /tmp/reaper-test.*) rm -rf -- "$d";; esac; done; }
trap cleanup EXIT
LSTART2='Mon Aug  3 11:00:00 2026'
LSTART2_EP=$(/bin/date -j -f '%a %b %e %T %Y' "$LSTART2" '+%s')

/bin/bash -n "$REAPER"
/usr/bin/plutil -lint "$PLIST" >/dev/null

assert() { if eval "$1"; then PASS=$((PASS+1)); else echo "FAIL: $2"; FAIL=$((FAIL+1)); fi; }
has() { printf '%s\n' "$OUT" | grep -E "$1" >/dev/null; }
nokill() { [[ ! -s "$KILL_LOG" ]]; }

setup() {
  BASE=$(mktemp -d /tmp/reaper-test.XXXXXX)
  TMP_ROOTS="$TMP_ROOTS $BASE"
  REV="$BASE/reviews"; WT="$BASE/main/.git/worktrees"; ST="$BASE/state"
  DENY_WT="$BASE/worktrees"; DENY_MAIN="$BASE/main"; DENY_CRAB="$BASE/crab"
  DENY_CS="$BASE/crab-source"; DENY_CB="$BASE/crab-bin"; DENY_CI="$BASE/ci"
  mkdir -p "$REV" "$WT" "$ST" "$DENY_WT" "$DENY_CRAB" "$DENY_CS" "$DENY_CB" "$DENY_CI" "$BASE/bin"
  KILL_LOG="$BASE/kill.log"; : >"$KILL_LOG"
  KILL_FAIL="$BASE/kill.fail"; rm -f "$KILL_FAIL"
  PS_DATA="$BASE/ps.out"; : >"$PS_DATA"
  PS_ALT="$BASE/ps.alt"; : >"$PS_ALT"
  CWD_MAP="$BASE/cwd.map"; : >"$CWD_MAP"
  CWD_ALT="$BASE/cwd.alt"; : >"$CWD_ALT"
  PS_N="$BASE/ps.n"; LSOF_N="$BASE/lsof.n"; : >"$PS_N"; : >"$LSOF_N"
  cat >"$BASE/bin/ps" <<'EOS'
#!/bin/bash
set -euo pipefail
data="${REAPER_PS_DATA:?}"
if [[ "${1:-}" == -p ]]; then
  n=0; [[ -s "${REAPER_PS_N:?}" ]] && n=$(cat "${REAPER_PS_N}")
  n=$((n+1)); printf '%s\n' "$n" >"${REAPER_PS_N}"
  flip="${REAPER_PS_P_FLIP:-0}"
  if [[ "$flip" -gt 0 && "$n" -gt "$flip" ]]; then data="${REAPER_PS_ALT:?}"; fi
  pid="$2"; while IFS= read -r line; do
    set -- $line; [[ "$1" == "$pid" ]] && { printf '%s\n' "$line"; exit 0; }
  done <"$data"; exit 1
fi
cat "$data"
EOS
  cat >"$BASE/bin/lsof" <<'EOS'
#!/bin/bash
set -euo pipefail
pid=; while [[ $# -gt 0 ]]; do case "$1" in -p) pid="$2"; shift 2;; *) shift;; esac; done
[[ -n "$pid" ]] || exit 1
n=0; [[ -s "${REAPER_LSOF_N:?}" ]] && n=$(cat "${REAPER_LSOF_N}")
n=$((n+1)); printf '%s\n' "$n" >"${REAPER_LSOF_N}"
map="${REAPER_CWD_MAP:?}"
flip="${REAPER_LSOF_FLIP:-0}"
if [[ "$flip" -gt 0 && "$n" -gt "$flip" ]]; then map="${REAPER_CWD_ALT:?}"; fi
path=$(awk -F= -v p="$pid" '$1==p {print $2; exit}' "$map") || true
[[ -n "${path:-}" ]] || exit 1
printf 'p%s\nfcwd\nn%s\n' "$pid" "$path"
EOS
  cat >"$BASE/bin/kill" <<'EOS'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >>"${REAPER_KILL_LOG:?}"
[[ ! -e "${REAPER_KILL_FAIL:-}" ]]
EOS
  chmod +x "$BASE/bin/ps" "$BASE/bin/lsof" "$BASE/bin/kill"
  export REAPER_PS="$BASE/bin/ps" REAPER_LSOF="$BASE/bin/lsof" REAPER_KILL="$BASE/bin/kill"
  export REAPER_DATE=/bin/date REAPER_REALPATH=/bin/realpath
  export REAPER_PS_DATA="$PS_DATA" REAPER_PS_ALT="$PS_ALT" REAPER_PS_N="$PS_N"
  export REAPER_CWD_MAP="$CWD_MAP" REAPER_CWD_ALT="$CWD_ALT" REAPER_LSOF_N="$LSOF_N"
  export REAPER_KILL_LOG="$KILL_LOG" REAPER_KILL_FAIL="$KILL_FAIL"
  export REAPER_PS_P_FLIP=0 REAPER_LSOF_FLIP=0
  export REAPER_REVIEWS_ROOT="$REV" REAPER_WORKTREES_ROOT="$WT" REAPER_STATE_DIR="$ST"
  export REAPER_DENY_ROOTS="$DENY_WT:$DENY_MAIN:$DENY_CRAB:$DENY_CS:$DENY_CB:$DENY_CI"
  export REAPER_MIN_AGE_S=3600 REAPER_STABLE_S=600 REAPER_GRACE_S=60
}

mk_review() {
  local name="$1" dir
  dir="$REV/$name"
  mkdir -p "$dir/sub" "$WT/$name"
  printf 'gitdir: %s\n' "$WT/$name" >"$dir/.git"
  printf '%s\n' "$dir/.git" >"$WT/$name/gitdir"
  printf '../..\n' >"$WT/$name/commondir"
  printf '%s\n' "$dir"
}
add_ps() { printf '%5s %5s %5s %s\n' "$1" "$2" "$3" "$4" >>"$PS_DATA"; }
add_ps_alt() { printf '%5s %5s %5s %s\n' "$1" "$2" "$3" "$4" >>"$PS_ALT"; }
set_cwd() { printf '%s=%s\n' "$1" "$2" >>"$CWD_MAP"; }
set_cwd_alt() { printf '%s=%s\n' "$1" "$2" >>"$CWD_ALT"; }
run() {
  : >"$PS_N"; : >"$LSOF_N"
  RC=0; OUT=$(REAPER_NOW_EPOCH="$1" REAPER_DRY_RUN="${2:-0}" /bin/bash "$REAPER" 2>&1) || RC=$?
}
prime() { # record then ready for TERM
  add_ps "$1" 1 "$UID_N" "$LSTART"; set_cwd "$1" "$2"
  NOW=$((LSTART_EP + 7200)); run "$NOW" 0
}

# --- positive path ---
setup; WTDIR=$(mk_review scratch-good); PID=4242; prime "$PID" "$WTDIR"
assert '[[ $RC -eq 0 ]] && has "action=record"' 'record first scan'
assert '[[ -f "$ST/$PID" ]]' 'state written'; assert nokill 'no kill on record'
run "$((NOW + 601))" 0
assert 'has "action=term"' 'TERM after stable'
assert 'grep -q -- "-TERM $PID" "$KILL_LOG"' 'TERM argv'
: >"$KILL_LOG"; run "$((NOW + 601 + 61))" 0
assert 'has "action=kill"' 'KILL after grace'
assert 'grep -q -- "-KILL $PID" "$KILL_LOG"' 'KILL argv'
assert '[[ ! -f "$ST/$PID" ]]' 'state cleaned after kill'

# dry-run
setup; WTDIR=$(mk_review scratch-dry); PID=4243; prime "$PID" "$WTDIR"
ACC=; run "$((NOW+601))" 1; ACC="$ACC$OUT"$'\n'
run "$((NOW+601+61))" 1; ACC="$ACC$OUT"$'\n'; OUT=$ACC
assert 'has "action=would-term" && has "action=would-kill"' 'dry-run actions'
assert nokill 'dry-run never kills'

# gate negatives
setup; WTDIR=$(mk_review u); PID=50; add_ps "$PID" 1 "$((UID_N+1))" "$LSTART"; set_cwd "$PID" "$WTDIR"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'wrong uid'; assert nokill 'wrong uid no kill'
setup; WTDIR=$(mk_review p); PID=51; add_ps "$PID" 99 "$UID_N" "$LSTART"; set_cwd "$PID" "$WTDIR"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'live parent'
setup; WTDIR=$(mk_review y); PID=52; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$WTDIR"
run "$((LSTART_EP+10))" 0; assert '! has "action=record"' 'young'
setup; WTDIR=$(mk_review o); PID=53; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$BASE/elsewhere"; mkdir -p "$BASE/elsewhere"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'cwd outside'
setup; WTDIR=$(mk_review e); PID=54; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'cwd equal reviews'
setup; mkdir -p "$REV/g1" "$BASE/evil-git"; printf 'gitdir: %s\n' "$BASE/evil-git" >"$REV/g1/.git"
PID=55; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/g1"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'gitdir outside'
setup; mkdir -p "$REV/g2"; printf 'gitdir: %s\n' "$WT" >"$REV/g2/.git"
PID=56; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/g2"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'gitdir equal root'
setup; mkdir -p "$REV/gd/.git"; PID=57; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/gd"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'git dir rejected'
setup; mkdir -p "$REV/ng"; PID=58; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/ng"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'missing gitfile'

# A fake review cannot borrow a legitimate worktree's ownership via a .git symlink.
setup; GOOD=$(mk_review good); mkdir -p "$REV/fake"; ln -s "$GOOD/.git" "$REV/fake/.git"; PID=69
add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/fake"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'gitfile symlink rejected'; assert nokill 'gitfile symlink no signal'

# A gitfile cannot name a symlink alias for otherwise-valid metadata.
setup; GOOD=$(mk_review real); ln -s "$WT/real" "$WT/alias"; printf 'gitdir: %s\n' "$WT/alias" >"$GOOD/.git"; PID=75
add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$GOOD"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'metadata symlink rejected'; assert nokill 'metadata symlink no signal'
setup; WTDIR=$(mk_review d); mkdir -p "$DENY_WT/x"; PID=59
add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$DENY_WT/x"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'deny root cwd'

# fabricated / unrelated / wrong-commondir metadata (cannot record)
setup; mkdir -p "$REV/fab" "$WT/fab"; printf 'gitdir: %s\n' "$WT/fab" >"$REV/fab/.git"
PID=70; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/fab"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'fabricated metadata'
assert nokill 'fabricated no signal'
setup; mkdir -p "$REV/un" "$WT/un" "$REV/other"; printf 'gitdir: %s\n' "$WT/un" >"$REV/un/.git"
printf 'x\n' >"$REV/other/.git"; printf '%s\n' "$REV/other/.git" >"$WT/un/gitdir"; printf '../..\n' >"$WT/un/commondir"
PID=71; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/un"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'unrelated gitdir backlink'
assert nokill 'unrelated no signal'
setup; mkdir -p "$REV/wc" "$WT/wc" "$BASE/other.git"; printf 'gitdir: %s\n' "$WT/wc" >"$REV/wc/.git"
printf '%s\n' "$REV/wc/.git" >"$WT/wc/gitdir"; printf '%s\n' "$BASE/other.git" >"$WT/wc/commondir"
PID=72; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$REV/wc"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'wrong commondir'
assert nokill 'wrong commondir no signal'

# PID reuse
setup; WTDIR=$(mk_review r1); WT2=$(mk_review r2); PID=60; prime "$PID" "$WTDIR"
: >"$PS_DATA"; add_ps "$PID" 1 "$UID_N" "$LSTART2"; : >"$CWD_MAP"; set_cwd "$PID" "$WT2"
run "$((LSTART2_EP + 7200))" 0
assert 'has "reason=fingerprint_changed"' 'pid reuse reset'; assert nokill 'pid reuse no signal'

# vanish before TERM: birth change mid-run
setup; WTDIR=$(mk_review vb); PID=61; prime "$PID" "$WTDIR"
add_ps_alt "$PID" 1 "$UID_N" "$LSTART2"; export REAPER_PS_P_FLIP=1
: >"$KILL_LOG"; run "$((NOW+601))" 0
assert 'has "reason=vanished" && ! has "action=term"' 'TERM vanish birth'
assert 'nokill && [[ ! -f "$ST/$PID" ]]' 'TERM vanish birth cleared'
export REAPER_PS_P_FLIP=0

# vanish before TERM: cwd change mid-run
setup; WTDIR=$(mk_review vc); WT2=$(mk_review vc2); PID=66; prime "$PID" "$WTDIR"
set_cwd_alt "$PID" "$WT2"; export REAPER_LSOF_FLIP=1
: >"$KILL_LOG"; run "$((NOW+601))" 0
assert 'has "reason=vanished" && ! has "action=term"' 'TERM vanish cwd'
assert 'nokill && [[ ! -f "$ST/$PID" ]]' 'TERM vanish cwd cleared'
export REAPER_LSOF_FLIP=0

# vanish before KILL: birth change mid-run
setup; WTDIR=$(mk_review vk); PID=67; prime "$PID" "$WTDIR"
run "$((NOW+601))" 0; : >"$KILL_LOG"
add_ps_alt "$PID" 1 "$UID_N" "$LSTART2"; export REAPER_PS_P_FLIP=1
run "$((NOW+601+61))" 0
assert 'has "reason=vanished" && ! has "action=kill"' 'KILL vanish birth'
assert 'nokill && [[ ! -f "$ST/$PID" ]]' 'KILL vanish birth cleared'
export REAPER_PS_P_FLIP=0

# vanish before KILL: cwd change mid-run
setup; WTDIR=$(mk_review vkc); WT2=$(mk_review vkc2); PID=68; prime "$PID" "$WTDIR"
run "$((NOW+601))" 0; : >"$KILL_LOG"
set_cwd_alt "$PID" "$WT2"; export REAPER_LSOF_FLIP=1
run "$((NOW+601+61))" 0
assert 'has "reason=vanished" && ! has "action=kill"' 'KILL vanish cwd'
assert 'nokill && [[ ! -f "$ST/$PID" ]]' 'KILL vanish cwd cleared'
export REAPER_LSOF_FLIP=0

# TERM signal failure retains eligibility; retry succeeds
setup; WTDIR=$(mk_review sf); PID=73; prime "$PID" "$WTDIR"
: >"$KILL_FAIL"; : >"$KILL_LOG"; run "$((NOW+601))" 0
assert 'has "reason=signal_failed" && ! has "action=term"' 'TERM signal_failed'
assert '[[ -f "$ST/$PID" ]] && grep -q "^term_sent=$" "$ST/$PID"' 'TERM fail keeps empty term_sent'
assert 'grep -q -- "-TERM $PID" "$KILL_LOG"' 'TERM fail still invoked kill'
rm -f "$KILL_FAIL"; : >"$KILL_LOG"; run "$((NOW+601))" 0
assert 'has "action=term"' 'TERM retry success'
assert 'grep -q "^term_sent=[0-9]" "$ST/$PID"' 'term_sent stamped after success'

# KILL signal failure retains grace-complete state; retry succeeds
setup; WTDIR=$(mk_review sk); PID=74; prime "$PID" "$WTDIR"
run "$((NOW+601))" 0; : >"$KILL_LOG"; : >"$KILL_FAIL"
run "$((NOW+601+61))" 0
assert 'has "reason=signal_failed" && ! has "action=kill"' 'KILL signal_failed'
assert '[[ -f "$ST/$PID" ]] && grep -q "^term_sent=[0-9]" "$ST/$PID"' 'KILL fail retains term_sent'
assert 'grep -q -- "-KILL $PID" "$KILL_LOG"' 'KILL fail still invoked kill'
rm -f "$KILL_FAIL"; : >"$KILL_LOG"; run "$((NOW+601+61))" 0
assert 'has "action=kill" && [[ ! -f "$ST/$PID" ]]' 'KILL retry success'

# KILL before grace
setup; WTDIR=$(mk_review g); PID=62; prime "$PID" "$WTDIR"
run "$((NOW+601))" 0; : >"$KILL_LOG"; run "$((NOW+601+10))" 0
assert 'has "reason=grace"' 'KILL blocked by grace'; assert nokill 'no KILL before grace'

# corrupt / unparsable / telemetry / prefix escape
setup; WTDIR=$(mk_review c); PID=63; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$WTDIR"
printf 'bogus\n' >"$ST/$PID"; run "$((LSTART_EP+7200))" 0
assert 'has "reason=corrupt_state" || has "action=record"' 'corrupt resets'; assert nokill 'corrupt no signal'
setup; printf 'not-a-ps-line\n' >"$PS_DATA"
RC=0; OUT=$(REAPER_NOW_EPOCH="$((LSTART_EP+7200))" REAPER_DRY_RUN=0 /bin/bash "$REAPER" 2>&1) || RC=$?
assert '[[ $RC -ne 0 ]]' 'unparsable ps fails'; assert nokill 'unparsable zero signals'
setup; WTDIR=$(mk_review t); PID=64; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$WTDIR"
OUT=$(REAPER_NOW_EPOCH="$((LSTART_EP+7200))" REAPER_DRY_RUN=1 /bin/bash "$REAPER" 2>&1)
assert 'printf "%s\n" "$OUT" | grep -E "^epoch=[0-9]+ pid=[0-9]+ age=[0-9]+ worktree=[A-Za-z0-9._-]+ action=[a-z_-]+ reason=[a-z_]+$" >/dev/null' 'telemetry grammar'
setup; mkdir -p "$BASE/reviews-evil/x" "$WT/evil"; printf 'gitdir: %s\n' "$WT/evil" >"$BASE/reviews-evil/x/.git"
printf '%s\n' "$BASE/reviews-evil/x/.git" >"$WT/evil/gitdir"; printf '../..\n' >"$WT/evil/commondir"
PID=65; add_ps "$PID" 1 "$UID_N" "$LSTART"; set_cwd "$PID" "$BASE/reviews-evil/x"
run "$((LSTART_EP+7200))" 0; assert '! has "action=record"' 'symlink/prefix escape'

echo "PASS=$PASS FAIL=$FAIL"
[[ "$FAIL" -eq 0 ]]
