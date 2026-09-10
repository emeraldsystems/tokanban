//! Read-only diagnostics for CLI configuration, Claude hooks and usage history.
//! Offline by default. An explicit --online performs one bounded authenticated
//! MCP tools/list request for the reporter's selected account. Neither mode
//! logs in, refreshes credentials, writes config/state, or reports session usage.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Args;
use serde::Serialize;
use serde_json::{json, Value};

use crate::commands::session::{resolve_reporter_connection, ReportStatusCode};
use crate::config::{self, AppConfig};
use crate::error::Result;
use crate::format::{colors, ColorConfig, OutputFormat, EM_DASH};

/// A report is considered stale once its last successful send is older than this.
const STALE_THRESHOLD_SECS: i64 = 24 * 3600;
/// Small allowance for clock skew before a timestamp is called "in the future".
const FUTURE_SKEW_ALLOWANCE_SECS: i64 = 60;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTION_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default, Args)]
pub struct DoctorArgs {
    /// Check the current reporter account using an authenticated, read-only MCP request
    #[arg(long, alias = "check-connection")]
    pub online: bool,
    /// Select the same Claude MCP config used by the reporter
    #[arg(long, requires = "online")]
    pub claude_config: Option<PathBuf>,
    /// Select the reporter's exact MCP endpoint; credentials must match that endpoint
    #[arg(long, requires = "online")]
    pub mcp_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Resolved input paths (kept separate from the checks themselves so tests can
// inject temporary directories instead of mutating process-global HOME/env).
// ---------------------------------------------------------------------------

/// Every filesystem location `doctor` looks at, pre-resolved by the caller.
///
/// `handle` resolves these from the real environment (HOME, CLAUDE_CONFIG_DIR,
/// cwd, platform config dir); tests construct this directly with fixture paths.
#[derive(Debug, Clone)]
pub struct DoctorPaths {
    /// Fully resolved tokanban config file path, if a platform config dir
    /// (or explicit `--config`) could be determined.
    pub config_path: Option<PathBuf>,
    /// Whether `config_path` came from `--config` rather than the platform default.
    pub config_is_override: bool,
    /// Resolved Claude Code home directory (`$CLAUDE_CONFIG_DIR` or `~/.claude`).
    pub claude_dir: Option<PathBuf>,
    /// Whether `claude_dir` came from the `CLAUDE_CONFIG_DIR` env var.
    pub claude_dir_from_env: bool,
    /// `~/.claude.json`, used by Claude Code for MCP server registration.
    pub home_claude_json: Option<PathBuf>,
    /// `.claude` directory relative to the current project (cwd).
    pub project_claude_dir: PathBuf,
    /// Tokanban usage-reporter state directory (see `commands::session`).
    pub state_dir: Option<PathBuf>,
}

fn resolve_doctor_paths(config_override: Option<&Path>) -> DoctorPaths {
    let config_is_override = config_override.is_some();
    let config_path = match config_override {
        Some(p) if p.is_relative() => std::env::current_dir().ok().map(|dir| dir.join(p)),
        Some(p) => Some(p.to_path_buf()),
        None => config::config_path().ok(),
    };

    let claude_dir_env = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let claude_dir_from_env = claude_dir_env.is_some();
    let home_dir = dirs::home_dir();
    let claude_dir = claude_dir_env.or_else(|| home_dir.as_ref().map(|h| h.join(".claude")));
    let home_claude_json = home_dir.as_ref().map(|h| h.join(".claude.json"));

    let project_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_claude_dir = project_dir.join(".claude");

    let state_dir = config::config_dir().ok().map(|d| d.join("usage-state"));

    DoctorPaths {
        config_path,
        config_is_override,
        claude_dir,
        claude_dir_from_env,
        home_claude_json,
        project_claude_dir,
        state_dir,
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Report shape
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub cli_version: String,
    pub config: ConfigCheck,
    pub claude: ClaudeCheck,
    pub reporter: ReporterCheck,
    pub connection: ConnectionCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    NotChecked,
    Connected,
    InvalidConfig,
    CredentialsMissing,
    UnsafeEndpoint,
    AuthRejected,
    Forbidden,
    RateLimited,
    NetworkError,
    Timeout,
    RedirectRejected,
    ServerError,
    EndpointRejected,
    MalformedResponse,
    ResponseTooLarge,
}

#[derive(Debug, Serialize)]
pub struct ConnectionCheck {
    pub status: ConnectionStatus,
    /// Current selected account only; independent of the aggregate historical reporter check.
    pub account_scope: &'static str,
    pub checked_at: Option<String>,
    pub endpoint_origin: Option<String>,
    pub account_fingerprint: Option<String>,
    pub http_status: Option<u16>,
    pub detail: &'static str,
}

impl ConnectionCheck {
    fn new(status: ConnectionStatus) -> Self {
        let mut value = Self {
            status,
            account_scope: "current_reporter_account",
            checked_at: None,
            endpoint_origin: None,
            account_fingerprint: None,
            http_status: None,
            detail: "",
        };
        value.set_status(status);
        value
    }
    fn set_status(&mut self, status: ConnectionStatus) {
        self.status = status;
        self.detail = match status {
            ConnectionStatus::NotChecked => "Current authentication reachability is unknown. Use --online for a read-only connection check.",
            ConnectionStatus::Connected => "The selected account reached the MCP endpoint and received a valid authenticated tools list. This does not verify usage reporting or billing.",
            ConnectionStatus::InvalidConfig => "The selected CLI configuration could not be read safely. Correct its configuration diagnostic before checking the connection.",
            ConnectionStatus::CredentialsMissing => "No usable credential was found for the selected reporter account and endpoint. Check TOKANBAN_API_KEY, the selected Claude MCP configuration, and CLI credentials; no other account was tried.",
            ConnectionStatus::UnsafeEndpoint => "The endpoint is unsafe for credentials. Use HTTPS, or HTTP only for loopback, without userinfo, query, or fragment.",
            ConnectionStatus::AuthRejected => "The endpoint rejected authentication. Check or replace the selected account's credential.",
            ConnectionStatus::Forbidden => "The endpoint refused this account's access. Check its workspace membership and permissions.",
            ConnectionStatus::RateLimited => "The endpoint rate-limited this check. Retry later; the command does not retry automatically.",
            ConnectionStatus::NetworkError => "The endpoint could not be reached. Check network connectivity and the selected API endpoint.",
            ConnectionStatus::Timeout => "The connection check exceeded its five-second deadline. Check the endpoint and retry later.",
            ConnectionStatus::RedirectRejected => "The endpoint returned a redirect. It was not followed and credentials were not sent to the redirect destination.",
            ConnectionStatus::ServerError => "The endpoint returned a server error. Retry later.",
            ConnectionStatus::EndpointRejected => "The endpoint rejected the tools/list request. Check the selected MCP endpoint.",
            ConnectionStatus::MalformedResponse => "The endpoint did not return a valid matching MCP tools/list response. Check the endpoint and server version.",
            ConnectionStatus::ResponseTooLarge => "The tools/list response exceeded the one-MiB diagnostic limit. The response was discarded.",
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigStatus {
    Ok,
    Missing,
    InsecurePermissions,
    InvalidToml,
    Unreadable,
    UnknownPath,
}

#[derive(Debug, Serialize)]
pub struct ConfigCheck {
    pub path: Option<String>,
    pub source: &'static str,
    pub exists: bool,
    pub status: ConfigStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions_mode: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ConfigSummary>,
}

/// Non-secret fields only: never includes token/access_token values.
#[derive(Debug, Serialize)]
pub struct ConfigSummary {
    pub workspace: Option<String>,
    pub project: Option<String>,
    pub api_url: String,
    pub credentials_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_state: Option<&'static str>,
}

impl ConfigSummary {
    fn from_config(config: &AppConfig, now: u64) -> Self {
        let credentials_configured =
            [&config.auth.access_token, &config.auth.token]
                .iter()
                .any(|token| {
                    token
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty())
                });
        let token_state = if !credentials_configured {
            None
        } else {
            Some(match config.auth.expires_at {
                None => "no_expiry",
                Some(exp) if exp <= now as i64 => "expired",
                Some(_) => "not_expired",
            })
        };
        ConfigSummary {
            workspace: config.defaults.workspace.clone(),
            project: config.defaults.project.clone(),
            // URLs can contain passwords or API keys in userinfo, paths and
            // query parameters. Only the origin is safe diagnostic output.
            api_url: url::Url::parse(&config.api.url)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|| "(invalid API URL)".to_string()),
            credentials_configured,
            token_state,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Detected,
    NotDetected,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct ClaudeFileCheck {
    pub label: String,
    pub path: String,
    pub exists: bool,
    pub readable: bool,
}

#[derive(Debug, Serialize)]
pub struct ClaudeCheck {
    pub claude_config_dir: Option<String>,
    pub claude_config_dir_source: &'static str,
    pub files: Vec<ClaudeFileCheck>,
    pub hook_status: Presence,
    pub startup_hook_status: Presence,
    pub plugin_status: Presence,
    pub plugin_enabled: Option<bool>,
    pub mcp_status: Presence,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReporterStatus {
    NoStateYet,
    Ok,
    Stale,
    FutureTimestamp,
    Invalid,
    DirectoryUnreadable,
}

#[derive(Debug, Serialize)]
pub struct ReporterCheck {
    pub state_dir: Option<String>,
    pub state_dir_exists: bool,
    pub session_count: usize,
    pub invalid_file_count: usize,
    pub status: ReporterStatus,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report: Option<LastReportSummary>,
    /// The most recent report attempt across all accounts/sessions in the
    /// state directory, success or not. Surfaced separately from
    /// `last_report` (which only reflects successful/heartbeat reports) so
    /// an ongoing failure is visible even before it has ever succeeded, or
    /// even if it happened after the last success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<LastAttemptSummary>,
}

#[derive(Debug, Serialize)]
pub struct LastReportSummary {
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_session_id: Option<String>,
    pub last_report_at: String,
    pub age_seconds: i64,
    pub total_tokens: Option<u64>,
    /// Outcome of the reporter's own auto-triggered `session_end` call for
    /// this session, if any — tracked separately so a failed close-out never
    /// masquerades as (or is masked by) a successful usage report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_end_status: Option<ReportStatusCode>,
}

#[derive(Debug, Serialize)]
pub struct LastAttemptSummary {
    pub status: ReportStatusCode,
    pub label: &'static str,
    pub at: String,
    pub age_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub next_step: &'static str,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn handle(
    args: &DoctorArgs,
    config_override: Option<&PathBuf>,
    api_url_override: Option<&str>,
    format: OutputFormat,
    no_color: bool,
) -> Result<()> {
    let paths = resolve_doctor_paths(config_override.map(PathBuf::as_path));
    let mut report = build_report(&paths, now_unix());
    if args.online {
        report.connection = check_connection(args, &paths, api_url_override).await;
    }
    let color = ColorConfig::new(no_color);

    match format.resolve() {
        OutputFormat::Json => crate::format::print_json(&report),
        _ => print!("{}", render_human(&report, &color)),
    }

    Ok(())
}

/// Build the full diagnostic report from pre-resolved paths. Pure and
/// filesystem-read-only: safe to call directly from tests with fixture paths.
pub fn build_report(paths: &DoctorPaths, now: u64) -> DoctorReport {
    DoctorReport {
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        config: inspect_config(paths, now),
        claude: inspect_claude(paths),
        reporter: inspect_reporter(paths, now),
        connection: ConnectionCheck::new(ConnectionStatus::NotChecked),
    }
}

async fn check_connection(
    args: &DoctorArgs,
    paths: &DoctorPaths,
    api_url_override: Option<&str>,
) -> ConnectionCheck {
    let mut report = ConnectionCheck::new(ConnectionStatus::InvalidConfig);
    report.checked_at = Some(chrono::Utc::now().to_rfc3339());
    let Some(config_path) = paths.config_path.as_ref() else {
        return report;
    };
    let Ok(mut config) = config::load_config(Some(config_path)) else {
        return report;
    };
    if let Some(endpoint) = api_url_override {
        config.api.url = endpoint.to_string();
    }
    let cwd = std::env::current_dir().ok();
    let selected = resolve_reporter_connection(
        &config,
        args.claude_config.as_deref(),
        cwd.as_deref(),
        args.mcp_url.as_deref(),
    );
    report.endpoint_origin = url::Url::parse(&selected.endpoint)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.origin().ascii_serialization());
    report.account_fingerprint = Some(selected.account_fingerprint);
    if selected.endpoint_status.is_some() {
        report.set_status(ConnectionStatus::UnsafeEndpoint);
        return report;
    }
    let Some(credential) = selected.credential else {
        report.set_status(ConnectionStatus::CredentialsMissing);
        return report;
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(CONNECTION_TIMEOUT)
        .connect_timeout(CONNECTION_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .build()
    else {
        report.set_status(ConnectionStatus::NetworkError);
        return report;
    };
    let response = client.post(&selected.endpoint).bearer_auth(credential)
        .header("MCP-Protocol-Version", "2024-11-05")
        .json(&json!({ "jsonrpc": "2.0", "id": "tokanban-doctor", "method": "tools/list", "params": {} }))
        .send().await;
    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            report.set_status(if error.is_timeout() {
                ConnectionStatus::Timeout
            } else {
                ConnectionStatus::NetworkError
            });
            return report;
        }
    };
    let status = response.status();
    report.http_status = Some(status.as_u16());
    if !status.is_success() {
        report.set_status(match status.as_u16() {
            300..=399 => ConnectionStatus::RedirectRejected,
            401 => ConnectionStatus::AuthRejected,
            403 => ConnectionStatus::Forbidden,
            429 => ConnectionStatus::RateLimited,
            500..=599 => ConnectionStatus::ServerError,
            _ => ConnectionStatus::EndpointRejected,
        });
        return report;
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_CONNECTION_RESPONSE_BYTES as u64)
    {
        report.set_status(ConnectionStatus::ResponseTooLarge);
        return report;
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk))
                if chunk.len() > MAX_CONNECTION_RESPONSE_BYTES.saturating_sub(body.len()) =>
            {
                report.set_status(ConnectionStatus::ResponseTooLarge);
                return report;
            }
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(error) => {
                report.set_status(if error.is_timeout() {
                    ConnectionStatus::Timeout
                } else {
                    ConnectionStatus::NetworkError
                });
                return report;
            }
        }
    }
    let valid = serde_json::from_slice::<Value>(&body)
        .ok()
        .is_some_and(|value| {
            value["jsonrpc"] == "2.0"
                && value["id"] == "tokanban-doctor"
                && value.get("error").is_none()
                && value.get("result").is_some_and(Value::is_object)
                && value
                    .pointer("/result/tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        tools.len() <= 2000
                            && tools.iter().all(|tool| {
                                tool.get("name")
                                    .and_then(Value::as_str)
                                    .is_some_and(|name| !name.is_empty() && name.len() <= 256)
                                    && tool.pointer("/inputSchema/type").and_then(Value::as_str)
                                        == Some("object")
                            })
                    })
        });
    report.set_status(if valid {
        ConnectionStatus::Connected
    } else {
        ConnectionStatus::MalformedResponse
    });
    report
}

// ---------------------------------------------------------------------------
// Config check
// ---------------------------------------------------------------------------

fn inspect_config(paths: &DoctorPaths, now: u64) -> ConfigCheck {
    let source = if paths.config_is_override {
        "cli_flag"
    } else {
        "default"
    };

    let path = match &paths.config_path {
        Some(p) => p.clone(),
        None => {
            return ConfigCheck {
                path: None,
                source,
                exists: false,
                status: ConfigStatus::UnknownPath,
                permissions_mode: None,
                detail: "Could not determine the default config directory for this platform. Pass --config explicitly.".to_string(),
                summary: None,
            };
        }
    };
    let path_str = path.display().to_string();

    let metadata = match fs::metadata(&path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return ConfigCheck {
                path: Some(path_str),
                source,
                exists: false,
                status: ConfigStatus::Unreadable,
                permissions_mode: None,
                detail: format!("Config file could not be inspected ({:?}).", error.kind()),
                summary: None,
            };
        }
    };
    if metadata.is_none() {
        return ConfigCheck {
            path: Some(path_str),
            source,
            exists: false,
            status: ConfigStatus::Missing,
            permissions_mode: None,
            detail: "No config file found; the CLI will use built-in defaults until `tokanban auth login` runs.".to_string(),
            summary: Some(ConfigSummary::from_config(&AppConfig::default(), now)),
        };
    }
    if !metadata.as_ref().is_some_and(|value| value.is_file()) {
        return ConfigCheck {
            path: Some(path_str),
            source,
            exists: true,
            status: ConfigStatus::Unreadable,
            permissions_mode: None,
            detail: "Config path does not point to a regular file.".to_string(),
            summary: None,
        };
    }

    #[cfg(unix)]
    let mode: Option<u32> = {
        use std::os::unix::fs::PermissionsExt;
        metadata.as_ref().map(|m| m.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode: Option<u32> = None;

    if let Some(mode) = mode {
        if mode > 0o600 {
            return ConfigCheck {
                path: Some(path_str),
                source,
                exists: true,
                status: ConfigStatus::InsecurePermissions,
                permissions_mode: Some(format!("{mode:o}")),
                detail: format!(
                    "Config file permissions are {mode:o}; the CLI requires private permissions. Set this file's permissions to 0600."
                ),
                summary: None,
            };
        }
    }

    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return ConfigCheck {
                path: Some(path_str),
                source,
                exists: true,
                status: ConfigStatus::Unreadable,
                permissions_mode: mode.map(|m| format!("{m:o}")),
                detail: format!("Config file could not be read ({:?}).", e.kind()),
                summary: None,
            };
        }
    };

    match toml::from_str::<AppConfig>(&contents) {
        Ok(config) => ConfigCheck {
            path: Some(path_str),
            source,
            exists: true,
            status: ConfigStatus::Ok,
            permissions_mode: mode.map(|m| format!("{m:o}")),
            detail: "Config file is valid. Credentials have not been checked with the server.".to_string(),
            summary: Some(ConfigSummary::from_config(&config, now)),
        },
        // Deliberately drop the underlying TOML error: `toml`'s Display output
        // quotes the offending source line, which can echo secret values.
        Err(_) => ConfigCheck {
            path: Some(path_str),
            source,
            exists: true,
            status: ConfigStatus::InvalidToml,
            permissions_mode: mode.map(|m| format!("{m:o}")),
            detail: "Config file exists but is not valid TOML. Fix or remove it (contents are not shown here to avoid leaking secrets).".to_string(),
            summary: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Claude usage hook / plugin check
// ---------------------------------------------------------------------------

fn inspect_claude(paths: &DoctorPaths) -> ClaudeCheck {
    let mut candidates: Vec<(&'static str, PathBuf)> = Vec::new();

    if let Some(dir) = &paths.claude_dir {
        candidates.push(("global settings", dir.join("settings.json")));
        candidates.push(("global local settings", dir.join("settings.local.json")));
        candidates.push(("plugin registry", dir.join("plugins").join("config.json")));
        candidates.push((
            "installed plugins",
            dir.join("plugins").join("installed_plugins.json"),
        ));
    }
    candidates.push((
        "project settings",
        paths.project_claude_dir.join("settings.json"),
    ));
    candidates.push((
        "project local settings",
        paths.project_claude_dir.join("settings.local.json"),
    ));
    if let Some(claude_json) = &paths.home_claude_json {
        candidates.push(("mcp config (~/.claude.json)", claude_json.clone()));
    }
    if let Some(dir) = &paths.claude_dir {
        let dir_claude_json = dir.join(".claude.json");
        if paths.home_claude_json.as_deref() != Some(dir_claude_json.as_path()) {
            candidates.push(("mcp config (claude dir)", dir_claude_json));
        }
    }

    let mut files = Vec::with_capacity(candidates.len());
    let mut hook_found = false;
    let mut startup_hook_found = false;
    let mut plugin_found = false;
    let mut plugin_settings = std::collections::BTreeMap::new();
    let mut mcp_found = false;
    let mut any_readable = false;

    for (label, path) in candidates {
        let exists = path.exists();
        let mut readable = false;

        if exists {
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(value) = serde_json::from_str::<Value>(&contents) {
                    readable = true;
                    any_readable = true;
                    if json_has_hook_command(&value, "tokanban") {
                        hook_found = true;
                    }
                    startup_hook_found |= json_has_named_hook(
                        &value,
                        "tokanban",
                        &["SessionStart"],
                        "session start-hook",
                    );
                    if json_mentions_plugin(&value) {
                        plugin_found = true;
                    }
                    if let Some(settings) = value.get("enabledPlugins").and_then(Value::as_object) {
                        for (key, enabled) in settings {
                            if is_tokanban_plugin(key) {
                                if let Some(enabled) = enabled.as_bool() {
                                    plugin_settings.insert(key.clone(), enabled);
                                }
                            }
                        }
                    }
                    if json_has_tokanban_mcp_server(&value) {
                        mcp_found = true;
                    }
                }
            }
        }

        files.push(ClaudeFileCheck {
            label: label.to_string(),
            path: path.display().to_string(),
            exists,
            readable,
        });
    }

    let verdict = |found: bool| -> Presence {
        if found {
            Presence::Detected
        } else if any_readable {
            Presence::NotDetected
        } else {
            Presence::Unknown
        }
    };

    let (claude_config_dir, claude_config_dir_source) =
        match (&paths.claude_dir, paths.claude_dir_from_env) {
            (Some(dir), true) => (Some(dir.display().to_string()), "CLAUDE_CONFIG_DIR"),
            (Some(dir), false) => (Some(dir.display().to_string()), "default"),
            (None, _) => (None, "unknown"),
        };

    ClaudeCheck {
        claude_config_dir,
        claude_config_dir_source,
        files,
        hook_status: verdict(hook_found),
        startup_hook_status: verdict(startup_hook_found),
        plugin_status: verdict(plugin_found),
        plugin_enabled: if plugin_settings.is_empty() {
            None
        } else {
            Some(plugin_settings.values().any(|enabled| *enabled))
        },
        mcp_status: verdict(mcp_found),
        detail: "Local settings scan for a Tokanban SessionStart mapping hook and Stop/SessionEnd reporter hook, plugin registration and MCP entry. Plugin enabled state reflects explicit settings, with project-local settings taking precedence. Managed policy, plugin default enablement and runtime loading are not checked; plugin-managed hooks may be absent from these files. Detection does not confirm successful reporting.".to_string(),
    }
}

fn json_has_hook_command(value: &Value, needle: &str) -> bool {
    json_has_named_hook(
        value,
        needle,
        &["Stop", "SessionEnd"],
        "session report-usage",
    )
}

fn json_has_named_hook(value: &Value, needle: &str, events: &[&str], command: &str) -> bool {
    let needle_lower = needle.to_ascii_lowercase();
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    for event in events {
        let Some(entries) = hooks.get(*event).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(hook_list) = entry.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for hook in hook_list {
                if hook.get("type").and_then(Value::as_str) != Some("command") {
                    continue;
                }
                if let Some(cmd) = hook.get("command").and_then(Value::as_str) {
                    let normalized = cmd
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_ascii_lowercase();
                    if normalized.contains(&needle_lower) && normalized.contains(command) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn is_tokanban_plugin(key: &str) -> bool {
    key.split('@')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("tokanban"))
}

fn json_mentions_plugin(value: &Value) -> bool {
    ["enabledPlugins", "plugins"].iter().any(|section| {
        value
            .get(section)
            .and_then(Value::as_object)
            .is_some_and(|entries| entries.keys().any(|key| is_tokanban_plugin(key)))
    })
}

/// Only checks for the presence of the `tokanban` key; never reads header values,
/// so an Authorization token cannot leak into the report.
fn json_has_tokanban_mcp_server(value: &Value) -> bool {
    value
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|servers| servers.contains_key("tokanban"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Usage reporter state check
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct ReporterTotals {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
}

impl ReporterTotals {
    fn total(&self) -> Option<u64> {
        self.input_tokens
            .checked_add(self.output_tokens)?
            .checked_add(self.cache_read_tokens)?
            .checked_add(self.cache_write_tokens)
    }
}

#[derive(Debug, serde::Deserialize)]
struct ReporterAttempt {
    at_unix: u64,
    status: ReportStatusCode,
}

#[derive(Debug, serde::Deserialize)]
struct ReporterStateFile {
    last_report_unix: u64,
    last_session_id: Option<String>,
    last_totals: ReporterTotals,
    #[serde(default)]
    usage_measured: Option<bool>,
    #[serde(default)]
    harness_session_id: Option<String>,
    #[serde(default)]
    last_attempt: Option<ReporterAttempt>,
    #[serde(default)]
    session_end: Option<ReporterAttempt>,
}

fn inspect_reporter(paths: &DoctorPaths, now: u64) -> ReporterCheck {
    let Some(state_dir) = &paths.state_dir else {
        return ReporterCheck {
            state_dir: None,
            state_dir_exists: false,
            session_count: 0,
            invalid_file_count: 0,
            status: ReporterStatus::DirectoryUnreadable,
            detail: "Could not determine the platform config directory, so the reporter state location is unknown.".to_string(),
            last_report: None,
            last_attempt: None,
        };
    };
    let state_dir_str = state_dir.display().to_string();

    if matches!(state_dir.try_exists(), Ok(false)) {
        return ReporterCheck {
            state_dir: Some(state_dir_str),
            state_dir_exists: false,
            session_count: 0,
            invalid_file_count: 0,
            status: ReporterStatus::NoStateYet,
            detail: "No reporter state directory found at the default location. Reporting history is unknown; hooks using --state-dir may store it elsewhere.".to_string(),
            last_report: None,
            last_attempt: None,
        };
    }

    let entries = match fs::read_dir(state_dir) {
        Ok(entries) => entries,
        Err(e) => {
            return ReporterCheck {
                state_dir: Some(state_dir_str),
                state_dir_exists: true,
                session_count: 0,
                invalid_file_count: 0,
                status: ReporterStatus::DirectoryUnreadable,
                detail: format!(
                    "Reporter state directory exists but could not be read ({:?}).",
                    e.kind()
                ),
                last_report: None,
                last_attempt: None,
            };
        }
    };

    let mut newest: Option<ReporterStateFile> = None;
    let mut newest_attempt: Option<(u64, ReportStatusCode, Option<String>)> = None;
    let mut session_count = 0usize;
    let mut invalid_file_count = 0usize;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                invalid_file_count += 1;
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        session_count += 1;
        let Some(state) = fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str::<ReporterStateFile>(&c).ok())
        else {
            invalid_file_count += 1;
            continue;
        };

        if let Some(attempt) = &state.last_attempt {
            let is_newer = newest_attempt
                .as_ref()
                .map(|(at, _, _)| attempt.at_unix > *at)
                .unwrap_or(true);
            if is_newer {
                let session_id = state
                    .harness_session_id
                    .clone()
                    .or_else(|| state.last_session_id.clone());
                newest_attempt = Some((attempt.at_unix, attempt.status, session_id));
            }
        }

        let success_valid = state.last_report_unix > 0
            && i64::try_from(state.last_report_unix)
                .ok()
                .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
                .is_some()
            && state.last_totals.total().is_some();

        if success_valid {
            let is_newer = newest
                .as_ref()
                .map(|n| state.last_report_unix > n.last_report_unix)
                .unwrap_or(true);
            if is_newer {
                newest = Some(state);
            }
        } else if state.last_attempt.is_none() {
            invalid_file_count += 1;
        }
    }

    let last_attempt = last_attempt_summary(newest_attempt, now);

    match newest {
        None => {
            let (status, detail) = if invalid_file_count > 0 {
                (
                    ReporterStatus::Invalid,
                    "Reporter state files are present but could not be parsed as valid usage state.".to_string(),
                )
            } else {
                (
                    ReporterStatus::NoStateYet,
                    "No accepted usage update or heartbeat has been recorded yet. Check the latest attempt below.".to_string(),
                )
            };
            ReporterCheck {
                state_dir: Some(state_dir_str),
                state_dir_exists: true,
                session_count,
                invalid_file_count,
                status,
                detail,
                last_report: None,
                last_attempt,
            }
        }
        Some(state) => {
            let age_seconds = i64::try_from(now)
                .unwrap_or(i64::MAX)
                .saturating_sub(state.last_report_unix as i64);
            let total_tokens = state
                .last_totals
                .total()
                .expect("validated reporter totals");
            let total_tokens = match state.usage_measured {
                Some(true) => Some(total_tokens),
                Some(false) => None,
                None if total_tokens > 0 => Some(total_tokens),
                None => None, // Legacy zero reports have no measurement provenance.
            };
            let last_report_at = chrono::DateTime::from_timestamp(state.last_report_unix as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| state.last_report_unix.to_string());

            let (status, mut detail) = if age_seconds < -FUTURE_SKEW_ALLOWANCE_SECS {
                (
                    ReporterStatus::FutureTimestamp,
                    "Last report timestamp is in the future; check the system clock.".to_string(),
                )
            } else if age_seconds > STALE_THRESHOLD_SECS {
                (
                    ReporterStatus::Stale,
                    format!(
                        "Last successful report was {} ago, older than the {}h staleness window. This may simply mean Claude Code hasn't run recently.",
                        format_duration(age_seconds),
                        STALE_THRESHOLD_SECS / 3600
                    ),
                )
            } else {
                (
                    ReporterStatus::Ok,
                    "Reporter has recorded a recent accepted update or heartbeat.".to_string(),
                )
            };
            if invalid_file_count > 0 {
                detail.push_str(&format!(
                    " {invalid_file_count} invalid or unreadable state entries were skipped."
                ));
            }

            ReporterCheck {
                state_dir: Some(state_dir_str),
                state_dir_exists: true,
                session_count,
                invalid_file_count,
                status,
                detail,
                last_report: Some(LastReportSummary {
                    session_id: state.last_session_id,
                    harness_session_id: state.harness_session_id,
                    last_report_at,
                    age_seconds,
                    total_tokens,
                    session_end_status: state.session_end.map(|attempt| attempt.status),
                }),
                last_attempt,
            }
        }
    }
}

fn last_attempt_summary(
    newest_attempt: Option<(u64, ReportStatusCode, Option<String>)>,
    now: u64,
) -> Option<LastAttemptSummary> {
    let (at_unix, status, session_id) = newest_attempt?;
    let age_seconds = i64::try_from(now)
        .unwrap_or(i64::MAX)
        .saturating_sub(at_unix as i64);
    let at = i64::try_from(at_unix)
        .ok()
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| at_unix.to_string());
    Some(LastAttemptSummary {
        status,
        label: status.label(),
        at,
        age_seconds,
        session_id,
        next_step: status.next_step(),
    })
}

fn format_duration(seconds: i64) -> String {
    let secs = seconds.unsigned_abs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

// ---------------------------------------------------------------------------
// Human rendering
// ---------------------------------------------------------------------------

fn render_human(report: &DoctorReport, color: &ColorConfig) -> String {
    let mut out = String::new();

    out.push_str(&format!("tokanban {}\n\n", report.cli_version));

    out.push_str(&format!("{}\n", color.bold("Config")));
    out.push_str(&format!(
        "  Path:    {}\n",
        report.config.path.as_deref().unwrap_or("(unknown)")
    ));
    out.push_str(&format!(
        "  Source:  {}\n",
        match report.config.source {
            "cli_flag" => "--config flag",
            _ => "default",
        }
    ));
    out.push_str(&format!(
        "  Status:  {}\n",
        paint_config_status(report.config.status, color)
    ));
    if let Some(mode) = &report.config.permissions_mode {
        out.push_str(&format!("  Mode:    {mode}\n"));
    }
    out.push_str(&format!("  {}\n", report.config.detail));
    if let Some(summary) = &report.config.summary {
        out.push_str(&format!(
            "  Workspace default: {}\n",
            summary.workspace.as_deref().unwrap_or(EM_DASH)
        ));
        out.push_str(&format!(
            "  Project default:   {}\n",
            summary.project.as_deref().unwrap_or(EM_DASH)
        ));
        out.push_str(&format!("  API origin:        {}\n", summary.api_url));
        out.push_str(&format!(
            "  Credentials:       {}\n",
            if summary.credentials_configured {
                "configured"
            } else {
                "not configured"
            }
        ));
        if let Some(state) = summary.token_state {
            out.push_str(&format!("  Token state:       {state}\n"));
        }
    }

    out.push('\n');
    out.push_str(&format!("{}\n", color.bold("Current reporter connection")));
    out.push_str(&format!("  Status: {:?}\n", report.connection.status));
    if let Some(origin) = &report.connection.endpoint_origin {
        out.push_str(&format!("  API origin: {origin}\n"));
    }
    if let Some(account) = &report.connection.account_fingerprint {
        out.push_str(&format!("  Account fingerprint: {account}\n"));
    }
    if let Some(checked) = &report.connection.checked_at {
        out.push_str(&format!("  Checked: {checked}\n"));
    }
    out.push_str(&format!("  {}\n", report.connection.detail));

    out.push('\n');
    out.push_str(&format!("{}\n", color.bold("Claude usage hook / plugin")));
    out.push_str(&format!(
        "  Session startup: {}\n",
        paint_presence(report.claude.startup_hook_status, color)
    ));
    out.push_str(&format!(
        "  Config dir: {} ({})\n",
        report
            .claude
            .claude_config_dir
            .as_deref()
            .unwrap_or("(unknown)"),
        report.claude.claude_config_dir_source
    ));
    out.push_str(&format!(
        "  Hook:       {}\n",
        paint_presence(report.claude.hook_status, color)
    ));
    out.push_str(&format!(
        "  Plugin registration: {}\n",
        paint_presence(report.claude.plugin_status, color)
    ));
    out.push_str(&format!(
        "  Plugin enabled: {}\n",
        match report.claude.plugin_enabled {
            Some(true) => "yes (local settings)",
            Some(false) => "no (local settings)",
            None => "unknown",
        }
    ));
    out.push_str(&format!(
        "  MCP server: {}\n",
        paint_presence(report.claude.mcp_status, color)
    ));
    for file in &report.claude.files {
        let state = if file.readable {
            "readable"
        } else if file.exists {
            "unreadable"
        } else {
            "not found"
        };
        out.push_str(&format!(
            "    - {:<28} {} [{}]\n",
            file.label, file.path, state
        ));
    }
    out.push_str(&format!("  {}\n", report.claude.detail));

    out.push('\n');
    out.push_str(&format!(
        "{}\n",
        color.bold("Usage reporter (recorded across local accounts)")
    ));
    out.push_str(&format!(
        "  State dir: {}\n",
        report.reporter.state_dir.as_deref().unwrap_or("(unknown)")
    ));
    out.push_str(&format!("  Sessions:  {}\n", report.reporter.session_count));
    out.push_str(&format!(
        "  Status:    {}\n",
        paint_reporter_status(report.reporter.status, color)
    ));
    if let Some(last) = &report.reporter.last_report {
        out.push_str(&format!(
            "  Last report: {} ({} {})\n",
            last.last_report_at,
            format_duration(last.age_seconds),
            if last.age_seconds < 0 {
                "from now"
            } else {
                "ago"
            }
        ));
        out.push_str(&format!(
            "  Session:     {}\n",
            last.session_id.as_deref().unwrap_or(EM_DASH)
        ));
        out.push_str(&format!(
            "  Tokens:      {}\n",
            last.total_tokens
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unmeasured".to_string())
        ));
        if let Some(status) = last.session_end_status {
            out.push_str(&format!(
                "  Session close: {}\n",
                paint_report_status(status, color)
            ));
        }
    }
    out.push_str(&format!("  {}\n", report.reporter.detail));
    if let Some(attempt) = &report.reporter.last_attempt {
        out.push('\n');
        out.push_str(&format!(
            "  Last attempt: {} ({} {}) session {}\n",
            paint_report_status(attempt.status, color),
            format_duration(attempt.age_seconds),
            if attempt.age_seconds < 0 {
                "from now"
            } else {
                "ago"
            },
            attempt.session_id.as_deref().unwrap_or(EM_DASH)
        ));
        out.push_str(&format!("  Next step:    {}\n", attempt.next_step));
    }

    out
}

fn paint_config_status(status: ConfigStatus, color: &ColorConfig) -> String {
    let (label, code) = match status {
        ConfigStatus::Ok => ("ok", colors::SUCCESS),
        ConfigStatus::Missing => ("missing", colors::MUTED),
        ConfigStatus::InsecurePermissions => ("insecure permissions", colors::ERROR),
        ConfigStatus::InvalidToml => ("invalid toml", colors::ERROR),
        ConfigStatus::Unreadable => ("unreadable", colors::ERROR),
        ConfigStatus::UnknownPath => ("unknown path", colors::MUTED),
    };
    color.paint(label, code)
}

fn paint_presence(presence: Presence, color: &ColorConfig) -> String {
    let (label, code) = match presence {
        Presence::Detected => ("detected", colors::SUCCESS),
        Presence::NotDetected => ("not detected", colors::MUTED),
        Presence::Unknown => ("unknown", colors::MUTED),
    };
    color.paint(label, code)
}

fn paint_reporter_status(status: ReporterStatus, color: &ColorConfig) -> String {
    let (label, code) = match status {
        ReporterStatus::Ok => ("ok", colors::SUCCESS),
        ReporterStatus::NoStateYet => ("no state yet", colors::MUTED),
        ReporterStatus::Stale => ("stale", colors::HIGH),
        ReporterStatus::FutureTimestamp => ("future timestamp", colors::ERROR),
        ReporterStatus::Invalid => ("invalid", colors::ERROR),
        ReporterStatus::DirectoryUnreadable => ("unreadable", colors::ERROR),
    };
    color.paint(label, code)
}

fn paint_report_status(status: ReportStatusCode, color: &ColorConfig) -> String {
    let code = match status {
        ReportStatusCode::Success => colors::SUCCESS,
        ReportStatusCode::Heartbeat
        | ReportStatusCode::SessionStarted
        | ReportStatusCode::NoTranscriptPath
        | ReportStatusCode::NoSessionMapping => colors::MUTED,
        ReportStatusCode::RateLimited
        | ReportStatusCode::ServerError
        | ReportStatusCode::NetworkError => colors::HIGH,
        ReportStatusCode::TranscriptUnreadable
        | ReportStatusCode::TranscriptUnsupported
        | ReportStatusCode::CredentialsMissing
        | ReportStatusCode::UnsafeEndpoint
        | ReportStatusCode::AuthRejected
        | ReportStatusCode::UsageRejected
        | ReportStatusCode::MalformedResponse => colors::ERROR,
    };
    color.paint(status.label(), code)
}
