/// Integration tests for the Tokanban CLI
/// These tests verify full workflows with mock API server

mod common;

use common::MockServer;
use tokanban::api::ApiClient;
use tokanban::commands::{project, task};
use tokanban::commands::project::ProjectCommand;
use tokanban::commands::task::TaskCommand;
use tokanban::config::AppConfig;
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use wiremock::{Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn test_api_client_with_mock_server() {
    // Setup mock server with task response
    let server = MockServer::start().await;
    let task_response = serde_json::json!({
        "key": "TEST-1",
        "title": "Test Task",
        "status": "Todo"
    });
    server.mock_get("/api/tasks/TEST-1", task_response).await;

    // Create API client pointing to mock server
    let client = ApiClient::new(&server.base_url(), 30, Some("test_token".to_string())).unwrap();

    // Make request and verify response
    let result: Result<serde_json::Value, _> = client.get("/api/tasks/TEST-1").await;
    assert!(result.is_ok());

    let task = result.unwrap();
    assert_eq!(task["key"], "TEST-1");
    assert_eq!(task["title"], "Test Task");
}

#[tokio::test]
async fn test_ctx_creation_with_config() {
    let mut config = AppConfig::default();
    config.defaults.workspace = Some("test-ws".to_string());
    config.defaults.project = Some("TEST".to_string());

    let ctx = Ctx::new(
        config,
        None,
        false,
        false,
        OutputFormat::Json,
        false,
    );

    assert!(ctx.is_ok());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.config.defaults.workspace, Some("test-ws".to_string()));
}

#[tokio::test]
async fn test_token_refresh_flow_integration() {
    let server = MockServer::start().await;

    // Mock token refresh endpoint
    let token_response = serde_json::json!({
        "access_token": "tk_access_refreshed",
        "refresh_token": "tk_user_original",
        "expires_in": 3600,
        "token_type": "Bearer"
    });
    server.mock_post("/oauth/token", token_response).await;

    // Create client and refresh token
    let client = ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result = client.refresh_token("tk_user_original").await;

    assert!(result.is_ok());
    let tokens = result.unwrap();
    assert_eq!(tokens.access_token, "tk_access_refreshed");
}

#[tokio::test]
async fn test_error_rendering() {
    use tokanban::error::CliError;

    let error = CliError::Api {
        code: "TEST_ERROR".to_string(),
        message: "This is a test error".to_string(),
        details: Some("Error details".to_string()),
        hint: Some("Try this instead".to_string()),
    };

    let rendered = error.render();
    assert!(rendered.contains("TEST_ERROR"));
    assert!(rendered.contains("This is a test error"));
    assert!(rendered.contains("Detail:"));
    assert!(rendered.contains("Hint:"));
}

#[tokio::test]
async fn test_missing_required_error() {
    use tokanban::error::CliError;

    let error = CliError::MissingRequired("project".into(), "project".into());
    let rendered = error.render();

    assert!(!rendered.is_empty());
    assert!(rendered.contains("project"));
}

#[tokio::test]
async fn test_not_authenticated_error() {
    use tokanban::error::CliError;

    let error = CliError::NotAuthenticated;
    let rendered = error.render();

    assert!(rendered.contains("auth login"));
}

#[tokio::test]
async fn test_insecure_config_error() {
    use tokanban::error::CliError;

    let error = CliError::InsecureConfig { mode: 0o644 };
    let rendered = error.render();

    assert!(rendered.contains("insecure") || rendered.contains("permission"));
}

#[tokio::test]
async fn test_config_loading_with_valid_file() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = common::setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = common::ConfigBuilder::new()
        .workspace("test-ws")
        .project("TEST")
        .build();

    fs::write(&config_path, config_content).unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let config = tokanban::config::load_config(Some(&config_path));
    assert!(config.is_ok());

    let config = config.unwrap();
    assert_eq!(config.defaults.workspace, Some("test-ws".to_string()));
    assert_eq!(config.defaults.project, Some("TEST".to_string()));
}

#[tokio::test]
async fn test_format_resolution() {
    use tokanban::format::OutputFormat;

    // Auto should resolve to something concrete
    let resolved = OutputFormat::Auto.resolve();
    assert!(resolved != OutputFormat::Auto);
    assert!(resolved == OutputFormat::Json || resolved == OutputFormat::Table);
}

#[tokio::test]
async fn test_output_format_is_json() {
    use tokanban::format::OutputFormat;

    assert!(OutputFormat::Json.is_json());
    assert!(!OutputFormat::Table.is_json());
    assert!(!OutputFormat::Card.is_json());
    assert!(!OutputFormat::Inline.is_json());
}

#[tokio::test]
async fn test_output_format_is_tui() {
    use tokanban::format::OutputFormat;

    assert!(OutputFormat::Table.is_tui());
    assert!(OutputFormat::Card.is_tui());
    assert!(OutputFormat::Inline.is_tui());
    assert!(!OutputFormat::Json.is_tui());
}

#[tokio::test]
async fn test_api_list_response_structure() {
    let server = MockServer::start().await;

    let list_response = serde_json::json!({
        "items": [
            {"key": "TEST-1", "title": "Task 1"},
            {"key": "TEST-2", "title": "Task 2"}
        ],
        "total": 2,
        "page": 1,
        "limit": 50
    });
    server.mock_get("/api/tasks", list_response).await;

    let client = ApiClient::new(&server.base_url(), 30, Some("token".to_string())).unwrap();
    let result: Result<serde_json::Value, _> = client.get("/api/tasks").await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(resp["items"].is_array());
    assert_eq!(resp["total"], 2);
    assert_eq!(resp["page"], 1);
    assert_eq!(resp["limit"], 50);
}

#[tokio::test]
async fn test_patch_request() {
    let server = MockServer::start().await;

    let response = serde_json::json!({
        "id": "task-1",
        "key": "TEST-1",
        "message": "Task updated successfully"
    });
    server.mock_post("/api/tasks/TEST-1", response).await;

    let client = ApiClient::new(&server.base_url(), 30, Some("token".to_string())).unwrap();
    let body = serde_json::json!({"status": "In Progress"});

    let result: Result<serde_json::Value, _> = client.post("/api/tasks/TEST-1", &body).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_request() {
    let server = MockServer::start().await;

    let response = serde_json::json!({"success": true});
    server.mock_delete("/api/tasks/TEST-1", response).await;

    let client = ApiClient::new(&server.base_url(), 30, Some("token".to_string())).unwrap();

    let result: Result<serde_json::Value, _> = client.delete("/api/tasks/TEST-1").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ctx_with_quiet_flag() {
    let config = AppConfig::default();

    let ctx = Ctx::new(
        config,
        None,
        true, // quiet
        false,
        OutputFormat::Json,
        false,
    );

    assert!(ctx.is_ok());
    assert!(ctx.unwrap().quiet);
}

#[tokio::test]
async fn test_ctx_with_verbose_flag() {
    let config = AppConfig::default();

    let ctx = Ctx::new(
        config,
        None,
        false,
        true, // verbose
        OutputFormat::Json,
        false,
    );

    assert!(ctx.is_ok());
    assert!(ctx.unwrap().verbose);
}

#[tokio::test]
async fn test_color_config_disabled() {
    let config = AppConfig::default();

    let ctx = Ctx::new(
        config,
        None,
        false,
        false,
        OutputFormat::Json,
        true, // no_color
    );

    assert!(ctx.is_ok());
    assert!(!ctx.unwrap().color.enabled);
}

#[tokio::test]
async fn test_project_list_does_not_require_workspace_default() {
    let server = MockServer::start().await;
    let response = serde_json::json!({
        "data": [
            {
                "id": "proj_123",
                "name": "Platform",
                "key_prefix": "PLAT",
                "description": null,
                "workflow_config": {},
                "defaults": {},
                "created_at": 1770000000,
                "updated_at": 1770000000
            }
        ],
        "pagination": {
            "hasMore": false
        }
    });
    server.mock_get("/v1/projects", response).await;

    let mut config = AppConfig::default();
    config.api.url = server.base_url();
    config.auth.access_token = Some("tk_user_test".to_string());

    let mut ctx = Ctx::new(
        config,
        None,
        false,
        false,
        OutputFormat::Json,
        false,
    )
    .unwrap();

    let result = tokanban::commands::project::handle(
        &ProjectCommand::List { workspace: None },
        &mut ctx,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_project_set_resolves_name_to_canonical_project_id() {
    let server = MockServer::start().await;
    let response = serde_json::json!({
        "data": [
            {
                "id": "proj_123",
                "name": "Workspace",
                "key_prefix": "WS",
                "created_at": 1770000000,
                "updated_at": 1770000000
            }
        ],
        "pagination": {
            "hasMore": false
        }
    });
    server.mock_get("/v1/projects", response).await;

    let temp_dir = common::setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let mut config = AppConfig::default();
    config.api.url = server.base_url();
    config.auth.access_token = Some("tk_user_test".to_string());

    let mut ctx = Ctx::new(
        config,
        Some(config_path.clone()),
        true,
        false,
        OutputFormat::Json,
        false,
    )
    .unwrap();

    project::handle(
        &ProjectCommand::Set {
            key: "Workspace".to_string(),
        },
        &mut ctx,
    )
    .await
    .unwrap();

    assert_eq!(ctx.config.defaults.project.as_deref(), Some("proj_123"));

    let reloaded = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(reloaded.defaults.project.as_deref(), Some("proj_123"));
}

#[tokio::test]
async fn test_task_create_resolves_project_reference_before_posting() {
    let server = MockServer::start().await;

    let projects_response = serde_json::json!({
        "data": [
            {
                "id": "proj_123",
                "name": "Workspace",
                "key_prefix": "WS",
                "created_at": 1770000000,
                "updated_at": 1770000000
            }
        ],
        "pagination": {
            "hasMore": false
        }
    });
    server.mock_get("/v1/projects", projects_response).await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/proj_123/tasks"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "task_123",
            "key": "WS-1",
            "message": "Task created successfully"
        })))
        .mount(&server.inner)
        .await;

    let mut config = AppConfig::default();
    config.api.url = server.base_url();
    config.auth.access_token = Some("tk_user_test".to_string());
    config.defaults.project = Some("Workspace".to_string());

    let ctx = Ctx::new(
        config,
        None,
        true,
        false,
        OutputFormat::Json,
        false,
    )
    .unwrap();

    task::handle(
        &TaskCommand::Create {
            title: "Create a tetris clone python game".to_string(),
            project: None,
            priority: Some("High".to_string()),
            assignee: None,
            sprint: None,
            description: None,
        },
        &ctx,
    )
    .await
    .unwrap();
}
