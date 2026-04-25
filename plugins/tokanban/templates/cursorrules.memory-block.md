## Tokanban Memory

When a coding session starts and a Tokanban project or working directory is known:

1. Call `session_start`
2. Call `memory_relevant_now`
3. Read the continuation prompt before starting implementation

Store memory during work:

- `memory_create_fact` for non-obvious findings
- `memory_create_decision` for durable choices
- `memory_supersede` for refinements
- `flag_contradiction` for invalidated facts

Use project and workdir roots together when possible so board context and repo context complement each other. Treat harness names only as provenance metadata.

Before stopping, call `session_end` with:

- completed
- remaining
- learned
- decisions_made
- files_touched
- continuation_prompt
