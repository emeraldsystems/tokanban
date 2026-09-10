/// Tests for `tokanban repo` — local Git discovery and repository-memory
/// binding (TKB-129). Git fixtures use the real `git` binary in tempdirs;
/// "missing git" is simulated with an injectable `GitRunner` instead of
/// touching `PATH`, so these tests are safe to run in parallel.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::Parser;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{body_json, method, path as wpath};
use wiremock::{Mock, MockServer, ResponseTemplate};

use tokanban::cli::{Cli, Command as CliCommand};
use tokanban::commands::repo::ScopeCommand;
use tokanban::commands::repo::{
    self, filter_alias_candidates, normalize_remote, GitOutput, GitReport, GitRunner, RepoCommand,
    RepositoryRecord, SystemGitRunner,
};
use tokanban::config::AppConfig;
use tokanban::ctx::Ctx;
use tokanban::error::CliError;
use tokanban::format::OutputFormat;

// ---------------------------------------------------------------------------
// Git fixture helpers (real git, run in tempdirs)
// ---------------------------------------------------------------------------

fn run_git(dir: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tokanban Test")
        .env("GIT_AUTHOR_EMAIL", "test@tokanban.invalid")
        .env("GIT_COMMITTER_NAME", "Tokanban Test")
        .env("GIT_COMMITTER_EMAIL", "test@tokanban.invalid")
        .output()
        .expect("failed to spawn git — required for repo_tests fixtures");
    assert!(
        output.status.success(),
        "git {:?} failed in {:?}: {}",
        args,
        dir,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to spawn git");
    assert!(
        output.status.success(),
        "git {:?} failed in {:?}",
        args,
        dir
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Initializes a repo at `dir` with a single commit on branch `main`, and
/// returns the commit SHA.
fn init_repo_with_commit(dir: &Path) -> String {
    fs::create_dir_all(dir).unwrap();
    run_git(dir, &["init", "-q"]);
    run_git(dir, &["checkout", "-q", "-b", "main"]);
    fs::write(dir.join("README.md"), "hello\n").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", "init"]);
    git_stdout(dir, &["rev-parse", "HEAD"])
}

struct MissingGitRunner;

impl GitRunner for MissingGitRunner {
    fn run(&self, _cwd: &Path, _args: &[&str]) -> std::io::Result<GitOutput> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "git not found",
        ))
    }
}

fn as_repository(report: GitReport) -> repo::GitCheckoutInfo {
    match report {
        GitReport::Repository(info) => info,
        other => panic!("expected GitReport::Repository, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Offline Git discovery
// ---------------------------------------------------------------------------

#[test]
fn detects_main_checkout_with_branch_and_normalizes_remote() {
    let temp = tempdir().unwrap();
    let repo_dir = temp.path().join("repo");
    init_repo_with_commit(&repo_dir);
    run_git(
        &repo_dir,
        &[
            "remote",
            "add",
            "origin",
            "https://user:secret@github.com/acme/widgets.git?token=abc#frag",
        ],
    );

    let info = as_repository(repo::inspect_git(&repo_dir, &SystemGitRunner));

    assert_eq!(info.checkout_kind, "main");
    assert_eq!(info.branch.as_deref(), Some("main"));
    assert!(!info.detached);
    assert_eq!(info.remote_name.as_deref(), Some("origin"));
    assert_eq!(
        info.remote_url.as_deref(),
        Some("https://github.com/acme/widgets")
    );
    assert!(!info.remote_url.as_deref().unwrap().contains("secret"));
    assert!(info.head_commit.is_some());
    assert_eq!(
        PathBuf::from(&info.repo_root),
        fs::canonicalize(&repo_dir).unwrap()
    );
}

/// Linked worktrees have a `.git` *file* (not directory) pointing at the
/// private gitdir under the main repo's `.git/worktrees/<name>`. This is
/// resolved entirely via `git rev-parse --git-dir`/`--git-common-dir`; the
/// implementation never reads that file itself.
#[test]
fn detects_linked_worktree_kind_distinct_from_main() {
    let temp = tempdir().unwrap();
    let main_dir = temp.path().join("main");
    init_repo_with_commit(&main_dir);
    let worktree_dir = temp.path().join("wt");
    run_git(
        &main_dir,
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            worktree_dir.to_str().unwrap(),
        ],
    );

    let main_info = as_repository(repo::inspect_git(&main_dir, &SystemGitRunner));
    let wt_info = as_repository(repo::inspect_git(&worktree_dir, &SystemGitRunner));

    assert_eq!(main_info.checkout_kind, "main");
    assert_eq!(wt_info.checkout_kind, "worktree");
    assert_eq!(wt_info.branch.as_deref(), Some("feature"));
    assert_eq!(main_info.common_git_dir, wt_info.common_git_dir);
    assert_ne!(main_info.git_dir, wt_info.git_dir);
}

#[test]
fn detects_detached_head() {
    let temp = tempdir().unwrap();
    let dir = temp.path().join("repo");
    let sha = init_repo_with_commit(&dir);
    run_git(&dir, &["checkout", "-q", &sha]);

    let info = as_repository(repo::inspect_git(&dir, &SystemGitRunner));

    assert!(info.detached);
    assert!(info.branch.is_none());
    assert_eq!(info.head_commit.as_deref(), Some(sha.as_str()));
}

#[test]
fn reports_not_a_repository_for_plain_directory() {
    let temp = tempdir().unwrap();
    let report = repo::inspect_git(temp.path(), &SystemGitRunner);
    assert!(matches!(report, GitReport::NotARepository { .. }));
}

#[test]
fn reports_git_not_available_when_binary_missing() {
    let temp = tempdir().unwrap();
    let report = repo::inspect_git(temp.path(), &MissingGitRunner);
    assert!(matches!(report, GitReport::NotAvailable { .. }));
}

/// `repo inspect` (offline) must work with no network and no config/token at
/// all — this test never starts a mock server.
#[test]
fn offline_inspect_handles_missing_and_present_repositories_without_network() {
    let temp = tempdir().unwrap();
    let ok =
        repo::handle_inspect_offline(Some(temp.path().to_path_buf()), OutputFormat::Json, true);
    assert!(ok.is_ok());

    let repo_dir = temp.path().join("repo");
    init_repo_with_commit(&repo_dir);
    let ok = repo::handle_inspect_offline(Some(repo_dir), OutputFormat::Json, true);
    assert!(ok.is_ok());
}

#[test]
fn resolve_target_dir_rejects_nonexistent_path_via_bind() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("does-not-exist");
    let server_free_config = AppConfig::default();
    let ctx = Ctx::new(
        server_free_config,
        None,
        false,
        false,
        OutputFormat::Json,
        false,
    )
    .unwrap();

    let result = tokio_test::block_on(repo::handle(
        &RepoCommand::Bind {
            repository_id: "repo-1".into(),
            path: Some(missing),
            branch: None,
            kind: None,
            expected_revision: None,
        },
        &ctx,
    ));
    assert!(matches!(result, Err(CliError::InvalidInput(_))));
}

// ---------------------------------------------------------------------------
// normalize_remote (credential/query/fragment stripping)
// ---------------------------------------------------------------------------

#[test]
fn normalize_remote_covers_http_ssh_and_local_forms() {
    assert_eq!(
        normalize_remote("https://user:secret@github.com/acme/widgets.git?token=abc#frag"),
        "https://github.com/acme/widgets"
    );
    assert_eq!(
        normalize_remote("git@github.com:acme/widgets.git"),
        "ssh://github.com/acme/widgets"
    );
    assert_eq!(
        normalize_remote("ssh://deploy@git.example.com:2222/acme/widgets.git"),
        "ssh://git.example.com:2222/acme/widgets"
    );
    assert_eq!(
        normalize_remote("/local/path/to/repo.git"),
        "/local/path/to/repo.git"
    );
}

// ---------------------------------------------------------------------------
// Fork / same-name identities are never merged
// ---------------------------------------------------------------------------

fn repo_record(id: &str, name: &str, remote: &str) -> RepositoryRecord {
    serde_json::from_value(json!({
        "id": id,
        "name": name,
        "canonical_remote": remote,
        "revision": 1
    }))
    .unwrap()
}

#[test]
fn filter_alias_candidates_keeps_same_name_forks_as_distinct_entries() {
    let candidates = vec![
        repo_record("repo-a", "widgets", "https://github.com/acme/widgets"),
        repo_record("repo-b", "widgets", "https://github.com/forked/widgets.git"),
    ];

    let matches = filter_alias_candidates("widgets", None, candidates);

    assert_eq!(matches.len(), 2);
    let ids: Vec<&str> = matches.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"repo-a"));
    assert!(ids.contains(&"repo-b"));
}

#[test]
fn filter_alias_candidates_matches_by_normalized_remote_too() {
    let candidates = vec![repo_record(
        "repo-a",
        "unrelated-name",
        "https://user:secret@github.com/acme/widgets.git",
    )];

    let matches = filter_alias_candidates(
        "some-other-dir",
        Some("https://github.com/acme/widgets"),
        candidates,
    );

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "repo-a");
}

// ---------------------------------------------------------------------------
// REST wiring (wiremock)
// ---------------------------------------------------------------------------

fn test_ctx(server: &MockServer) -> Ctx {
    let mut config = AppConfig::default();
    config.api.url = server.uri();
    config.auth.access_token = Some("tk_should_never_leak".into());
    Ctx::new(config, None, false, false, OutputFormat::Json, false).unwrap()
}

fn scope_plan() -> serde_json::Value {
    json!({
        "request": {"repository_id":"repo-1","memory_ids":["memory-1"],"memory_scope":"branch","branch":"main"},
        "response": {"repository_id":"repo-1","namespace":"production","count":1,"preview_fingerprint":"a".repeat(64),
          "items":[{"memory_id":"memory-1","content":"Reviewed source fact","type":"fact","original_working_directory":"/old-checkout",
            "before":{"repository_id":null,"memory_scope":"legacy","branch":null,"experiment":null,"scope_revision":0},
            "after":{"repository_id":"repo-1","memory_scope":"branch","branch":"main","experiment":null,"scope_revision":1}}]}
    })
}

#[tokio::test]
async fn scope_preview_only_calls_the_read_only_preview_endpoint() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let plan = scope_plan();
    Mock::given(method("POST"))
        .and(wpath("/v1/memory/scopes/preview"))
        .and(body_json(plan["request"].clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan["response"].clone()))
        .expect(1)
        .mount(&server)
        .await;
    repo::handle(
        &RepoCommand::Scope(ScopeCommand::Preview {
            repository_id: "repo-1".into(),
            memory_ids: vec!["memory-1".into()],
            scope: Some("branch".into()),
            branch: Some("main".into()),
            experiment: None,
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn scope_apply_transmits_the_saved_selection_and_fingerprint_without_previewing() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let temp = tempdir().unwrap();
    let plan = scope_plan();
    let file = temp.path().join("plan.json");
    fs::write(&file, plan.to_string()).unwrap();
    let mut expected = plan["request"].clone();
    expected["preview_fingerprint"] = plan["response"]["preview_fingerprint"].clone();
    Mock::given(method("POST")).and(wpath("/v1/memory/scopes/apply")).and(body_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"operation-1","repository_id":"repo-1","kind":"associate","created_at":1788999000,"count":1,"replayed":false})))
        .expect(1).mount(&server).await;
    repo::handle(
        &RepoCommand::Scope(ScopeCommand::Apply { plan: file }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn inconsistent_or_malformed_plan_is_rejected_without_a_request() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let temp = tempdir().unwrap();
    let file = temp.path().join("plan.json");
    for corrupt in [
        json!({"secret":"tk_should_never_leak"}),
        {
            let mut plan = scope_plan();
            plan["request"]["memory_ids"] = json!(["different"]);
            plan
        },
        {
            let mut plan = scope_plan();
            plan["response"]["preview_fingerprint"] = json!("invalid");
            plan
        },
    ] {
        fs::write(&file, corrupt.to_string()).unwrap();
        let error = repo::handle(
            &RepoCommand::Scope(ScopeCommand::Apply { plan: file.clone() }),
            &ctx,
        )
        .await
        .unwrap_err();
        assert!(!error.to_string().contains("tk_should_never_leak"));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn scope_restore_is_explicit_and_preserves_conflict_guidance() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    Mock::given(method("POST")).and(wpath("/v1/memory/scopes/operation-1/restore")).and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"error":{"code":"MEMORY_SCOPE_CHANGED","message":"Memory changed; review current scope before retrying."}})))
        .expect(1).mount(&server).await;
    let error = repo::handle(
        &RepoCommand::Scope(ScopeCommand::Restore {
            operation_id: "operation-1".into(),
        }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("Memory changed"));
}

#[test]
fn revision_arguments_are_safe_integers_and_unbind_requires_positive_revision() {
    for invalid in ["-1", "abc", "9007199254740992"] {
        assert!(Cli::try_parse_from([
            "tokanban",
            "repo",
            "bind",
            "repo-1",
            "--expected-revision",
            invalid
        ])
        .is_err());
    }
    assert!(Cli::try_parse_from([
        "tokanban",
        "repo",
        "bind",
        "repo-1",
        "--expected-revision",
        "0"
    ])
    .is_ok());
    assert!(
        Cli::try_parse_from(["tokanban", "repo", "unbind", "--expected-revision", "0"]).is_err()
    );
}

#[test]
fn malformed_and_scp_remotes_do_not_expose_credentials_or_query_secrets() {
    assert_eq!(
        normalize_remote("https://user:secret@[broken/repo"),
        "<unrecognized remote>"
    );
    assert_eq!(
        normalize_remote("git@github.com:acme/widgets.git?token=secret#private"),
        "ssh://github.com/acme/widgets"
    );
}

#[tokio::test]
async fn create_sends_normalized_remote_and_resolved_project_id() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);

    Mock::given(method("GET"))
        .and(wpath("/v1/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id": "proj-1", "key": "PLAT", "name": "Platform", "key_prefix": "PLAT"}]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(wpath("/v1/memory/repositories"))
        .and(body_json(json!({
            "name": "widgets",
            "canonical_remote": "https://github.com/acme/widgets",
            "project_id": "proj-1"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "repo-1",
            "name": "widgets",
            "canonical_remote": "https://github.com/acme/widgets",
            "project_id": "proj-1",
            "revision": 1, "created_at": 1788999000, "updated_at": 1788999000
        })))
        .expect(1)
        .mount(&server)
        .await;

    repo::handle(
        &RepoCommand::Create {
            name: "widgets".into(),
            remote: Some("https://user:secret@github.com/acme/widgets.git?token=abc".into()),
            project: Some("PLAT".into()),
        },
        &ctx,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn bind_conflict_surfaces_binding_changed_error_code() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let temp = tempdir().unwrap();
    init_repo_with_commit(temp.path());

    Mock::given(method("POST"))
        .and(wpath("/v1/memory/checkouts"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "error": {
                "code": "BINDING_CHANGED",
                "message": "Binding changed since last read",
                "details": null,
                "hint": "Re-run `tokanban repo inspect --binding` for the latest revision."
            }
        })))
        .mount(&server)
        .await;

    let result = repo::handle(
        &RepoCommand::Bind {
            repository_id: "repo-1".into(),
            path: Some(temp.path().to_path_buf()),
            branch: None,
            kind: None,
            expected_revision: Some(3),
        },
        &ctx,
    )
    .await;

    match result {
        Err(CliError::Api { code, .. }) => assert_eq!(code, "BINDING_CHANGED"),
        other => panic!("expected Api error with BINDING_CHANGED, got {other:?}"),
    }
}

#[tokio::test]
async fn unbind_sends_working_directory_and_expected_revision_in_delete_body() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let temp = tempdir().unwrap();
    let dir = fs::canonicalize(temp.path()).unwrap();

    Mock::given(method("DELETE"))
        .and(wpath("/v1/memory/checkouts"))
        .and(body_json(json!({
            "working_directory": dir.display().to_string(),
            "expected_revision": 4
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"active": 0})))
        .expect(1)
        .mount(&server)
        .await;

    repo::handle(
        &RepoCommand::Unbind {
            path: Some(temp.path().to_path_buf()),
            expected_revision: 4,
        },
        &ctx,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn inspect_with_binding_reports_none_when_no_checkout_exists() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);
    let temp = tempdir().unwrap();
    init_repo_with_commit(temp.path());

    Mock::given(method("GET"))
        .and(wpath("/v1/memory/checkouts"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "items": [], "next_offset": null })),
        )
        .mount(&server)
        .await;

    repo::handle(
        &RepoCommand::Inspect {
            path: Some(temp.path().to_path_buf()),
            binding: true,
        },
        &ctx,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn inspect_without_binding_flag_never_calls_the_network() {
    // No MockServer at all: if the offline path tried to reach the network,
    // this would fail to connect and return an error instead of Ok.
    let mut config = AppConfig::default();
    config.api.url = "http://127.0.0.1:1".to_string(); // reserved/unroutable port
    let ctx = Ctx::new(config, None, false, false, OutputFormat::Json, false).unwrap();
    let temp = tempdir().unwrap();
    init_repo_with_commit(temp.path());

    let result = repo::handle(
        &RepoCommand::Inspect {
            path: Some(temp.path().to_path_buf()),
            binding: false,
        },
        &ctx,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn unauthorized_list_error_never_echoes_the_access_token() {
    let server = MockServer::start().await;
    let ctx = test_ctx(&server);

    Mock::given(method("GET"))
        .and(wpath("/v1/memory/repositories"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "code": "AUTH_INVALID_TOKEN",
                "message": "Invalid or expired token",
                "details": null,
                "hint": "Run `tokanban auth login` to authenticate."
            }
        })))
        .mount(&server)
        .await;

    let result = repo::handle(
        &RepoCommand::List {
            limit: 50,
            offset: 0,
        },
        &ctx,
    )
    .await;
    let err = result.expect_err("expected an auth error");
    assert!(!err.render().contains("tk_should_never_leak"));
    match err {
        CliError::Api { code, .. } => assert_eq!(code, "AUTH_INVALID_TOKEN"),
        other => panic!("expected Api error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// CLI parsing
// ---------------------------------------------------------------------------

#[test]
fn cli_parses_repo_inspect_path_and_binding_flag() {
    let cli = Cli::try_parse_from(["tokanban", "repo", "inspect"]).unwrap();
    match cli.command {
        CliCommand::Repo(RepoCommand::Inspect { path, binding }) => {
            assert!(path.is_none());
            assert!(!binding);
        }
        other => panic!("expected Repo::Inspect, got {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "tokanban",
        "repo",
        "inspect",
        "--path",
        "/tmp/x",
        "--binding",
    ])
    .unwrap();
    match cli.command {
        CliCommand::Repo(RepoCommand::Inspect { path, binding }) => {
            assert_eq!(path, Some(PathBuf::from("/tmp/x")));
            assert!(binding);
        }
        other => panic!("expected Repo::Inspect, got {other:?}"),
    }
}

#[test]
fn cli_requires_expected_revision_for_unbind_but_not_for_bind() {
    assert!(Cli::try_parse_from(["tokanban", "repo", "unbind"]).is_err());
    assert!(
        Cli::try_parse_from(["tokanban", "repo", "unbind", "--expected-revision", "3"]).is_ok()
    );
    assert!(Cli::try_parse_from(["tokanban", "repo", "bind", "repo-1"]).is_ok());
    assert!(Cli::try_parse_from([
        "tokanban",
        "repo",
        "bind",
        "repo-1",
        "--branch",
        "main",
        "--kind",
        "worktree",
        "--expected-revision",
        "2"
    ])
    .is_ok());
}

#[test]
fn cli_parses_repo_create_with_remote_and_project() {
    let cli = Cli::try_parse_from([
        "tokanban",
        "repo",
        "create",
        "widgets",
        "--remote",
        "https://github.com/acme/widgets",
        "--project",
        "PLAT",
    ])
    .unwrap();
    match cli.command {
        CliCommand::Repo(RepoCommand::Create {
            name,
            remote,
            project,
        }) => {
            assert_eq!(name, "widgets");
            assert_eq!(remote.as_deref(), Some("https://github.com/acme/widgets"));
            assert_eq!(project.as_deref(), Some("PLAT"));
        }
        other => panic!("expected Repo::Create, got {other:?}"),
    }
}
