use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub defaults: DefaultsConfig,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub api: ApiConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Refresh token (long-lived)
    pub token: Option<String>,

    /// Access token (short-lived, auto-refreshed)
    pub access_token: Option<String>,

    /// Unix timestamp for access token expiry
    pub expires_at: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Default workspace slug
    pub workspace: Option<String>,

    /// Default project key
    pub project: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UiConfig {
    /// Disable color output
    #[serde(default)]
    pub no_color: bool,

    /// Default output format: auto, json, table, card
    #[serde(default = "default_format")]
    pub format: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            no_color: false,
            format: default_format(),
        }
    }
}

fn default_format() -> String {
    "auto".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiConfig {
    /// API base URL
    #[serde(default = "default_api_url")]
    pub url: String,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            url: default_api_url(),
            timeout_secs: default_timeout(),
        }
    }
}

fn default_api_url() -> String {
    "https://api.tokanban.com".to_string()
}

fn default_timeout() -> u64 {
    30
}
