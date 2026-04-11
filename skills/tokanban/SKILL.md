---
name: tokanban
description: "Use when the user asks about Tokanban task management, wants to create/update/list tasks, manage projects, run sprints, invite team members, manage agent tokens, update workflows, import from Jira/CSV, or visualize a board. Trigger phrases: 'tokanban', 'create a task', 'list tasks', 'sprint board', 'kanban board', 'project backlog', 'assign task', 'close task', 'import from jira', 'tokanban agent', 'task priority', 'workflow status'."
---

# Tokanban CLI & MCP Reference

Tokanban is an agent-first task management system. Users interact via the `tokanban` CLI or through AI agents connected via the MCP server.

When helping users, prefer running CLI commands directly via Bash when the CLI is installed, or guide them to use MCP tools if working through an agent integration.

## Global Flags

All commands accept these flags:

| Flag | Description |
|------|-------------|
| `--workspace` | Override default workspace |
| `--project` | Override default project |
| `--format json\|table\|card` | Output format |
| `--quiet` | Suppress non-error output |
| `--verbose` | Detailed logging |
| `--no-color` | Strip ANSI codes |
| `--config <path>` | Custom config file |
| `--api-url <url>` | Override API endpoint |

## Tasks

The primary entity in Tokanban. Each task has a key (e.g., `PLAT-42`), title, status, priority, assignee, and optional description, labels, sprint, and due date.

### Create a task

```bash
tokanban task create "Title here" \
  --project PLAT \
  --priority high \
  --assignee username \
  --sprint <sprint-id> \
  --description "Details..."
```

Priority values: `urgent`, `high`, `medium`, `low`, `none` (case-insensitive).

### List tasks

```bash
tokanban task list \
  --project PLAT \
  --status in_progress \
  --assignee username \
  --priority high \
  --sprint <sprint-id> \
  --due 2026-04-15 \
  --limit 50
```

All filters are optional. Default limit is 25 (max 100). Use `--cursor` for pagination.

### View a task

```bash
tokanban task view PLAT-42
```

Shows full detail: title, description, status, priority, assignee, labels, sprint, comments, and timestamps.

### Update a task

```bash
tokanban task update PLAT-42 \
  --title "New title" \
  --status in_progress \
  --assignee username \
  --priority urgent \
  --sprint <sprint-id>
```

### Search tasks

```bash
tokanban task search "authentication bug" --project PLAT --limit 20
```

Free-text search across task titles and descriptions.

### Close and reopen

```bash
tokanban task close PLAT-42 --reason "Completed in PR #123"
tokanban task reopen PLAT-42
```

## Projects

Projects group tasks under a workspace. Each project has a key prefix (e.g., `PLAT`) used for task keys.

```bash
# Create a project
tokanban project create "Platform" --key_prefix PLAT

# List all projects
tokanban project list

# View project details
tokanban project view PLAT

# Update a project
tokanban project update PLAT --name "Platform v2"

# Archive a project
tokanban project archive PLAT

# Set default project (saved to config)
tokanban project set PLAT
```

After running `project set`, the `--project` flag can be omitted from other commands.

## Sprints

Time-boxed iterations for organizing work.

```bash
# Create a sprint
tokanban sprint create \
  --project PLAT \
  --name "Sprint 1" \
  --start 2026-04-14 \
  --end 2026-04-28

# List sprints
tokanban sprint list --project PLAT

# View sprint details
tokanban sprint view <sprint-id>

# Update sprint dates or name
tokanban sprint update <sprint-id> --name "Sprint 1 (Extended)" --end 2026-05-02

# Activate a sprint
tokanban sprint activate <sprint-id>

# Close a sprint
tokanban sprint close <sprint-id>
```

Assign tasks to a sprint with `tokanban task update PLAT-42 --sprint <sprint-id>`.

## Comments

Add discussion threads to tasks.

```bash
# Add a comment
tokanban comment add PLAT-42 "Looks good, merging now"

# Add from stdin (for longer comments)
echo "Detailed review notes..." | tokanban comment add PLAT-42

# List comments
tokanban comment list PLAT-42

# Edit a comment
tokanban comment edit <comment-id> "Updated text"

# Delete a comment
tokanban comment delete <comment-id>
```

## Members

Manage workspace team members.

```bash
# Invite a member
tokanban member invite teammate@company.com --role member

# List members
tokanban member list

# Update a member's role
tokanban member update <user-id> --role admin

# Revoke access
tokanban member revoke <user-id>
```

Roles: `admin`, `editor`, `viewer`.

## Agent Tokens

Create scoped API tokens for AI agents and automation.

```bash
# Create an agent token
tokanban agent create "My Claude" \
  --type claude-code \
  --scopes "tasks:read,tasks:write,projects:read"

# List agents
tokanban agent list

# View agent details
tokanban agent view <agent-id>

# View agent scopes
tokanban agent scopes <agent-id>

# Rotate key (1-hour grace period for old key)
tokanban agent rotate <agent-id>

# Revoke permanently
tokanban agent revoke <agent-id>
```

## Workflows

Customize the status workflow for a project.

```bash
# Show current workflow
tokanban workflow show --project PLAT

# Add a status
tokanban workflow update --project PLAT --add_status "In Review"

# Remove a status (must have no open tasks)
tokanban workflow update --project PLAT --remove_status "Blocked"

# Migrate tasks between statuses
tokanban workflow update --project PLAT --migrate "Blocked:To Do"
```

## Workspaces

Top-level organizational unit.

```bash
# Create a workspace
tokanban workspace create "Acme Corp"

# List workspaces
tokanban workspace list

# Set default workspace
tokanban workspace set acme-corp

# Show current workspace
tokanban workspace current
```

## Import

Bring data from external tools.

```bash
# Import from Jira JSON export
tokanban import jira export.json --project PLAT

# Import from CSV
tokanban import csv tasks.csv --project PLAT
```

CSV format: expects columns for title, description, priority, status, assignee.

## Visualizations

Generate interactive HTML views.

```bash
# Kanban board (opens in browser)
tokanban viz kanban --project PLAT

# Save to file instead
tokanban viz kanban --project PLAT --output board.html

# Sprint burndown chart
tokanban viz burndown --project PLAT --sprint <sprint-id>

# Project timeline
tokanban viz timeline --project PLAT
```

## Authentication

```bash
# Login via browser OAuth
tokanban auth login

# Check current session
tokanban auth status

# Logout and clear credentials
tokanban auth logout
```

Credentials are stored at `~/.config/tokanban/config.toml`.

## Shell Completions

```bash
# Generate for your shell
tokanban completion bash > ~/.bash_completion.d/tokanban
tokanban completion zsh  > ~/.zsh/completions/_tokanban
tokanban completion fish > ~/.config/fish/completions/tokanban.fish
```

## Common Workflows

### Set up a new project

```bash
tokanban project create "Web App" --key_prefix WEB
tokanban project set WEB
tokanban sprint create --name "Sprint 1" --start 2026-04-14 --end 2026-04-28
tokanban task create "Set up CI/CD" --priority high
tokanban task create "Design landing page" --priority medium
```

### Daily standup review

```bash
tokanban task list --status in_progress --assignee me
tokanban viz kanban
```

### Sprint planning

```bash
tokanban sprint create --name "Sprint 2" --start 2026-04-28 --end 2026-05-12
tokanban task list --status backlog --limit 50
# Assign tasks to the sprint:
tokanban task update WEB-15 --sprint <new-sprint-id>
tokanban task update WEB-16 --sprint <new-sprint-id>
tokanban sprint activate <new-sprint-id>
```

### Close out a sprint

```bash
tokanban sprint close <sprint-id>
tokanban viz burndown --sprint <sprint-id> --output sprint-1-report.html
```

### Onboard a new team member

```bash
tokanban member invite new-hire@company.com --role member
tokanban agent create "New Hire's Claude" --type claude-code --scopes "tasks:read,tasks:write"
```

For full setup and MCP configuration instructions, see the `tokanban:setup` skill.
