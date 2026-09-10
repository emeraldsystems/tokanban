---
name: tokanban-setup
description: "Use when the user wants to set up Tokanban, install the Tokanban CLI, configure the Tokanban MCP server, sign up for Tokanban, connect an AI agent to Tokanban, or get started with Tokanban. Trigger phrases: 'install tokanban', 'set up tokanban', 'configure tokanban mcp', 'tokanban signup', 'get started with tokanban', 'connect tokanban'."
---

# Tokanban Setup

Guide the user through installing and configuring Tokanban for their environment.

## Step 1: Determine What the User Needs

Ask which parts of setup the user needs:

1. **CLI installation** -- the `tokanban` command-line tool (Rust binary)
2. **MCP server configuration** -- connecting an AI agent (Claude Code, Codex CLI, Cursor, etc.) to Tokanban
3. **Both**

If the user's intent is clear from context, skip the question and proceed.

## Step 2: CLI Installation

Offer the installation method that fits the user's environment.

### Cargo (recommended)

```bash
cargo install tokanban
tokanban auth login
```

### Homebrew (coming soon)

```bash
brew install tokanban/tap/tokanban
tokanban auth login
```

Note: the Homebrew tap is not yet published. Recommend `cargo install` for now.

### curl

```bash
curl -fsSL https://app.tokanban.com/install.sh | sh
tokanban auth login
```

The install script uses `cargo install` under the hood until pre-built binaries are published.

### Post-install

`tokanban auth login` opens the browser for OAuth authentication. The credential is stored locally at `~/.config/tokanban/config.toml`.

Verify installation:

```bash
tokanban auth status
```

### Usage reporting

If the user installs the Tokanban Claude Code plugin, token usage reporting is enabled automatically through plugin hooks. The SessionStart hook calls `tokanban session start-hook` to register the actual Claude session identity and supply its canonical Tokanban ID as context. Repeated starts reuse that ID. Stop and SessionEnd hooks call `tokanban session report-usage` to send measured cumulative tokens through MCP; absent telemetry sends a heartbeat without inventing zero usage. The reporter is best-effort and silent: failures do not affect the agent session.

For CLI-only installs, run `tokanban init` instead of hand-editing settings. It is idempotent and safe to re-run:

```bash
tokanban init --dry-run          # preview planned changes (also the default with no flags)
tokanban init --yes              # apply: MCP config + memory block + usage hooks
tokanban init --harness codex --yes    # --harness: claude-code, codex, or cursor
tokanban init --target-dir ./my-repo --yes
```

Without `--yes`, `init` only previews what it would do — nothing is written until you confirm with `--yes`. Each step is skip-if-present: existing MCP entries, memory blocks, and hooks are left untouched, and `init` refuses to touch a config file it can't safely parse rather than risk corrupting it. If the Claude Code marketplace plugin is already enabled, `init` detects that and does not register a duplicate usage hook.

## Step 3: MCP Server Configuration

The Tokanban MCP server is a remote HTTP endpoint. Configuration depends on the user's agent.

For Claude Code, Codex, or Cursor, `tokanban init --harness <claude-code|codex|cursor>` automates this step (see the "Usage reporting" note above). The manual snippets below are for advanced/manual setups or other MCP clients.

### Claude Code

Add to `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json` if that env var is set) or run `claude mcp add`. Claude Code's Streamable HTTP transport type is `"http"` (not `"url"`) — see [code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp):

```json
{
  "mcpServers": {
    "tokanban": {
      "type": "http",
      "url": "https://api.tokanban.com/mcp",
      "headers": {
        "Authorization": "Bearer ${TOKANBAN_API_KEY}",
        "X-Tokanban-Tool-Scope": "core,memory"
      }
    }
  }
}
```

`${TOKANBAN_API_KEY}` is resolved from your shell environment by Claude Code itself; the key is never written into this file by `tokanban init`.

### Codex CLI

Codex uses **TOML**, not JSON: `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`) — see [developers.openai.com/codex/mcp](https://developers.openai.com/codex/mcp). Add:

```toml
[mcp_servers.tokanban]
url = "https://api.tokanban.com/mcp"
bearer_token_env_var = "TOKANBAN_API_KEY"
http_headers = { "X-Tokanban-Tool-Scope" = "core,memory" }
```

`bearer_token_env_var` tells Codex which environment variable to read the token from at runtime — Codex does not support `${VAR}` interpolation inside header/url strings the way Claude Code does. `tokanban init --harness codex --yes` appends this table as text to the end of your existing `config.toml`, preserving all existing content, comments, and formatting byte-for-byte (it never round-trips/reserializes the file). Restart Codex CLI to pick up the change.

### Cursor

`tokanban init --harness cursor --yes` writes `~/.cursor/mcp.json` (or `<project>/.cursor/mcp.json` if that already exists) directly, using Cursor's own `${env:VAR}` environment-interpolation syntax (distinct from Claude Code's `${VAR}`) and no `"type"` field:

```json
{
  "mcpServers": {
    "tokanban": {
      "url": "https://api.tokanban.com/mcp",
      "headers": {
        "Authorization": "Bearer ${env:TOKANBAN_API_KEY}",
        "X-Tokanban-Tool-Scope": "core,memory"
      }
    }
  }
}
```

To do it via the UI instead, go to Settings > MCP Servers and add a remote server:

```
Name:    tokanban
URL:     https://api.tokanban.com/mcp
Headers: Authorization: Bearer <your-api-key>
```

### Manual / Other MCP clients

```
URL:     https://api.tokanban.com/mcp
Auth:    Authorization: Bearer <your-api-key>
Method:  POST (JSON-RPC 2.0)
```

The server exposes task management, agent memory, project admin, sprint, and visualization tools. Discover them via the `tools/list` method.

### Trimming the tool surface (lower per-session token usage)

Every advertised tool's schema is re-sent in the model's cached prompt prefix on
every turn, so a large tool surface inflates a session's token usage. You can ask
the server to advertise only the tool groups you need with the
`X-Tokanban-Tool-Scope` header (or the `?tools=` query param), comma-separated:

| Group | Tools |
|-------|-------|
| `core` | tasks, project entities, sprints, tables/burndown, `usage_report`, `list_projects`, `list_members` |
| `memory` | `session_*` and `memory_*` (cross-session memory) |
| `admin` | project/member/workflow/rule administration and agent-key management |

The Claude Code plugin ships with `"X-Tokanban-Tool-Scope": "core,memory"`, which
covers day-to-day task + memory work while dropping the rarely-needed admin/key
tools. To get everything back, set the header to `core,memory,admin` (or omit it
entirely — no scope means all groups). Unknown group names are ignored, and
`tools/call` is never scoped, so a scoped session can still invoke any tool it
holds a valid API key for.

### Getting an API key

If the user does not have an API key:

1. Sign up at https://app.tokanban.com/signup
2. After signing in, navigate to Settings > API Keys
3. Or use the CLI: `tokanban auth login` stores the key automatically

For agent-specific keys with scoped permissions:

```bash
tokanban agent create "My Claude" --type claude-code --scopes "tasks:read,tasks:write,projects:read"
```

## Step 4: Enable Agent Memory

If the user wants cross-session context, enable Tokanban memory at the same time as MCP setup.

### Required scopes

Agent keys need memory scopes in addition to task scopes:

```bash
tokanban agent create "My Claude" --type claude-code --scopes "tasks:read,tasks:write,projects:read,memory:read,memory:write"
```

If the user already has an older agent key, guide them to rotate or recreate it so the key includes `memory:read` and `memory:write`.

### Behavioral block

`tokanban init` installs this automatically (skip-if-already-present). To do it by hand, add the appropriate memory block template to the harness config:

- Claude Code: `cli/templates/CLAUDE.md.memory-block.md`
- Codex CLI: `cli/templates/AGENTS.md.memory-block.md`
- Cursor: `cli/templates/cursorrules.memory-block.md`

These blocks teach the harness to call:

1. `session_start`
2. `memory_relevant_now`
3. triage candidate memories during work instead of writing every finding immediately
4. `session_end` with a continuation prompt at the end

### Verification

Ask the user to start a short session and verify the harness can:

1. reuse the startup hook's canonical session ID, or call `session_start` once with the actual harness session identity when available
2. call `memory_relevant_now`
3. defer at least one candidate and write at least one explicit "remember this" item immediately
4. close the session with `session_end`

## Step 5: Verify Setup

### CLI verification

```bash
tokanban project list
tokanban task create "Test task" --priority Medium
tokanban task list
```

### MCP verification

Ask the user to prompt their agent with:

> "List all my Tokanban tasks"

or

> "Create a task in Tokanban: Set up CI/CD pipeline, priority High"

The agent uses the MCP tools automatically.

## Step 6: Initial Configuration

Help the user set defaults so they can omit `--project` and `--workspace` flags:

```bash
tokanban workspace list
tokanban workspace set <workspace-slug>
tokanban project list
tokanban project set <project-key>
```

Suggest useful next steps:

- Invite team members: `tokanban member invite teammate@company.com --role member`
- Create a sprint: `tokanban sprint create --name "Sprint 1" --start 2026-04-14 --end 2026-04-28`
- Generate shell completions: `tokanban completion zsh > ~/.zsh/completions/_tokanban`
