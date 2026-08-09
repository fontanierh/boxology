#!/bin/bash
# Fixture processes only. Every signal target is created by this test.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; S="$ROOT/supervise.sh"; WT="$(cd "$ROOT/../.." && pwd)"; PASS=0; FAIL=0; TMP=; FIXTURE_PIDS=; FIXTURE_GROUPS=
ok() { if eval "$1"; then PASS=$((PASS+1)); else printf 'FAIL: %s\n' "$2"; FAIL=$((FAIL+1)); fi; }
birth() { local r; r=$(/bin/ps -p "$1" -o pid=,lstart=,comm= 2>/dev/null) || return; [[ -n "$r" ]] || return; printf '%s\n' "$r" | /usr/bin/shasum -a 256 | /usr/bin/awk '{print $1}'; }
track_pid() { local p=$1 f i; for i in $(jot 20); do f=$(birth "$p"); [[ -n "$f" ]] && break; /bin/sleep .01; done; [[ -n "$f" ]] && FIXTURE_PIDS="$FIXTURE_PIDS $p:$f"; }
track_group() { local g=$1 f i members= p; f=$(birth "$g"); [[ -n "$f" ]] && FIXTURE_GROUPS="$FIXTURE_GROUPS $g:$f"; for i in $(jot 50); do members=$(/bin/ps -axo pid=,pgid= | /usr/bin/awk -v x="$g" '$2==x {print $1}'); set -- $members; [[ $# -ge 2 ]] && break; /bin/sleep .01; done; for p in $members; do track_pid "$p"; done; }
cleanup() { local d e p f; trap - EXIT; for e in $FIXTURE_PIDS; do p=${e%%:*}; f=${e#*:}; [[ "$p" =~ ^[1-9][0-9]*$ && "$p" -gt 1 && "$(birth "$p")" == "$f" ]] && /bin/kill -KILL "$p" 2>/dev/null; done; for e in $FIXTURE_GROUPS; do p=${e%%:*}; f=${e#*:}; [[ "$p" =~ ^[1-9][0-9]*$ && "$p" -gt 1 && "$(birth "$p")" == "$f" ]] && /bin/kill -KILL "-$p" 2>/dev/null; done; /bin/sleep .1; for d in $TMP; do case "$d" in /tmp/dw-test.*|/private/tmp/dw-test.*) /bin/rm -rf "$d";; esac; done; }
trap cleanup EXIT
/bin/bash -n "$S" || exit 1

env_for() { export DELIVERY_WORKER_TEST_MODE=1 DW_STATE_DIR="$B/state" DW_GRACE_SECONDS=0; }
new() { B=$(/bin/realpath "$(mktemp -d /tmp/dw-test.XXXXXX)"); TMP="$TMP $B"; /bin/mkdir -m 700 "$B/state"; env_for; }
wait_record() { local i; for i in $(jot 100); do [[ -f "$DW_STATE_DIR/$1.record" ]] && return; /bin/sleep .02; done; return 1; }
run_real() { /bin/bash "$S" run --run-id "$1" --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- "$2" "${3:-}"; }

# Gated launch is a dedicated group; a clean command clears without signaling.
new; OUT=$(run_real clean /usr/bin/true); RC=$?
ok '[[ $RC -eq 0 && ! -e "$DW_STATE_DIR/clean.record" && "$OUT" == *"action=clear reason=normal"* ]]' 'clean launch and zero-signal cleanup'

# A durable-write failure aborts the still-gated guardian and removes partial state.
new; DELIVERY_WORKER_TEST_MODE=1 DW_STATE_DIR="$DW_STATE_DIR" DW_GRACE_SECONDS=0 DW_FAIL_RECORD_WRITE=1 /bin/bash "$S" run --run-id writefail --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 300 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; GUARD=; for i in $(jot 100); do GUARD=$(/bin/ps -axo pid=,ppid=,pgid= | /usr/bin/awk -v s="$SUP" '$2==s && $1==$3 {print $1; exit}'); [[ -n "$GUARD" ]] && break; /bin/sleep .02; done
[[ -n "$GUARD" ]] || exit 1; track_pid "$GUARD"; wait "$SUP"; RC=$?; /bin/kill -0 "$GUARD" 2>/dev/null; LIVE=$?; LEFT=0
for f in "$DW_STATE_DIR/writefail.identity" "$DW_STATE_DIR"/writefail.gate.* "$DW_STATE_DIR"/writefail.record* "$DW_STATE_DIR"/.writefail.launch.*; do [[ -e "$f" ]] && LEFT=$((LEFT+1)); done
ok '[[ $RC -ne 0 && $LIVE -ne 0 && $LEFT -eq 0 ]]' 'record-write failure aborts gated group and partial state'

# Interrupt only the fixture supervisor, then TERM the exact owned fixture group.
new; /bin/bash "$S" run --run-id term --phase repair --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record term || exit 1; read PID PGID < <(/usr/bin/awk -F= '/^pid=/{p=$2}/^pgid=/{g=$2}END{print p,g}' "$DW_STATE_DIR/term.record"); track_group "$PID"
PST=$(/bin/ps -p "$PID" -o pid=,pgid=); /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(/bin/bash "$S" reap --run-id term); RC=$?
ok '[[ $PST == *"$PID"*"$PGID"* && $PID == "$PGID" ]]' 'guardian PID equals PGID'
ok '[[ $RC -eq 0 && "$OUT" == *"action=clear reason=term"* && ! -e "$DW_STATE_DIR/term.record" ]]' 'TERM empties fixture group'

# TERM-resistant fixture reaches KILL; the unrelated same-name sleep survives.
new; /bin/sleep 300 & OTHER=$!
track_pid "$OTHER"
/bin/bash "$S" run --run-id kill --phase validation --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'trap "" TERM; exec /bin/sleep 300' >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record kill || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/kill.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
OUT=$(/bin/bash "$S" reap --run-id kill); RC=$?; /bin/kill -0 "$OTHER" 2>/dev/null; LIVE=$?; /bin/kill -TERM "$OTHER"; wait "$OTHER" 2>/dev/null
ok '[[ $RC -eq 0 && "$OUT" == *"action=clear reason=kill"* ]]' 'verified survivor receives KILL'
ok '[[ $LIVE -eq 0 ]]' 'unrelated same-name process untouched'

# Dry-run performs proof but neither advances state nor signals the group.
new; /bin/bash "$S" run --run-id dry --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record dry || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/dry.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
BEFORE=$(/sbin/md5 -q "$DW_STATE_DIR/dry.record"); OUT=$(/bin/bash "$S" reap --run-id dry --dry-run); RC=$?; AFTER=$(/sbin/md5 -q "$DW_STATE_DIR/dry.record")
PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/dry.record"); /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -eq 0 && $BEFORE == "$AFTER" && $LIVE -eq 0 && "$OUT" == *"action=would-term reason=verified"* ]]' 'dry-run has zero signal and state advance'
/bin/bash "$S" reap --run-id dry >/dev/null

# Empty-group dry-run reports the pending clear without changing the record.
new; /bin/bash "$S" run --run-id empty --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/false >"$B/run.out" 2>&1
BEFORE=$(/sbin/md5 -q "$DW_STATE_DIR/empty.record"); OUT=$(/bin/bash "$S" reap --run-id empty --dry-run); RC=$?; AFTER=$(/sbin/md5 -q "$DW_STATE_DIR/empty.record")
ok '[[ $RC -eq 0 && $BEFORE == "$AFTER" && "$OUT" == *"action=would-clear reason=group_empty"* ]]' 'empty-group dry-run would clear without state change'; /bin/bash "$S" reap --run-id empty >/dev/null

# Records are private and permissive/corrupt ownership never signals.
new; /bin/bash "$S" run --run-id bad --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/sleep 30 >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record bad || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/bad.record"); track_group "$PID"; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
/bin/chmod 644 "$DW_STATE_DIR/bad.record"; OUT=$(/bin/bash "$S" reap --run-id bad 2>&1); RC=$?; /bin/kill -0 "$PID" 2>/dev/null; LIVE=$?
ok '[[ $RC -ne 0 && $LIVE -eq 0 && "$OUT" == *"reason=unsafe_record"* ]]' 'permissive record fails closed'
/bin/chmod 600 "$DW_STATE_DIR/bad.record"; /bin/bash "$S" reap --run-id bad >/dev/null

# A missing guardian never proves its still-live process group empty.
new; /bin/bash "$S" run --run-id orphan --phase implement --harness codex --worktree "$WT" --cwd "$WT" -- /bin/bash -c 'trap "" TERM; exec /bin/sleep 300' >"$B/run.out" 2>&1 & SUP=$!
track_pid "$SUP"; wait_record orphan || exit 1; PID=$(/usr/bin/awk -F= '/^pid=/{print $2}' "$DW_STATE_DIR/orphan.record"); track_group "$PID"; /bin/sleep .05; /bin/kill -TERM "$SUP"; wait "$SUP" 2>/dev/null
CHILD=$(/bin/ps -axo pid=,pgid= | /usr/bin/awk -v g="$PID" '$2==g && $1!=g {print $1; exit}')
[[ -n "$CHILD" ]] || exit 1; /bin/kill -TERM "$PID"; /bin/sleep .1; OUT=$(/bin/bash "$S" reap --run-id orphan 2>&1); RC=$?; /bin/kill -0 "$CHILD" 2>/dev/null; LIVE=$?
track_pid "$CHILD"
ok '[[ $RC -ne 0 && $LIVE -eq 0 && -f "$DW_STATE_DIR/orphan.record" && "$OUT" == *"reason=leader_missing"* ]]' 'guardian absence with survivor fails closed'
/bin/kill -KILL "-$PID"; /bin/sleep .1

# Phase and checkout scope are explicit positive refusals.
new; /bin/chmod 755 "$DW_STATE_DIR"; OUT=$(/bin/bash "$S" status --run-id mode 2>&1); RC=$?; /bin/chmod 700 "$DW_STATE_DIR"
ok '[[ $RC -ne 0 && "$OUT" == *"reason=state_mode"* ]]' 'permissive state directory refused'
new; OUT=$(/bin/bash "$S" run --run-id spec --phase spec --harness codex --worktree "$WT" --cwd "$WT" -- /usr/bin/true 2>&1); RC=$?
ok '[[ $RC -ne 0 && "$OUT" == *"reason=unsafe_phase"* ]]' 'spec phase refused'
OUT=$(/bin/bash "$S" run --run-id main --phase implement --harness codex --worktree /Users/jim/module-based-engineering --cwd /Users/jim/module-based-engineering -- /usr/bin/true 2>&1); RC=$?
ok '[[ $RC -ne 0 && "$OUT" == *"reason=unsafe_worktree"* ]]' 'main checkout refused'

printf 'PASS=%s FAIL=%s\n' "$PASS" "$FAIL"; [[ $FAIL -eq 0 ]]
