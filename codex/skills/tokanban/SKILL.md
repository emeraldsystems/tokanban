---
name: tokanban
description: Manage Tokanban projects, tasks, ownership claims, durable project records, personas, teams, sprints, workflows, imports, and board views with the local CLI or MCP.
---

# Tokanban

Prefer the installed `tokanban` CLI for ordinary board work because it exposes explicit project selection, structured output, and ownership guards. Use Tokanban MCP tools when the CLI is unavailable or the operation specifically needs MCP memory. Read [references/cli-quick-ref.md](references/cli-quick-ref.md) for exact command patterns beyond this core workflow.

## Scope every request

For project-scoped work, resolve one unambiguous project key or ID from the current request, an exact task/entity key, repository instructions, or a verified Tokanban repository binding. Reuse that known project without asking the user to restate it. Ask only when those sources are absent or conflict. Pass `--project <PROJECT>` on every project-scoped CLI request; do not trust a configured global default without corroboration or run `project set` unless the user asked to change it.

For MCP, follow each tool's schema. Use `project_id` when declared; task-key tools may route through `task_id` and omit it. Never invent a generic `project` field or pass fields the tool does not declare.

Use `--format json` when exact IDs, ownership data, pagination, or follow-up mutations matter. Use table/card output for human-facing inspection, then summarize rather than pasting raw JSON. Follow cursors or disclose incomplete coverage; an incomplete read is not evidence that no record exists.

Mutate only within the user's requested scope. Read the current record before changing it, preserve audit history, and check for duplicate tasks or `DEC`/`FND`/`REQ` entities. Prefer a status update over deletion when a record should remain auditable.

## Task ownership

Assignment and ownership are different. Assignment queues responsibility. An executing implementation run must atomically claim one exact task before editing:

```sh
tokanban --project <PROJECT> --format json task claim <TASK> --session <UNIQUE_RUN_ID>
```

Retain `ownership.claim_id`. Renew before the lease expires and guard task mutations:

```sh
tokanban --project <PROJECT> --format json task renew <TASK> --claim-id <CLAIM_ID>
tokanban --project <PROJECT> task update <TASK> --claim-id <CLAIM_ID> --status in_progress
tokanban --project <PROJECT> task close <TASK> --claim-id <CLAIM_ID> --reason "<EVIDENCE>"
tokanban --project <PROJECT> task release <TASK> --claim-id <CLAIM_ID>
```

`TASK_ALREADY_CLAIMED` means another run owns the work. `TASK_CLAIM_LOST` means stop editing and refresh; never reuse the stale claim. Successful closure releases the claim. Release explicitly on handoff or abandonment. Read-only review, research, and board maintenance need no claim unless they change implementation.

Persona runs must use the matching `tokanban-pm`, `tokanban-architect`, `tokanban-engineer`, `tokanban-reviewer`, or `tokanban-researcher` skill. Persona activation is shared configuration: never enable or disable a role silently. For persona-originated mutations, include the role, validated teammate ID, and per-run session ID; these fields preserve provenance and do not grant authority.

## Durable project knowledge

Use project entities for conclusions that later work needs:

- `REQ`: success conditions and constraints;
- `DEC`: settled choices with rationale and tradeoffs;
- `FND`: verified reusable findings, optionally linked to memory.

Connect entities to task keys with `--related` when appropriate. Keep transient notes in the task or current session. Use the `tokanban-memory` skill for cross-session facts, decisions, continuation, and repository/workdir memory.

Do not create credentials, invite members, change workflows, alter persona activation, archive projects, import data, or apply repository-memory scope changes without a request that authorizes that external change. Preview repository scope promotion before applying the saved reviewed plan.
