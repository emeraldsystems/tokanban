---
name: tokanban-setup
description: Install or configure Tokanban for Codex, including CLI authentication, MCP setup, project-local skills, and optional memory behavior.
---

# Tokanban Setup for Codex

Use the local CLI to inspect and apply setup when available. Determine the target repository explicitly; do not assume the current directory is the user's intended target.

## Install and authenticate

If `tokanban` is absent, install with the user's preferred supported method. Cargo is the direct option:

```sh
cargo install tokanban
tokanban auth login
tokanban auth status
```

Authentication opens a browser and stores the credential in the Tokanban config. Never print, copy into project files, or commit the key.

## Initialize Codex

Preview first, then apply when the user has authorized setup:

```sh
tokanban init --harness codex --target-dir <REPO>
tokanban init --harness codex --target-dir <REPO> --yes
```

Codex initialization is idempotent. It configures the Tokanban MCP entry, installs the Codex behavioral block in the target repository, and installs the bundled skills under `<REPO>/.agents/skills`. It preserves existing content. An identical skill is a no-op; a conflicting existing skill is refused individually instead of overwritten.

To install only the skill bundles, use:

```sh
tokanban init --harness codex --skills-only --target-dir <REPO>
tokanban init --harness codex --skills-only --target-dir <REPO> --yes
```

`--skills-only` requires the explicit `--harness codex` selection and skips MCP configuration and AGENTS.md changes. Do not describe this flow as installing a Claude plugin, hooks, or token telemetry; Codex setup makes no such assumption.

Restart Codex after changing MCP configuration or skill discovery. Verify with read-only checks such as `tokanban auth status`, `tokanban doctor`, and an explicitly project-scoped list request.

## Manual MCP fallback

Prefer `tokanban init`. For a manual Codex setup, add this to `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`):

```toml
[mcp_servers.tokanban]
url = "https://api.tokanban.com/mcp"
bearer_token_env_var = "TOKANBAN_API_KEY"
http_headers = { "X-Tokanban-Tool-Scope" = "core,memory" }
```

Set `TOKANBAN_API_KEY` in the runtime environment. `bearer_token_env_var` names the variable; do not place `${TOKANBAN_API_KEY}` in a header string. Use `core,memory,admin` only when the user needs administrative tools. Existing agent keys need `memory:read,memory:write` for memory calls.

Do not create test tasks during verification unless the user asked for that board mutation. If setup or a verification read fails, report the failing layer—CLI auth, Codex MCP configuration, scopes, or project selection—rather than treating it as an empty account.
