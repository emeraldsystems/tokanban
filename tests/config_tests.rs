/// Tests for config file handling

mod common;

use common::{setup_temp_config, ConfigBuilder};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn test_config_read_valid_file() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = ConfigBuilder::new()
        .workspace("test-workspace")
        .project("TEST")
        .build();
    fs::write(&config_path, config_content).unwrap();

    // Set proper permissions
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let config = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(config.defaults.workspace, Some("test-workspace".to_string()));
    assert_eq!(config.defaults.project, Some("TEST".to_string()));
}

#[test]
fn test_config_write_all_fields() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config = tokanban::config::AppConfig {
        auth: tokanban::config::AuthConfig {
            token: Some("tk_refresh_test".to_string()),
            access_token: Some("tk_access_test".to_string()),
            expires_at: Some(1234567890),
        },
        defaults: tokanban::config::DefaultsConfig {
            workspace: Some("test-ws".to_string()),
            project: Some("TEST".to_string()),
        },
        ui: tokanban::config::UiConfig {
            no_color: false,
            format: "json".to_string(),
        },
        api: tokanban::config::ApiConfig {
            url: "https://api.example.com".to_string(),
            timeout_secs: 45,
        },
    };

    tokanban::config::save_config(&config, Some(&config_path)).unwrap();
    assert!(config_path.exists());

    // Verify it can be read back
    let reloaded = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(reloaded.defaults.workspace, Some("test-ws".to_string()));
    assert_eq!(reloaded.api.timeout_secs, 45);
}

#[test]
fn test_config_defaults_resolution() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    // Minimal config with only [auth] section
    let minimal_config = r#"[auth]
token = "tk_test"
access_token = "tk_access"
expires_at = 1234567890
"#;
    fs::write(&config_path, minimal_config).unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let config = tokanban::config::load_config(Some(&config_path)).unwrap();
    // Defaults are None when not specified, which is correct behavior
    assert_eq!(config.defaults.workspace, None);
    assert_eq!(config.ui.format, "auto");
    assert_eq!(config.api.timeout_secs, 30);
    assert_eq!(config.api.url, "https://api.tokanban.com");
}

#[test]
fn test_config_permission_check() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = ConfigBuilder::new().build();
    fs::write(&config_path, config_content).unwrap();

    // Set overly permissive permissions (644 > 600)
    let perms = fs::Permissions::from_mode(0o644);
    fs::set_permissions(&config_path, perms).unwrap();

    let result = tokanban::config::load_config(Some(&config_path));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("insecure") || err_msg.contains("permission"));
}

#[test]
fn test_config_missing_file() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("nonexistent.toml");

    // Missing config should return default, not error
    let config = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(config.defaults.workspace, None);
    assert_eq!(config.ui.format, "auto");
}

#[test]
fn test_config_invalid_toml() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    fs::write(&config_path, "[invalid toml\nthis = ").unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let result = tokanban::config::load_config(Some(&config_path));
    assert!(result.is_err());
}

#[test]
fn test_config_workspace_default() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = ConfigBuilder::new()
        .workspace("prod-workspace")
        .build();
    fs::write(&config_path, config_content).unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let config = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(config.defaults.workspace, Some("prod-workspace".to_string()));
}

#[test]
fn test_config_project_default() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = ConfigBuilder::new()
        .project("BACKEND")
        .build();
    fs::write(&config_path, config_content).unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let config = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(config.defaults.project, Some("BACKEND".to_string()));
}

#[test]
fn test_config_update_preserves_other_fields() {
    let temp_dir = setup_temp_config();
    let config_path = temp_dir.path().join("config.toml");

    let initial = ConfigBuilder::new()
        .workspace("ws1")
        .project("PROJ1")
        .build();
    fs::write(&config_path, initial).unwrap();
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&config_path, perms).unwrap();

    let mut config = tokanban::config::load_config(Some(&config_path)).unwrap();
    config.defaults.workspace = Some("ws2".to_string());
    tokanban::config::save_config(&config, Some(&config_path)).unwrap();

    let reloaded = tokanban::config::load_config(Some(&config_path)).unwrap();
    assert_eq!(reloaded.defaults.workspace, Some("ws2".to_string()));
    assert_eq!(reloaded.defaults.project, Some("PROJ1".to_string()));
}
