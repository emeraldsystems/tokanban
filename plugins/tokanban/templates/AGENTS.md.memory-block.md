## Agent Memory (Tokanban)

Use Tokanban memory tools automatically when working in a known Tokanban project and/or local working directory.

At session start:

1. Call `session_start({ project_id, working_directory, source_harness, task_id, key_files, partition_path })`
2. Call `memory_relevant_now({ project_id, working_directory, files, task_id, module, partition_path })`
3. Read the continuation prompt before proceeding

During work:

- learned a durable constraint or verified behavior -> `memory_create_fact`
- made a durable choice between options -> `memory_create_decision`
- refined a prior fact -> `memory_supersede`
- invalidated a prior fact -> `flag_contradiction`
- actively evaluating a fact in multi-agent work -> `memory_claim`

Partition model:

- use `project_id` for shared Tokanban board context
- use `working_directory` for repo/worktree context
- use both when both are known

Do not split memory by harness at the top level. `claude-code`, `codex`, and other harness names are provenance metadata. The actual retrieval roots are project and/or workdir so board-shared and repo-local memory reinforce each other.

At session end:

1. Call `session_end({ session_id, completed, remaining, learned, decisions_made, files_touched, continuation_prompt })`
2. Make the continuation prompt specific enough that the next agent can continue without re-reading the entire codebase
