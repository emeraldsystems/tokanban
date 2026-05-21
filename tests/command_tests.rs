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
// MEMORY COMMAND PARSING
// ============================================================================

#[test]
fn test_memory_score_parses_with_inline_json() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "score",
        "--input",
        "{\"kind\":\"fact\",\"content\":\"Verified deploy rule.\"}",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Score {
            input,
            input_file,
        }) => {
            assert_eq!(
                input,
                Some("{\"kind\":\"fact\",\"content\":\"Verified deploy rule.\"}".to_string())
            );
            assert!(input_file.is_none());
        }
        other => panic!("expected Command::Memory::Score, got {other:?}"),
    }
}

#[test]
fn test_memory_score_parses_with_input_file() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "score",
        "--input-file",
        "candidate.json",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Score {
            input,
            input_file,
        }) => {
            assert!(input.is_none());
            assert_eq!(input_file.unwrap().to_string_lossy(), "candidate.json");
        }
        other => panic!("expected Command::Memory::Score, got {other:?}"),
    }
}

#[test]
fn test_memory_candidate_add_parses_with_scope() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "candidate",
        "add",
        "--input",
        "{\"kind\":\"decision\",\"content\":\"Still under review.\"}",
        "--project-id",
        "proj-123",
        "--working-directory",
        "/tmp/work",
        "--task-id",
        "TKB-74",
        "--module",
        "memory-gate",
        "--note",
        "hold until session end",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Candidate(
            commands::memory::CandidateCommand::Add {
                input,
                input_file,
                project_id,
                working_directory,
                task_id,
                module,
                note,
            },
        )) => {
            assert_eq!(
                input,
                Some("{\"kind\":\"decision\",\"content\":\"Still under review.\"}".to_string())
            );
            assert!(input_file.is_none());
            assert_eq!(project_id.as_deref(), Some("proj-123"));
            assert_eq!(working_directory.as_deref(), Some("/tmp/work"));
            assert_eq!(task_id.as_deref(), Some("TKB-74"));
            assert_eq!(module.as_deref(), Some("memory-gate"));
            assert_eq!(note.as_deref(), Some("hold until session end"));
        }
        other => panic!("expected Command::Memory::Candidate::Add, got {other:?}"),
    }
}

#[test]
fn test_memory_candidate_list_parses_with_filters() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "candidate",
        "list",
        "--project-id",
        "proj-123",
        "--module",
        "memory-gate",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Candidate(
            commands::memory::CandidateCommand::List {
                project_id,
                working_directory,
                task_id,
                module,
            },
        )) => {
            assert_eq!(project_id.as_deref(), Some("proj-123"));
            assert!(working_directory.is_none());
            assert!(task_id.is_none());
            assert_eq!(module.as_deref(), Some("memory-gate"));
        }
        other => panic!("expected Command::Memory::Candidate::List, got {other:?}"),
    }
}

#[test]
fn test_memory_candidate_clear_parses_with_all() {
    let cli = Cli::try_parse_from(["tokanban", "memory", "candidate", "clear", "--all"]).unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Candidate(
            commands::memory::CandidateCommand::Clear {
                all,
                ids,
                project_id,
                working_directory,
                task_id,
                module,
            },
        )) => {
            assert!(all);
            assert!(ids.is_empty());
            assert!(project_id.is_none());
            assert!(working_directory.is_none());
            assert!(task_id.is_none());
            assert!(module.is_none());
        }
        other => panic!("expected Command::Memory::Candidate::Clear, got {other:?}"),
    }
}

#[test]
fn test_memory_candidate_review_parses_with_filters() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "candidate",
        "review",
        "--project-id",
        "proj-123",
        "--working-directory",
        "/tmp/work",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Candidate(
            commands::memory::CandidateCommand::Review {
                project_id,
                working_directory,
                task_id,
                module,
            },
        )) => {
            assert_eq!(project_id.as_deref(), Some("proj-123"));
            assert_eq!(working_directory.as_deref(), Some("/tmp/work"));
            assert!(task_id.is_none());
            assert!(module.is_none());
        }
        other => panic!("expected Command::Memory::Candidate::Review, got {other:?}"),
    }
}

#[test]
fn test_memory_candidate_clear_parses_with_ids() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "memory",
        "candidate",
        "clear",
        "--id",
        "cand_123",
        "--id",
        "cand_456",
    ])
    .unwrap();

    match cli.command {
        tokanban::cli::Command::Memory(commands::memory::MemoryCommand::Candidate(
            commands::memory::CandidateCommand::Clear {
                all,
                ids,
                project_id,
                working_directory,
                task_id,
                module,
            },
        )) => {
            assert!(!all);
            assert_eq!(ids, vec!["cand_123".to_string(), "cand_456".to_string()]);
            assert!(project_id.is_none());
            assert!(working_directory.is_none());
            assert!(task_id.is_none());
            assert!(module.is_none());
        }
        other => panic!("expected Command::Memory::Candidate::Clear, got {other:?}"),
    }
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
        "test",
        "list",
        "--project",
        "TEST",
        "--status",
        "In Progress",
    ])
    .unwrap();

    match cli.cmd {
        commands::task::TaskCommand::List {
            project, status, ..
        } => {
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
        "test",
        "create",
        "Fix the bug",
        "--project",
        "PLAT",
        "--priority",
        "high",
        "--assignee",
        "bob",
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
            assert_eq!(assignee, Some("bob".to_string()));
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
        "test",
        "update",
        "PLAT-42",
        "--status",
        "Done",
        "--priority",
        "low",
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
        "test",
        "search",
        "auth bug",
        "--project",
        "PLAT",
        "--limit",
        "10",
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
// ENTITY COMMAND PARSING
// ============================================================================

#[test]
fn test_entity_create_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::entity::EntityCommand,
    }

    let cli = TestCli::try_parse_from([
        "test",
        "create",
        "DEC",
        "Use ProjectDO storage",
        "--project",
        "TKB",
        "--content",
        "Keep project knowledge strongly consistent.",
        "--memory-ref",
        "fact_123",
        "--related",
        "TKB-82",
    ])
    .unwrap();

    match cli.cmd {
        commands::entity::EntityCommand::Create {
            kind,
            title,
            project,
            content,
            memory_refs,
            related_keys,
            ..
        } => {
            assert_eq!(kind, "DEC");
            assert_eq!(title, "Use ProjectDO storage");
            assert_eq!(project, Some("TKB".to_string()));
            assert_eq!(
                content.as_deref(),
                Some("Keep project knowledge strongly consistent.")
            );
            assert_eq!(memory_refs, vec!["fact_123".to_string()]);
            assert_eq!(related_keys, vec!["TKB-82".to_string()]);
        }
        _ => panic!("expected EntityCommand::Create"),
    }
}

#[test]
fn test_entity_list_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::entity::EntityCommand,
    }

    let cli = TestCli::try_parse_from([
        "test",
        "list",
        "--project",
        "TKB",
        "--kind",
        "REQ",
        "--status",
        "active",
        "--query",
        "oauth",
        "--limit",
        "10",
    ])
    .unwrap();

    match cli.cmd {
        commands::entity::EntityCommand::List {
            project,
            kind,
            status,
            query,
            limit,
            ..
        } => {
            assert_eq!(project, Some("TKB".to_string()));
            assert_eq!(kind, Some("REQ".to_string()));
            assert_eq!(status, Some("active".to_string()));
            assert_eq!(query, Some("oauth".to_string()));
            assert_eq!(limit, 10);
        }
        _ => panic!("expected EntityCommand::List"),
    }
}

#[test]
fn test_entity_update_parses() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::entity::EntityCommand,
    }

    let cli = TestCli::try_parse_from([
        "test",
        "update",
        "TKB-FND-1",
        "--status",
        "archived",
        "--clear-memory-refs",
    ])
    .unwrap();

    match cli.cmd {
        commands::entity::EntityCommand::Update {
            key,
            status,
            clear_memory_refs,
            ..
        } => {
            assert_eq!(key, "TKB-FND-1");
            assert_eq!(status, Some("archived".to_string()));
            assert!(clear_memory_refs);
        }
        _ => panic!("expected EntityCommand::Update"),
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
    let cli =
        TestCli::try_parse_from(["test", "create", "My Project", "--key-prefix", "PROJ"]).unwrap();
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
        "test",
        "create",
        "--project",
        "PLAT",
        "--name",
        "Sprint 13",
        "--start",
        "2026-04-15",
        "--end",
        "2026-04-29",
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

    let cli = TestCli::try_parse_from(["test", "invite", "alice@example.com", "--role", "editor"])
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
// AGENT COMMAND PARSING
// ============================================================================

#[test]
fn test_agent_create_parses_memory_scopes() {
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: commands::agent::AgentCommand,
    }

    let cli = TestCli::try_parse_from([
        "test",
        "create",
        "Memory Claude",
        "--type",
        "claude-code",
        "--scopes",
        "tasks:read,tasks:write,projects:read,memory:read,memory:write",
    ])
    .unwrap();

    match cli.cmd {
        commands::agent::AgentCommand::Create {
            name,
            r#type,
            scopes,
            ..
        } => {
            assert_eq!(name, "Memory Claude");
            assert_eq!(r#type, "claude-code");
            assert_eq!(
                scopes,
                "tasks:read,tasks:write,projects:read,memory:read,memory:write"
            );
        }
        _ => panic!("expected AgentCommand::Create"),
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
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_global_workspace_and_project_overrides_apply_to_config() {
    let cli = Cli::parse_from([
        "tokanban",
        "--workspace",
        "ws_123",
        "--project",
        "PLAT",
        "--api-url",
        "https://api.example.com",
        "--no-color",
        "task",
        "list",
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
