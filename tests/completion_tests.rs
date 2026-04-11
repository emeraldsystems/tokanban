/// Tests for shell completion generation (Task #14)
///
/// Tests verify that completion scripts are generated for bash, zsh, and fish,
/// with proper error handling for invalid shells, and that all commands/flags
/// appear in the generated scripts.

mod common;

use std::process::Command;
use tokanban::cli::Cli;
use clap::CommandFactory;
use clap_complete::shells;

/// Helper: generate a completion script for the given shell into a String.
fn generate_completion<G: clap_complete::Generator>(gen: G) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(gen, &mut cmd, "tokanban", &mut buf);
    String::from_utf8(buf).expect("completion script should be valid UTF-8")
}

// ============================================================================
// BASH COMPLETION TESTS
// ============================================================================

#[test]
fn test_bash_completion_generated() {
    let script = generate_completion(shells::Bash);
    assert!(!script.is_empty(), "Bash completion script should not be empty");
    assert!(script.contains("tokanban"), "Bash script should reference binary name");
}

#[test]
fn test_bash_completion_syntax_valid() {
    let script = generate_completion(shells::Bash);
    assert!(script.contains("_tokanban"), "Bash script should define _tokanban function");
    assert!(script.contains("COMPREPLY"), "Bash script should use COMPREPLY");
    assert!(script.contains("complete"), "Bash script should register completion");
}

// ============================================================================
// ZSH COMPLETION TESTS
// ============================================================================

#[test]
fn test_zsh_completion_generated() {
    let script = generate_completion(shells::Zsh);
    assert!(!script.is_empty(), "Zsh completion script should not be empty");
    assert!(script.contains("tokanban"), "Zsh script should reference binary name");
}

#[test]
fn test_zsh_completion_syntax_valid() {
    let script = generate_completion(shells::Zsh);
    assert!(script.contains("#compdef"), "Zsh script should start with #compdef");
}

// ============================================================================
// FISH COMPLETION TESTS
// ============================================================================

#[test]
fn test_fish_completion_generated() {
    let script = generate_completion(shells::Fish);
    assert!(!script.is_empty(), "Fish completion script should not be empty");
    assert!(script.contains("tokanban"), "Fish script should reference binary name");
}

#[test]
fn test_fish_completion_syntax_valid() {
    let script = generate_completion(shells::Fish);
    assert!(
        script.contains("complete -c tokanban"),
        "Fish script should use 'complete -c tokanban'"
    );
}

// ============================================================================
// COMMAND NAMES IN COMPLETIONS
// ============================================================================

#[test]
fn test_all_command_names_in_bash() {
    let script = generate_completion(shells::Bash);
    for cmd in &[
        "auth", "workspace", "project", "task", "sprint",
        "comment", "member", "agent", "workflow", "import",
        "viz", "completion",
    ] {
        assert!(script.contains(cmd), "Bash completions should contain command '{cmd}'");
    }
}

#[test]
fn test_auth_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["login", "logout", "status"] {
        assert!(script.contains(sub), "Completions should contain auth subcommand '{sub}'");
    }
}

#[test]
fn test_task_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["create", "list", "view", "update", "search", "close", "reopen"] {
        assert!(script.contains(sub), "Completions should contain task subcommand '{sub}'");
    }
}

#[test]
fn test_project_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["create", "list", "view", "update", "archive"] {
        assert!(script.contains(sub), "Completions should contain project subcommand '{sub}'");
    }
}

#[test]
fn test_sprint_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["create", "list", "view", "start", "close"] {
        assert!(script.contains(sub), "Completions should contain sprint subcommand '{sub}'");
    }
}

#[test]
fn test_member_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["invite", "list", "update", "revoke"] {
        assert!(script.contains(sub), "Completions should contain member subcommand '{sub}'");
    }
}

#[test]
fn test_agent_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["create", "list", "view", "rotate", "revoke", "scopes"] {
        assert!(script.contains(sub), "Completions should contain agent subcommand '{sub}'");
    }
}

#[test]
fn test_viz_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["kanban", "burndown", "timeline"] {
        assert!(script.contains(sub), "Completions should contain viz subcommand '{sub}'");
    }
}

#[test]
fn test_import_subcommands_in_completions() {
    let script = generate_completion(shells::Bash);
    for sub in &["jira", "csv"] {
        assert!(script.contains(sub), "Completions should contain import subcommand '{sub}'");
    }
}

// ============================================================================
// GLOBAL FLAG NAMES IN COMPLETIONS
// ============================================================================

#[test]
fn test_global_flags_in_completions() {
    let script = generate_completion(shells::Bash);
    for flag in &[
        "--format", "--quiet", "--verbose", "--no-color",
        "--workspace", "--project", "--config", "--api-url",
    ] {
        assert!(script.contains(flag), "Completions should contain global flag '{flag}'");
    }
}

#[test]
fn test_help_and_version_flags() {
    let script = generate_completion(shells::Bash);
    assert!(script.contains("--help"), "Completions should contain --help");
    assert!(script.contains("--version"), "Completions should contain --version");
}

// ============================================================================
// CROSS-SHELL CONSISTENCY
// ============================================================================

#[test]
fn test_all_shells_contain_all_commands() {
    let commands = [
        "auth", "workspace", "project", "task", "sprint",
        "comment", "member", "agent", "workflow", "import",
        "viz", "completion",
    ];
    let bash = generate_completion(shells::Bash);
    let zsh = generate_completion(shells::Zsh);
    let fish = generate_completion(shells::Fish);

    for cmd in &commands {
        assert!(bash.contains(cmd), "Bash missing command '{cmd}'");
        assert!(zsh.contains(cmd), "Zsh missing command '{cmd}'");
        assert!(fish.contains(cmd), "Fish missing command '{cmd}'");
    }
}

#[test]
fn test_all_shells_contain_global_flags() {
    let flags = ["--format", "--quiet", "--verbose", "--no-color"];
    // Fish completions use `-l <name>` instead of `--name`, so check the bare name.
    let flag_names = ["format", "quiet", "verbose", "no-color"];
    let bash = generate_completion(shells::Bash);
    let zsh = generate_completion(shells::Zsh);
    let fish = generate_completion(shells::Fish);

    for flag in &flags {
        assert!(bash.contains(flag), "Bash missing flag '{flag}'");
        assert!(zsh.contains(flag), "Zsh missing flag '{flag}'");
    }
    for name in &flag_names {
        assert!(fish.contains(name), "Fish missing flag '{name}'");
    }
}

#[test]
fn test_shells_generate_different_output() {
    let bash = generate_completion(shells::Bash);
    let zsh = generate_completion(shells::Zsh);
    let fish = generate_completion(shells::Fish);

    assert_ne!(bash, zsh, "Bash and Zsh should produce different scripts");
    assert_ne!(bash, fish, "Bash and Fish should produce different scripts");
    assert_ne!(zsh, fish, "Zsh and Fish should produce different scripts");
}

// ============================================================================
// COMMAND-SPECIFIC FLAG COMPLETIONS
// ============================================================================

#[test]
fn test_task_create_flags_in_completions() {
    let script = generate_completion(shells::Bash);
    for flag in &["--priority", "--assignee", "--sprint", "--status"] {
        assert!(script.contains(flag), "Completions should contain task create flag '{flag}'");
    }
}

#[test]
fn test_member_invite_role_flag() {
    let script = generate_completion(shells::Bash);
    assert!(script.contains("--role"), "Completions should contain member invite --role flag");
}

#[test]
fn test_workflow_update_flags() {
    let script = generate_completion(shells::Bash);
    for flag in &["--add-status", "--remove-status", "--migrate"] {
        assert!(script.contains(flag), "Completions should contain workflow update flag '{flag}'");
    }
}

// ============================================================================
// ERROR HANDLING (library-level)
// ============================================================================

#[test]
fn test_unknown_shell_returns_error() {
    let result = tokanban::commands::completion::handle("powershell");
    assert!(result.is_err(), "Unknown shell should return an error");
}

#[test]
fn test_unknown_shell_error_message() {
    let result = tokanban::commands::completion::handle("powershell");
    let err = result.unwrap_err();
    let rendered = err.render();
    assert!(rendered.contains("powershell"), "Error message should mention the unknown shell name");
}

#[test]
fn test_shell_name_case_insensitive() {
    let result = tokanban::commands::completion::handle("BASH");
    assert!(result.is_ok(), "Shell name should be case-insensitive");
}

#[test]
fn test_shell_name_mixed_case() {
    let result = tokanban::commands::completion::handle("FiSh");
    assert!(result.is_ok(), "Mixed-case shell name should work");
}

// ============================================================================
// BINARY INTEGRATION TESTS
// ============================================================================

#[test]
fn test_completion_bash_via_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["completion", "bash"])
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty());
    assert!(stdout.contains("tokanban"));
}

#[test]
fn test_completion_zsh_via_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["completion", "zsh"])
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty());
    assert!(stdout.contains("#compdef"));
}

#[test]
fn test_completion_fish_via_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["completion", "fish"])
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty());
    assert!(stdout.contains("complete -c tokanban"));
}

#[test]
fn test_completion_invalid_shell_exits_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["completion", "powershell"])
        .output()
        .expect("failed to run binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown shell") || stderr.contains("powershell"));
}

#[test]
fn test_completion_missing_shell_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["completion"])
        .output()
        .expect("failed to run binary");
    assert!(!output.status.success());
}

#[test]
fn test_completion_command_appears_in_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["--help"])
        .output()
        .expect("failed to run binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("completion"));
}
