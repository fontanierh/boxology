# Kimi Code / Kimi K3 operator runbook

This path uses the Kimi Code CLI and the model alias `kimi-code/k3` (service model ID `k3`).
It is the `spec` worker.

Kimi K3 is reachable through other harnesses — Cursor, for example, serves `kimi-k3-high`. Do not
use those. Repository policy binds K3 to this CLI so the worker, its account, and its evidence stay
attributable.

## Install, verify, and update

```sh
/Users/jim/.kimi-code/bin/kimi --version
/Users/jim/.kimi-code/bin/kimi --help
/Users/jim/.kimi-code/bin/kimi doctor
```

Verified installed version: `0.31.0`.

`kimi upgrade` misdetects this machine as a native Windows install and refuses to self-update.
Update with the official installer instead; it backs the previous binary up to `kimi.bak`:

```sh
curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash
```

## Authenticate

The delivery loop authenticates with a Kimi Code coding-plan API key, not the interactive OAuth
session, so a worker never depends on a live login. The key is written to
`/Users/jim/.kimi-code/config.toml` (mode `600`, outside the repository):

```toml
[providers."apikey:kimi-code"]
type = "kimi"
api_key = "sk-kimi-..."
base_url = "https://api.kimi.com/coding/v1"
```

`[models."kimi-code/k3"]` points at that provider, so `-m kimi-code/k3` uses the key. The managed
OAuth provider is retained and still reachable as `kimi-code/k3-oauth`; it is not the configured
candidate. Confirm both resolve with:

```sh
/Users/jim/.kimi-code/bin/kimi provider list
```

Credential env vars are not read from the shell for provider auth — the CLI takes provider keys from
`config.toml` only. The exception is the web services below.

Do not add third-party gateways, provider rewrites, or committed Kimi configuration for this
workflow.

## Model and effort

```sh
KIMI_MODEL_THINKING_EFFORT=high /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3
```

`kimi-code/k3` supports `low`, `high`, and `max`, and defaults to `high`. The CLI has no effort
flag; set `KIMI_MODEL_THINKING_EFFORT` in the process environment. The configured `spec` effort is
`high` — set it explicitly rather than relying on the alias default.

## Non-interactive delivery worker

```sh
KIMI_CODE_NO_AUTO_UPDATE=1 KIMI_DISABLE_CRON=1 KIMI_MODEL_THINKING_EFFORT=high \
  /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3 -p "Complete directive"
```

`-p` is fully unattended on its own: a smoke run created a file and ran a shell command with no
approval prompt. Do **not** add `--auto` or `--yolo`; `0.31.0` still rejects the combination with
`error: Cannot combine --prompt with --auto.` Launch it in the isolated task worktree as the working
directory, pass a complete directive, and capture stdout and stderr.

`--output-format` accepts `text` (default) or `stream-json`. The run prints a resumable session ID;
record it in the task's delivery evidence.

## Full-permissions interactive mode

```sh
KIMI_MODEL_THINKING_EFFORT=high /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3 --auto
```

`--auto` is fully unattended, including sensitive approvals. `--yolo` skips ordinary tool approvals
but may still ask questions. Neither belongs in a `-p` worker launch.

## Web browsing

`WebSearch` and `FetchURL` are built-in agent tools; they need no MCP installation or enablement
flag. A smoke run confirmed both work under `-p`. They are served by the `moonshot_search` and
`moonshot_fetch` entries in `config.toml`, which currently authenticate with the OAuth session.

If that session lapses, force them onto the API key by exporting the two variables held in
`/Users/jim/.config/boxology-delivery-loop/credentials.env`; an env value replaces both the
configured and the OAuth credential:

```sh
set -a; . /Users/jim/.config/boxology-delivery-loop/credentials.env; set +a
```

`kimi web` starts Kimi's local browser UI and is unrelated to the agent's `WebSearch` and `FetchURL`
tools.

## Safety

`--auto` and `-p` grant broad unattended file and shell authority. A worktree is an operational
boundary, not an OS sandbox. Use an isolated worktree, put no secrets in prompts, and disable cron
and auto-update for delivery workers. Direct workers not to spawn subagents, create schedules, leave
background work, add directories, or leave the assigned worktree.

## Official references

- [`kimi` command](https://moonshotai.github.io/kimi-code/en/reference/kimi-command.html)
- [Providers and models](https://moonshotai.github.io/kimi-code/en/configuration/providers.html)
- [Environment variables](https://moonshotai.github.io/kimi-code/en/configuration/env-vars.html)
- [Built-in tools](https://moonshotai.github.io/kimi-code/en/reference/tools.html)
