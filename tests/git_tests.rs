use clap::Parser;
use serde_json::json;
use tokanban::cli::Cli;
use tokanban::commands::git::{self, GitCommand};
use tokanban::config::AppConfig;
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
fn ctx(server: &MockServer) -> Ctx {
    let mut config = AppConfig::default();
    config.api.url = server.uri();
    config.auth.access_token = Some("test-token".into());
    Ctx::new(config, None, false, false, OutputFormat::Json, true).unwrap()
}
fn link() -> serde_json::Value {
    json!({"id":"link", "full_name":"owner/repo", "number":7, "relation":"completes", "evidence_state":"unknown", "url":"https://github.com/owner/repo/pull/7", "binding_generation": 3, "evidence": null})
}
#[test]
fn parses_explicit_claims_for_link_and_unlink() {
    assert!(Cli::try_parse_from([
        "tokanban",
        "git",
        "link",
        "TKB-1",
        "--url",
        "https://github.com/owner/repo/pull/7",
        "--claim-id",
        "lease"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "tokanban",
        "git",
        "unlink",
        "TKB-1",
        "link",
        "--claim-id",
        "lease"
    ])
    .is_ok());
}
#[tokio::test]
async fn authenticated_link_and_unlink_preserve_claims_and_relation() {
    let server = MockServer::start().await;
    let ctx = ctx(&server);
    Mock::given(method("POST"))
        .and(path("/v1/tasks/TKB-1/git-links"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(
            json!({"repository_id":"repo", "number":7, "relation":"completes", "claim_id":"lease"}),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(link()))
        .expect(1)
        .mount(&server)
        .await;
    git::handle(
        &GitCommand::Link {
            key: "TKB-1".into(),
            repository: Some("repo".into()),
            url: None,
            number: Some(7),
            relation: "completes".into(),
            claim_id: Some("lease".into()),
        },
        &ctx,
    )
    .await
    .unwrap();
    Mock::given(method("DELETE"))
        .and(path("/v1/tasks/TKB-1/git-links/link"))
        .and(query_param("claim_id", "lease/a+1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"removed":true})))
        .expect(1)
        .mount(&server)
        .await;
    git::handle(
        &GitCommand::Unlink {
            key: "TKB-1".into(),
            link_id: "link".into(),
            claim_id: Some("lease/a+1".into()),
        },
        &ctx,
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn does_not_hide_server_claim_conflicts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(409).set_body_json(
            json!({"error":{"code":"TASK_CLAIM_LOST", "message":"Claim no longer held"}}),
        ))
        .mount(&server)
        .await;
    let result = git::handle(
        &GitCommand::Link {
            key: "TKB-1".into(),
            repository: Some("repo".into()),
            url: None,
            number: Some(7),
            relation: "related".into(),
            claim_id: Some("old".into()),
        },
        &ctx(&server),
    )
    .await;
    assert!(format!("{:?}", result.unwrap_err()).contains("TASK_CLAIM_LOST"));
}
#[tokio::test]
async fn invalid_link_input_has_no_network_side_effects() {
    let server = MockServer::start().await;
    for (repo, number, relation) in [
        (None, None, "related"),
        (Some("repo".into()), Some(7), "deployed"),
    ] {
        assert!(git::handle(
            &GitCommand::Link {
                key: "TKB-1".into(),
                repository: repo,
                url: None,
                number,
                relation: relation.into(),
                claim_id: None
            },
            &ctx(&server)
        )
        .await
        .is_err());
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
#[tokio::test]
async fn configured_project_resolution_and_no_browser_connect_validate_destination() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v1/projects")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"project", "key":"TKB", "key_prefix":"TKB", "name":"Tokanban"}],"pagination":{"hasMore":false}}))).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/project/git/installations/start"))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            json!({"installation_url":"https://attacker.invalid/apps/x/installations/new"}),
        ))
        .mount(&server)
        .await;
    assert!(git::handle(
        &GitCommand::Connect {
            project: Some("TKB".into()),
            no_browser: true
        },
        &ctx(&server)
    )
    .await
    .is_err());
}

#[tokio::test]
async fn policy_updates_send_the_reviewed_document_and_do_not_retry_revision_conflicts() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v1/projects")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"project", "key":"TKB", "key_prefix":"TKB", "name":"Tokanban"}],"pagination":{"hasMore":false}}))).mount(&server).await;
    let document = json!({ "expected_revision": 2, "binding_generation": 3, "policy": { "automation_enabled": true, "review_status": "in_review", "completion_status": "done", "completion_mode": "verified_checks", "required_checks": ["CI"], "require_approval": true } });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("policy.json");
    std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    Mock::given(method("PATCH")).and(path("/v1/projects/project/git/repositories/repo/policy")).and(body_json(document))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"error":{"code":"GIT_POLICY_REVISION_CHANGED", "message":"Review current policy"}}))).expect(1).mount(&server).await;
    let error = git::handle(
        &GitCommand::Policy {
            project: Some("TKB".into()),
            repository: "repo".into(),
            file: Some(file),
        },
        &ctx(&server),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:?}").contains("GIT_POLICY_REVISION_CHANGED"));
}
#[test]
fn parses_explicit_policy_document_command() {
    assert!(Cli::try_parse_from([
        "tokanban",
        "git",
        "policy",
        "--project",
        "TKB",
        "--repository",
        "repo",
        "--file",
        "policy.json"
    ])
    .is_ok());
}

#[test]
fn provenance_pagination_is_bounded_before_networking() {
    assert!(Cli::try_parse_from([
        "tokanban",
        "git",
        "provenance",
        "TKB-1",
        "--limit",
        "50",
        "--offset",
        "40"
    ])
    .is_ok());
    for (flag, value) in [("--limit", "0"), ("--limit", "51"), ("--offset", "10001")] {
        assert!(
            Cli::try_parse_from(["tokanban", "git", "provenance", "TKB-1", flag, value]).is_err()
        );
    }
}

#[tokio::test]
async fn provenance_reads_one_page_and_labels_private_historical_sources() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v1/tasks/TKB-1/session-provenance"))
        .and(query_param("limit", "2")).and(query_param("offset", "4"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "task_id": "task", "project_id": "project", "namespace": "production", "private_to_current_user": true,
            "items": [{"session_id":"session", "source_harness":"claude-code", "status":"ended", "association":"reviewed_followup",
                "git_provenance":{"snapshot":{"branch":"feature", "deployment_state":"unknown"}, "current_pull_requests":[]}}], "next_offset":6
        }}))).expect(1).mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    tokanban::config::save_config(&ctx(&server).config, Some(&config_path)).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
            .arg("--config")
            .arg(config_path)
            .args([
                "--format",
                "table",
                "--no-color",
                "git",
                "provenance",
                "TKB-1",
                "--limit",
                "2",
                "--offset",
                "4",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("private to your current user (production namespace)"));
    assert!(text.contains("reviewed_followup"));
    assert!(text.contains("Deployment status remains unknown"));
    assert!(text.contains("--offset 6"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn inspected_policy_document_can_be_reviewed_and_submitted_without_removing_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v1/projects")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"project", "key":"TKB", "key_prefix":"TKB", "name":"Tokanban"}],"pagination":{"hasMore":false}}))).mount(&server).await;
    let policy = json!({ "automation_enabled":false, "review_status":null, "completion_status":null, "completion_mode":"merge_only", "required_checks":[], "require_approval":false });
    Mock::given(method("GET")).and(path("/v1/projects/project/git/health")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"repositories":[{"id":"repo", "policy_revision":2, "generation":3, "policy":policy}]}))).mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    tokanban::config::save_config(&ctx(&server).config, Some(&config_path)).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_tokanban"))
            .arg("--config")
            .arg(config_path)
            .args(["git", "policy", "--project", "TKB", "--repository", "repo"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        document,
        json!({"expected_revision":2,"binding_generation":3,"policy":policy})
    );
    let file = dir.path().join("policy.json");
    std::fs::write(&file, &output.stdout).unwrap();
    Mock::given(method("PATCH"))
        .and(path("/v1/projects/project/git/repositories/repo/policy"))
        .and(body_json(document))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"revision":2,"policy":policy})),
        )
        .expect(1)
        .mount(&server)
        .await;
    git::handle(
        &GitCommand::Policy {
            project: Some("TKB".into()),
            repository: "repo".into(),
            file: Some(file),
        },
        &ctx(&server),
    )
    .await
    .unwrap();
}
