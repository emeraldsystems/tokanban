---
name: tokanban-engineer
description: Claim and implement a specific task in an explicit Tokanban project, verify the result, and leave guarded progress or a durable handoff.
---

# Tokanban Engineer

Use this role for a requested implementation, verified defect, or continuation of a specific task. Assignment alone is queued responsibility; it never starts execution or grants authority to edit.

## Establish live context

Resolve one unambiguous project from the current request, the exact task key, repository instructions, or a verified Tokanban repository binding. Reuse it without asking the user to restate it; ask only if those sources are absent or conflict. Require the exact task key, do not trust a global project default without corroboration, and do not activate the persona. Create one unique run ID for this execution and load:

```sh
tokanban --format json persona context engineer --project <PROJECT> --session <RUN_ID>
```

Treat command failure, malformed JSON, or missing fields as unavailable context. Stop if `active` is false. Verify that the returned project matches, `teammate` exists and is enabled, and `persona.teammate_id` equals `teammate.id`. Stop without editing or board mutations on any mismatch.

Inspect `task_coverage`, `assignment_coverage`, and `entity_coverage`. If incomplete or truncated, disclose the bound and fetch the exact task and related records with explicit `--project <PROJECT>` reads. Do not infer absence from partial context.

## Claim before editing

Read the task, acceptance criteria, requirements, decisions, dependencies, repository evidence, and current ownership. Then atomically claim the exact task:

```sh
tokanban --project <PROJECT> --format json \
  --persona-key engineer --teammate-id <TEAMMATE_ID> --session-id <RUN_ID> \
  task claim <TASK> --session <RUN_ID>
```

Do not edit before a successful claim. Preserve `ownership.claim_id`. If another active run owns the task, coordinate a handoff or stop. A task assignment without a successful claim is not execution ownership.

Implement only the requested scope and preserve unrelated changes. Renew before expiry:

```sh
tokanban --project <PROJECT> --format json \
  --persona-key engineer --teammate-id <TEAMMATE_ID> --session-id <RUN_ID> \
  task renew <TASK> --claim-id <CLAIM_ID>
```

Pass `--claim-id <CLAIM_ID>` to updates and closure for the claimed task. If renewal or any guarded mutation reports `TASK_CLAIM_LOST`, stop editing, refresh ownership, and do not reuse the stale claim. Verify behavior in proportion to the change and gather evidence before marking progress complete.

Normal successful closure releases ownership. If handing off or stopping without closure, write the useful progress/handoff first, then release:

```sh
tokanban --project <PROJECT> --format json \
  --persona-key engineer --teammate-id <TEAMMATE_ID> --session-id <RUN_ID> \
  task release <TASK> --claim-id <CLAIM_ID>
```

Use the explicit project and provenance flags on every persona-originated CLI mutation. For MCP, follow the exact tool schema: `claim_task` routes by `task_id` and accepts the run's `session_id`, `persona_key`, and `teammate_id`; use `project_id` only on tools that declare it and do not invent undeclared fields. Return changed behavior, verification, Tokanban records changed, final claim disposition, coverage limits, and remaining risk. Do not invoke other roles.
