---
name: tokanban-memory
description: "Use when the user asks about agent memory, wants to remember something across sessions, asks what was decided, asks what they were working on, starts a new coding session, or is about to end one. Trigger phrases: 'remember this', 'memory', 'what did we decide', 'what was I working on', 'continue where I left off', 'session context', 'continuation prompt', 'stale decision', 'supersede this fact'."
---

# Tokanban Agent Memory

Use Tokanban memory tools to preserve context across sessions without collapsing unrelated projects and worktrees into one flat pool.

## When to Use This Skill

Use this skill when any of the following is true:

1. A new coding session is starting and the active Tokanban project and/or local working directory is known.
2. The user asks to remember a non-obvious fact, constraint, or conclusion.
3. The agent is making an architectural or implementation decision that should be preserved with evidence.
4. The user asks what was learned, decided, or left unfinished in earlier work.
5. The agent is wrapping up and should leave a continuation prompt for the next session.

## Core Model

Tokanban memory is user-owned, but every session and memory write is routed through one or both top-level roots:

- `project/<project_id>` for Tokanban board/project context shared across harnesses
- `workdir/<working_directory_key>` for repo/worktree-local context

When both roots are present, attach both. This is the normal best case. Project-shared and workdir-local memory should complement each other, not compete. `source_harness` is provenance metadata only; it is not the primary partition key.

## Session Lifecycle

At session start:

1. Call `session_start({ project_id, working_directory, source_harness, task_id, key_files, partition_path })`
2. Immediately call `memory_relevant_now({ project_id, working_directory, files, task_id, module, partition_path })`
3. Read the returned continuation prompt before starting work

At session end:

1. If the local Tokanban CLI helper is available, run `tokanban memory candidate review --project-id ... --working-directory ...` for the active roots
2. Promote only the items that still deserve durable memory
3. Call `session_end({ session_id, completed, remaining, learned, decisions_made, files_touched, continuation_prompt })`
4. Make the continuation prompt specific and actionable for the next agent
5. After `session_end` succeeds, clear only the returned `clear_after_session_end_ids`

Recommended item shapes:

- `completed` / `remaining`: `[{ description, task_id?, priority? }]`
- `learned`: `[{ fact, confidence?, provenance?, task_id?, module?, key_files? }]`
- `decisions_made`: `[{ decision, supporting_facts?, task_id?, module?, key_files? }]`

Compatibility note:

- the server also accepts plain strings and `{ text: "..." }` shorthand for these arrays
- `decisions_made` also accepts `supporting_fact_ids` as an alias for `supporting_facts`

## Durable Write Gate

Durable memory is promoted, not streamed. Treat new findings and choices as candidates first.

Explicit user requests such as "remember this" bypass the gate and should be written immediately.

For agent-initiated writes, evaluate two questions:

- **Richness:** will this still matter later, help future sessions, and be costly to rediscover?
- **Promotion readiness:** is this stable, verified, clearly worded, and ready to persist now?

Action rule:

- explicit user request -> write now
- high richness + high readiness -> write now
- high richness + low/medium readiness -> defer to `session_end` review
- low richness or short-lived context -> do not write durable memory

Prefer deferral when the item is still a hypothesis, still likely to change in the next few tool calls, or is mostly useful as short-term working context.

If the local Tokanban CLI helper is available:

- use `tokanban memory score` with JSON input to offload the scoring step
- use `tokanban memory candidate add` to persist deferred items outside the prompt buffer
- use `tokanban memory candidate review` before `session_end`; copy `session_end_contract.learned`, `session_end_contract.decisions_made`, and `session_end_contract.remaining` into the wrap-up payload

If the harness has no dedicated scratchpad or candidate buffer yet, keep the deferred list lightweight in working notes and review it before `session_end`.

## When to Create Facts Now

Call `memory_create_fact` after the gate says the item is both durable and ready:

- A technical constraint
- A verified behavior
- A dependency limitation
- A repo-specific convention
- A board/project rule that affects implementation

Good fact shape:

- concise
- testable
- confidence-bearing
- tied to the active roots

## When to Create Decisions Now

Call `memory_create_decision` after the choice is actually made and should outlive the current session:

- choosing between implementation approaches
- adopting a long-lived convention
- accepting a tradeoff that should be revisited later only with evidence

Always attach `supporting_fact_ids` when they exist.

## When to Supersede vs Contradict

Use `memory_supersede` when the earlier fact was incomplete and the new fact refines it.

Use `flag_contradiction` when the earlier fact is no longer true.

Rule of thumb:

- refinement = `memory_supersede`
- invalidation = `flag_contradiction`

## Kanban Integration

When picking up a Tokanban task:

1. Fetch the task
2. Call `memory_relevant_now` with `project_id`, `working_directory`, and `task_id`
3. While working, keep task-relevant candidates light until they are ready to promote
4. Attach `task_id` to any facts or decisions that you do promote
5. At session end, include completed and remaining items that reference the task

## Partition Guidance

Use both roots whenever they are available:

- board-only work: `project_id`
- repo-only exploration: `working_directory`
- normal implementation work against a board in a repo: both

Do not treat `claude-code` and `codex` as separate top-level scopes. That separation belongs in provenance, not in the partition model.

## Continuation Prompt Standard

A good continuation prompt is:

- under 200 tokens when possible
- explicit about the next step
- grounded in what already shipped
- clear about any unresolved risk

Use this shape:

`Continue <task or area>: what is done, what remains, and the next concrete action.`
