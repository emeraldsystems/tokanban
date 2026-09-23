# Tokanban CLI quick reference

Use an explicit `--project <PROJECT>` on every project-scoped request. Global flags may appear before or after subcommands. Add `--format json` when parsing IDs, ownership, provenance, or pagination. Run `tokanban <command> --help` when the installed version differs from this reference.

## Tasks and ownership

```sh
tokanban task create "<TITLE>" --project <PROJECT> [--priority urgent|high|medium|low|none] [--assignee <USER>] [--sprint <ID>] [--description <TEXT>]
tokanban task list --project <PROJECT> [--status <STATUS>] [--assignee <USER_OR_ID>] [--priority <P>] [--sprint <ID>] [--ready] [--available] [--cursor <CURSOR>] [--limit 100]
tokanban task view <TASK> --project <PROJECT>
tokanban task search "<QUERY>" --project <PROJECT> [--limit 20]
tokanban task update <TASK> --project <PROJECT> [--claim-id <ID>] [--title <T>] [--status <S>] [--assignee <U>] [--priority <P>] [--sprint <ID>] [--description <D>]
tokanban task close <TASK> --project <PROJECT> [--claim-id <ID>] [--reason <TEXT>]
tokanban task reopen <TASK> --project <PROJECT>
tokanban task claim <TASK> --project <PROJECT> --session <RUN_ID> [--ttl-seconds 1800]
tokanban task renew <TASK> --project <PROJECT> --claim-id <CLAIM_ID> [--ttl-seconds 1800]
tokanban task release <TASK> --project <PROJECT> --claim-id <CLAIM_ID>
```

Claims last 60–3600 seconds. Retain the claim ID returned by `claim`; use it for updates and closure. Each independently executing run needs a distinct session ID.

## Requirements, decisions, and findings

Keys follow `PROJECT-REQ-N`, `PROJECT-DEC-N`, and `PROJECT-FND-N`.

```sh
tokanban entity create REQ "<TITLE>" --project <PROJECT> [--content <TEXT>] [--related <KEY>]
tokanban entity create DEC "<TITLE>" --project <PROJECT> [--content <TEXT>] [--related <KEY>]
tokanban entity create FND "<TITLE>" --project <PROJECT> [--content <TEXT>] [--memory-ref <ID>] [--related <KEY>]
tokanban entity list --project <PROJECT> [--kind REQ|DEC|FND] [--status <STATUS>] [--query <TEXT>]
tokanban entity view <KEY> --project <PROJECT>
tokanban entity update <KEY> --project <PROJECT> [--title <T>] [--content <C>] [--status <S>] [--memory-ref <ID>] [--related <KEY>]
tokanban entity delete <KEY> --project <PROJECT>
```

## Personas and provenance

Supported persona keys: `pm`, `architect`, `engineer`, `reviewer`, `researcher`.

```sh
tokanban persona list --project <PROJECT>
tokanban persona teammates --project <PROJECT>
tokanban persona assignments <ROLE> --project <PROJECT> [--limit 100]
tokanban --format json persona context <ROLE> --project <PROJECT> [--session <RUN_ID>] [--compact]
tokanban persona configure --project <PROJECT> [--enable <ROLE>] [--disable <ROLE>]
```

Activation is shared project configuration. Assignment does not launch a worker or claim a task. A context failure is unavailable state, not an empty board. Validate `active`, requested project identity, enabled teammate identity, and the context coverage fields.

Persona-originated mutations use hidden global provenance flags:

```sh
tokanban --project <PROJECT> --persona-key <ROLE> --teammate-id <TEAMMATE_ID> --session-id <RUN_ID> <mutation>
```

## Projects, teams, and sprints

```sh
tokanban project list
tokanban project view <PROJECT> --project <PROJECT>
tokanban project create "<NAME>" --key-prefix <KEY> --persona <ROLE> [--persona <ROLE> ...]
tokanban project update <PROJECT> --name "<NAME>"
tokanban project archive <PROJECT>

tokanban team list
tokanban team create "<NAME>"
tokanban team view <TEAM_ID>
tokanban team update <TEAM_ID> --name "<NAME>"
tokanban team add-member <TEAM_ID> --type human|ai --member-id <ID>
tokanban team remove-member <TEAM_ID> --type human|ai --member-id <ID>
tokanban team delete <TEAM_ID>

tokanban sprint create --project <PROJECT> --name "<NAME>" --start <YYYY-MM-DD> --end <YYYY-MM-DD>
tokanban sprint list --project <PROJECT>
tokanban sprint view <SPRINT_ID> --project <PROJECT>
tokanban sprint update <SPRINT_ID> --project <PROJECT> [--name <N>] [--start <D>] [--end <D>]
tokanban sprint activate <SPRINT_ID> --project <PROJECT>
tokanban sprint close <SPRINT_ID> --project <PROJECT>
```

New projects enable PM by default when `--persona` is omitted. Use repeated
`--persona` flags when the user requested a different initial set.

## Comments, workflow, import, and views

```sh
tokanban comment add <TASK> "<BODY>" --project <PROJECT>
tokanban comment list <TASK> --project <PROJECT>
tokanban comment edit <COMMENT_ID> "<BODY>" --project <PROJECT>
tokanban comment delete <COMMENT_ID> --project <PROJECT>

tokanban workflow show --project <PROJECT>
tokanban workflow update --project <PROJECT> --add-status "<STATUS>"
tokanban workflow update --project <PROJECT> --remove-status "<STATUS>"
tokanban workflow update --project <PROJECT> --migrate "<FROM>:<TO>"

tokanban import jira <FILE> --project <PROJECT>
tokanban import csv <FILE> --project <PROJECT>
tokanban viz kanban --project <PROJECT> [--output <FILE>]
tokanban viz burndown --project <PROJECT> --sprint <ID> [--output <FILE>]
tokanban viz timeline --project <PROJECT> [--output <FILE>]
```

## Members and agent keys

These affect workspace access and credentials; perform them only on explicit request.

```sh
tokanban member list
tokanban member invite <EMAIL> --role admin|editor|viewer
tokanban member update <USER_ID> --role admin|editor|viewer
tokanban member revoke <USER_ID>

tokanban agent list
tokanban agent create "<NAME>" --type codex --scopes "<COMMA_SEPARATED_SCOPES>"
tokanban agent view <AGENT_ID>
tokanban agent scopes <AGENT_ID>
tokanban agent rotate <AGENT_ID>
tokanban agent revoke <AGENT_ID>
```

Memory-capable keys need `memory:read,memory:write` in addition to relevant project/task scopes.

## Authentication, workspace defaults, and completions

These commands change account or local configuration rather than project data. Change defaults or credentials only when requested.

```sh
tokanban auth login
tokanban auth status
tokanban auth logout

tokanban workspace list
tokanban workspace current
tokanban workspace set <SLUG>
tokanban project set <PROJECT>

tokanban completion bash
tokanban completion zsh
tokanban completion fish
```

## Repository memory

`repo inspect` is offline unless `--binding` is passed. Binding and scope operations are authenticated writes. Repository identity is explicit; name or remote matches never bind automatically.

```sh
tokanban repo inspect [--path <PATH>] [--binding]
tokanban repo create "<NAME>" [--remote <URL>] [--project <PROJECT_ID>]
tokanban repo list [--limit <N>] [--offset <N>]
tokanban repo aliases [--path <PATH>]
tokanban repo bind <REPOSITORY_ID> [--path <PATH>] [--branch <B>] [--kind main|worktree|clone|unknown] [--expected-revision <N>]
tokanban repo unbind --expected-revision <N> [--path <PATH>]
tokanban repo history [--checkout-id <ID>] [--path <PATH>]
```

Preview memory scope changes and save the exact JSON plan before applying it:

```sh
tokanban repo scope preview --repository-id <ID> --memory-id <ID> [--memory-id <ID> ...] [--scope repository|branch|workdir|experiment] [--branch <B>] [--experiment <E>] --format json
tokanban repo scope apply --plan <REVIEWED_PLAN_JSON>
tokanban repo scope restore <OPERATION_ID>
```
