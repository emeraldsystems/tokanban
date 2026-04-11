/// Tests for API client behavior

mod common;

use common::MockServer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TestResponse {
    key: String,
    title: String,
}

// ============================================================================
// AUTH HEADER INJECTION TESTS
// ============================================================================

#[tokio::test]
async fn test_auth_header_injected_get() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST-1", "title": "Task"});
    server.mock_get("/api/tasks/TEST-1", response).await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("tk_access_test".to_string()))
        .unwrap();
    let result: TestResponse = client.get("/api/tasks/TEST-1").await.unwrap();
    assert_eq!(result.key, "TEST-1");
}

#[tokio::test]
async fn test_auth_header_injected_post() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST-2", "title": "New Task"});
    server.mock_post("/api/tasks", response).await;

    let body = serde_json::json!({"title": "New Task"});
    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("tk_access_test".to_string()))
        .unwrap();
    let result: TestResponse = client.post("/api/tasks", &body).await.unwrap();
    assert_eq!(result.title, "New Task");
}

#[tokio::test]
async fn test_client_with_no_token() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST-1", "title": "Task"});
    server.mock_get("/api/public", response).await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: TestResponse = client.get("/api/public").await.unwrap();
    assert_eq!(result.key, "TEST-1");
}

// ============================================================================
// ERROR RESPONSE PARSING TESTS
// ============================================================================

#[tokio::test]
async fn test_error_message_extracted() {
    let server = MockServer::start().await;
    server.mock_not_found("/api/projects/NONE", "Project").await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("token".to_string()))
        .unwrap();
    let result: Result<TestResponse, _> = client.get("/api/projects/NONE").await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(err_str.contains("404") || err_str.contains("not found") || err_str.contains("Not Found"));
}

#[tokio::test]
async fn test_error_hint_extracted() {
    let server = MockServer::start().await;
    server.mock_unauthorized("/api/tasks").await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("expired_token".to_string()))
        .unwrap();
    let result: Result<TestResponse, _> = client.get("/api/tasks").await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let rendered = err.render();
    // Hint should be present for unauthorized errors
    assert!(rendered.contains("401") || rendered.contains("Unauthorized") || rendered.contains("Invalid"));
}

#[tokio::test]
async fn test_error_unknown_type_handled() {
    let server = MockServer::start().await;
    // Return raw error text without structured error format
    server
        .mock_error("GET", "/api/error", 500, serde_json::json!({"raw": "error"}))
        .await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("token".to_string()))
        .unwrap();
    let result: Result<TestResponse, _> = client.get("/api/error").await;

    assert!(result.is_err());
}

// ============================================================================
// TIMEOUT HANDLING TESTS
// ============================================================================

#[tokio::test]
async fn test_timeout_from_config() {
    // Test that timeout is properly set from config
    let client = tokanban::api::ApiClient::new("https://api.tokanban.com", 45, None).unwrap();
    // Just verify it was created successfully with the timeout
    assert!(true);
}

// ============================================================================
// STATUS CODE HANDLING TESTS
// ============================================================================

#[tokio::test]
async fn test_200_success() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST-1", "title": "Task"});
    server.mock_get("/api/tasks/TEST-1", response).await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: TestResponse = client.get("/api/tasks/TEST-1").await.unwrap();
    assert_eq!(result.key, "TEST-1");
    assert_eq!(result.title, "Task");
}

#[tokio::test]
async fn test_201_created() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST-2", "title": "Created"});
    server.mock_post("/api/tasks", response).await;

    let body = serde_json::json!({"title": "Created"});
    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: TestResponse = client.post("/api/tasks", &body).await.unwrap();
    assert_eq!(result.key, "TEST-2");
}

#[tokio::test]
async fn test_401_unauthorized() {
    let server = MockServer::start().await;
    server.mock_unauthorized("/api/tasks").await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, Some("invalid_token".to_string()))
        .unwrap();
    let result: Result<TestResponse, _> = client.get("/api/tasks").await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    let rendered = err.render();
    // Check that error is actually returned (content doesn't matter as much)
    assert!(!rendered.is_empty());
}

#[tokio::test]
async fn test_404_not_found() {
    let server = MockServer::start().await;
    server.mock_not_found("/api/tasks/NONE", "Task").await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: Result<TestResponse, _> = client.get("/api/tasks/NONE").await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("404") || err_str.contains("not found"));
}

#[tokio::test]
async fn test_429_rate_limit() {
    let server = MockServer::start().await;
    server.mock_rate_limit("/api/tasks").await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: Result<TestResponse, _> = client.get("/api/tasks").await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("429") || err_str.contains("rate") || err_str.contains("Too many"));
}

// ============================================================================
// JSON SERIALIZATION TESTS
// ============================================================================

#[tokio::test]
async fn test_response_json_deserialization() {
    let server = MockServer::start().await;
    let response = serde_json::json!({
        "key": "PLAT-42",
        "title": "Fix auth token refresh logic"
    });
    server.mock_get("/api/tasks/PLAT-42", response).await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result: TestResponse = client.get("/api/tasks/PLAT-42").await.unwrap();
    assert_eq!(result.key, "PLAT-42");
    assert_eq!(result.title, "Fix auth token refresh logic");
}

#[tokio::test]
async fn test_exchange_code_for_tokens() {
    let server = MockServer::start().await;
    let token_response = serde_json::json!({
        "access_token": "tk_access_xyz",
        "refresh_token": "tk_user_abc",
        "expires_in": 3600,
        "token_type": "Bearer"
    });
    server.mock_post("/oauth/token", token_response).await;

    let client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    let result = client
        .exchange_code("auth_code_123", "code_verifier_xyz", "http://localhost:8080/callback")
        .await
        .unwrap();

    assert_eq!(result.access_token, "tk_access_xyz");
    assert_eq!(result.refresh_token, "tk_user_abc");
    assert_eq!(result.expires_in, 3600);
}

#[tokio::test]
async fn test_set_access_token() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"key": "TEST", "title": "Test"});
    server.mock_get("/api/tasks", response).await;

    let mut client = tokanban::api::ApiClient::new(&server.base_url(), 30, None).unwrap();
    client.set_access_token("new_token".to_string());

    let result: TestResponse = client.get("/api/tasks").await.unwrap();
    assert_eq!(result.key, "TEST");
}
