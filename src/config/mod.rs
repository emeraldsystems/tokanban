mod types;

pub use types::*;

use crate::error::{CliError, Result};
use std::fs;
use std::path::PathBuf;

/// Returns the default config directory for the current platform.
pub fn config_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| {
        CliError::Config("Could not determine config directory for this platform.".into())
    })?;
    Ok(base.join("tokanban"))
}

/// Returns the default config file path for the current platform.
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Load config from disk, returning defaults if file doesn't exist.
pub fn load_config(path: Option<&PathBuf>) -> Result<AppConfig> {
    let path = match path {
        Some(p) => p.clone(),
        None => config_path()?,
    };

    if !path.exists() {
        return Ok(AppConfig::default());
    }

    check_permissions(&path)?;

    let contents = fs::read_to_string(&path)?;
    let config: AppConfig = toml::from_str(&contents)?;
    Ok(config)
}

/// Save config to disk, creating the directory if needed.
pub fn save_config(config: &AppConfig, path: Option<&PathBuf>) -> Result<()> {
    let path = match path {
        Some(p) => p.clone(),
        None => config_path()?,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(config)?;
    fs::write(&path, contents)?;

    // Set permissions to 0600 on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Check that config file permissions are not overly permissive (Unix only).
#[cfg(unix)]
fn check_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode > 0o600 {
        return Err(CliError::InsecureConfig { mode });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &PathBuf) -> Result<()> {
    Ok(())
}
