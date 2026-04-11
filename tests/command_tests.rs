/// Tests for command parsing with clap

mod common;

use clap::Parser;
use tokanban::cli::Cli;
use tokanban::commands;

// We can't test the top-level Cli struct directly (it's in main.rs, not lib.rs),
// but we can test each subcommand enum via clap's try_parse_from.
// We also run the binary to test global flags.

// ============================================================================
// AUTH COMMAND PARSING
// ============================================================================

#[test]
fn test_auth_login_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::auth::AuthCommand,
    }

    let cli = TestCli::try_parse_from(["test", "login"]).unwrap();
    assert!(matches!(cli.cmd, commands::auth::AuthCommand::Login));
}

#[test]
fn test_auth_logout_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::auth::AuthCommand,
    }

    let cli = TestCli::try_parse_from(["test", "logout"]).unwrap();
    assert!(matches!(cli.cmd, commands::auth::AuthCommand::Logout));
}

#[test]
fn test_auth_status_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::auth::AuthCommand,
    }

    let cli = TestCli::try_parse_from(["test", "status"]).unwrap();
    assert!(matches!(cli.cmd, commands::auth::AuthCommand::Status));
}

// ============================================================================
// TASK COMMAND PARSING
// ============================================================================

#[test]
fn test_task_list_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "list",
        "--project", "TEST",
        "--status", "In Progress",
    ])
    .unwrap();

    match cli.cmd {
        commands::task::TaskCommand::List { project, status, .. } => {
            assert_eq!(project, Some("TEST".to_string()));
            assert_eq!(status, Some("In Progress".to_string()));
        }
        _ => panic!("expected TaskCommand::List"),
    }
}

#[test]
fn test_task_create_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "create", "Fix the bug",
        "--project", "PLAT",
        "--priority", "high",
        "--assignee", "sven",
    ])
    .unwrap();

    match cli.cmd {
        commands::task::TaskCommand::Create {
            title,
            project,
            priority,
            assignee,
            ..
        } => {
            assert_eq!(title, "Fix the bug");
            assert_eq!(project, Some("PLAT".to_string()));
            assert_eq!(priority, Some("high".to_string()));
            assert_eq!(assignee, Some("sven".to_string()));
        }
        _ => panic!("expected TaskCommand::Create"),
    }
}

#[test]
fn test_task_view_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli = TestCli::try_parse_from(["test", "view", "PLAT-42"]).unwrap();
    match cli.cmd {
        commands::task::TaskCommand::View { key } => {
            assert_eq!(key, "PLAT-42");
        }
        _ => panic!("expected TaskCommand::View"),
    }
}

#[test]
fn test_task_update_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "update", "PLAT-42",
        "--status", "Done",
        "--priority", "low",
    ])
    .unwrap();

    match cli.cmd {
        commands::task::TaskCommand::Update {
            key,
            status,
            priority,
            ..
        } => {
            assert_eq!(key, "PLAT-42");
            assert_eq!(status, Some("Done".to_string()));
            assert_eq!(priority, Some("low".to_string()));
        }
        _ => panic!("expected TaskCommand::Update"),
    }
}

#[test]
fn test_task_close_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli =
        TestCli::try_parse_from(["test", "close", "PLAT-42", "--reason", "Duplicate"]).unwrap();

    match cli.cmd {
        commands::task::TaskCommand::Close { key, reason } => {
            assert_eq!(key, "PLAT-42");
            assert_eq!(reason, Some("Duplicate".to_string()));
        }
        _ => panic!("expected TaskCommand::Close"),
    }
}

#[test]
fn test_task_search_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "search", "auth bug",
        "--project", "PLAT",
        "--limit", "10",
    ])
    .unwrap();

    match cli.cmd {
        commands::task::TaskCommand::Search {
            query,
            project,
            limit,
        } => {
            assert_eq!(query, "auth bug");
            assert_eq!(project, Some("PLAT".to_string()));
            assert_eq!(limit, 10);
        }
        _ => panic!("expected TaskCommand::Search"),
    }
}

// ============================================================================
// PROJECT COMMAND PARSING
// ============================================================================

#[test]
fn test_project_list_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::project::ProjectCommand,
    }

    let cli = TestCli::try_parse_from(["test", "list"]).unwrap();
    assert!(matches!(
        cli.cmd,
        commands::project::ProjectCommand::List { .. }
    ));
}

#[test]
fn test_project_create_requires_name_and_prefix() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::project::ProjectCommand,
    }

    // Missing key_prefix should fail
    let result = TestCli::try_parse_from(["test", "create", "My Project"]);
    assert!(result.is_err());

    // With both required args should succeed
    let cli = TestCli::try_parse_from([
        "test", "create", "My Project",
        "--key-prefix", "PROJ",
    ])
    .unwrap();
    match cli.cmd {
        commands::project::ProjectCommand::Create { name, key_prefix } => {
            assert_eq!(name, "My Project");
            assert_eq!(key_prefix, "PROJ");
        }
        _ => panic!("expected ProjectCommand::Create"),
    }
}

// ============================================================================
// WORKSPACE COMMAND PARSING
// ============================================================================

#[test]
fn test_workspace_set_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::workspace::WorkspaceCommand,
    }

    let cli = TestCli::try_parse_from(["test", "set", "my-workspace"]).unwrap();
    match cli.cmd {
        commands::workspace::WorkspaceCommand::Set { slug } => {
            assert_eq!(slug, "my-workspace");
        }
        _ => panic!("expected WorkspaceCommand::Set"),
    }
}

// ============================================================================
// SPRINT COMMAND PARSING
// ============================================================================

#[test]
fn test_sprint_create_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::sprint::SprintCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "create",
        "--project", "PLAT",
        "--name", "Sprint 13",
        "--start", "2026-04-15",
        "--end", "2026-04-29",
    ])
    .unwrap();

    match cli.cmd {
        commands::sprint::SprintCommand::Create {
            project,
            name,
            start,
            end,
        } => {
            assert_eq!(project, Some("PLAT".to_string()));
            assert_eq!(name, "Sprint 13");
            assert_eq!(start, "2026-04-15");
            assert_eq!(end, "2026-04-29");
        }
        _ => panic!("expected SprintCommand::Create"),
    }
}

// ============================================================================
// COMMENT COMMAND PARSING
// ============================================================================

#[test]
fn test_comment_add_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::comment::CommentCommand,
    }

    let cli = TestCli::try_parse_from(["test", "add", "PLAT-42", "Looks good to me"]).unwrap();
    match cli.cmd {
        commands::comment::CommentCommand::Add { task_key, body } => {
            assert_eq!(task_key, "PLAT-42");
            assert_eq!(body, Some("Looks good to me".to_string()));
        }
        _ => panic!("expected CommentCommand::Add"),
    }
}

// ============================================================================
// MEMBER COMMAND PARSING
// ============================================================================

#[test]
fn test_member_invite_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::member::MemberCommand,
    }

    let cli = TestCli::try_parse_from([
        "test", "invite", "alice@example.com",
        "--role", "editor",
    ])
    .unwrap();

    match cli.cmd {
        commands::member::MemberCommand::Invite { email, role, .. } => {
            assert_eq!(email, "alice@example.com");
            assert_eq!(role, "editor");
        }
        _ => panic!("expected MemberCommand::Invite"),
    }
}

// ============================================================================
// REQUIRED ARGUMENTS TESTS
// ============================================================================

#[test]
fn test_task_create_requires_title() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let result = TestCli::try_parse_from(["test", "create"]);
    assert!(result.is_err());
}

#[test]
fn test_task_view_requires_key() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::task::TaskCommand,
    }

    let result = TestCli::try_parse_from(["test", "view"]);
    assert!(result.is_err());
}

// ============================================================================
// GLOBAL FLAGS VIA BINARY
// ============================================================================

#[test]
fn test_binary_help_flag() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .arg("--help")
        .output()
        .expect("failed to run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tokanban"));
    assert!(stdout.contains("--format"));
    assert!(stdout.contains("--quiet"));
    assert!(stdout.contains("--no-color"));
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--api-url"));
}

#[test]
fn test_binary_version_flag() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .arg("--version")
        .output()
        .expect("failed to run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tokanban"));
    assert!(stdout.contains("0.1.2"));
}

#[test]
fn test_global_workspace_and_project_overrides_apply_to_config() {
    let cli = Cli::parse_from([
        "tokanban",
        "--workspace", "ws_123",
        "--project", "PLAT",
        "--api-url", "https://api.example.com",
        "--no-color",
        "task", "list",
    ]);

    let mut config = tokanban::config::AppConfig::default();
    cli.apply_overrides(&mut config);

    assert_eq!(config.defaults.workspace.as_deref(), Some("ws_123"));
    assert_eq!(config.defaults.project.as_deref(), Some("PLAT"));
    assert_eq!(config.api.url, "https://api.example.com");
    assert!(config.ui.no_color);
}

#[test]
fn test_binary_subcommand_help() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["task", "--help"])
        .output()
        .expect("failed to run binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("update"));
    assert!(stdout.contains("search"));
    assert!(stdout.contains("close"));
    assert!(stdout.contains("reopen"));
}

#[test]
fn test_binary_unknown_command_exits_error() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .arg("nonexistent")
        .output()
        .expect("failed to run binary");

    assert!(!output.status.success());
}
