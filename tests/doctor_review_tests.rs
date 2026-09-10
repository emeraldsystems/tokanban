use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokanban::commands::doctor::{build_report, DoctorPaths, Presence, ReporterStatus};

const NOW: u64 = 1_800_000_000;

fn paths(root: &Path) -> DoctorPaths {
    DoctorPaths {
        config_path: Some(root.join("config.toml")),
        config_is_override: true,
        claude_dir: Some(root.join("claude")),
        claude_dir_from_env: true,
        home_claude_json: Some(root.join("claude.json")),
        project_claude_dir: root.join("project/.claude"),
        state_dir: Some(root.join("usage-state")),
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn state(timestamp: u64) -> Value {
    json!({
        "last_report_unix": timestamp,
        "last_session_id": "fixture-session",
        "last_totals": {
            "input_tokens": 0, "output_tokens": 0,
            "cache_read_tokens": 0, "cache_write_tokens": 0
        }
    })
}

#[test]
fn doctor_hides_secrets_in_api_urls_and_does_not_validate_tokens_offline() {
    let dir = TempDir::new().unwrap();
    let paths = paths(dir.path());
    write(paths.config_path.as_ref().unwrap(), &format!(
        "[api]\nurl = 'https://url-user:url-password@example.test/private-path?key=query-secret#fragment-secret'\n[auth]\naccess_token = 'token-secret'\nexpires_at = {}\n", NOW + 100
    ));
    let report = build_report(&paths, NOW);
    let summary = report.config.summary.as_ref().unwrap();
    assert_eq!(summary.api_url, "https://example.test");
    assert_eq!(summary.token_state, Some("not_expired"));
    let output = serde_json::to_string(&report).unwrap();
    for secret in [
        "url-user",
        "url-password",
        "private-path",
        "query-secret",
        "fragment-secret",
        "token-secret",
    ] {
        assert!(!output.contains(secret), "exposed {secret}");
    }
}

#[test]
fn doctor_rejects_incomplete_and_overflowing_reporter_state_without_panicking() {
    let dir = TempDir::new().unwrap();
    let paths = paths(dir.path());
    let state_path = paths.state_dir.as_ref().unwrap().join("fixture.json");
    let mut overflowing = state(NOW);
    overflowing["last_totals"]["input_tokens"] = json!(u64::MAX);
    overflowing["last_totals"]["output_tokens"] = json!(1);
    for invalid in [
        json!({"last_report_unix": NOW}),
        state(0),
        state(u64::MAX),
        overflowing,
    ] {
        write(&state_path, &invalid.to_string());
        let report = build_report(&paths, NOW);
        assert_eq!(report.reporter.status, ReporterStatus::Invalid);
        assert_eq!(report.reporter.invalid_file_count, 1);
        assert!(report.reporter.last_report.is_none());
    }

    write(&state_path, &state(NOW).to_string());
    write(&paths.state_dir.as_ref().unwrap().join("broken.json"), "{");
    let report = build_report(&paths, NOW);
    assert_eq!(report.reporter.status, ReporterStatus::Ok);
    assert_eq!(report.reporter.invalid_file_count, 1);
    assert_eq!(report.reporter.last_report.unwrap().total_tokens, None);
    assert!(report.reporter.detail.contains("skipped"));
}

#[test]
fn doctor_distinguishes_registered_plugins_from_local_enablement() {
    let dir = TempDir::new().unwrap();
    let paths = paths(dir.path());
    let global = paths.claude_dir.as_ref().unwrap().join("settings.json");
    write(
        &global,
        &json!({
            "enabledPlugins": {"not-tokanban@example": true},
            "extraKnownMarketplaces": {"tokanban": {}},
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "tokanban task list"}]}]}
        })
        .to_string(),
    );
    let report = build_report(&paths, NOW);
    assert_eq!(report.claude.plugin_status, Presence::NotDetected);
    assert_eq!(report.claude.hook_status, Presence::NotDetected);
    assert_eq!(report.claude.plugin_enabled, None);

    write(
        &global,
        &json!({"enabledPlugins": {"tokanban@example": true}}).to_string(),
    );
    write(
        &paths.project_claude_dir.join("settings.local.json"),
        &json!({"enabledPlugins": {"tokanban@example": false}}).to_string(),
    );
    let report = build_report(&paths, NOW);
    assert_eq!(report.claude.plugin_status, Presence::Detected);
    assert_eq!(report.claude.plugin_enabled, Some(false));
}

#[test]
fn doctor_binary_handles_bad_config_without_authentication_or_writes() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("custom.toml");
    let malformed = "[auth]\naccess_token = 'secret-do-not-echo\n";
    write(&config, malformed);
    for format in ["json", "table"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tokanban"))
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .env("USERPROFILE", dir.path())
            .env("APPDATA", dir.path().join("appdata"))
            .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
            .env("CLAUDE_CONFIG_DIR", dir.path().join("claude"))
            .env_remove("TOKANBAN_API_KEY")
            .args([
                "--config",
                "custom.toml",
                "--api-url",
                "http://127.0.0.1:9",
                "--format",
                format,
                "--no-color",
                "doctor",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("secret-do-not-echo"));
        assert!(!text.contains('\x1b'));
        if format == "json" {
            let report: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(report["config"]["status"], "invalid_toml");
            assert_eq!(report["config"]["source"], "cli_flag");
            assert_eq!(
                report["config"]["path"],
                dir.path()
                    .canonicalize()
                    .unwrap()
                    .join("custom.toml")
                    .to_str()
                    .unwrap()
            );
            assert_eq!(
                report["claude"]["claude_config_dir_source"],
                "CLAUDE_CONFIG_DIR"
            );
        }
        assert_eq!(fs::read_to_string(&config).unwrap(), malformed);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
