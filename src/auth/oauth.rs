use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;
use url::Url;

use crate::config::{AppConfig, save_config};
use crate::error::{CliError, Result};

/// Generate a cryptographically random string for session IDs.
fn random_string(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|_| rand::rng().random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

#[derive(Debug, Deserialize)]
struct CliAuthStatus {
    authorized: bool,
    api_key: Option<String>,
    user_id: Option<String>,
    workspace_id: Option<String>,
    name: Option<String>,
}

fn app_base_url(api_base_url: &str) -> Result<String> {
    let mut url = Url::parse(api_base_url)
        .map_err(|e| CliError::Config(format!("Invalid API URL '{}': {e}", api_base_url)))?;

    if let Some(host) = url.host_str() {
        if host.starts_with("api.") {
            let app_host = host.replacen("api.", "app.", 1);
            url.set_host(Some(&app_host))
                .map_err(|_| CliError::Config(format!("Could not derive app host from '{}'.", api_base_url)))?;
        }
    }

    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);

    Ok(url.as_str().trim_end_matches('/').to_string())
}

async fn fetch_cli_auth_status(client: &Client, app_base: &str, session_id: &str) -> Result<CliAuthStatus> {
    let response = client
        .get(format!("{}/auth/cli/status", app_base))
        .query(&[("session_id", session_id)])
        .send()
        .await?;

    if response.status().is_success() {
        return Ok(response.json::<CliAuthStatus>().await?);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(CliError::Config(format!(
        "CLI authorization check failed with HTTP {}. {}",
        status.as_u16(),
        if body.is_empty() {
            "Ensure the app auth/cli endpoints are deployed."
        } else {
            body.trim()
        }
    )))
}

fn persist_cli_auth(config: &mut AppConfig, status: &CliAuthStatus, api_key: String) {
    let previous_workspace = config.defaults.workspace.clone();

    config.auth.token = None;
    config.auth.access_token = Some(api_key);
    config.auth.expires_at = None;

    if let Some(workspace_id) = &status.workspace_id {
        if previous_workspace.as_deref() != Some(workspace_id.as_str()) {
            config.defaults.project = None;
        }
        config.defaults.workspace = Some(workspace_id.clone());
    }
}

/// Run the browser-based CLI authorization flow backed by the Pages app.
pub async fn run_login_flow(config: &mut AppConfig, config_path: Option<&std::path::PathBuf>) -> Result<()> {
    let session_id = format!("sess_cli_{}", random_string(18));
    let app_base = app_base_url(&config.api.url)?;
    let auth_url = format!(
        "{}/auth/cli?session={}",
        app_base,
        url::form_urlencoded::byte_serialize(session_id.as_bytes()).collect::<String>(),
    );

    eprintln!("Opening browser for authentication...");
    eprintln!("If the browser does not open, visit:");
    eprintln!("  {auth_url}");

    if let Err(e) = open::that(&auth_url) {
        eprintln!("Could not open browser: {e}");
        eprintln!("Open the URL above manually.");
    }

    eprintln!("Waiting for authorization...");

    let client = Client::builder()
        .timeout(Duration::from_secs(config.api.timeout_secs))
        .build()?;

    for _ in 0..150 {
        let status = fetch_cli_auth_status(&client, &app_base, &session_id).await?;
        if status.authorized {
            let api_key = status.api_key.clone().ok_or_else(|| {
                CliError::Config("CLI authorization completed without returning an API key.".into())
            })?;

            persist_cli_auth(config, &status, api_key);

            save_config(config, config_path)?;

            eprintln!("Authenticated.");
            if let Some(name) = status.name {
                eprintln!("  User: {name}");
            }
            if let Some(workspace_id) = status.workspace_id {
                eprintln!("  Workspace ID: {workspace_id}");
            }
            if let Some(user_id) = status.user_id {
                eprintln!("  User ID: {user_id}");
            }
            return Ok(());
        }

        sleep(Duration::from_secs(2)).await;
    }

    Err(CliError::Config(
        "Authentication timed out. Complete authorization in the browser and retry.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{CliAuthStatus, persist_cli_auth};
    use crate::config::AppConfig;

    fn authorized_status(workspace_id: Option<&str>) -> CliAuthStatus {
        CliAuthStatus {
            authorized: true,
            api_key: Some("tk_user_test".to_string()),
            user_id: Some("user_123".to_string()),
            workspace_id: workspace_id.map(ToString::to_string),
            name: Some("Test User".to_string()),
        }
    }

    #[test]
    fn persist_cli_auth_sets_workspace_default() {
        let mut config = AppConfig::default();
        let status = authorized_status(Some("ws_123"));

        persist_cli_auth(&mut config, &status, "tk_user_static".to_string());

        assert_eq!(config.auth.access_token.as_deref(), Some("tk_user_static"));
        assert_eq!(config.auth.token, None);
        assert_eq!(config.auth.expires_at, None);
        assert_eq!(config.defaults.workspace.as_deref(), Some("ws_123"));
    }

    #[test]
    fn persist_cli_auth_clears_project_when_workspace_changes() {
        let mut config = AppConfig::default();
        config.defaults.workspace = Some("ws_old".to_string());
        config.defaults.project = Some("PLAT".to_string());

        let status = authorized_status(Some("ws_new"));
        persist_cli_auth(&mut config, &status, "tk_user_static".to_string());

        assert_eq!(config.defaults.workspace.as_deref(), Some("ws_new"));
        assert_eq!(config.defaults.project, None);
    }

    #[test]
    fn persist_cli_auth_preserves_project_when_workspace_matches() {
        let mut config = AppConfig::default();
        config.defaults.workspace = Some("ws_same".to_string());
        config.defaults.project = Some("PLAT".to_string());

        let status = authorized_status(Some("ws_same"));
        persist_cli_auth(&mut config, &status, "tk_user_static".to_string());

        assert_eq!(config.defaults.workspace.as_deref(), Some("ws_same"));
        assert_eq!(config.defaults.project.as_deref(), Some("PLAT"));
    }
}
