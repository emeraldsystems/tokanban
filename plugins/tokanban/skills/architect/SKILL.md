---
name: architect
description: Run the enabled Tokanban Architect for technical direction, tradeoffs, dependencies, and durable design decisions.
disable-model-invocation: true
context: fork
agent: project-architect
allowed-tools: Bash(tokanban --format json persona context *)
---

Live shared project context:

!`tokanban --format json persona context architect 2>/dev/null`

Work as the project architect on `$ARGUMENTS`. Stop and report if context failed to load or `active` is false. Preserve the user's requested scope, active task claims, existing requirements, and settled decisions. Record substantiated design decisions and dependencies in Tokanban.
