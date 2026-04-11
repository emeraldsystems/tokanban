/// Mock HTTP server for API testing using wiremock

use wiremock::{Mock, MockServer as WireMockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

/// Wrapper around wiremock's MockServer for convenient test setup
pub struct MockServer {
    pub inner: WireMockServer,
}

impl MockServer {
    /// Create a new mock server and start it
    pub async fn start() -> Self {
        let inner = WireMockServer::start().await;
        Self { inner }
    }

    /// Get the base URL for API calls
    pub fn base_url(&self) -> String {
        self.inner.uri()
    }

    /// Setup a mock GET endpoint
    pub async fn mock_get(&self, path_str: &str, response_json: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_json))
            .mount(&self.inner)
            .await;
    }

    /// Setup a mock POST endpoint
    pub async fn mock_post(&self, path_str: &str, response_json: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(201).set_body_json(response_json))
            .mount(&self.inner)
            .await;
    }

    /// Setup a mock DELETE endpoint
    pub async fn mock_delete(&self, path_str: &str, response_json: serde_json::Value) {
        Mock::given(method("DELETE"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_json))
            .mount(&self.inner)
            .await;
    }

    /// Setup a mock error response using the API's structured error format:
    /// { "error": { "code": "...", "message": "...", "details": "...", "hint": "..." } }
    pub async fn mock_error(&self, method_name: &str, path_str: &str, status: u16, error_json: serde_json::Value) {
        let method_matcher = match method_name.to_uppercase().as_str() {
            "GET" => method("GET"),
            "POST" => method("POST"),
            "PUT" => method("PUT"),
            "PATCH" => method("PATCH"),
            "DELETE" => method("DELETE"),
            _ => panic!("unknown method: {}", method_name),
        };

        Mock::given(method_matcher)
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(status).set_body_json(error_json))
            .mount(&self.inner)
            .await;
    }

    /// Setup a structured API error response.
    /// Matches the Tokanban API format: { "error": { "code": ..., "message": ..., ... } }
    pub async fn mock_api_error(
        &self,
        method_name: &str,
        path_str: &str,
        status: u16,
        code: &str,
        message: &str,
        hint: Option<&str>,
    ) {
        let error_body = serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "details": null,
                "hint": hint,
            }
        });
        self.mock_error(method_name, path_str, status, error_body).await;
    }

    /// Setup a 401 unauthorized response
    pub async fn mock_unauthorized(&self, path_str: &str) {
        self.mock_api_error(
            "GET",
            path_str,
            401,
            "AUTH_INVALID_TOKEN",
            "Invalid or expired token",
            Some("Run `tokanban auth login` to authenticate."),
        ).await;
    }

    /// Setup a 404 not found response
    pub async fn mock_not_found(&self, path_str: &str, resource: &str) {
        self.mock_api_error(
            "GET",
            path_str,
            404,
            "NOT_FOUND",
            &format!("{} not found", resource),
            Some(&format!("The {} you requested does not exist", resource)),
        ).await;
    }

    /// Setup a rate limit response
    pub async fn mock_rate_limit(&self, path_str: &str) {
        self.mock_api_error(
            "GET",
            path_str,
            429,
            "RATE_LIMIT",
            "Too many requests (60/minute)",
            Some("Wait before retrying"),
        ).await;
    }
}
