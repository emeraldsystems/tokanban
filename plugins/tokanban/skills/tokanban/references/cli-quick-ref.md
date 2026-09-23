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
| `tokanban task list --available` | Open, unblocked tasks without an active ownership claim |
| `tokanban task claim <KEY> --session <RUN_ID> [--ttl-seconds 1800]` | Atomically claim work for a distinct agent run |
| `tokanban task renew <KEY> --claim-id <CLAIM_ID>` | Renew before expiry; a lost claim is rejected |
| `tokanban task release <KEY> --claim-id <CLAIM_ID>` | Release work for another session |

When coordinating agent work, claim a task before starting, retain the returned
`ownership.claim_id`, and include `--claim-id` on `task update` and `task close`.
Use a unique session ID for each independently executing run. Renew well before
the default 30-minute expiry; stop and refresh if renewal or a guarded update
returns `TASK_CLAIM_LOST`. Claims preserve the human assignee and workflow status.

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

## Persona Commands

| Command | Description |
|---------|-------------|
| `tokanban persona list` | Show shared activation and teammate IDs |
| `tokanban persona configure --enable <KEY> [--disable <KEY>]` | Change enabled built-in roles |
| `tokanban persona teammates` | List persistent assignable AI teammates |
| `tokanban persona assignments <KEY>` | List tasks queued for a persona teammate |
| `tokanban --format json persona context <KEY> [--session RUN]` | Load live project, task, entity, activation, and teammate context |

Built-in keys are `pm`, `architect`, `engineer`, `reviewer`, and `researcher`.
Assignment does not create a run or ownership claim. Context failures must not be
interpreted as disabled activation or an empty board.

## Team Commands

| Command | Description |
|---------|-------------|
| `tokanban team list` | List teams |
| `tokanban team create <NAME>` | Create a mixed-member team |
| `tokanban team view <ID>` | View human and AI membership |
| `tokanban team update <ID> --name <NAME>` | Rename a team |
| `tokanban team add-member <ID> --type human\|ai --member-id <ID>` | Add a human or AI teammate |
| `tokanban team remove-member <ID> --type human\|ai --member-id <ID>` | Remove a member |
| `tokanban team delete <ID>` | Delete a team |

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

## Repository Memory Commands
`repo inspect` is read-only and offline by default (no auth/network); every other subcommand is an explicit, authenticated REST action. Identity is always explicit — nothing is bound by matching a folder name or remote URL. `--expected-revision` is a non-negative integer (0 or omitted means "first bind"); invalid values are rejected before any network call.

| Command | Description |
|---------|-------------|
| `tokanban repo inspect [--path P] [--binding]` | Local Git discovery (root, worktree/common git dir, branch/detached HEAD, normalized remote); `--binding` also fetches the current binding |
| `tokanban repo create <NAME> [--remote URL] [--project ID]` | Create a new repository-memory identity |
| `tokanban repo list [--limit N] [--offset N]` | List repository identities |
| `tokanban repo aliases [--path P]` | Show name/remote matches for manual review only — never binds automatically |
| `tokanban repo bind <REPOSITORY_ID> [--path P] [--branch B] [--kind main\|worktree\|clone\|unknown] [--expected-revision N]` | Bind (or rebind) a working directory; omit `--expected-revision` for the first bind |
| `tokanban repo unbind --expected-revision N [--path P]` | Deactivate the binding for a working directory |
| `tokanban repo history [--checkout-id ID] [--path P]` | Show checkout binding history |

### Repository Scope Commands (historical promotion)
Explicit, reviewable promotion of existing facts/decisions into repository-shared (or branch/workdir/experiment) scope. Preview never writes. Apply never re-previews silently — it only ever resends a plan file already shown to the user.

| Command | Description |
|---------|-------------|
| `tokanban repo scope preview --repository-id ID --memory-id ID [--memory-id ID ...] [--scope repository\|branch\|workdir\|experiment] [--branch B] [--experiment E]` | Preview a scope change for 1-50 explicit memory IDs (read-only); save with `--format json > plan.json` |
| `tokanban repo scope apply --plan plan.json` | Apply exactly the reviewed selection and fingerprint from a saved preview plan file |
| `tokanban repo scope restore <OPERATION_ID>` | Restore the scopes from before a prior scope operation (idempotent) |

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
