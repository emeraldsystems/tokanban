/// Test fixtures and data generators

use serde_json::json;
use std::collections::HashMap;

/// Generate test task data
pub fn task_fixture() -> serde_json::Value {
    json!({
        "key": "TEST-42",
        "title": "Fix auth token refresh logic",
        "description": "The refresh token endpoint returns 401 when expired",
        "status": "In Progress",
        "priority": "High",
        "assignee": {
            "id": "user-abc",
            "email": "sven@tokanban.io",
            "name": "sven"
        },
        "sprint": {
            "id": "sprint-12",
            "name": "Sprint 12",
            "start_date": "2026-04-01",
            "end_date": "2026-04-15"
        },
        "created_at": "2026-03-15T10:30:00Z",
        "updated_at": "2026-04-01T14:22:15Z"
    })
}

/// Generate test project data
pub fn project_fixture() -> serde_json::Value {
    json!({
        "key": "TEST",
        "name": "Test Project",
        "description": "A test project for integration testing",
        "lead": {
            "id": "user-abc",
            "email": "sven@tokanban.io",
            "name": "sven"
        },
        "created_at": "2026-01-15T10:30:00Z",
        "updated_at": "2026-04-01T14:22:15Z"
    })
}

/// Generate test workspace data
pub fn workspace_fixture() -> serde_json::Value {
    json!({
        "id": "workspace-abc",
        "name": "Test Workspace",
        "slug": "test-workspace",
        "created_at": "2025-01-01T00:00:00Z"
    })
}

/// Generate test user data
pub fn user_fixture() -> serde_json::Value {
    json!({
        "id": "user-abc",
        "email": "sven@tokanban.io",
        "name": "sven",
        "role": "admin"
    })
}

/// Generate test token response
pub fn token_response() -> serde_json::Value {
    json!({
        "refresh_token": "tk_user_abc123xyz",
        "access_token": "tk_access_xyz789abc",
        "expires_in": 3600
    })
}

/// Generate config file content as string
pub fn config_content(token: &str, workspace: &str, project: &str) -> String {
    format!(
        r#"[auth]
token = "{}"
access_token = "tk_access_xyz789abc"
expires_at = {}

[defaults]
workspace = "{}"
project = "{}"

[ui]
no_color = false
format = "auto"

[api]
url = "https://api.tokanban.com"
timeout_secs = 30
"#,
        token,
        chrono::Utc::now().timestamp() + 3600,
        workspace,
        project
    )
}

/// Generate invalid config content
pub fn invalid_config_content() -> String {
    "[invalid toml syntax\nthis is not valid = ".to_string()
}

/// Helper to build list response JSON
pub fn list_response(items: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "items": items,
        "total": items.len(),
        "page": 1,
        "limit": 50
    })
}

/// Helper to build error response JSON
pub fn error_response(error_type: &str, message: &str, hint: &str, fix: &str) -> serde_json::Value {
    json!({
        "error": error_type,
        "message": message,
        "hint": hint,
        "fix": fix
    })
}

/// Helper to build config content programmatically
pub struct ConfigBuilder {
    token: String,
    access_token: String,
    expires_at: i64,
    workspace: String,
    project: String,
    no_color: bool,
    format: String,
    api_url: String,
    timeout_secs: u64,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            token: "tk_user_test".to_string(),
            access_token: "tk_access_test".to_string(),
            expires_at: now + 3600,
            workspace: "test-workspace".to_string(),
            project: "TEST".to_string(),
            no_color: false,
            format: "auto".to_string(),
            api_url: "https://api.tokanban.com".to_string(),
            timeout_secs: 30,
        }
    }

    pub fn token(mut self, token: &str) -> Self {
        self.token = token.to_string();
        self
    }

    pub fn workspace(mut self, workspace: &str) -> Self {
        self.workspace = workspace.to_string();
        self
    }

    pub fn project(mut self, project: &str) -> Self {
        self.project = project.to_string();
        self
    }

    pub fn no_color(mut self, no_color: bool) -> Self {
        self.no_color = no_color;
        self
    }

    pub fn expired_token(mut self) -> Self {
        let now = chrono::Utc::now().timestamp();
        self.expires_at = now - 100; // Already expired
        self
    }

    pub fn build(self) -> String {
        format!(
            r#"[auth]
token = "{}"
access_token = "{}"
expires_at = {}

[defaults]
workspace = "{}"
project = "{}"

[ui]
no_color = {}
format = "{}"

[api]
url = "{}"
timeout_secs = {}
"#,
            self.token,
            self.access_token,
            self.expires_at,
            self.workspace,
            self.project,
            self.no_color,
            self.format,
            self.api_url,
            self.timeout_secs
        )
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
