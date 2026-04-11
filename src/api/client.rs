use reqwest::{Client, Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;

use crate::error::{CliError, Result};

use super::types::{ApiErrorResponse, TokenResponse};

/// HTTP client wrapper for the Tokanban REST API.
pub struct ApiClient {
    client: Client,
    base_url: String,
    access_token: Option<String>,
}

impl ApiClient {
    pub fn new(base_url: &str, timeout_secs: u64, access_token: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            access_token,
        })
    }

    pub fn set_access_token(&mut self, token: String) {
        self.access_token = Some(token);
    }

    /// Send a GET request to the given API path.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::GET, path, Option::<&()>::None).await
    }

    /// Send a POST request with a JSON body.
    pub async fn post<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.request(Method::POST, path, Some(body)).await
    }

    /// Send a PATCH request with a JSON body.
    pub async fn patch<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    /// Send a DELETE request.
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::DELETE, path, Option::<&()>::None)
            .await
    }

    /// Send a multipart POST request (used for file uploads, e.g., import).
    pub async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).multipart(form);
        if let Some(token) = &self.access_token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        Self::parse_response(resp).await
    }

    /// Exchange an authorization code for tokens (used in OAuth flow).
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse> {
        let body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
            "client_id": "tokanban-cli",
        });

        let resp = self
            .client
            .post(format!("{}/oauth/token", self.base_url))
            .json(&body)
            .send()
            .await?;

        Self::parse_response(resp).await
    }

    /// Refresh an access token using a refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": "tokanban-cli",
        });

        let resp = self
            .client
            .post(format!("{}/oauth/token", self.base_url))
            .json(&body)
            .send()
            .await?;

        Self::parse_response(resp).await
    }

    async fn request<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);

        let mut req = self.client.request(method, &url);

        if let Some(token) = &self.access_token {
            req = req.bearer_auth(token);
        }

        if let Some(body) = body {
            req = req.json(body);
        }

        let resp = req.send().await?;
        Self::parse_response(resp).await
    }

    async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T> {
        let status = resp.status();

        if status.is_success() {
            let parsed = resp.json::<T>().await?;
            return Ok(parsed);
        }

        // Try to parse structured error from API
        let body = resp.text().await.unwrap_or_default();
        if let Ok(api_err) = serde_json::from_str::<ApiErrorResponse>(&body) {
            return Err(CliError::Api {
                code: api_err.error.code,
                message: api_err.error.message,
                details: api_err.error.details,
                hint: api_err.error.hint,
            });
        }

        // Fallback for non-structured errors
        Err(CliError::Api {
            code: status.as_str().to_string(),
            message: if body.is_empty() {
                status
                    .canonical_reason()
                    .unwrap_or("Unknown error")
                    .to_string()
            } else {
                body
            },
            details: None,
            hint: if status == StatusCode::UNAUTHORIZED {
                Some("Run `tokanban auth login` to authenticate.".into())
            } else {
                None
            },
        })
    }
}
