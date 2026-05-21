## Tokanban Memory

When a coding session starts and a Tokanban project or working directory is known:

1. Call `session_start`
2. Call `memory_relevant_now`
3. Read the continuation prompt before starting implementation

During work:

- treat new findings and choices as candidates first
- explicit user request to remember something -> write it immediately
- `memory_create_fact` / `memory_create_decision` only for items that are both durable and ready now
- otherwise defer them until session end review
- `memory_supersede` for refinements
- `flag_contradiction` for invalidated facts

Use project and workdir roots together when possible so board context and repo context complement each other. Treat harness names only as provenance metadata.
Only pass `project_id` when the Tokanban board already exists. If the repo/worktree is known but no board is confirmed yet, use `working_directory` only and ask before creating a new project.

Before stopping, if the local Tokanban CLI helper is available run `tokanban memory candidate review --project-id ... --working-directory ...`, copy `session_end_contract.learned`, `session_end_contract.decisions_made`, and `session_end_contract.remaining` into the wrap-up payload, then call `session_end` with:

- completed
- remaining
- learned
- decisions_made
- files_touched
- continuation_prompt

Prefer canonical item shapes:

- `completed/remaining: [{ description }]`
- `learned: [{ fact, confidence? }]`
- `decisions_made: [{ decision, supporting_facts? }]`

After `session_end` succeeds, clear only `clear_after_session_end_ids`.
