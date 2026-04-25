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

1. Call `session_end({ session_id, completed, remaining, learned, decisions_made, files_touched, continuation_prompt })`
2. Make the continuation prompt specific and actionable for the next agent

## When to Create Facts

Call `memory_create_fact` when you learn something that will matter later:

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

## When to Create Decisions

Call `memory_create_decision` when:

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
3. While working, attach `task_id` to facts and decisions that matter to that task
4. At session end, include completed and remaining items that reference the task

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
