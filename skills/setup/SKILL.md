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

## Step 3: MCP Server Configuration

The Tokanban MCP server is a remote HTTP endpoint. Configuration depends on the user's agent.

### Claude Code

Add to `~/.claude.json` or run `claude mcp add`:

```json
{
  "mcpServers": {
    "tokanban": {
      "type": "url",
      "url": "https://api.tokanban.com/mcp",
      "headers": {
        "Authorization": "Bearer <your-api-key>"
      }
    }
  }
}
```

### Codex CLI

Add to `~/.codex/config.json`:

```json
{
  "mcpServers": {
    "tokanban": {
      "type": "url",
      "url": "https://api.tokanban.com/mcp",
      "headers": {
        "Authorization": "Bearer <your-api-key>"
      }
    }
  }
}
```

Restart Codex CLI to pick up the change.

### Cursor

In Cursor, go to Settings > MCP Servers and add a remote server:

```
Name:    tokanban
Type:    URL
URL:     https://api.tokanban.com/mcp
Headers: Authorization: Bearer <your-api-key>
```

### Manual / Other MCP clients

```
URL:     https://api.tokanban.com/mcp
Auth:    Authorization: Bearer <your-api-key>
Method:  POST (JSON-RPC 2.0)
```

The server exposes 29 tools for task management, project admin, sprints, and visualizations. Discover them via the `tools/list` method.

### Getting an API key

If the user does not have an API key:

1. Sign up at https://app.tokanban.com/signup
2. After signing in, navigate to Settings > API Keys
3. Or use the CLI: `tokanban auth login` stores the key automatically

For agent-specific keys with scoped permissions:

```bash
tokanban agent create "My Claude" --type claude-code --scopes "tasks:read,tasks:write,projects:read"
```

## Step 4: Verify Setup

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

## Step 5: Initial Configuration

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
