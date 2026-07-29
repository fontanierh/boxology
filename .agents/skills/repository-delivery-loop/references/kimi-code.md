# Kimi Code / Kimi K3 operator runbook

This path uses the user's managed Kimi Code subscription and the CLI model alias `kimi-code/k3` (service model ID `k3`).

## Verify and authenticate

```sh
/Users/jim/.kimi-code/bin/kimi --version
/Users/jim/.kimi-code/bin/kimi --help
```

Complete managed-subscription OAuth once:

```sh
/Users/jim/.kimi-code/bin/kimi login
```

Do not add API keys, provider rewrites, third-party gateways, endpoints, or committed Kimi configuration for this workflow.

## Model and effort

```sh
KIMI_MODEL_THINKING_EFFORT=max /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3
```

Use `low` for lighter deliberation, `high` for deeper reasoning, and `max` for maximum reasoning effort. Kimi Code CLI v0.30.0 has no effort CLI flag; set `KIMI_MODEL_THINKING_EFFORT` in the process environment.

## Full-permissions interactive mode

```sh
KIMI_MODEL_THINKING_EFFORT=max /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3 --auto
```

`--auto` is fully unattended, including sensitive approvals. `--yolo` skips ordinary tool approvals but may still ask questions.

## Non-interactive delivery worker

```sh
KIMI_CODE_NO_AUTO_UPDATE=1 KIMI_DISABLE_CRON=1 KIMI_MODEL_THINKING_EFFORT=max /Users/jim/.kimi-code/bin/kimi -m kimi-code/k3 -p "Complete directive"
```

`-p` is already unattended. Never combine it with `--auto` or `--yolo`. Launch it in the isolated task worktree, pass a complete directive, and capture stdout and stderr.

## Web browsing

`WebSearch` and `FetchURL` are built-in agent tools; they need no MCP installation or enablement flag. Availability still depends on the host-provided search and fetch services.

Example prompt:

> Use WebSearch to find the current official documentation for the topic, then use FetchURL on the most relevant official result and summarize it with the source URL.

`kimi web` starts Kimi's local browser UI and is unrelated to the agent's `WebSearch` and `FetchURL` tools.

## Safety

`--auto` and `-p` grant broad unattended file and shell authority. A worktree is an operational boundary, not an OS sandbox. Use an isolated worktree, put no secrets in prompts, and disable cron and auto-update for delivery workers. Direct workers not to spawn subagents, create schedules, leave background work, add directories, or leave the assigned worktree.

## Official references

- [`kimi` command](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-command.html)
- [Providers and models](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/providers.html)
- [Environment variables](https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/env-vars.html)
- [Built-in tools](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/tools.html)
- [Interaction and input](https://www.kimi.com/code/docs/en/kimi-code-cli/guides/interaction.html)
