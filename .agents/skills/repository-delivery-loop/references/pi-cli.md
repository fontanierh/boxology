# Pi CLI implementation worker

This path uses Pi's xAI provider with `xai/grok-4.5` at high thinking effort. It is the repository's default `implement` worker, and `repair` inherits it.

## Pinned installation

Install the official current package without lifecycle scripts:

```text
npm install -g --ignore-scripts @earendil-works/pi-coding-agent@0.83.0
/opt/homebrew/bin/pi --version
```

The expected version is `0.83.0`. If the binary is missing or another version is installed, restore the pin before launching a worker. Do not use the deprecated `@mariozechner/pi-coding-agent` package.

## Authentication and model

Load `XAI_API_KEY` from `/Users/jim/.config/boxology-delivery-loop/credentials.env`. Keep the key in the process environment and out of argv, prompts, logs, sessions, and repository files. Never use `--api-key`.

Verify the configured model without exposing the credential:

```text
set -a
source /Users/jim/.config/boxology-delivery-loop/credentials.env
set +a
/opt/homebrew/bin/pi --provider xai --list-models grok
```

The list must contain `xai/grok-4.5`. Pi accepts reasoning effort separately; `effort = "high"` maps exactly to `--thinking high`.

## Exact unattended launch

Run from the assigned worktree as the process working directory:

```text
set -a
source /Users/jim/.config/boxology-delivery-loop/credentials.env
set +a
PI_TELEMETRY=0 /opt/homebrew/bin/pi \
  -p \
  --no-session \
  --approve \
  --model xai/grok-4.5 \
  --thinking high \
  "<complete worker directive>"
```

- `-p` runs one non-interactive turn and exits.
- `--no-session` prevents the worker from inheriting or persisting conversation state.
- `--approve` permits the already-assigned project context for this run.
- The default built-in `read`, `bash`, `edit`, and `write` tools are required for implementation.

Do not add `--continue`, `--resume`, `--session`, `--fork`, `--api-key`, or interactive mode. Do not let the worker start nested agents, background work, or leave the assigned worktree. The directive must repeat the exact worktree path, repository instructions, task scope, validation limits, and ban on commits or external writes.

## Availability evidence

Treat only explicit authentication rejection, unknown model/effort, unavailable CLI, or provider usage exhaustion as configured fallback evidence. A timeout, transport error, rejected implementation, test failure, or review finding is not fallback evidence.

Official references:

- [Pi installation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/index.md)
- [Pi providers and `XAI_API_KEY`](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/providers.md)
- [Pi settings and thinking levels](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md)
