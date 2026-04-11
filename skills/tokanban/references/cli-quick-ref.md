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
`--format json` | `--format table` | `--format card`
