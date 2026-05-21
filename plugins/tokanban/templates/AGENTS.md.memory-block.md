## Agent Memory (Tokanban)

Use Tokanban memory tools automatically when working in a known Tokanban project and/or local working directory.

At session start:

1. Call `session_start({ project_id, working_directory, source_harness, task_id, key_files, partition_path })`
2. Call `memory_relevant_now({ project_id, working_directory, files, task_id, module, partition_path })`
3. Read the continuation prompt before proceeding

During work:

- treat new findings and choices as candidates first, not immediate durable writes
- explicit user request to remember something -> write it immediately
- high richness + high promotion readiness -> `memory_create_fact` or `memory_create_decision`
- high richness but not yet stable -> defer it for `session_end` review
- low richness or short-lived context -> do not write durable memory
- refined a prior fact -> `memory_supersede`
- invalidated a prior fact -> `flag_contradiction`
- actively evaluating a fact in multi-agent work -> `memory_claim`

Partition model:

- use `project_id` for shared Tokanban board context
- use `working_directory` for repo/worktree context
- use both when both are known

Project guard:

- only pass `project_id` if it is already known to exist in Tokanban
- if you only know the repo/worktree and do not have a confirmed Tokanban board yet, use `working_directory` without `project_id`
- if a likely project does not exist yet, ask before creating it; do not invent a project id or write project-scoped memory against an unconfirmed board

Do not split memory by harness at the top level. `claude-code`, `codex`, and other harness names are provenance metadata. The actual retrieval roots are project and/or workdir so board-shared and repo-local memory reinforce each other.

At session end:

1. If the local Tokanban CLI helper is available, run `tokanban memory candidate review --project-id ... --working-directory ...`
2. Copy `session_end_contract.learned`, `session_end_contract.decisions_made`, and `session_end_contract.remaining` into the wrap-up payload
3. Call `session_end({ session_id, completed, remaining, learned, decisions_made, files_touched, continuation_prompt })`
   Prefer canonical item shapes:
   `completed/remaining: [{ description }]`
   `learned: [{ fact, confidence? }]`
   `decisions_made: [{ decision, supporting_facts? }]`
4. After `session_end` succeeds, clear only `clear_after_session_end_ids`
5. Make the continuation prompt specific enough that the next agent can continue without re-reading the entire codebase
