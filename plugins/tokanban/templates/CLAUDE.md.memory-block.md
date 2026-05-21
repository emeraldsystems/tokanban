## Tokanban Memory

At the start of a coding session, call `session_start` with the current `project_id` and/or `working_directory`, then call `memory_relevant_now` before doing substantive work.

During work:

- treat new findings and choices as candidates first
- explicit user request to remember something -> write it immediately
- high richness + high promotion readiness -> `memory_create_fact` or `memory_create_decision`
- otherwise defer until session end or keep it in working notes only
- refined understanding -> `memory_supersede`
- invalidated understanding -> `flag_contradiction`

Use both roots when available:

- `project/<project_id>` for shared board context
- `workdir/<working_directory_key>` for repo/worktree context

Only pass `project_id` when the Tokanban project already exists and is confirmed. If only the local repo/worktree is known, use `working_directory` alone and ask before creating a new Tokanban project.

These roots should complement each other. `source_harness` is provenance only.

Before ending the session, if the local Tokanban CLI helper is available run `tokanban memory candidate review --project-id ... --working-directory ...`, copy `session_end_contract.learned`, `session_end_contract.decisions_made`, and `session_end_contract.remaining` into the wrap-up payload, then call `session_end` with completed work, remaining work, and a continuation prompt for the next agent. After `session_end` succeeds, clear only `clear_after_session_end_ids`. Prefer:

- `completed/remaining: [{ description }]`
- `learned: [{ fact, confidence? }]`
- `decisions_made: [{ decision, supporting_facts? }]`
