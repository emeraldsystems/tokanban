#!/bin/bash
set -u

input=$(cat)
event=""
session_id=""
agent_id=""
if command -v jq >/dev/null 2>&1; then
  event=$(printf '%s' "$input" | jq -r '.hook_event_name // empty' 2>/dev/null || true)
  session_id=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null || true)
  agent_id=$(printf '%s' "$input" | jq -r '.agent_id // empty' 2>/dev/null || true)
fi

# Project/user/plugin hooks also run inside subagents. PM guidance belongs in
# the main conversation; specialists load and gate their own persona context.
if [ "$event" = "UserPromptSubmit" ] && [ -n "$agent_id" ]; then
  exit 0
fi

if [ "${TOKANBAN_PERSONA_CONTEXT_UNSUPPORTED:-}" = "1" ]; then
  exit 0
fi

emit_context() {
  local message="$1"
  if [ "$event" = "SubagentStop" ] && command -v jq >/dev/null 2>&1; then
    jq -n --arg context "$message" '{
      hookSpecificOutput: {
        hookEventName: "SubagentStop",
        additionalContext: $context
      }
    }'
  else
    printf '%s\n' "$message"
  fi
}

warn_unavailable() {
  local message="$1"
  local permanent="${2:-false}"
  if [ "$permanent" = "true" ] || [ "${TOKANBAN_PERSONA_CONTEXT_WARNED:-}" != "1" ]; then
    emit_context "$message"
  fi
  if [ "$event" = "SessionStart" ] && [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    if [ "$permanent" = "true" ]; then
      printf '%s\n' 'export TOKANBAN_PERSONA_CONTEXT_UNSUPPORTED=1' >> "$CLAUDE_ENV_FILE"
    else
      # Suppress duplicate warnings without suppressing later retries. A user
      # may authenticate or select a project after the session starts.
      printf '%s\n' 'export TOKANBAN_PERSONA_CONTEXT_WARNED=1' >> "$CLAUDE_ENV_FILE"
    fi
  fi
}

if ! tokanban persona --help >/dev/null 2>&1; then
  warn_unavailable "TOKANBAN PM CONTEXT UNAVAILABLE: the installed tokanban CLI does not support project personas. Upgrade the CLI/plugin together before relying on persona activation or coordination. This is unavailable context, not a disabled persona or empty board." true
  exit 0
fi

args=(--format json persona context pm --compact)
if [ -n "$session_id" ]; then
  args=(--format json --session-id "$session_id" persona context pm --session "$session_id" --compact)
fi

if ! context=$(tokanban "${args[@]}" 2>/dev/null); then
  warn_unavailable "TOKANBAN PM CONTEXT UNAVAILABLE: live project context could not be loaded. Check authentication, the selected project, and API/CLI compatibility. Do not interpret this as a disabled persona or empty board."
  exit 0
fi
if [ -z "$context" ]; then
  warn_unavailable "TOKANBAN PM CONTEXT UNAVAILABLE: the persona context command returned no data. Do not interpret this as a disabled persona or empty board."
  exit 0
fi

checkpoint="TOKANBAN PM CHECKPOINT: This is live shared project context. An explicit active=false means PM is disabled; a missing/failed response never means an empty board. When active, quietly reconcile evidence-backed board inconsistencies, preserve claims and audit history, and raise only material choices or blockers. Coordinate at most two useful enabled Tokanban specialists in this active session; do not imply hosted/background execution."
emit_context "${checkpoint}
${context}"

# Persist only the run ID. Persona/teammate are project-specific and must be
# supplied explicitly so project switches cannot inherit stale attribution.
if [ "$event" = "SessionStart" ] && [ -n "${CLAUDE_ENV_FILE:-}" ] && [ -n "$session_id" ]; then
  printf 'export TOKANBAN_SESSION_ID=%q\n' "$session_id" >> "$CLAUDE_ENV_FILE"
fi
