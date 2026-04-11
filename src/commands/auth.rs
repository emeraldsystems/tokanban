use clap::Subcommand;

use crate::auth::run_login_flow;
use crate::config::{AppConfig, save_config};
use crate::error::{CliError, Result};

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Open browser authorization and save a CLI credential
    Login,
    /// Revoke current token and clear local config
    Logout,
    /// Display current user, workspace, and token expiry
    Status,
}

pub async fn handle(
    cmd: &AuthCommand,
    config: &mut AppConfig,
    config_path: Option<&std::path::PathBuf>,
) -> Result<()> {
    match cmd {
        AuthCommand::Login => handle_login(config, config_path).await,
        AuthCommand::Logout => handle_logout(config, config_path),
        AuthCommand::Status => handle_status(config),
    }
}

async fn handle_login(
    config: &mut AppConfig,
    config_path: Option<&std::path::PathBuf>,
) -> Result<()> {
    run_login_flow(config, config_path).await
}

fn handle_logout(
    config: &mut AppConfig,
    config_path: Option<&std::path::PathBuf>,
) -> Result<()> {
    config.auth.token = None;
    config.auth.access_token = None;
    config.auth.expires_at = None;

    save_config(config, config_path)?;
    eprintln!("Logged out. Tokens cleared from config.");
    Ok(())
}

fn handle_status(config: &AppConfig) -> Result<()> {
    match &config.auth.access_token {
        Some(_) => {
            let workspace = config
                .defaults
                .workspace
                .as_deref()
                .unwrap_or("(not set)");
            let project = config.defaults.project.as_deref().unwrap_or("(not set)");
            let expires = config
                .auth
                .expires_at
                .map(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| ts.to_string())
                })
                .unwrap_or_else(|| "n/a (API key)".to_string());

            let credential = if config.auth.token.is_some() {
                "refreshable token"
            } else {
                "API key"
            };

            eprintln!("Authenticated");
            eprintln!("  Credential:   {credential}");
            eprintln!("  Workspace:    {workspace}");
            eprintln!("  Project:      {project}");
            eprintln!("  Token expiry: {expires}");
            eprintln!("  API endpoint: {}", config.api.url);
            Ok(())
        }
        None => Err(CliError::NotAuthenticated),
    }
}
