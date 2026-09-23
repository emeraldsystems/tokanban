---
name: tokanban-memory
description: Retrieve and preserve Tokanban memory across Codex sessions, including scoped facts, decisions, deferred candidates, and actionable handoffs.
---

# Tokanban Memory

Use the Tokanban MCP memory tools for durable reads and writes. The local CLI provides scoring and a deferred-candidate buffer, but does not replace the MCP session lifecycle.

## Scope and start the session

Use explicit roots whenever known:

- `project_id` for memory shared by work on one Tokanban project;
- `working_directory` for repository or worktree-local context;
- both for normal project implementation in a checkout.

`source_harness` must be `codex`. It is provenance, not a partition root. Do not separate memory merely because an earlier session used another harness.

At the beginning of relevant work, reuse a canonical Tokanban `session_id` already supplied to this Codex run. Otherwise call `session_start` once with `source_harness: "codex"`, the explicit roots, and the actual harness session identity when available. Do not invent a harness identity from a path. Include known `task_id`, `key_files`, `partition_path`, `session_kind`, or parent session only when accurate.

Immediately call `memory_relevant_now` with that `session_id` and the same explicit roots plus current files, task, module, or partition. Read the continuation prompt before proceeding. A failed call means memory is unavailable; it does not mean no memory exists.

## Promote selectively

Write an explicit user request such as “remember this” immediately. For agent-initiated memory, persist only information that will matter later, is costly to rediscover, and is stable and verified. Defer a useful but immature hypothesis. Drop transient working state.

Use `memory_create_fact` for verified constraints, behavior, dependencies, or repository conventions. Use `memory_create_decision` for a settled choice that should outlive the session, attaching supporting fact IDs when available. Use `memory_supersede` for a refinement and `flag_contradiction` when an earlier fact is no longer true.

For local candidate triage, use structured JSON rather than prose-shaped shell arguments:

```sh
tokanban memory score --input-file <REQUEST_JSON>
tokanban memory candidate add --input-file <REQUEST_JSON> --project-id <PROJECT_ID> --working-directory <PATH> [--task-id <ID>] [--module <NAME>]
tokanban memory candidate review --project-id <PROJECT_ID> --working-directory <PATH> [--task-id <ID>] [--module <NAME>] --format json
```

Supply at least one root when adding a candidate. Before session end, review deferred candidates and promote only those that remain durable and ready. Copy the review response's `session_end_contract` fields into the final handoff when appropriate.

## End with a handoff

Call `session_end` with concrete `completed`, `remaining`, `learned`, `decisions_made`, `files_touched`, and an actionable continuation prompt. Reference task IDs, modules, and key files where useful. Keep the continuation focused on what is done, what remains, and the next concrete action. After a successful end, clear only candidate IDs explicitly returned in `clear_after_session_end_ids`.

Do not assume Codex hooks, automatic session close, or token telemetry. If the MCP memory tools are unavailable, retain only the local candidate buffer where appropriate and tell the user that the durable session handoff was not written.
