# Tokanban CLI Quick Reference

## Task Commands
| Command | Description |
|---------|-------------|
| `tokanban task create "<title>" [--priority P] [--assignee U] [--sprint S]` | Create a task |
| `tokanban task list [--status S] [--assignee U] [--priority P] [--limit N]` | List/filter tasks |
| `tokanban task view <KEY>` | View task detail |
| `tokanban task update <KEY> [--title T] [--status S] [--priority P] [--assignee U]` | Update a task |
| `tokanban task search "<query>" [--limit N]` | Full-text search |
| `tokanban task close <KEY> [--reason R]` | Close a task |
| `tokanban task reopen <KEY>` | Reopen a task |

## Project Entity Commands
Keys follow `PROJECT-{DEC,FND,REQ}-<id>`, for example `PLAT-DEC-1`.

| Command | Description |
|---------|-------------|
| `tokanban entity create DEC "<title>" [--content C] [--memory-ref M] [--related K]` | Record a core project decision |
| `tokanban entity create FND "<title>" [--content C] [--memory-ref M]` | Record a useful finding |
| `tokanban entity create REQ "<title>" [--content C] [--related K]` | Record a requirement that gates success |
| `tokanban entity list [--kind DEC\|FND\|REQ] [--status S] [--query Q]` | List/filter entities |
| `tokanban entity view <KEY>` | View entity detail |
| `tokanban entity update <KEY> [--title T] [--content C] [--status S]` | Update an entity |
| `tokanban entity delete <KEY>` | Delete an entity |

## Project Commands
| Command | Description |
|---------|-------------|
| `tokanban project create "<name>" --key_prefix <KEY>` | Create project |
| `tokanban project list` | List projects |
| `tokanban project view <KEY>` | View project |
| `tokanban project update <KEY> [--name N]` | Update project |
| `tokanban project archive <KEY>` | Archive project |
| `tokanban project set <KEY>` | Set default project |

## Sprint Commands
| Command | Description |
|---------|-------------|
| `tokanban sprint create --name N --start D --end D` | Create sprint |
| `tokanban sprint list` | List sprints |
| `tokanban sprint view <ID>` | View sprint |
| `tokanban sprint update <ID> [--name N] [--start D] [--end D]` | Update sprint |
| `tokanban sprint activate <ID>` | Activate sprint |
| `tokanban sprint close <ID>` | Close sprint |

## Member Commands
| Command | Description |
|---------|-------------|
| `tokanban member invite <email> --role <role>` | Invite member |
| `tokanban member list` | List members |
| `tokanban member update <user-id> --role <role>` | Change role |
| `tokanban member revoke <user-id>` | Remove member |

## Agent Commands
| Command | Description |
|---------|-------------|
| `tokanban agent create "<name>" --type T --scopes S` | Create agent token |
| `tokanban agent list` | List agents |
| `tokanban agent view <ID>` | View agent |
| `tokanban agent scopes <ID>` | List scopes |
| `tokanban agent rotate <ID>` | Rotate key |
| `tokanban agent revoke <ID>` | Revoke agent |

Memory-capable agents: add `memory:read,memory:write` to the `--scopes` list and install the harness blocks from `templates/`.

## Other Commands
| Command | Description |
|---------|-------------|
| `tokanban comment add <KEY> "<body>"` | Add comment |
| `tokanban comment list <KEY>` | List comments |
| `tokanban workflow show` | Show workflow |
| `tokanban workflow update --add_status S` | Add status |
| `tokanban workspace list` | List workspaces |
| `tokanban workspace set <slug>` | Set default |
| `tokanban viz kanban` | Open kanban board |
| `tokanban viz burndown --sprint <ID>` | Burndown chart |
| `tokanban viz timeline` | Project timeline |
| `tokanban import jira <file>` | Import from Jira |
| `tokanban import csv <file>` | Import from CSV |
| `tokanban auth login` | Login |
| `tokanban auth status` | Check session |
| `tokanban completion <shell>` | Generate completions |

## Priority Values
`urgent` | `high` | `medium` | `low` | `none`

## Member Roles
`admin` | `editor` | `viewer`

## Output Formats
`--format table` for human-readable task lists and backlog reviews
`--format json` for parsing, exact IDs, or follow-up command inputs
`--format card` when a card-style view is preferable, though `table` is usually the best list default

In agent responses: use bullets for a single task, use tables for multi-task lists, and always leave a blank line before markdown tables.
