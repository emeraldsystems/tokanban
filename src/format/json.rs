/// JSON output helpers.
///
/// When stdout is not a TTY or `--format json` is passed, we output raw JSON.
/// For non-TTY piped output we skip pretty-printing (compact JSON per §8.1).

use serde::Serialize;

/// Print a value as JSON. Uses compact format for non-TTY, pretty for TTY.
pub fn render_json<T: Serialize>(value: &T, pretty: bool) -> Result<String, serde_json::Error> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
}

/// Standard JSON wrapper for list responses.
#[derive(Serialize)]
pub struct JsonList<T: Serialize> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: u32,
    pub limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Standard JSON wrapper for mutation responses.
#[derive(Serialize)]
pub struct JsonMutation {
    pub action: String,
    pub resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub message: String,
}
