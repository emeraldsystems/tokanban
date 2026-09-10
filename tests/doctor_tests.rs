/// Tests for `tokanban doctor` — read-only, offline diagnostics.
///
/// Every test builds its own `DoctorPaths` pointing at a fresh `tempfile`
/// directory tree instead of touching the real HOME / CLAUDE_CONFIG_DIR, so
/// these are safe to run in parallel with the rest of the suite.
use std::fs;
use std::path::Path;

use serde_json::json;
use tokanban::commands::doctor::{
    build_report, ConfigStatus, DoctorPaths, Presence, ReporterStatus,
};

fn base_paths(root: &Path) -> DoctorPaths {
    DoctorPaths {
        config_path: Some(root.join("config.toml")),
        config_is_override: true,
        claude_dir: Some(root.join("claude_home")),
        claude_dir_from_env: false,
        home_claude_json: Some(root.join("home").join(".claude.json")),
        project_claude_dir: root.join("project").join(".claude"),
        state_dir: Some(root.join("usage-state")),
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

// ---------------------------------------------------------------------------
// Config checks
// ---------------------------------------------------------------------------

#[test]
fn missing_config_reports_defaults_and_no_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.config.status, ConfigStatus::Missing);
    assert!(!report.config.exists);
    let summary = report.config.summary.as_ref().expect("defaults summary");
    assert!(!summary.credentials_configured);
    assert_eq!(summary.api_url, "https://api.tokanban.com");
}

#[test]
fn default_config_source_is_reported_when_not_overridden() {
    let temp = tempfile::tempdir().unwrap();
    let mut paths = base_paths(temp.path());
    paths.config_is_override = false;

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.config.source, "default");
}

#[test]
fn malformed_toml_config_flags_invalid_and_redacts_secret() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config_path = paths.config_path.clone().unwrap();
    let secret = "tk_super_secret_should_never_leak";
    fs::write(
        &config_path,
        format!("[auth]\ntoken = \"{secret}\"\nthis is not valid toml ="),
    )
    .unwrap();
    #[cfg(unix)]
    set_mode(&config_path, 0o600);

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.config.status, ConfigStatus::InvalidToml);
    assert!(report.config.summary.is_none());

    let json_out = serde_json::to_string(&report).unwrap();
    assert!(!json_out.contains(secret));
    assert!(!json_out.contains("this is not valid toml"));
}

#[cfg(unix)]
#[test]
fn insecure_permissions_config_is_flagged_and_redacted() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config_path = paths.config_path.clone().unwrap();
    let secret = "tk_should_not_appear_in_output";
    fs::write(&config_path, format!("[auth]\ntoken = \"{secret}\"\n")).unwrap();
    set_mode(&config_path, 0o644);

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.config.status, ConfigStatus::InsecurePermissions);
    assert_eq!(report.config.permissions_mode.as_deref(), Some("644"));
    assert!(report.config.summary.is_none());

    let json_out = serde_json::to_string(&report).unwrap();
    assert!(!json_out.contains(secret));
}

#[test]
fn valid_custom_config_reports_summary_without_leaking_token_value() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config_path = paths.config_path.clone().unwrap();
    let secret = "tk_access_value_12345";
    let toml = format!(
        "[auth]\naccess_token = \"{secret}\"\nexpires_at = 9999999999\n\n[defaults]\nworkspace = \"acme\"\nproject = \"WEB\"\n"
    );
    fs::write(&config_path, toml).unwrap();
    #[cfg(unix)]
    set_mode(&config_path, 0o600);

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.config.status, ConfigStatus::Ok);
    assert_eq!(report.config.source, "cli_flag");
    let summary = report.config.summary.as_ref().unwrap();
    assert!(summary.credentials_configured);
    assert_eq!(summary.token_state, Some("not_expired"));
    assert_eq!(summary.workspace.as_deref(), Some("acme"));
    assert_eq!(summary.project.as_deref(), Some("WEB"));

    let json_out = serde_json::to_string(&report).unwrap();
    assert!(!json_out.contains(secret));
}

// ---------------------------------------------------------------------------
// Claude usage hook / plugin checks
// ---------------------------------------------------------------------------

#[test]
fn hook_detected_in_global_settings() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let claude_dir = paths.claude_dir.clone().unwrap();
    fs::create_dir_all(&claude_dir).unwrap();
    let settings = json!({
        "hooks": {
            "Stop": [{
                "matcher": "*",
                "hooks": [{"type": "command", "command": "tokanban session report-usage"}]
            }]
        }
    });
    fs::write(claude_dir.join("settings.json"), settings.to_string()).unwrap();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.hook_status, Presence::Detected);
}

#[test]
fn hook_not_detected_when_settings_exist_without_match() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let claude_dir = paths.claude_dir.clone().unwrap();
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        json!({"hooks": {}}).to_string(),
    )
    .unwrap();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.hook_status, Presence::NotDetected);
    assert_eq!(report.claude.plugin_status, Presence::NotDetected);
}

#[test]
fn hook_and_plugin_unknown_when_no_files_exist() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.hook_status, Presence::Unknown);
    assert_eq!(report.claude.plugin_status, Presence::Unknown);
    assert_eq!(report.claude.mcp_status, Presence::Unknown);
}

#[test]
fn claude_config_dir_reflects_env_override() {
    let temp = tempfile::tempdir().unwrap();
    let mut paths = base_paths(temp.path());
    paths.claude_dir_from_env = true;
    let expected_dir = paths.claude_dir.as_ref().unwrap().display().to_string();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.claude_config_dir_source, "CLAUDE_CONFIG_DIR");
    assert_eq!(report.claude.claude_config_dir, Some(expected_dir));
}

#[test]
fn plugin_detected_via_enabled_plugins() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let claude_dir = paths.claude_dir.clone().unwrap();
    fs::create_dir_all(&claude_dir).unwrap();
    let settings = json!({"enabledPlugins": {"tokanban@tokanban": true}});
    fs::write(claude_dir.join("settings.json"), settings.to_string()).unwrap();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.plugin_status, Presence::Detected);
}

#[test]
fn mcp_detected_without_leaking_authorization_header() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let claude_json = paths.home_claude_json.clone().unwrap();
    fs::create_dir_all(claude_json.parent().unwrap()).unwrap();
    let secret = "Bearer tk_mcp_secret_value";
    let contents = json!({
        "mcpServers": {
            "tokanban": {
                "type": "url",
                "url": "https://api.tokanban.com/mcp",
                "headers": {"Authorization": secret}
            }
        }
    });
    fs::write(&claude_json, contents.to_string()).unwrap();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.claude.mcp_status, Presence::Detected);

    let json_out = serde_json::to_string(&report).unwrap();
    assert!(!json_out.contains("tk_mcp_secret_value"));
}

// ---------------------------------------------------------------------------
// Usage reporter state checks
// ---------------------------------------------------------------------------

#[test]
fn reporter_missing_state_dir_is_no_state_yet() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.reporter.status, ReporterStatus::NoStateYet);
    assert_eq!(report.reporter.session_count, 0);
    assert!(report.reporter.last_report.is_none());
}

#[test]
fn reporter_recent_state_is_ok() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.clone().unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let now = 1_700_000_000u64;
    let state = json!({
        "last_report_unix": now - 60,
        "last_session_id": "sess_abc",
        "last_totals": {
            "input_tokens": 100,
            "output_tokens": 20,
            "cache_read_tokens": 5,
            "cache_write_tokens": 3
        }
    });
    fs::write(state_dir.join("sess_abc.json"), state.to_string()).unwrap();

    let report = build_report(&paths, now);

    assert_eq!(report.reporter.status, ReporterStatus::Ok);
    assert_eq!(report.reporter.session_count, 1);
    let last = report.reporter.last_report.as_ref().unwrap();
    assert_eq!(last.session_id.as_deref(), Some("sess_abc"));
    assert_eq!(last.total_tokens, Some(128));
    assert_eq!(last.age_seconds, 60);
}

#[test]
fn reporter_stale_state_is_flagged() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.clone().unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let now = 1_700_000_000u64;
    let stale_age = 30 * 3600; // older than the 24h staleness window
    let state = json!({
        "last_report_unix": now - stale_age,
        "last_session_id": "sess_old",
        "last_totals": {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0}
    });
    fs::write(state_dir.join("sess_old.json"), state.to_string()).unwrap();

    let report = build_report(&paths, now);

    assert_eq!(report.reporter.status, ReporterStatus::Stale);
}

#[test]
fn reporter_future_timestamp_is_flagged() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.clone().unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let now = 1_700_000_000u64;
    let state = json!({
        "last_report_unix": now + 3600,
        "last_session_id": "sess_future",
        "last_totals": {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0}
    });
    fs::write(state_dir.join("sess_future.json"), state.to_string()).unwrap();

    let report = build_report(&paths, now);

    assert_eq!(report.reporter.status, ReporterStatus::FutureTimestamp);
}

#[test]
fn reporter_invalid_state_file_is_flagged() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.clone().unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("broken.json"), "{}").unwrap();

    let report = build_report(&paths, 1_700_000_000);

    assert_eq!(report.reporter.status, ReporterStatus::Invalid);
    assert_eq!(report.reporter.session_count, 1);
}

#[test]
fn reporter_picks_newest_session_among_several() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.clone().unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    let now = 1_700_000_000u64;
    fs::write(
        state_dir.join("older.json"),
        json!({"last_report_unix": now - 500, "last_session_id": "older", "last_totals": {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0}}).to_string(),
    )
    .unwrap();
    fs::write(
        state_dir.join("newer.json"),
        json!({"last_report_unix": now - 10, "last_session_id": "newer", "last_totals": {"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0}}).to_string(),
    )
    .unwrap();

    let report = build_report(&paths, now);

    assert_eq!(report.reporter.session_count, 2);
    let last = report.reporter.last_report.as_ref().unwrap();
    assert_eq!(last.session_id.as_deref(), Some("newer"));
}

#[test]
fn startup_detection_is_separate_from_usage_hook_detection() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("settings.json"), json!({"hooks": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "tokanban session start-hook"}]}]
    }}).to_string()).unwrap();
    let report = build_report(&paths, 1_700_000_000);
    assert_eq!(report.claude.startup_hook_status, Presence::Detected);
    assert_eq!(report.claude.hook_status, Presence::NotDetected);
}
