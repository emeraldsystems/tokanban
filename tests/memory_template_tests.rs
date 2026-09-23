use std::fs;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};

fn template(path: &str) -> String {
    let full_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&full_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", full_path.display()))
}

#[test]
fn codex_memory_block_covers_review_then_session_end_flow() {
    let body = template("templates/AGENTS.md.memory-block.md");

    assert!(body.contains("session_start"));
    assert!(body.contains("memory_relevant_now"));
    assert!(body.contains("tokanban memory candidate review"));
    assert!(body.contains("session_end_contract.learned"));
    assert!(body.contains("session_end("));
    assert!(body.contains("clear_after_session_end_ids"));
}

#[test]
fn claude_memory_block_covers_review_then_session_end_flow() {
    let body = template("templates/CLAUDE.md.memory-block.md");

    assert!(body.contains("session_start"));
    assert!(body.contains("memory_relevant_now"));
    assert!(body.contains("tokanban memory candidate review"));
    assert!(body.contains("session_end_contract.decisions_made"));
    assert!(body.contains("session_end"));
    assert!(body.contains("clear_after_session_end_ids"));
}

#[test]
fn plugin_hooks_register_usage_reporter() {
    let body = template("hooks/hooks.json");

    assert!(body.contains("\"Stop\""));
    assert!(body.contains("\"SessionEnd\""));
    assert!(body.contains("tokanban session report-usage"));
}

#[test]
#[cfg(unix)]
fn persona_hook_retries_after_transient_session_start_failure() {
    if Command::new("jq").arg("--version").output().is_err() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let mock_cli = bin_dir.join("tokanban");
    fs::write(
        &mock_cli,
        r#"#!/bin/bash
if [ "${1:-}" = "persona" ] && [ "${2:-}" = "--help" ]; then
  exit 0
fi
if [ ! -f "$HOOK_TEST_STATE" ]; then
  touch "$HOOK_TEST_STATE"
  exit 1
fi
printf '%s\n' '{"active":true,"persona":{"key":"pm"},"tasks":[]}'
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&mock_cli).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mock_cli, permissions).unwrap();

    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/tokanban/hooks/persona-context.sh");
    let env_file = temp.path().join("session-env");
    let state_file = temp.path().join("attempted");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let run_hook = |input: &str, warned: bool| {
        let mut child = Command::new("bash")
            .arg(&script)
            .env("PATH", &path)
            .env("CLAUDE_ENV_FILE", &env_file)
            .env("HOOK_TEST_STATE", &state_file)
            .env(
                "TOKANBAN_PERSONA_CONTEXT_WARNED",
                if warned { "1" } else { "0" },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };

    let first = run_hook(
        r#"{"hook_event_name":"SessionStart","session_id":"session-1"}"#,
        false,
    );
    assert!(first.status.success());
    assert!(String::from_utf8_lossy(&first.stdout).contains("CONTEXT UNAVAILABLE"));
    let persisted = fs::read_to_string(&env_file).unwrap();
    assert!(persisted.contains("TOKANBAN_PERSONA_CONTEXT_WARNED=1"));
    assert!(!persisted.contains("TOKANBAN_PERSONA_CONTEXT_UNSUPPORTED"));

    let second = run_hook(
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"session-1"}"#,
        true,
    );
    assert!(second.status.success());
    let recovered = String::from_utf8_lossy(&second.stdout);
    assert!(recovered.contains("TOKANBAN PM CHECKPOINT"));
    assert!(recovered.contains(r#""active":true"#));

    let stopped = run_hook(
        r#"{"hook_event_name":"SubagentStop","session_id":"session-1","agent_id":"agent-1"}"#,
        true,
    );
    assert!(stopped.status.success());
    let stopped: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(
        stopped["hookSpecificOutput"]["hookEventName"],
        "SubagentStop"
    );
    assert!(stopped["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("TOKANBAN PM CHECKPOINT"));
}
