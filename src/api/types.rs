use serde::de::{Deserializer, Error as DeError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Standard API error response from the Tokanban backend.
#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorBody,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
    pub hint: Option<String>,
}

/// Paginated list response wrapper.
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub limit: u64,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct PaginatedResponseWire<T> {
    #[serde(default)]
    items: Option<Vec<T>>,
    #[serde(default)]
    data: Option<Vec<T>>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    page: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    pagination: Option<PaginationMeta>,
}

#[derive(Debug, Default, Deserialize)]
struct PaginationMeta {
    #[serde(default)]
    cursor: Option<String>,
}

fn default_page() -> u64 {
    1
}

fn deserialize_optional_stringish<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(other) => Err(D::Error::custom(format!(
            "expected string or number for timestamp, got {other}"
        ))),
    }
}

impl<'de, T> Deserialize<'de> for PaginatedResponse<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PaginatedResponseWire::<T>::deserialize(deserializer)?;
        let items = wire.items.or(wire.data).unwrap_or_default();
        let count = items.len() as u64;

        Ok(Self {
            total: wire.total.unwrap_or(count),
            page: wire.page.unwrap_or(default_page()),
            limit: wire.limit.unwrap_or(count),
            cursor: wire
                .cursor
                .or_else(|| wire.pagination.and_then(|pagination| pagination.cursor)),
            items,
        })
    }
}

/// Token exchange response from OAuth flow.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: Option<String>,
}

/// Generic mutation response (create/update).
#[derive(Debug, Serialize, Deserialize)]
pub struct MutationResponse {
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Task API types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub key: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<AssigneeInfo>,
    #[serde(default)]
    pub sprint: Option<SprintRef>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskDetailResponse {
    pub id: String,
    pub key: String,
    pub title: String,
    pub status: String,
    #[serde(default, rename = "type")]
    pub task_type: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<AssigneeInfo>,
    #[serde(default)]
    pub sprint: Option<SprintRef>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub estimate: Option<f64>,
    #[serde(default)]
    pub reporter: Option<AssigneeInfo>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub comments_count: u32,
    #[serde(default)]
    pub comments: Vec<CommentItem>,
    #[serde(default)]
    pub blocked_by: Vec<TaskRef>,
    #[serde(default)]
    pub blocks: Vec<TaskRef>,
    #[serde(default)]
    pub activity: Vec<ActivityItem>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssigneeInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SprintRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskRef {
    pub key: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommentItem {
    pub id: String,
    pub author: AssigneeInfo,
    pub body: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActivityItem {
    pub actor: String,
    pub description: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Project API types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectItem {
    pub id: String,
    #[serde(default)]
    pub key: String,
    pub name: String,
    pub key_prefix: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub task_count: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_stringish")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectDetailResponse {
    pub id: String,
    #[serde(default)]
    pub key: String,
    pub name: String,
    pub key_prefix: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub task_count: Option<u64>,
    #[serde(default)]
    pub member_count: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_stringish")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_stringish")]
    pub updated_at: Option<String>,
}
