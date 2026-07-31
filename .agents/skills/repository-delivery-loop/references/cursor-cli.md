# Cursor CLI / Grok 4.5 operator runbook

This path uses the user's Cursor account and the CLI model alias `cursor-grok-4.5-high`.
It is the `implement` and `repair` worker.

## Install, verify, and update

```sh
curl https://cursor.com/install -fsS | bash
/Users/jim/.local/bin/cursor-agent --version
/Users/jim/.local/bin/cursor-agent update
```

The installer creates two symlinks in `/Users/jim/.local/bin`, `cursor-agent` and `agent`, both
pointing at the versioned binary. Always invoke the absolute `cursor-agent` path; `agent` is a
generic name that can collide with other tools on `PATH`.

Verified installed version: `2026.07.23-e383d2b`.

## Authenticate

Authentication is an API key, not an interactive login:

```sh
export CURSOR_API_KEY=...        # or pass --api-key <key>
/Users/jim/.local/bin/cursor-agent status
```

`status` prints the authenticated account. The key itself lives in
`/Users/jim/.config/boxology-delivery-loop/credentials.env` (mode `600`, outside the repository).
Load it into the process environment; never place it in argv, a prompt, a committed file, or a log:

```sh
set -a; . /Users/jim/.config/boxology-delivery-loop/credentials.env; set +a
```

The CLI caches a session after a successful call and will then report itself logged in even without
the variable. Do not rely on that cache. Always export `CURSOR_API_KEY` for a delivery worker so the
run does not depend on residual local state.

## Model and effort

```sh
/Users/jim/.local/bin/cursor-agent --list-models
```

Cursor encodes reasoning effort in the model identifier rather than in a separate flag. The Grok 4.5
family is:

| Alias | Meaning |
| --- | --- |
| `cursor-grok-4.5-low` | Grok 4.5, low effort |
| `cursor-grok-4.5-medium` | Grok 4.5, medium effort |
| `cursor-grok-4.5-high` | Grok 4.5, high effort — the configured `implement` candidate |
| `cursor-grok-4.5-*-fast` | Same effort, priority capacity |

Because effort is part of the identifier, the configured `effort` in `models.toml` is honored by
selecting the matching suffix. `effort = "high"` requires `--model cursor-grok-4.5-high`. Never
satisfy a configured effort with a different suffix.

Cursor also serves models from other vendors, including `kimi-k3-high` and `claude-opus-5-*`. Do not
use them. Repository policy binds Kimi K3 to the Kimi Code CLI and Opus 5 to the Claude harness;
routing either through Cursor would misrepresent the worker and its account.

## Non-interactive delivery worker

```sh
CURSOR_API_KEY=... /Users/jim/.local/bin/cursor-agent \
  -p \
  --model cursor-grok-4.5-high \
  --force \
  --trust \
  --approve-mcps \
  --sandbox disabled \
  --output-format text \
  --workspace /exact/task/worktree \
  "Complete directive"
```

- `-p` is print/headless mode. It is already unattended and has access to all tools, including file
  write and shell; a write succeeds under `-p` even without `--force`.
- `--force` (alias `--yolo`) allows commands unless explicitly denied. Without it, shell steps are
  filtered through the allow/deny list in `~/.cursor/cli-config.json`, which is narrow by default.
- `--trust` accepts the workspace without prompting, and `--approve-mcps` accepts MCP servers.
- `--sandbox disabled` is required for the Cargo-heavy gates this repository runs.
- `--workspace` sets the working directory. Pass the exact assigned worktree.
- `--output-format` accepts `text`, `json`, or `stream-json`, and only applies with `-p`. `json`
  emits a single terminal object carrying `result`, `is_error`, `session_id`, and `usage`; use it
  when a run must be parsed rather than read.

Capture stdout and stderr. Record `session_id` in the task's delivery evidence.

## Web browsing

Web search and fetch are served as dynamic tools and need no flag or MCP installation; the worker
resolves them through `GetDynamicTools` and `CallDynamicTool`. A smoke run confirmed it will search
the web unprompted by any enabling switch. Availability still depends on Cursor's hosted services.

## Flags to forbid in delivery directives

- `-w` / `--worktree` and `--worktree-base`: the loop assigns worktrees itself. Letting the CLI
  create one under `~/.cursor/worktrees` would move the worker off its assigned tree and out of the
  operator's audit path.
- `--add-dir`: keeps the worker confined to the assigned worktree.
- `--resume` / `--continue`: every phase must start a fresh session. Reuse would leak one phase's
  context into another and break reviewer independence.
- `--mode plan` / `--plan`: read-only modes; the implementation worker must be able to edit.

## Safety

`-p --force --sandbox disabled` grants broad unattended file and shell authority. A worktree is an
operational boundary, not an OS sandbox. Use an isolated worktree, put no secrets in prompts, and
direct workers not to commit, push, open PRs, perform any other external write, spawn background
work, or leave the assigned worktree.

## Official references

- [CLI overview](https://cursor.com/docs/cli/overview)
- [Headless usage](https://cursor.com/docs/cli/headless)
- [Parameters reference](https://cursor.com/docs/cli/reference/parameters)
- [Models](https://cursor.com/docs/cli/reference/models)
