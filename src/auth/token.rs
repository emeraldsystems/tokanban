use crate::api::ApiClient;
use crate::config::{AppConfig, save_config};
use crate::error::{CliError, Result};

/// Check if the access token is valid and refresh if needed.
/// Returns the current valid access token.
pub async fn ensure_valid_token(
    config: &mut AppConfig,
    api: &mut ApiClient,
    config_path: Option<&std::path::PathBuf>,
) -> Result<String> {
    let access_token = config.auth.access_token.as_ref();
    let refresh_token = config.auth.token.as_ref();
    let expires_at = config.auth.expires_at;
    let now = chrono::Utc::now().timestamp();

    if let Some(token) = access_token {
        if refresh_token.is_none() || expires_at.is_none() {
            return Ok(token.clone());
        }

        if now < expires_at.unwrap_or(0) - 60 {
            return Ok(token.clone());
        }
    }

    let refresh_token = refresh_token.ok_or(CliError::NotAuthenticated)?;

    // Refresh the token
    let tokens = api
        .refresh_token(refresh_token)
        .await
        .map_err(|_| CliError::TokenRefreshFailed)?;

    let new_access = tokens.access_token.clone();
    config.auth.access_token = Some(tokens.access_token);
    config.auth.token = Some(tokens.refresh_token);
    config.auth.expires_at = Some(now + tokens.expires_in);

    save_config(config, config_path)?;
    api.set_access_token(new_access.clone());

    Ok(new_access)
}
