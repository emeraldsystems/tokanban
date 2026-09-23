use serde_json::json;
use tokanban::commands::persona::{self, PersonaCommand};
use tokanban::config::AppConfig;
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx(server: &MockServer) -> Ctx {
    let mut config = AppConfig::default();
    config.api.url = server.uri();
    config.auth.access_token = Some("test-token".into());
    config.defaults.project = Some("TKB".into());
    Ctx::new(config, None, false, false, OutputFormat::Json, true).unwrap()
}

async fn mock_project_resolution(server: &MockServer, expected: u64) {
    Mock::given(method("GET"))
        .and(path("/v1/projects"))
        .and(query_param("limit", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"project-1","key":"TKB","key_prefix":"TKB","name":"Tokanban"}],
            "total": 1
        })))
        .expect(expected)
        .mount(server)
        .await;
}

#[tokio::test]
async fn configure_merges_shared_activation_and_puts_full_enabled_set() {
    let server = MockServer::start().await;
    mock_project_resolution(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/personas"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "personas": [
                {"key":"pm","name":"Project Manager","description":"Delivery","enabled":true,"teammate_id":"mate-pm"},
                {"key":"engineer","name":"Engineer","description":"Implementation","enabled":false,"teammate_id":null},
                {"key":"researcher","name":"Researcher","description":"Evidence","enabled":true,"teammate_id":"mate-r"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/v1/projects/project-1/personas"))
        .and(body_json(json!({"enabled":["pm","engineer"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"personas":[]})))
        .expect(1)
        .mount(&server)
        .await;

    persona::handle(
        &PersonaCommand::Configure {
            project: None,
            enable: vec!["engineer".into()],
            disable: vec!["researcher".into()],
        },
        &ctx(&server),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn context_aggregates_live_project_state_for_enabled_persona() {
    let server = MockServer::start().await;
    mock_project_resolution(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"project-1","key":"TKB","key_prefix":"TKB","name":"Tokanban"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/personas"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "personas": [
                {"key":"pm","name":"Project Manager","description":"Delivery","enabled":true,"teammate_id":"mate-pm"},
                {"key":"engineer","name":"Engineer","description":"Implementation","enabled":true,"teammate_id":"mate-e"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/teammates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"mate-pm","project_id":"project-1","persona_key":"pm","name":"Project Manager","enabled":true}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/tasks"))
        .and(query_param("limit", "100"))
        .and(query_param_is_missing("assignee_id"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"task-old","key":"TKB-0","title":"Old completion","status":"done"}],
            "total": 2,
            "cursor": "next-page"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/tasks"))
        .and(query_param("limit", "100"))
        .and(query_param("cursor", "next-page"))
        .and(query_param_is_missing("assignee_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"task-active","key":"TKB-2","title":"Active work beyond page one","status":"in_progress"}],
            "total": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/tasks"))
        .and(query_param("limit", "100"))
        .and(query_param("assignee_id", "mate-pm"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"task-1","key":"TKB-1","title":"Queued for PM","status":"todo","assignee_id":"mate-pm"}],
            "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/entities"))
        .and(query_param("limit", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"id":"req-1","key":"TKB-REQ-1","project_id":"project-1","entity_number":1,"kind":"requirement","title":"Active session execution","content":"No hosted worker","status":"active"}],
            "total": 1
        })))
        .expect(1)
        .mount(&server)
        .await;

    persona::handle(
        &PersonaCommand::Context {
            persona: "pm".into(),
            project: None,
            session: Some("session-1".into()),
            compact: true,
        },
        &ctx(&server),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn disabled_context_is_explicit_and_does_not_read_board_as_empty() {
    let server = MockServer::start().await;
    mock_project_resolution(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id":"project-1","key":"TKB","key_prefix":"TKB","name":"Tokanban"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/personas"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "personas": [{"key":"reviewer","name":"Reviewer","description":"Quality","enabled":false,"teammate_id":null}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/projects/project-1/teammates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[]})))
        .expect(1)
        .mount(&server)
        .await;

    persona::handle(
        &PersonaCommand::Context {
            persona: "reviewer".into(),
            project: None,
            session: None,
            compact: true,
        },
        &ctx(&server),
    )
    .await
    .unwrap();
}
