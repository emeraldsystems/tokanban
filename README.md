# tokanban

Agent-first task management and cross-session memory CLI for development teams using AI coding agents.

Tokanban eliminates the traditional project management UI. Day-to-day task management happens through your AI agent (Claude Code, Codex, Cursor) via MCP, or through this CLI.

## Install

Install with the script:

```sh
curl -fsSL https://app.tokanban.com/install.sh | sh
```

The install script downloads a matching pre-built binary when one is available and falls back to `cargo install` otherwise.

Or build from source with Cargo:

```sh
cargo install --locked tokanban
```

Pre-built binaries are available for Linux (x86_64, aarch64), macOS (x86_64, Apple Silicon), and Windows (x86_64) on the [GitHub Releases](https://github.com/emeraldsystems/tokanban/releases) page.

## Quick start

```sh
# Authenticate
tokanban auth login

# Select a default project once
tokanban project list
tokanban project set PLAT

# Create a task
tokanban task create "Fix auth token refresh" --priority High

# List tasks
tokanban task list

# View task detail
tokanban task view PLAT-1

# Search
tokanban task search "auth"
```

## Features

- Full CRUD for tasks, projects, sprints, comments, workflows
- Agent key management (create, rotate, revoke)
- Cross-session memory setup for Claude Code, Codex CLI, and Cursor
- Member and team management
- Shell completions (bash, zsh, fish)
- JSON output when piped (`tokanban task list | jq`)

## Agent Memory

To enable Tokanban memory for an agent, create a key with memory scopes:

```sh
tokanban agent create "My Claude" \
  --type claude-code \
  --scopes "tasks:read,tasks:write,projects:read,memory:read,memory:write"
```

Then add the matching memory block from `templates/` to your harness config:

- Claude Code: `templates/CLAUDE.md.memory-block.md`
- Codex CLI: `templates/AGENTS.md.memory-block.md`
- Cursor: `templates/cursorrules.memory-block.md`

## Documentation

- Website: https://tokanban.com
- API: https://api.tokanban.com
- Spec: See `spec/` in the repository

## Claude Code Plugin

This repository is also a [Claude Code plugin marketplace](https://code.claude.com/docs/en/plugin-marketplaces). Install the Tokanban plugin from inside Claude Code:

```text
/plugin marketplace add emeraldsystems/tokanban
/plugin install tokanban@tokanban
```

Then set your API key so the bundled MCP server can reach the API:

```sh
export TOKANBAN_API_KEY="$(tokanban agent create 'My Claude' --type claude-code --scopes 'tasks:read,tasks:write,projects:read,memory:read,memory:write' --print-key)"
```

The plugin lives at `plugins/tokanban/` and provides:

- **Setup skill** (`tokanban:setup`) -- guided CLI installation and MCP server configuration
- **Memory skill** (`tokanban:memory`) -- session start/end and fact/decision capture guidance
- **Tokanban skill** (`tokanban:tokanban`) -- full CLI reference for tasks, projects, sprints, workflows, and team management
- **Hooks and templates** -- memory reminders and harness-ready behavior blocks for Claude Code, Codex CLI, and Cursor
- **MCP integration** -- auto-configures the Tokanban MCP server (set `TOKANBAN_API_KEY` env var)

## License

BSD-3-Clause
