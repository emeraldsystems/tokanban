use clap::Parser;
use serde_json::json;
use tokanban::cli::Cli;
use tokanban::commands::task::{self, TaskCommand};
use tokanban::config::AppConfig;
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx(server: &MockServer) -> Ctx {
    let mut config = AppConfig::default();
    config.api.url = server.uri();
    config.auth.access_token = Some("test-token".into());
    Ctx::new(config, None, false, false, OutputFormat::Json, false).unwrap()
}

#[test]
fn claim_requires_a_session_and_a_bounded_lifetime() {
    assert!(Cli::try_parse_from(["tokanban", "task", "claim", "TKB-1"]).is_err());
    assert!(Cli::try_parse_from([
        "tokanban",
        "task",
        "claim",
        "TKB-1",
        "--session",
        "run",
        "--ttl-seconds",
        "3601"
    ])
    .is_err());
    assert!(
        Cli::try_parse_from(["tokanban", "task", "claim", "TKB-1", "--session", "run"]).is_ok()
    );
    assert!(
        Cli::try_parse_from(["tokanban", "task", "renew", "TKB-1", "--claim-id", "lease"]).is_ok()
    );
    assert!(Cli::try_parse_from([
        "tokanban",
        "task",
        "update",
        "TKB-1",
        "--claim-id",
        "lease",
        "--status",
        "in_progress"
    ])
    .is_ok());
}

#[tokio::test]
async fn ownership_commands_preserve_server_tokens_and_send_guarded_updates() {
    let server = MockServer::start().await;
    let ctx = ctx(&server);
    let response = json!({"id":"task", "key":"TKB-1", "title":"Work", "status":"todo", "ownership": {
        "claim_id":"lease", "actor_id":"agent", "actor_name":"Worker", "actor_type":"agent", "user_id":"user",
        "session_id":"run", "acquired_at":"2026-09-05T00:00:00Z", "expires_at":"2026-09-05T00:30:00Z", "state":"active"
    }});
    for (suffix, body) in [
        ("", json!({"session_id":"run", "ttl_seconds":1800})),
        ("/renew", json!({"claim_id":"lease", "ttl_seconds":1800})),
        ("/release", json!({"claim_id":"lease"})),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/v1/tasks/TKB-1/claim{suffix}")))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("PATCH"))
        .and(path("/v1/tasks/TKB-1"))
        .and(body_json(json!({"claim_id":"lease", "title":"Updated"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;
    task::handle(
        &TaskCommand::Claim {
            key: "TKB-1".into(),
            session: "run".into(),
            ttl_seconds: 1800,
        },
        &ctx,
    )
    .await
    .unwrap();
    task::handle(
        &TaskCommand::Renew {
            key: "TKB-1".into(),
            claim_id: "lease".into(),
            ttl_seconds: 1800,
        },
        &ctx,
    )
    .await
    .unwrap();
    task::handle(
        &TaskCommand::Update {
            key: "TKB-1".into(),
            claim_id: Some("lease".into()),
            title: Some("Updated".into()),
            status: None,
            assignee: None,
            priority: None,
            sprint: None,
            description: None,
        },
        &ctx,
    )
    .await
    .unwrap();
    task::handle(
        &TaskCommand::Release {
            key: "TKB-1".into(),
            claim_id: "lease".into(),
        },
        &ctx,
    )
    .await
    .unwrap();
    let detail: tokanban::api::TaskDetailResponse = serde_json::from_value(response).unwrap();
    assert_eq!(detail.ownership.unwrap().claim_id, "lease");
}

#[tokio::test]
async fn available_filter_reaches_the_api() {
    let server = MockServer::start().await;
    let ctx = ctx(&server);
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"id":"project","name":"Test","key_prefix":"TKB"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project/tasks"))
        .and(query_param("available", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[],"pagination":{"hasMore":false}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    task::handle(
        &TaskCommand::List {
            project: Some("TKB".into()),
            status: None,
            assignee: None,
            sprint: None,
            priority: None,
            due: None,
            ready: false,
            available: true,
            cursor: None,
            limit: 25,
        },
        &ctx,
    )
    .await
    .unwrap();
}
