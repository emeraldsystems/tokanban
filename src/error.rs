use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{message}")]
    Api {
        code: String,
        message: String,
        details: Option<String>,
        hint: Option<String>,
    },

    #[error("Authentication required. Run `tokanban auth login` to authenticate.")]
    NotAuthenticated,

    #[error("Access token expired and refresh failed. Run `tokanban auth login`.")]
    TokenRefreshFailed,

    #[error("Config file has insecure permissions (mode {mode}). Expected 0600 or stricter.")]
    InsecureConfig { mode: u32 },

    #[error("Missing required value: {0}. Provide via --{0} flag or set a default with `tokanban {1} set`.")]
    MissingRequired(String, String),

    #[error("{0}")]
    InvalidInput(String),

    #[error("{0}")]
    Config(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    TomlDeserialize(#[from] toml::de::Error),

    #[error(transparent)]
    TomlSerialize(#[from] toml::ser::Error),
}

impl CliError {
    /// Format the error for terminal output following Tokanban design guidelines.
    /// No emoji, no "Oops!", structured with Hint/Fix when available.
    /// Return the appropriate exit code for this error (§6.3).
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::NotAuthenticated | CliError::TokenRefreshFailed => 2,
            CliError::InsecureConfig { .. } | CliError::Config(_) => 5,
            CliError::Api { code, .. } => {
                match code.as_str() {
                    "auth.forbidden" | "403" => 3,
                    "api.rate_limit" | "429" => 4,
                    "auth.unauthenticated" | "401" => 2,
                    _ => 1,
                }
            }
            CliError::MissingRequired(_, _) | CliError::InvalidInput(_) => 1,
            _ => 1,
        }
    }

    pub fn render(&self) -> String {
        match self {
            CliError::Api {
                code,
                message,
                details,
                hint,
            } => {
                let mut out = format!("Error [{code}]: {message}");
                if let Some(d) = details {
                    out.push_str(&format!("\n  Detail: {d}"));
                }
                if let Some(h) = hint {
                    out.push_str(&format!("\n  Hint: {h}"));
                }
                out
            }
            other => format!("Error: {other}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, CliError>;
