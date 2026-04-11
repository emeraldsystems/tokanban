# tokanban

Agent-first task management CLI for development teams using AI coding agents.

Tokanban eliminates the traditional project management UI. Day-to-day task management happens through your AI agent (Claude Code, Codex, Cursor) via MCP, or through this CLI.

## Install

```sh
cargo install tokanban
```

Or use the install script:

```sh
curl -fsSL https://app.tokanban.com/install.sh | sh
```

The installer currently uses Cargo, so a working Rust toolchain is still required.

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
- Member and team management
- Shell completions (bash, zsh, fish)
- JSON output when piped (`tokanban task list | jq`)
- Jira and CSV import

## Documentation

- Website: https://tokanban.com
- API: https://api.tokanban.com
- Spec: See `spec/` in the repository

## License

MIT
