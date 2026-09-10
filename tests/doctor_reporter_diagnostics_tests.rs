/// Tests for `tokanban doctor`'s integration with the usage reporter's
/// per-attempt diagnostics (TKB-138): surfacing bounded, non-secret status
/// codes and next steps even before any report has ever succeeded, and the
/// accepted reports without making network requests.
use std::fs;
use std::path::Path;

use serde_json::json;
use tokanban::commands::doctor::{build_report, DoctorPaths, ReporterStatus};
use tokanban::commands::session::ReportStatusCode;

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

fn zero_totals() -> serde_json::Value {
    json!({"input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0})
}

#[test]
fn doctor_surfaces_attempt_only_failure_before_any_success() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.as_ref().unwrap();
    fs::create_dir_all(state_dir).unwrap();
    let state = json!({
        "last_report_unix": 0,
        "last_session_id": null,
        "last_totals": zero_totals(),
        "harness_session_id": "harness-abc",
        "last_attempt": {"at_unix": 500, "status": "credentials_missing"}
    });
    fs::write(state_dir.join("acct1__harness-abc.json"), state.to_string()).unwrap();

    let report = build_report(&paths, 600);

    // No successful report has ever landed for this session, so it must not
    // be misreported as a healthy "ok" state.
    assert_eq!(report.reporter.status, ReporterStatus::NoStateYet);
    assert_eq!(report.reporter.invalid_file_count, 0);
    assert!(report.reporter.last_report.is_none());

    let attempt = report
        .reporter
        .last_attempt
        .as_ref()
        .expect("attempt should surface");
    assert_eq!(attempt.status, ReportStatusCode::CredentialsMissing);
    assert_eq!(attempt.session_id.as_deref(), Some("harness-abc"));
    assert_eq!(attempt.age_seconds, 100);
    assert!(!attempt.next_step.is_empty());

    let json_out = serde_json::to_string(&report).unwrap();
    assert!(json_out.contains("credentials_missing"));
}

#[test]
fn doctor_prefers_most_recent_attempt_across_files_even_if_older_success_exists() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let state_dir = paths.state_dir.as_ref().unwrap();
    fs::create_dir_all(state_dir).unwrap();

    // Account A: succeeded a while ago.
    let success_state = json!({
        "last_report_unix": 100,
        "last_session_id": "sess_a",
        "last_totals": {"input_tokens": 10, "output_tokens": 5, "cache_read_tokens": 0, "cache_write_tokens": 0},
        "harness_session_id": "harness-a",
        "last_attempt": {"at_unix": 100, "status": "success"}
    });
    fs::write(
        state_dir.join("acctA__harness-a.json"),
        success_state.to_string(),
    )
    .unwrap();

    // Account B: has never succeeded, but attempted more recently and failed.
    let failing_state = json!({
        "last_report_unix": 0,
        "last_session_id": null,
        "last_totals": zero_totals(),
        "harness_session_id": "harness-b",
        "last_attempt": {"at_unix": 400, "status": "network_error"}
    });
    fs::write(
        state_dir.join("acctB__harness-b.json"),
        failing_state.to_string(),
    )
    .unwrap();

    let report = build_report(&paths, 500);

    // The successful report is still the one reflected in `last_report`.
    let last_report = report.reporter.last_report.expect("success should surface");
    assert_eq!(last_report.session_id.as_deref(), Some("sess_a"));

    // But the most recent attempt overall (the later failure) is what's
    // surfaced as `last_attempt`, so an ongoing problem on another account
    // isn't masked by an older success.
    let attempt = report
        .reporter
        .last_attempt
        .as_ref()
        .expect("attempt should surface");
    assert_eq!(attempt.status, ReportStatusCode::NetworkError);
    assert_eq!(attempt.session_id.as_deref(), Some("harness-b"));
}

#[test]
fn doctor_distinguishes_heartbeat_measured_zero_and_ambiguous_legacy_zero() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let dir = paths.state_dir.as_ref().unwrap();
    fs::create_dir_all(dir).unwrap();
    for (measurement, expected) in [(Some(false), None), (Some(true), Some(0)), (None, None)] {
        let mut state =
            json!({"last_report_unix":100,"last_session_id":"session","last_totals":zero_totals()});
        if let Some(value) = measurement {
            state["usage_measured"] = json!(value);
        }
        fs::write(dir.join("state.json"), state.to_string()).unwrap();
        let report = build_report(&paths, 110);
        assert_eq!(report.reporter.last_report.unwrap().total_tokens, expected);
    }
}
