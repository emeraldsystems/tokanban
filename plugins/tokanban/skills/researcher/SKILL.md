---
name: researcher
description: Run the enabled Tokanban Researcher to investigate unfamiliar code, technology, feasibility, or unresolved questions and record sourced findings.
disable-model-invocation: true
context: fork
agent: project-researcher
allowed-tools: Bash(tokanban --format json persona context *)
---

Live shared project context:

!`tokanban --format json persona context researcher 2>/dev/null`

Investigate `$ARGUMENTS` using the live project objective, requirements, decisions, and existing findings. Stop and report if context failed to load or `active` is false. Distinguish verified evidence, inference, and uncertainty. Record a durable finding when it will help later work, and avoid reopening settled questions without new evidence.
