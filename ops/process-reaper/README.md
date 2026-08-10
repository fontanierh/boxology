# Boxology review-orphan process reaper

Repo-owned, fail-closed helper that reaps orphaned same-UID PPID=1 processes
whose canonical cwd proves ownership by a Boxology review scratch worktree under
`/Users/jim/.codex/reviews`, with a gitfile pointing into
`/Users/jim/module-based-engineering/.git/worktrees`.

The checked-in files are inert until an operator installs them, and the checked-in plist defaults
to dry-run. On the project Mac, the source-identical script is currently installed under
`/Users/jim/.codex/process-reaper`, its launch agent runs every five minutes, and the installed
plist deliberately sets `REAPER_DRY_RUN=0` (verified 2026-08-10).

## Safety gates

Every candidate must pass: same UID, PPID exactly 1, pid not 0/1/self; cwd
canonical and strictly beneath the reviews root; not under deny roots
(`.codex/worktrees`, main checkout, `.crab`, `crab-source`, `crab-bin`,
`/tmp/boxology-ci*`); `.git` regular file whose metadata dir is strictly
beneath the worktrees root, with `gitdir` backlink to that `.git` and
`commondir` resolving to the canonical parent of the worktrees root; age ≥ 3600s
(lstart via `/bin/date -j`); identical
pid+lstart+cwd across scans with `first_seen` ≥ 600s; fresh revalidation
immediately before every signal. TERM first; KILL only after ≥ 60s grace on a
later scan. Individual PID only — no pkill/killall/process-group logic.

## Install (operator)

```sh
install -d -m 700 /Users/jim/.codex/process-reaper/state
install -m 700 ops/process-reaper/reaper.sh /Users/jim/.codex/process-reaper/reaper.sh
install -m 600 ops/process-reaper/com.fontanierh.boxology-review-reaper.plist \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-review-reaper.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.fontanierh.boxology-review-reaper.plist"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-review-reaper.plist"
```

## Dry-run observation

Plist ships `REAPER_DRY_RUN=1`. Logs `action=would-term` / `action=would-kill`
with sanitized `epoch pid age worktree action reason` fields only. Inspect:

```sh
tail -f /Users/jim/.codex/process-reaper/reaper.log
launchctl print "gui/$(id -u)/com.fontanierh.boxology-review-reaper"
```

## Enforcement flip

For an installation still using the checked-in dry-run default, edit the installed plist
`REAPER_DRY_RUN` to `0`, then:

```sh
launchctl bootout "gui/$(id -u)/com.fontanierh.boxology-review-reaper"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.fontanierh.boxology-review-reaper.plist"
```

## Rollback

```sh
launchctl bootout "gui/$(id -u)/com.fontanierh.boxology-review-reaper"
rm -f "$HOME/Library/LaunchAgents/com.fontanierh.boxology-review-reaper.plist"
```

## Tests

```sh
bash ops/process-reaper/tests/run-tests.sh
```

Fixture stubs inject `ps`/`lsof`/`kill`; the suite never touches live process tools.
