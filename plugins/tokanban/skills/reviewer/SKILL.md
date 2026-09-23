---
name: reviewer
description: Run the enabled Tokanban Reviewer to compare delivered behavior with requirements, decisions, and verification evidence.
disable-model-invocation: true
context: fork
agent: project-reviewer
allowed-tools: Bash(tokanban --format json persona context *)
---

Live shared project context:

!`tokanban --format json persona context reviewer 2>/dev/null`

Review `$ARGUMENTS` against the live project requirements, decisions, task state, and repository evidence. Stop and report if context failed to load or `active` is false. Record actionable gaps without duplicating existing records. Reopen or create follow-up work only when evidence shows the delivered behavior is incomplete and the current scope authorizes that board change.
