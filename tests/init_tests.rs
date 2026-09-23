/// Tests for `tokanban init` — idempotent harness bootstrap.
///
/// Every test builds its own `InitPaths` pointing at a fresh `tempfile`
/// directory tree instead of touching the real HOME / CLAUDE_CONFIG_DIR /
/// CODEX_HOME, mirroring `doctor_tests.rs`.
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use tokanban::commands::init::{detect_harness, run_init, Harness, InitPaths, StepStatus};
use tokanban::config::AppConfig;
use toml::Value as TomlValue;

fn base_paths(root: &Path) -> InitPaths {
    InitPaths {
        target_dir: root.join("project"),
        claude_dir: Some(root.join("claude_home")),
        claude_dir_from_env: false,
        // Default (no CLAUDE_CONFIG_DIR override): the active account is
        // `~/.claude.json`; `claude_home/.claude.json` is the diagnostic-only
        // "other account" location.
        claude_json_primary: Some(root.join("home").join(".claude.json")),
        claude_json_diagnostic_candidates: vec![root.join("claude_home").join(".claude.json")],
        claude_plugin_candidates: vec![
            root.join("claude_home").join("settings.json"),
            root.join("claude_home").join("settings.local.json"),
            root.join("claude_home").join("plugins").join("config.json"),
            root.join("claude_home")
                .join("plugins")
                .join("installed_plugins.json"),
            root.join("project").join(".claude").join("settings.json"),
            root.join("project")
                .join(".claude")
                .join("settings.local.json"),
        ],
        codex_config_candidates: vec![
            root.join("codex_home").join("config.toml"),
            root.join("project").join(".codex").join("config.toml"),
        ],
        cursor_mcp_candidates: vec![
            root.join("home").join(".cursor").join("mcp.json"),
            root.join("project").join(".cursor").join("mcp.json"),
        ],
    }
}

/// Same as `base_paths`, but with `CLAUDE_CONFIG_DIR` "set": the override
/// dir becomes the active account and `~/.claude.json` becomes diagnostic-only.
fn base_paths_with_config_dir_override(root: &Path) -> InitPaths {
    let mut paths = base_paths(root);
    paths.claude_dir_from_env = true;
    paths.claude_json_primary = Some(root.join("claude_home").join(".claude.json"));
    paths.claude_json_diagnostic_candidates = vec![root.join("home").join(".claude.json")];
    paths
}

fn read_json(path: &Path) -> Value {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("failed to parse {} as JSON: {e}", path.display()))
}

fn read_toml(path: &Path) -> TomlValue {
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    toml::from_str(&contents)
        .unwrap_or_else(|e| panic!("failed to parse {} as TOML: {e}", path.display()))
}

fn toml_get<'a>(value: &'a TomlValue, key: &str) -> Option<&'a TomlValue> {
    value.as_table()?.get(key)
}

fn tokanban_codex_server(doc: &TomlValue) -> &TomlValue {
    toml_get(doc, "mcp_servers")
        .and_then(|v| toml_get(v, "tokanban"))
        .expect("mcp_servers.tokanban table")
}

fn restore_env(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => env::set_var(key, value),
        None => env::remove_var(key),
    }
}

fn step<'a>(
    steps: &'a [tokanban::commands::init::InitStep],
    name_contains: &str,
) -> &'a tokanban::commands::init::InitStep {
    steps
        .iter()
        .find(|s| s.name.contains(name_contains))
        .unwrap_or_else(|| panic!("no step matching '{name_contains}' in {steps:?}"))
}

// ---------------------------------------------------------------------------
// Dry-run / preview semantics
// ---------------------------------------------------------------------------

#[test]
fn dry_run_writes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::ClaudeCode, false, &config).unwrap();

    assert!(!report.applied);
    assert!(report.any_pending_or_applied_write());
    for s in &report.steps {
        assert!(matches!(
            s.status,
            StepStatus::WouldCreate | StepStatus::Skipped
        ));
    }

    assert!(!paths.claude_json_primary.as_ref().unwrap().exists());
    assert!(!paths.target_dir.join("CLAUDE.md").exists());
    assert!(!paths
        .claude_dir
        .as_ref()
        .unwrap()
        .join("settings.json")
        .exists());
}

#[test]
fn repeat_apply_is_idempotent_and_does_not_duplicate_entries() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let first = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    for s in &first.steps {
        assert_eq!(s.status, StepStatus::Created, "step {} unexpected", s.name);
    }

    let mcp_json_after_first = read_json(paths.claude_json_primary.as_ref().unwrap());
    let settings_after_first = read_json(&paths.claude_dir.as_ref().unwrap().join("settings.json"));
    let claude_md_after_first = fs::read_to_string(paths.target_dir.join("CLAUDE.md")).unwrap();

    let second = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    for s in &second.steps {
        assert_eq!(
            s.status,
            StepStatus::AlreadyPresent,
            "step {} should be a no-op on the second run",
            s.name
        );
    }

    assert_eq!(
        mcp_json_after_first,
        read_json(paths.claude_json_primary.as_ref().unwrap())
    );
    assert_eq!(
        settings_after_first,
        read_json(&paths.claude_dir.as_ref().unwrap().join("settings.json"))
    );
    assert_eq!(
        claude_md_after_first,
        fs::read_to_string(paths.target_dir.join("CLAUDE.md")).unwrap()
    );

    assert_eq!(
        settings_after_first["hooks"]["SessionStart"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        settings_after_first["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("session start-hook")
    );
    assert_eq!(
        settings_after_first["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        settings_after_first["hooks"]["SessionEnd"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// ---------------------------------------------------------------------------
// Transport formats must match each harness's real schema
// ---------------------------------------------------------------------------

#[test]
fn claude_mcp_entry_uses_http_transport_type_not_url() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();

    let doc = read_json(paths.claude_json_primary.as_ref().unwrap());
    let server = &doc["mcpServers"]["tokanban"];
    assert_eq!(server["type"], "http");
    assert_eq!(
        server["headers"]["Authorization"],
        "Bearer ${TOKANBAN_API_KEY}"
    );
}

#[test]
fn cursor_mcp_entry_uses_env_colon_placeholder_and_omits_type_field() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    run_init(&paths, Harness::Cursor, true, &config).unwrap();

    let doc = read_json(&paths.cursor_mcp_candidates[0]);
    let server = &doc["mcpServers"]["tokanban"];
    assert_eq!(
        server["headers"]["Authorization"],
        "Bearer ${env:TOKANBAN_API_KEY}"
    );
    assert!(server.get("type").is_none());
}

#[test]
fn codex_mcp_config_is_valid_toml_with_bearer_token_env_var() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Created);

    let doc = read_toml(&paths.codex_config_candidates[0]);
    let server = tokanban_codex_server(&doc);
    assert_eq!(
        server
            .as_table()
            .unwrap()
            .get("bearer_token_env_var")
            .unwrap()
            .as_str()
            .unwrap(),
        "TOKANBAN_API_KEY"
    );
    let headers = server
        .as_table()
        .unwrap()
        .get("http_headers")
        .unwrap()
        .as_table()
        .unwrap();
    assert_eq!(
        headers
            .get("X-Tokanban-Tool-Scope")
            .unwrap()
            .as_str()
            .unwrap(),
        "core,memory"
    );
    assert!(server.as_table().unwrap().get("type").is_none());
}

// ---------------------------------------------------------------------------
// Codex: TOML preservation, refusal, duplicate-avoidance, idempotence
// ---------------------------------------------------------------------------

#[test]
fn codex_preserves_existing_toml_comments_and_content_verbatim() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let primary = &paths.codex_config_candidates[0];
    fs::create_dir_all(primary.parent().unwrap()).unwrap();
    let original = "# my codex config\n[some_other_table]\nfoo = \"bar\"\n";
    fs::write(primary, original).unwrap();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Updated);

    let new_contents = fs::read_to_string(primary).unwrap();
    assert!(new_contents.starts_with(original));
    assert!(new_contents.contains("[mcp_servers.tokanban]"));

    let doc = toml::from_str::<TomlValue>(&new_contents).unwrap();
    let other = toml_get(&doc, "some_other_table").unwrap();
    assert_eq!(
        other
            .as_table()
            .unwrap()
            .get("foo")
            .unwrap()
            .as_str()
            .unwrap(),
        "bar"
    );
}

#[test]
fn codex_malformed_toml_is_left_unchanged_and_refused_without_leaking_content() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let primary = &paths.codex_config_candidates[0];
    fs::create_dir_all(primary.parent().unwrap()).unwrap();
    let secret = "tk_codex_secret_should_never_leak";
    let malformed = format!("[mcp_servers\nnote = \"{secret}\" not valid toml at all");
    fs::write(primary, &malformed).unwrap();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert!(!mcp_step.detail.contains(secret));
    assert_eq!(fs::read_to_string(primary).unwrap(), malformed);
}

#[test]
fn codex_incompatible_mcp_servers_shape_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let primary = &paths.codex_config_candidates[0];
    fs::create_dir_all(primary.parent().unwrap()).unwrap();
    let contents = "mcp_servers = \"not-a-table\"\n";
    fs::write(primary, contents).unwrap();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert_eq!(fs::read_to_string(primary).unwrap(), contents);
}

#[test]
fn codex_project_local_duplicate_prevents_redundant_global_entry() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let project_config = &paths.codex_config_candidates[1];
    fs::create_dir_all(project_config.parent().unwrap()).unwrap();
    fs::write(
        project_config,
        "[mcp_servers.tokanban]\nurl = \"https://api.tokanban.com/mcp\"\n",
    )
    .unwrap();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::AlreadyPresent);
    assert!(!paths.codex_config_candidates[0].exists());
}

#[test]
fn codex_config_dir_override_missing_home_refuses_instead_of_using_project_config() {
    let temp = tempfile::tempdir().unwrap();
    let mut paths = base_paths(temp.path());
    // Simulate no CODEX_HOME and no resolvable home dir at all.
    paths.codex_config_candidates = Vec::new();
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
}

#[test]
fn codex_repeat_apply_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let first = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&first.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Created);
    let after_first = fs::read_to_string(&paths.codex_config_candidates[0]).unwrap();

    let second = run_init(&paths, Harness::Codex, true, &config).unwrap();
    let mcp_step = step(&second.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::AlreadyPresent);
    let after_second = fs::read_to_string(&paths.codex_config_candidates[0]).unwrap();

    assert_eq!(after_first, after_second);
    assert_eq!(after_second.matches("[mcp_servers.tokanban]").count(), 1);
}

// ---------------------------------------------------------------------------
// Cursor: idempotence
// ---------------------------------------------------------------------------

#[test]
fn cursor_repeat_apply_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    run_init(&paths, Harness::Cursor, true, &config).unwrap();
    let after_first = read_json(&paths.cursor_mcp_candidates[0]);

    let report = run_init(&paths, Harness::Cursor, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::AlreadyPresent);
    let after_second = read_json(&paths.cursor_mcp_candidates[0]);

    assert_eq!(after_first, after_second);
}

// ---------------------------------------------------------------------------
// Preserving unrelated content
// ---------------------------------------------------------------------------

#[test]
fn preserves_unrelated_settings_content_when_adding_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    fs::write(
        &settings_path,
        json!({
            "someUnrelatedTopLevelKey": "keep-me",
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo hi"}]}]
            }
        })
        .to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Updated);

    let settings = read_json(&settings_path);
    assert_eq!(settings["someUnrelatedTopLevelKey"], "keep-me");
    assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(settings["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
}

#[test]
fn behavior_block_preserves_existing_content_and_appends_once() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    fs::create_dir_all(&paths.target_dir).unwrap();
    let claude_md = paths.target_dir.join("CLAUDE.md");
    fs::write(
        &claude_md,
        "# My Project\n\nSome unrelated project notes.\n",
    )
    .unwrap();

    run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let after_first = fs::read_to_string(&claude_md).unwrap();
    assert!(after_first.starts_with("# My Project\n\nSome unrelated project notes.\n"));
    assert!(after_first.contains("## Tokanban Memory"));

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let behavior_step = step(&report.steps, "Behavior block");
    assert_eq!(behavior_step.status, StepStatus::AlreadyPresent);

    let after_second = fs::read_to_string(&claude_md).unwrap();
    assert_eq!(after_first, after_second);
    assert_eq!(after_second.matches("## Tokanban Memory").count(), 1);
}

// ---------------------------------------------------------------------------
// Refusing incompatible / malformed existing config (Claude/Cursor JSON)
// ---------------------------------------------------------------------------

#[test]
fn malformed_mcp_json_is_left_unchanged_and_refused() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_json = paths.claude_json_primary.as_ref().unwrap();
    fs::create_dir_all(claude_json.parent().unwrap()).unwrap();
    let secret = "tk_should_never_leak_into_diagnostics";
    let malformed = format!("{{\"mcpServers\": {{ \"note\": \"{secret}\" this is not valid json");
    fs::write(claude_json, &malformed).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();

    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert!(!mcp_step.detail.contains(secret));
    assert_eq!(fs::read_to_string(claude_json).unwrap(), malformed);
}

#[test]
fn malformed_settings_json_is_left_unchanged_and_refused() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    let malformed = "{ not valid json at all";
    fs::write(&settings_path, malformed).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();

    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Refused);
    assert_eq!(fs::read_to_string(&settings_path).unwrap(), malformed);

    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Created);
}

#[test]
fn incompatible_mcp_servers_shape_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_json = paths.claude_json_primary.as_ref().unwrap();
    fs::create_dir_all(claude_json.parent().unwrap()).unwrap();
    let contents = json!({"mcpServers": "not-an-object"}).to_string();
    fs::write(claude_json, &contents).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert_eq!(fs::read_to_string(claude_json).unwrap(), contents);
}

// ---------------------------------------------------------------------------
// Plugin enable/disable semantics: presence != enablement, project overrides global
// ---------------------------------------------------------------------------

#[test]
fn plugin_managed_hooks_are_skipped_and_settings_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    let contents = json!({"enabledPlugins": {"tokanban@tokanban": true}}).to_string();
    fs::write(&settings_path, &contents).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Skipped);
    assert!(hook_step.detail.to_lowercase().contains("plugin"));
    assert_eq!(fs::read_to_string(&settings_path).unwrap(), contents);
}

#[test]
fn plugin_disabled_explicitly_still_registers_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    fs::write(
        &settings_path,
        json!({"enabledPlugins": {"tokanban@tokanban": false}}).to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Updated);

    let settings = read_json(&settings_path);
    assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
}

#[test]
fn plugin_disabled_at_project_level_overrides_global_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        json!({"enabledPlugins": {"tokanban@tokanban": true}}).to_string(),
    )
    .unwrap();

    let project_claude = paths.target_dir.join(".claude");
    fs::create_dir_all(&project_claude).unwrap();
    fs::write(
        project_claude.join("settings.json"),
        json!({"enabledPlugins": {"tokanban@tokanban": false}}).to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_ne!(
        hook_step.status,
        StepStatus::Skipped,
        "explicit per-project disable must override a global enable"
    );
}

#[test]
fn plugin_enabled_at_project_level_overrides_global_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        json!({"enabledPlugins": {"tokanban@tokanban": false}}).to_string(),
    )
    .unwrap();

    let project_claude = paths.target_dir.join(".claude");
    fs::create_dir_all(&project_claude).unwrap();
    fs::write(
        project_claude.join("settings.json"),
        json!({"enabledPlugins": {"tokanban@tokanban": true}}).to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Skipped);
}

#[test]
fn plugin_detected_via_secondary_registry_file_still_skips_hooks() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let plugin_config = paths
        .claude_dir
        .as_ref()
        .unwrap()
        .join("plugins")
        .join("config.json");
    fs::create_dir_all(plugin_config.parent().unwrap()).unwrap();
    fs::write(
        &plugin_config,
        json!({"enabledPlugins": {"tokanban@tokanban": true}}).to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Skipped);
    assert!(!paths
        .claude_dir
        .as_ref()
        .unwrap()
        .join("settings.json")
        .exists());
}

// ---------------------------------------------------------------------------
// Partial hook registration: only the missing event is added
// ---------------------------------------------------------------------------

#[test]
fn partial_hooks_stop_present_adds_start_and_end() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    fs::write(
        &settings_path,
        json!({
            "hooks": {
                "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "tokanban session report-usage"}]}]
            }
        })
        .to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Updated);
    assert!(hook_step.detail.contains("SessionEnd"));
    assert!(!hook_step.detail.contains("Stop and"));

    let settings = read_json(&settings_path);
    assert_eq!(
        settings["hooks"]["SessionStart"].as_array().unwrap().len(),
        1
    );
    assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(settings["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
}

#[test]
fn partial_hooks_sessionend_present_adds_start_and_stop() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    fs::write(
        &settings_path,
        json!({
            "hooks": {
                "SessionEnd": [{"matcher": "*", "hooks": [{"type": "command", "command": "tokanban session report-usage"}]}]
            }
        })
        .to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Updated);

    let settings = read_json(&settings_path);
    assert_eq!(
        settings["hooks"]["SessionStart"].as_array().unwrap().len(),
        1
    );
    assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(settings["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
}

#[test]
fn all_session_hooks_present_is_already_present_and_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let claude_dir = paths.claude_dir.as_ref().unwrap();
    fs::create_dir_all(claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");
    let contents = json!({
        "hooks": {
            "SessionStart": [{"matcher":"*", "hooks":[{"type":"command","command":"tokanban session start-hook"}]}],
            "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "tokanban session report-usage"}]}],
            "SessionEnd": [{"matcher": "*", "hooks": [{"type": "command", "command": "tokanban session report-usage"}]}]
        }
    })
    .to_string();
    fs::write(&settings_path, &contents).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::AlreadyPresent);
    assert_eq!(fs::read_to_string(&settings_path).unwrap(), contents);
}

// ---------------------------------------------------------------------------
// Account isolation: CLAUDE_CONFIG_DIR is primary; a match in a *different*
// account/location must never suppress installing into the active one.
// ---------------------------------------------------------------------------

#[test]
fn claude_cross_account_entry_does_not_suppress_active_account_install() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path()); // no CLAUDE_CONFIG_DIR override; active = home/.claude.json
    let config = AppConfig::default();

    let other_account_file = &paths.claude_json_diagnostic_candidates[0];
    fs::create_dir_all(other_account_file.parent().unwrap()).unwrap();
    let other_contents = json!({
        "mcpServers": {"tokanban": {"type": "http", "url": "https://api.tokanban.com/mcp"}}
    })
    .to_string();
    fs::write(other_account_file, &other_contents).unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");

    assert_eq!(
        mcp_step.status,
        StepStatus::Created,
        "a tokanban entry in a different account must not suppress installing into the active one"
    );
    assert!(mcp_step.detail.to_lowercase().contains("different account"));

    // The active account now has the entry, and the other account's file
    // was never touched.
    let active = read_json(paths.claude_json_primary.as_ref().unwrap());
    assert!(active["mcpServers"]["tokanban"].is_object());
    assert_eq!(
        fs::read_to_string(other_account_file).unwrap(),
        other_contents
    );
}

#[test]
fn claude_config_dir_override_makes_it_the_active_primary_location() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths_with_config_dir_override(temp.path());
    let config = AppConfig::default();

    // Entry exists only in the non-active (home) location.
    let non_active = &paths.claude_json_diagnostic_candidates[0];
    fs::create_dir_all(non_active.parent().unwrap()).unwrap();
    fs::write(
        non_active,
        json!({"mcpServers": {"tokanban": {"type": "http", "url": "https://api.tokanban.com/mcp"}}})
            .to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Created);

    let active = read_json(paths.claude_json_primary.as_ref().unwrap());
    assert!(active["mcpServers"]["tokanban"].is_object());
}

#[test]
fn claude_config_dir_override_already_present_is_gated_on_override_dir_only() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths_with_config_dir_override(temp.path());
    let config = AppConfig::default();

    let active = paths.claude_json_primary.as_ref().unwrap();
    fs::create_dir_all(active.parent().unwrap()).unwrap();
    fs::write(
        active,
        json!({"mcpServers": {"tokanban": {"type": "http", "url": "https://api.tokanban.com/mcp"}}})
            .to_string(),
    )
    .unwrap();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::AlreadyPresent);
}

// ---------------------------------------------------------------------------
// Auth guidance never leaks a key value, and is explicit about CLI login
// not populating TOKANBAN_API_KEY.
// ---------------------------------------------------------------------------

#[test]
fn mcp_entry_uses_env_placeholder_and_never_embeds_the_real_key() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let prior = env::var_os("TOKANBAN_API_KEY");
    let secret = "tk_super_secret_value_never_in_output";
    env::set_var("TOKANBAN_API_KEY", secret);

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();

    restore_env("TOKANBAN_API_KEY", prior);

    let mcp_step = step(&report.steps, "MCP server configuration");
    assert!(!mcp_step.detail.contains(secret));

    let mcp_json = read_json(paths.claude_json_primary.as_ref().unwrap());
    let header = mcp_json["mcpServers"]["tokanban"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    assert_eq!(header, "Bearer ${TOKANBAN_API_KEY}");
    assert!(
        !fs::read_to_string(paths.claude_json_primary.as_ref().unwrap())
            .unwrap()
            .contains(secret)
    );
}

#[test]
fn auth_guidance_clarifies_cli_login_does_not_set_env_token() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let prior = env::var_os("TOKANBAN_API_KEY");
    env::remove_var("TOKANBAN_API_KEY");

    let mut config = AppConfig::default();
    config.auth.access_token = Some("tk_cli_oauth_session".to_string());

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    restore_env("TOKANBAN_API_KEY", prior);

    let mcp_step = step(&report.steps, "MCP server configuration");
    assert!(!mcp_step.detail.contains("tk_cli_oauth_session"));
    assert!(mcp_step.detail.contains("does not set TOKANBAN_API_KEY"));
}

// ---------------------------------------------------------------------------
// API URL validation: never leak credentials/query/fragment
// ---------------------------------------------------------------------------

#[test]
fn mcp_url_with_embedded_credentials_is_rejected_and_never_leaked() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let mut config = AppConfig::default();
    let secret = "s3cr3t_password";
    config.api.url = format!("https://user:{secret}@api.tokanban.com/mcp");

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert!(!mcp_step.detail.contains(secret));
    assert!(!mcp_step.detail.contains("user:"));
    assert!(!paths.claude_json_primary.as_ref().unwrap().exists());
}

#[test]
fn mcp_url_with_query_string_is_rejected_and_never_leaked() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let mut config = AppConfig::default();
    let secret = "leaked_token_abc";
    config.api.url = format!("https://api.tokanban.com/mcp?token={secret}");

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
    assert!(!mcp_step.detail.contains(secret));
    assert!(!paths.claude_json_primary.as_ref().unwrap().exists());
}

#[test]
fn mcp_url_with_fragment_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let mut config = AppConfig::default();
    config.api.url = "https://api.tokanban.com/mcp#frag".to_string();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
}

#[test]
fn mcp_plain_http_non_loopback_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let mut config = AppConfig::default();
    config.api.url = "http://example.com".to_string();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Refused);
}

#[test]
fn mcp_http_loopback_url_is_accepted_for_local_fixtures() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let mut config = AppConfig::default();
    config.api.url = "http://127.0.0.1:9999".to_string();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let mcp_step = step(&report.steps, "MCP server configuration");
    assert_eq!(mcp_step.status, StepStatus::Created);

    let mcp_json = read_json(paths.claude_json_primary.as_ref().unwrap());
    assert_eq!(
        mcp_json["mcpServers"]["tokanban"]["url"],
        "http://127.0.0.1:9999/mcp"
    );
}

// ---------------------------------------------------------------------------
// Non-Claude harnesses: honest limitation, no MCP/CLAUDE.md duplication
// ---------------------------------------------------------------------------

#[test]
fn codex_skips_usage_hook_with_honest_note_and_writes_agents_md() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::Codex, true, &config).unwrap();

    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Skipped);
    assert!(hook_step.detail.contains("Claude Code"));

    assert!(paths.target_dir.join("AGENTS.md").exists());
    assert!(!paths.target_dir.join("CLAUDE.md").exists());
    let skills = paths.target_dir.join(".agents/skills");
    for name in [
        "tokanban",
        "tokanban-setup",
        "tokanban-memory",
        "tokanban-pm",
        "tokanban-architect",
        "tokanban-engineer",
        "tokanban-reviewer",
        "tokanban-researcher",
    ] {
        assert!(skills.join(name).join("SKILL.md").is_file());
        assert!(skills.join(name).join("agents/openai.yaml").is_file());
    }
    assert!(skills
        .join("tokanban/references/cli-quick-ref.md")
        .is_file());

    let doc = read_toml(&paths.codex_config_candidates[0]);
    assert!(toml_get(&doc, "mcp_servers")
        .and_then(|v| toml_get(v, "tokanban"))
        .is_some());
}

#[test]
fn cursor_skips_usage_hook_and_writes_cursorrules() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::Cursor, true, &config).unwrap();

    let hook_step = step(&report.steps, "Usage reporting hook");
    assert_eq!(hook_step.status, StepStatus::Skipped);
    assert!(hook_step.detail.contains("Claude Code"));

    assert!(paths.target_dir.join(".cursorrules").exists());
    let mcp_json = read_json(&paths.cursor_mcp_candidates[0]);
    assert!(mcp_json["mcpServers"]["tokanban"].is_object());
}

// ---------------------------------------------------------------------------
// Harness auto-detection
// ---------------------------------------------------------------------------

#[test]
fn detect_harness_prefers_claude_when_its_dir_exists() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    fs::create_dir_all(paths.claude_dir.as_ref().unwrap()).unwrap();
    fs::create_dir_all(paths.codex_config_candidates[0].parent().unwrap()).unwrap();

    assert_eq!(detect_harness(&paths), Harness::ClaudeCode);
}

#[test]
fn detect_harness_defaults_to_claude_then_falls_back_to_codex() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    assert_eq!(detect_harness(&paths), Harness::ClaudeCode);

    fs::create_dir_all(paths.codex_config_candidates[0].parent().unwrap()).unwrap();
    assert_eq!(detect_harness(&paths), Harness::Codex);
}

#[test]
fn detect_harness_finds_cursor_when_only_cursor_dir_exists() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths(temp.path());
    fs::create_dir_all(paths.cursor_mcp_candidates[0].parent().unwrap()).unwrap();

    assert_eq!(detect_harness(&paths), Harness::Cursor);
}

// ---------------------------------------------------------------------------
// CLAUDE_CONFIG_DIR is reflected in hook diagnostics
// ---------------------------------------------------------------------------

#[test]
fn claude_config_dir_override_is_reflected_in_hook_step_detail() {
    let temp = tempfile::tempdir().unwrap();
    let paths = base_paths_with_config_dir_override(temp.path());
    let config = AppConfig::default();

    let report = run_init(&paths, Harness::ClaudeCode, true, &config).unwrap();
    let hook_step = step(&report.steps, "Usage reporting hook");
    assert!(hook_step.detail.contains("CLAUDE_CONFIG_DIR override: yes"));
}
