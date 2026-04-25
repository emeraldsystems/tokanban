## Tokanban Memory

At the start of a coding session, call `session_start` with the current `project_id` and/or `working_directory`, then call `memory_relevant_now` before doing substantive work.

During work:

- non-obvious learning -> `memory_create_fact`
- durable implementation choice -> `memory_create_decision`
- refined understanding -> `memory_supersede`
- invalidated understanding -> `flag_contradiction`

Use both roots when available:

- `project/<project_id>` for shared board context
- `workdir/<working_directory_key>` for repo/worktree context

These roots should complement each other. `source_harness` is provenance only.

Before ending the session, call `session_end` with completed work, remaining work, and a continuation prompt for the next agent.
