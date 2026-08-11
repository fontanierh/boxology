#!/bin/bash
# Fixture processes only. Every signal target is created by this test.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; S="$ROOT/supervise.sh"; PASS=0; FAIL=0; TMP=; FIXTURE_PIDS=; FIXTURE_GROUPS=
ok() { if eval "$1"; then PASS=$((PASS+1)); else printf 'FAIL: %s\n' "$2"; FAIL=$((FAIL+1)); fi; }
birth() { local r; r=$(/bin/ps -p "$1" -o pid=,lstart=,comm= 2>/dev/null) || return; [[ -n "$r" ]] || return; printf '%s\n' "$r" | /usr/bin/shasum -a 256 | /usr/bin/awk '{print $1}'; }
track_pid() { local p=$1 f i; for i in $(jot 20); do f=$(birth "$p"); [[ -n "$f" ]] && break; /bin/sleep .01; done; [[ -n "$f" ]] && FIXTURE_PIDS="$FIXTURE_PIDS $p:$f"; }
track_group() { local g=$1 f i members= p; f=$(birth "$g"); [[ -n "$f" ]] && FIXTURE_GROUPS="$FIXTURE_GROUPS $g:$f"; for i in $(jot 50); do members=$(/bin/ps -axo pid=,pgid= | /usr/bin/awk -v x="$g" '$2==x {print $1}'); set -- $members; [[ $# -ge 2 ]] && break; /bin/sleep .01; done; for p in $members; do track_pid "$p"; done; }
cleanup() { local d e p f; trap - EXIT; for e in $FIXTURE_PIDS; do p=${e%%:*}; f=${e#*:}; [[ "$p" =~ ^[1-9][0-9]*$ && "$p" -gt 1 && "$(birth "$p")" == "$f" ]] && /bin/kill -KILL "$p" 2>/dev/null; done; for e in $FIXTURE_GROUPS; do p=${e%%:*}; f=${e#*:}; [[ "$p" =~ ^[1-9][0-9]*$ && "$p" -gt 1 && "$(birth "$p")" == "$f" ]] && /bin/kill -KILL "-$p" 2>/dev/null; done; /bin/sleep .1; for d in $TMP; do case "$d" in /tmp/dw-test.*|/private/tmp/dw-test.*) /bin/rm -rf "$d";; esac; done; }
trap cleanup EXIT
/bin/bash -n "$S" || exit 1

env_for() { export DELIVERY_WORKER_TEST_MODE=1 DW_STATE_DIR="$B/state" DW_GRACE_SECONDS=0; }
new() { B=$(/bin/realpath "$(mktemp -d /tmp/dw-test.XXXXXX)"); TMP="$TMP $B"; /bin/mkdir -m 700 "$B/state"; env_for; }
wait_record() { local i; for i in $(jot 500); do [[ -f "$DW_STATE_DIR/$1.record" ]] && return; /bin/sleep .02; done; return 1; }
run_real() { /bin/bash "$S" run --run-id "$1" --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- "$2" "${3:-}"; }

# Build the same reciprocal linked-worktree topology production requires. The test caller may be a
# primary CI checkout or a local linked worktree; neither is itself treated as owned fixture state.
FIXTURE=$(/bin/realpath "$(mktemp -d /tmp/dw-test.XXXXXX)"); TMP="$TMP $FIXTURE"
REPO="$FIXTURE/repo"; ALLOWED="$FIXTURE/allowed"; WT="$ALLOWED/worker"; /bin/mkdir "$ALLOWED"
/usr/bin/git init -q "$REPO"; /usr/bin/git -C "$REPO" -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm base
/usr/bin/git -C "$REPO" worktree add -q --detach "$WT" HEAD
export DELIVERY_WORKER_TEST_MODE=1 DW_MAIN="$REPO" DW_COMMON_GIT="$REPO/.git" DW_ALLOWED_ROOT="$ALLOWED" DW_DENIED_ROOTS="$ALLOWED/review-scratch:$ALLOWED/crab-runtime"

if [[ "${DW_FOCUS_REPEATS:-0}" =~ ^[1-9][0-9]*$ ]]; then
  for n in $(jot "$DW_FOCUS_REPEATS"); do
    new; RUN="churn-$n"; /bin/bash "$S" run --run-id "$RUN" --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'while :; do /bin/sleep .03 & /bin/sleep .01; done' >"$B/run.out" 2>&1 & SUP=$!
    track_pid "$SUP"; wait_record "$RUN" || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/$RUN.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
    OUT=$(/bin/bash "$S" reap --run-id "$RUN"); RC=$?; LEFT=$(/bin/ps -axo pgid= | /usr/bin/awk -v g="$PID" '$1==g {n++} END {print n+0}'); REASON=unknown; case "$OUT" in *' reason='*) REASON=${OUT#* reason=}; REASON=${REASON%% *};; esac
    ok '[[ $RC -eq 0 && $LEFT -eq 0 && ! -e "$DW_STATE_DIR/$RUN.record" ]]' "focused anchor/churn reap $n (rc=$RC reason=$REASON members=$LEFT)"
  done
  printf 'PASS=%s FAIL=%s\n' "$PASS" "$FAIL"; [[ $FAIL -eq 0 ]]; exit
fi

# Gated launch is a dedicated group; a clean command clears without signaling.
new; OUT=$(run_real clean /usr/bin/true); RC=$?
ok '[[ $RC -eq 0 && ! -e "$DW_STATE_DIR/clean.record" && "$OUT" == *"action=clear reason=normal"* ]]' 'clean launch and zero-signal cleanup'

# Child-created replacements cannot reach the unlinked one-way status/control pipes.
new; FORGE="$B/forge.result"; OUT=$(/bin/bash "$S" run --run-id forge --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 's="$DW_STATE_DIR/forge.status"; c="$DW_STATE_DIR/forge.control"; [[ ! -e "$s" && ! -e "$c" ]] || exit 90; /usr/bin/mkfifo "$s" "$c"; /usr/bin/perl -MFcntl=O_WRONLY,O_NONBLOCK -e '\''for $p (@ARGV) { sysopen(my $f,$p,O_WRONLY|O_NONBLOCK) and exit 1 }'\'' "$s" "$c" || exit 91; printf "blocked\n" >"$1"' fixture "$FORGE"); RC=$?
ok '[[ $RC -eq 0 && "$(<"$FORGE")" == blocked && ! -e "$DW_STATE_DIR/forge.status" && ! -e "$DW_STATE_DIR/forge.control" && "$OUT" == *"action=clear reason=normal"* ]]' 'child cannot forge anonymous status or release and replacements are cleaned'

# A durable-write failure aborts the still-gated guardian and removes partial state.
new; DELIVERY_WORKER_TEST_MODE=1 DW_STATE_DIR="$DW_STATE_DIR" DW_GRACE_SECONDS=0 DW_FAIL_RECORD_WRITE=1 /bin/bash "$S" run --run-id writefail --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; GUARD=; for i in $(jot 100); do GUARD=$(/bin/ps -axo pid=,ppid=,pgid= | /usr/bin/awk -v s="$SUP" '$2==s && $1==$3 {print $1; exit}'); [[ -n "$GUARD" ]] && break; /bin/sleep .02; done
[[ -n "$GUARD" ]] || exit 1; track_pid "$GUARD"; wait "$SUP"; RC=$?; /bin/kill -0 "$GUARD" 2>/dev/null; LIVE=$?; LEFT=0
for f in "$DW_STATE_DIR/writefail.identity" "$DW_STATE_DIR"/writefail.gate.* "$DW_STATE_DIR"/writefail.record* "$DW_STATE_DIR"/.writefail.launch.*; do [[ -e "$f" ]] && LEFT=$((LEFT+1)); done
ok '[[ $RC -ne 0 && $LIVE -ne 0 && $LEFT -eq 0 ]]' 'record-write failure aborts gated group and partial state'

# If the supervisor is interrupted as the command exits, the exact anchor remains until release.
new; /bin/bash "$S" run --run-id anchor --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep .1 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record anchor || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/anchor.record"); track_pid "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null; /bin/sleep .2
PST=$(/bin/ps -p "$PID" -o pid=,pgid=,state=); ARTIFACTS=0; for f in "$DW_STATE_DIR/anchor.identity" "$DW_STATE_DIR/anchor.control" "$DW_STATE_DIR/anchor.status" "$DW_STATE_DIR/anchor.gate.$SUP" "$DW_STATE_DIR/.anchor.launch.$SUP"; do [[ -e "$f" ]] && ARTIFACTS=$((ARTIFACTS+1)); done
OUT=$(/bin/bash "$S" reap --run-id anchor); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $PST == *"$PID"*"$PID"*s* && $ARTIFACTS -eq 0 && $RC -eq 0 && $LIVE -ne 0 && "$OUT" == *"action=clear reason=kill"* ]]' 'anchor holds after interrupted EOF, launch artifacts are gone, and reaper performs one final KILL'

# Interrupt only the fixture supervisor, then TERM the exact owned fixture group.
new; /bin/bash "$S" run --run-id term --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record term || exit 1; read PID PGID < <(/usr/bin/awk -F= '/^pid=/{p=$2}/^pgid=/{g=$2}END{print p,g}' "$DW_STATE_DIR/term.record"); track_group "$PID"
PST=$(/bin/ps -p "$PID" -o pid=,pgid=,state=); SESSION=$(/usr/bin/awk -F= '/^session=/{print $2}' "$DW_STATE_DIR/term.record")
JOIN="$B/join"; /usr/bin/perl -MPOSIX -e '$SIG{TERM}=sub{exit 0}; $r=POSIX::setpgid(0,0+$ARGV[0]); $ok=defined($r); open(F, ">", $ARGV[1]) or exit 2; print F ($ok ? "1\n" : "0\n"); close(F); sleep 300' "$PID" "$JOIN" & SIB=$!; track_pid "$SIB"
for i in $(jot 100); do [[ -f "$JOIN" ]] && break; /bin/sleep .01; done; [[ -f "$JOIN" ]] || exit 1; JOINED=$(<"$JOIN"); SIB_PGID=$(/bin/ps -p "$SIB" -o pgid= | /usr/bin/awk '{print $1}')
/bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null; OUT=$(/bin/bash "$S" reap --run-id term); RC=$?; /bin/kill -0 "$SIB" 2>/dev/null; SIB_LIVE=$?; /bin/kill -TERM "$SIB"; wait "$SIB" 2>/dev/null
ok '[[ $PST == *"$PID"*"$PGID"*s* && $PID == "$PGID" && $SESSION == "$PID" ]]' 'guardian is the recorded process-group and session leader'
ok '[[ $JOINED == 0 && $SIB_PGID != "$PID" && $SIB_LIVE -eq 0 ]]' 'outside sibling cannot join the guardian session group and survives its TERM'
ok '[[ $RC -eq 0 && "$OUT" == *"action=clear reason=kill"* && ! -e "$DW_STATE_DIR/term.record" ]]' 'TERM leaves the resistant anchor for the one final group KILL'

# A disappeared non-TTY launcher and normal owned child churn remain safely reapable.
new; /bin/bash "$S" run --run-id churn --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'while :; do /bin/sleep .03 & /bin/sleep .01; done' >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record churn || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/churn.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
PPID_NOW=; for i in $(jot 100); do PPID_NOW=$(/bin/ps -p "$PID" -o ppid= | /usr/bin/awk '{print $1}'); [[ "$PPID_NOW" == 1 ]] && break; /bin/sleep .01; done
OUT=$(/bin/bash "$S" reap --run-id churn --dry-run); RC=$?; LIVE=$(/bin/ps -axo pgid= | /usr/bin/awk -v g="$PID" '$1==g {n++} END {print n+0}')
ok '[[ $RC -eq 0 && "$PPID_NOW" == 1 && "$LIVE" -gt 1 && "$OUT" == *"action=would-term reason=verified"* ]]' 'reparented guardian with churning descendants passes dry-run proof'
OUT=$(/bin/bash "$S" reap --run-id churn); RC=$?; LEFT=$(/bin/ps -axo pgid= | /usr/bin/awk -v g="$PID" '$1==g {print}'); REASON=unknown; case "$OUT" in *' reason='*) REASON=${OUT#* reason=}; REASON=${REASON%% *};; esac; LEFT_COUNT=$(printf '%s\n' "$LEFT" | /usr/bin/awk 'NF {n++} END {print n+0}')
ok '[[ $RC -eq 0 && -z "$LEFT" && "$OUT" == *"action=clear reason=kill"* && ! -e "$DW_STATE_DIR/churn.record" ]]' "reparented guardian with churning descendants is reaped as its owned group (rc=$RC reason=$REASON members=$LEFT_COUNT)"

# A rostered member may exec after preparation; a two-way private test barrier confirms resistance.
new; LOCK="$B/.cargo-lock"; : >"$LOCK"; : >"$DW_STATE_DIR/transition.test-arm"; /bin/chmod 600 "$DW_STATE_DIR/transition.test-arm"
/bin/bash "$S" run --run-id transition --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'while [[ ! -e "$DW_STATE_DIR/transition.test-trigger" ]]; do /bin/sleep .01; done; trap "" TERM; exec 4<>"$1"; printf "/bin/sleep\n" >"$DW_STATE_DIR/transition.test-ready"; /bin/chmod 600 "$DW_STATE_DIR/transition.test-ready"; exec /bin/sleep 300' fixture "$LOCK" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record transition || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/transition.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(/bin/bash "$S" reap --run-id transition); RC=$?; LEFT=$(/bin/ps -axo pgid= | /usr/bin/awk -v g="$PID" '$1==g {print}'); REASON=unknown; case "$OUT" in *' reason='*) REASON=${OUT#* reason=}; REASON=${REASON%% *};; esac; LEFT_COUNT=$(printf '%s\n' "$LEFT" | /usr/bin/awk 'NF {n++} END {print n+0}')
ok '[[ $RC -eq 0 && -z "$LEFT" && "$OUT" == *"action=clear reason=kill owned_lock=released"* && ! -e "$DW_STATE_DIR/transition.record" ]]' "post-barrier exec and Cargo lock are proven before final KILL (rc=$RC reason=$REASON members=$LEFT_COUNT)"

# Force every bounded observation roster to change; lock telemetry must degrade truthfully to unknown.
new; LOCK="$B/.cargo-lock"; : >"$LOCK"; : >"$DW_STATE_DIR/unstable.test-unstable"; /bin/chmod 600 "$DW_STATE_DIR/unstable.test-unstable"
/bin/bash "$S" run --run-id unstable --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'exec 4<>"$1"; last=; while :; do if [[ -f "$DW_STATE_DIR/unstable.test-unstable-trigger" ]]; then n=$(<"$DW_STATE_DIR/unstable.test-unstable-trigger"); if [[ "$n" != "$last" ]]; then /bin/sleep 300 & last=$n; printf "%s\n" "$n" >"$DW_STATE_DIR/unstable.test-unstable-ready"; /bin/chmod 600 "$DW_STATE_DIR/unstable.test-unstable-ready"; fi; fi; /bin/sleep .01; done' fixture "$LOCK" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record unstable || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/unstable.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
printf '%s\n' '#!/bin/bash' '/usr/bin/awk -F= '\''/^lock_complete=/{print $2}'\'' "$CHECK_RECORD" >"$CHECK_OUT"' 'exec /bin/kill "$@"' >"$B/check-kill"; /bin/chmod 700 "$B/check-kill"; export CHECK_RECORD="$DW_STATE_DIR/unstable.record" CHECK_OUT="$B/lock-complete"
OUT=$(DW_KILL="$B/check-kill" /bin/bash "$S" reap --run-id unstable); RC=$?; COMPLETE=$(<"$CHECK_OUT")
ok '[[ $RC -eq 0 && $COMPLETE == 0 && "$OUT" == *"action=clear reason=kill owned_lock=unknown"* ]]' 'bounded roster instability persists lock_complete=0 and emits only owned_lock=unknown'

# A process that deliberately leaves the anchor PGID but remains in its session is reported, not signaled.
new; ESC_FILE="$B/escaped.pid"; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 0' >"$B/log-kill"; /bin/chmod 700 "$B/log-kill"; export DW_KILL_LOG="$B/escape-kill.log"
/bin/bash "$S" run --run-id escape --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/perl -MPOSIX -e 'POSIX::setpgid(0,0); open(F, ">", $ARGV[0]) or exit 2; print F "$$\n"; close(F); sleep 300' "$ESC_FILE" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record escape || exit 1; for i in $(jot 100); do [[ -f "$ESC_FILE" ]] && break; /bin/sleep .01; done; [[ -f "$ESC_FILE" ]] || exit 1
PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/escape.record"); ESC=$(<"$ESC_FILE"); track_pid "$PID"; track_pid "$ESC"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(DW_KILL="$B/log-kill" /bin/bash "$S" reap --run-id escape 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; ANCHOR_LIVE=$?; /bin/kill -0 "$ESC" 2>/dev/null; ESC_LIVE=$?; SIGNALS=0; [[ -e "$DW_KILL_LOG" ]] && SIGNALS=$(wc -l <"$DW_KILL_LOG")
ok '[[ $RC -eq 70 && $ANCHOR_LIVE -eq 0 && $ESC_LIVE -eq 0 && $SIGNALS -eq 0 && "$OUT" == *"reason=session_escape"* ]]' 'same-session process-group escape fails closed without any signal'
/bin/kill -KILL "$ESC" 2>/dev/null; /bin/sleep .1; /bin/bash "$S" reap --run-id escape >/dev/null

# A direct command that creates a new session remains an anchored descendant and is refused.
new; SID_FILE="$B/setsid.pid"; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 0' >"$B/setsid-kill"; /bin/chmod 700 "$B/setsid-kill"; export DW_KILL_LOG="$B/setsid-kill.log"
/bin/bash "$S" run --run-id setsid --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/perl -MPOSIX -e 'defined(POSIX::setsid()) or exit 2; open(F, ">", $ARGV[0]) or exit 3; print F "$$\n"; close(F); sleep 300' "$SID_FILE" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record setsid || exit 1; for i in $(jot 100); do [[ -f "$SID_FILE" ]] && break; /bin/sleep .01; done; [[ -f "$SID_FILE" ]] || exit 1
PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/setsid.record"); ESC=$(<"$SID_FILE"); track_pid "$PID"; track_pid "$ESC"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(DW_KILL="$B/setsid-kill" /bin/bash "$S" reap --run-id setsid 2>&1); RC=$?; SIGNALS=0; [[ -e "$DW_KILL_LOG" ]] && SIGNALS=$(wc -l <"$DW_KILL_LOG"); /bin/kill -0 "$ESC" 2>/dev/null; ESC_LIVE=$?
ok '[[ $RC -eq 70 && $SIGNALS -eq 0 && $ESC_LIVE -eq 0 && "$OUT" == *"reason=session_escape"* ]]' 'direct setsid descendant is detected across sessions and receives no signal'
/bin/kill -KILL "$ESC" 2>/dev/null; /bin/sleep .1; /bin/bash "$S" reap --run-id setsid >/dev/null

# TERM-resistant fixture reaches KILL; the unrelated same-name sleep survives.
new; /bin/sleep 300 & OTHER=$!
track_pid "$OTHER"
/bin/bash "$S" run --run-id kill --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'trap "" TERM; exec /bin/sleep 300' >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record kill || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/kill.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(/bin/bash "$S" reap --run-id kill); RC=$?; /bin/kill -0 "$OTHER" 2>/dev/null; LIVE=$?; /bin/kill -TERM "$OTHER"; wait "$OTHER" 2>/dev/null
LEFT=$(/bin/ps -axo pgid= | /usr/bin/awk -v g="$PID" '$1==g {print}')
ok '[[ $RC -eq 0 && "$OUT" == *"action=clear reason=kill"* && -z "$LEFT" ]]' 'partial group exit reaches KILL and leaves no owned survivor'
ok '[[ $LIVE -eq 0 ]]' 'unrelated same-name process untouched'

# Dry-run performs proof but neither advances state nor signals the group.
new; /bin/bash "$S" run --run-id dry --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record dry || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/dry.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
BEFORE=$(/sbin/md5 -q "$DW_STATE_DIR/dry.record"); OUT=$(/bin/bash "$S" reap --run-id dry --dry-run); RC=$?; AFTER=$(/sbin/md5 -q "$DW_STATE_DIR/dry.record")
PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/dry.record"); /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 0 && $BEFORE == "$AFTER" && $LIVE -eq 0 && "$OUT" == "run=dry phase=implement harness=codex pid=$PID pgid=$PID action=would-term reason=verified owned_lock=not_observed" ]]' 'dry-run has exact sanitized telemetry, zero signal, and zero state advance'
/bin/bash "$S" reap --run-id dry >/dev/null

# release_sent is irreversible: a fresh reaper has no inherited writer and never substitutes a signal.
new; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 0' >"$B/release-kill"; /bin/chmod 700 "$B/release-kill"; export DW_KILL_LOG="$B/release-kill.log"
/bin/bash "$S" run --run-id releasehold --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record releasehold || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/releasehold.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
/usr/bin/awk '/^stage=/{print "stage=release_sent"; next}{print}' "$DW_STATE_DIR/releasehold.record" >"$B/release.record"; /bin/mv "$B/release.record" "$DW_STATE_DIR/releasehold.record"; /bin/chmod 600 "$DW_STATE_DIR/releasehold.record"
OUT=$(DW_KILL="$B/release-kill" /bin/bash "$S" reap --run-id releasehold 2>&1); RC=$?; SIGNALS=0; [[ -e "$DW_KILL_LOG" ]] && SIGNALS=$(wc -l <"$DW_KILL_LOG"); /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 70 && $SIGNALS -eq 0 && $LIVE -eq 0 && "$OUT" == *"reason=release_pending"* ]]' 'release_sent resume is signal-free without the inherited writer'
/bin/kill -KILL "-$PID" 2>/dev/null; /bin/sleep .1

# term_prepared retries TERM with the same anchor; term_sent resumes directly at KILL.
new; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 1' >"$B/fail-signal"; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exec /bin/kill "$@"' >"$B/pass-signal"; /bin/chmod 700 "$B/fail-signal" "$B/pass-signal"; FAIL_SIGNAL="$B/fail-signal"; PASS_SIGNAL="$B/pass-signal"; export DW_KILL_LOG="$B/intent.log"
/bin/bash "$S" run --run-id termintent --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!; track_pid "$SUP"; wait_record termintent || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/termintent.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
DW_KILL="$FAIL_SIGNAL" /bin/bash "$S" reap --run-id termintent >/dev/null 2>&1; FIRST_RC=$?; STAGE=$(/usr/bin/awk -F= '/^stage=/{print $2}' "$DW_STATE_DIR/termintent.record")
OUT=$(DW_KILL="$PASS_SIGNAL" /bin/bash "$S" reap --run-id termintent); RC=$?; TERMS=$(/usr/bin/grep -c -- '^-TERM ' "$DW_KILL_LOG"); KILLS=$(/usr/bin/grep -c -- '^-KILL ' "$DW_KILL_LOG")
ok '[[ $FIRST_RC -eq 71 && $STAGE == term_prepared && $RC -eq 0 && $TERMS -eq 2 && $KILLS -eq 1 && "$OUT" == *"reason=kill"* ]]' 'term_prepared resume repeats TERM once, then records and performs KILL'

new; printf '%s\n' '#!/bin/bash' '/bin/kill -KILL "$PPID"' 'exit 0' >"$B/crash-sleep"; /bin/chmod 700 "$B/crash-sleep"; export DW_KILL_LOG="$B/sent.log"
/bin/bash "$S" run --run-id termsent --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!; track_pid "$SUP"; wait_record termsent || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/termsent.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
DW_KILL="$PASS_SIGNAL" DW_SLEEP="$B/crash-sleep" /bin/bash "$S" reap --run-id termsent >/dev/null 2>&1; CRASH_RC=$?; STAGE=$(/usr/bin/awk -F= '/^stage=/{print $2}' "$DW_STATE_DIR/termsent.record")
OUT=$(DW_KILL="$PASS_SIGNAL" /bin/bash "$S" reap --run-id termsent); RC=$?; TERMS=$(/usr/bin/grep -c -- '^-TERM ' "$DW_KILL_LOG"); KILLS=$(/usr/bin/grep -c -- '^-KILL ' "$DW_KILL_LOG")
ok '[[ $CRASH_RC -ne 0 && $STAGE == term_sent && $RC -eq 0 && $TERMS -eq 1 && $KILLS -eq 1 && "$OUT" == *"reason=kill"* ]]' 'term_sent resume does not repeat TERM and performs one anchor-proven KILL'

# kill_intent is repeatable only with the exact live anchor, and clears empty intent without a signal.
new; export DW_KILL_LOG="$B/kill-live.log"; /bin/bash "$S" run --run-id killlive --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!; track_pid "$SUP"; wait_record killlive || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/killlive.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
/usr/bin/awk '/^stage=/{print "stage=kill_intent"; next}{print}' "$DW_STATE_DIR/killlive.record" >"$B/killlive.record"; /bin/mv "$B/killlive.record" "$DW_STATE_DIR/killlive.record"; /bin/chmod 600 "$DW_STATE_DIR/killlive.record"
OUT=$(DW_KILL="$PASS_SIGNAL" /bin/bash "$S" reap --run-id killlive); RC=$?; TERMS=0; KILLS=$(/usr/bin/grep -c -- '^-KILL ' "$DW_KILL_LOG")
ok '[[ $RC -eq 0 && $TERMS -eq 0 && $KILLS -eq 1 && "$OUT" == *"reason=kill"* ]]' 'live kill_intent re-proves the anchor and sends one KILL'

new; /bin/bash "$S" run --run-id killempty --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/false >"$B/run.out" 2>&1; EMPTY_RC=$?; /usr/bin/awk '/^stage=/{print "stage=kill_intent"; next}{print}' "$DW_STATE_DIR/killempty.record" >"$B/killempty.record"; /bin/mv "$B/killempty.record" "$DW_STATE_DIR/killempty.record"; /bin/chmod 600 "$DW_STATE_DIR/killempty.record"; export DW_KILL_LOG="$B/kill-empty.log"
OUT=$(DW_KILL="$PASS_SIGNAL" /bin/bash "$S" reap --run-id killempty); RC=$?; SIGNALS=0; [[ -e "$DW_KILL_LOG" ]] && SIGNALS=$(wc -l <"$DW_KILL_LOG")
ok '[[ $EMPTY_RC -eq 1 && $RC -eq 0 && $SIGNALS -eq 0 && "$OUT" == *"reason=kill"* ]]' 'empty kill_intent clears without signaling a numeric group'

# A failed TERM retains resumable term_prepared state and exact private lock identity.
new; LOCK="$B/.cargo-lock"; : >"$LOCK"; printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 1' >"$B/fail-kill"; /bin/chmod 700 "$B/fail-kill"; export DW_KILL_LOG="$B/kill.log"
/bin/bash "$S" run --run-id lock --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'exec 4<>"$1"; exec /bin/sleep 300' fixture "$LOCK" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record lock || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/lock.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(DW_KILL="$B/fail-kill" /bin/bash "$S" reap --run-id lock 2>&1); RC=$?; STAGE=$(/usr/bin/awk -F= '/^stage=/{print $2}' "$DW_STATE_DIR/lock.record"); LOCK_ROW=$(/usr/bin/awk -F= '/^owned_lock=/{print $2}' "$DW_STATE_DIR/lock.record"); LOCK_ID=$(/usr/bin/stat -f '%d:%i' "$LOCK"); LOCK_PID=${LOCK_ROW%%|*}
ok '[[ $RC -eq 71 && "$OUT" == *"action=retain reason=term_failed owned_lock=unknown"* && "$STAGE" == term_prepared && "$LOCK_ROW" == "$LOCK_PID|$LOCK_ID|u|-|$LOCK" && "$LOCK_PID" =~ ^[1-9][0-9]*$ && "$(<"$B/kill.log")" == "-TERM -$PID" ]] && /usr/bin/grep -Eq "^member=$LOCK_PID:[a-f0-9]{64}$" "$DW_STATE_DIR/lock.record"' 'TERM failure retains the exact member and private lock observation'
cp "$DW_STATE_DIR/lock.record" "$B/lock.saved"; SAVE=$(/sbin/md5 -q "$B/lock.saved"); /usr/bin/awk '/^control_id=/{print "control_id=0:0"; next}{print}' "$B/lock.saved" >"$DW_STATE_DIR/lock.record"
OUT=$(/bin/bash "$S" reap --run-id lock 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 70 && $LIVE -eq 0 && "$OUT" == *"reason=ambiguous"* ]]' 'changed private-control identity fails closed'
cp "$B/lock.saved" "$DW_STATE_DIR/lock.record"; /usr/bin/awk '/^owned_lock=/{sub(/^owned_lock=[^|]*/, "owned_lock=999999")}{print}' "$B/lock.saved" >"$DW_STATE_DIR/lock.record"
OUT=$(/bin/bash "$S" reap --run-id lock 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -ne 0 && $LIVE -eq 0 && "$OUT" == *"reason=unsafe_record"* ]]' 'lock holder outside the exact owned-member set fails closed'
cp "$B/lock.saved" "$DW_STATE_DIR/lock.record"; OUT=$(/bin/bash "$S" reap --run-id lock); RC=$?
ok '[[ $RC -eq 0 && "$SAVE" == "$(/sbin/md5 -q "$B/lock.saved")" && "$OUT" == "run=lock phase=repair harness=codex pid=$PID pgid=$PID action=clear reason=kill owned_lock=released" && ! -e "$DW_STATE_DIR/lock.record" ]]' 'term_prepared resumes and proves the owned Cargo lock released'

# A shared Cargo lock is attributed, reported, and never broadens the signal target.
new; LOCK="$B/.cargo-lock"; : >"$LOCK"; /bin/bash -c 'exec 4<>"$1"; exec /bin/sleep 300' fixture "$LOCK" & OTHER=$!; track_pid "$OTHER"
/bin/bash "$S" run --run-id shared --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'exec 4<>"$1"; exec /bin/sleep 300' fixture "$LOCK" >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record shared || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/shared.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(/bin/bash "$S" reap --run-id shared); RC=$?; /bin/kill -0 "$OTHER" 2>/dev/null; LIVE=$?; /bin/kill -TERM "$OTHER"; wait "$OTHER" 2>/dev/null
ok '[[ $RC -eq 0 && $LIVE -eq 0 && "$OUT" == *"owned_lock=released shared_lock=held_by_other"* ]]' 'shared Cargo lock is reported while its unrelated holder survives'

# Empty-group dry-run reports the pending clear without changing the record.
new; /bin/bash "$S" run --run-id empty --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/false >"$B/run.out" 2>&1
RUN_RC=$?
BEFORE=$(/sbin/md5 -q "$DW_STATE_DIR/empty.record"); OUT=$(/bin/bash "$S" reap --run-id empty --dry-run); RC=$?; AFTER=$(/sbin/md5 -q "$DW_STATE_DIR/empty.record")
ok '[[ $RUN_RC -eq 1 && $RC -eq 0 && $BEFORE == "$AFTER" && "$OUT" == *"action=would-clear reason=group_empty"* ]]' 'status handshake returns exact command rc and empty dry-run changes no state'; /bin/bash "$S" reap --run-id empty >/dev/null

# Records are private and permissive/corrupt ownership never signals.
new; /bin/bash "$S" run --run-id bad --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record bad || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/bad.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
cp "$DW_STATE_DIR/bad.record" "$B/bad.saved"; /usr/bin/awk -v missing="$B/deleted-worktree" '/^worktree=/{print "worktree=" missing; next}{print}' "$B/bad.saved" >"$DW_STATE_DIR/bad.record"
OUT=$(/bin/bash "$S" reap --run-id bad --dry-run 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 70 && $LIVE -eq 0 && "$OUT" == *"reason=ambiguous"* ]]' 'deleted worktree identity fails closed without signaling'
cp "$B/bad.saved" "$DW_STATE_DIR/bad.record"; /usr/bin/awk '/^lstart=/{print "lstart=Mon Jan  1 00:00:00 2001"; next}{print}' "$B/bad.saved" >"$DW_STATE_DIR/bad.record"
OUT=$(/bin/bash "$S" reap --run-id bad --dry-run 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 70 && $LIVE -eq 0 && "$OUT" == *"reason=ambiguous"* ]]' 'stale guardian birth identity fails closed without signaling'
cp "$B/bad.saved" "$DW_STATE_DIR/bad.record"; /usr/bin/awk '/^schema=/{print "schema=1"; next}{print}' "$B/bad.saved" >"$DW_STATE_DIR/bad.record"
OUT=$(/bin/bash "$S" reap --run-id bad --dry-run 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 70 && $LIVE -eq 0 && "$OUT" == *"reason=legacy_shared_session"* ]]' 'legacy live group is inspectable but never group-signaled'
cp "$B/bad.saved" "$DW_STATE_DIR/bad.record"
/bin/chmod 644 "$DW_STATE_DIR/bad.record"; OUT=$(/bin/bash "$S" reap --run-id bad 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -ne 0 && $LIVE -eq 0 && "$OUT" == *"reason=unsafe_record"* ]]' 'permissive record fails closed'
/bin/chmod 600 "$DW_STATE_DIR/bad.record"; /bin/bash "$S" reap --run-id bad >/dev/null

# Model numeric PID/PGID/SID reuse after anchor loss: proof refuses and the kill shim stays untouched.
new; /bin/bash "$S" run --run-id recycle --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record recycle || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/recycle.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null; /bin/kill -KILL "-$PID"; /bin/sleep .1
printf '%s\n' '#!/bin/bash' 'if [[ "$1" == -axo ]]; then printf "%s %s %s %s Mon Jan  1 00:00:00 2001 /bin/bash\n" "$FAKE_PID" 1 "$FAKE_PID" "$FAKE_UID"; exit 0; fi' 'exec /bin/ps "$@"' >"$B/fake-ps"
printf '%s\n' '#!/bin/bash' '/usr/bin/awk -v sid="$FAKE_SID" '\''{printf "%s %s %s %s",$1,$2,$3,sid; for(i=4;i<=NF;i++) printf " %s",$i; print ""}'\'' "$1" >"$2"' >"$B/fake-sid"
printf '%s\n' '#!/bin/bash' 'printf "%s\n" "$*" >>"$DW_KILL_LOG"' 'exit 0' >"$B/recycle-kill"; /bin/chmod 700 "$B/fake-ps" "$B/fake-sid" "$B/recycle-kill"
export FAKE_PID="$PID" FAKE_SID="$PID" FAKE_UID=$(/usr/bin/id -u) DW_KILL_LOG="$B/recycle-kill.log"
OUT=$(DW_PS="$B/fake-ps" DW_SID_FILTER="$B/fake-sid" DW_KILL="$B/recycle-kill" /bin/bash "$S" reap --run-id recycle 2>&1); RC=$?; SIGNALS=0; [[ -e "$DW_KILL_LOG" ]] && SIGNALS=$(wc -l <"$DW_KILL_LOG")
ok '[[ $RC -eq 70 && $SIGNALS -eq 0 && -f "$DW_STATE_DIR/recycle.record" && "$OUT" == *"reason=ambiguous"* ]]' 'anchor loss plus numeric reuse ambiguity never signals the recycled group'

# Phase and checkout scope are explicit positive refusals.
new; /bin/chmod 755 "$DW_STATE_DIR"; OUT=$(/bin/bash "$S" status --run-id mode 2>&1); RC=$?; /bin/chmod 700 "$DW_STATE_DIR"
ok '[[ $RC -ne 0 && "$OUT" == *"reason=state_mode"* ]]' 'permissive state directory refused'
new; OUT=$(/bin/bash "$S" run --run-id spec --phase spec --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/true 2>&1); RC=$?
ok '[[ $RC -ne 0 && "$OUT" == *"reason=unsafe_phase"* ]]' 'spec phase refused'
OUT=$(/bin/bash "$S" run --run-id main --phase implement --harness codex --worktree "$REPO" --cwd "$REPO" -- /usr/bin/true 2>&1); RC=$?
ok '[[ $RC -ne 0 && "$OUT" == *"reason=unsafe_worktree"* ]]' 'main checkout refused'

# Valid linked fixtures prove denied review/Crab paths and another repository fail for their intended boundary.
new; REVIEW="$ALLOWED/review-scratch"; CRAB="$ALLOWED/crab-runtime"; UNRELATED="$ALLOWED/unrelated"
/usr/bin/git -C "$REPO" worktree add -q --detach "$REVIEW" HEAD; /usr/bin/git -C "$REPO" worktree add -q --detach "$CRAB" HEAD
OTHER_REPO="$B/other"; /usr/bin/git init -q "$OTHER_REPO"; /usr/bin/git -C "$OTHER_REPO" -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm base; /usr/bin/git -C "$OTHER_REPO" worktree add -q --detach "$UNRELATED" HEAD
REFUSED=0
for BLOCKED in "$REVIEW" "$CRAB" "$UNRELATED"; do OUT=$(/bin/bash "$S" run --run-id "deny-${BLOCKED##*/}" --phase implement --harness codex --worktree "$BLOCKED" --cwd "$BLOCKED" -- /usr/bin/true 2>&1); RC=$?; [[ $RC -ne 0 && "$OUT" == *"reason=unsafe_worktree"* ]] && REFUSED=$((REFUSED+1)); done
ok '[[ $REFUSED -eq 3 ]]' 'review scratch, Crab-shaped worktree, and unrelated repository are positively refused'

printf 'PASS=%s FAIL=%s\n' "$PASS" "$FAIL"; [[ $FAIL -eq 0 ]]
