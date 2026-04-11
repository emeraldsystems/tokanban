pub mod card;
pub mod inline;
pub mod json;
pub mod table;

use std::io::IsTerminal;

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Auto-detected: TUI in TTY, JSON otherwise.
    Auto,
    /// Human-readable table for lists.
    Table,
    /// Box-drawing card for detail views.
    Card,
    /// Single-line confirmation for mutations.
    Inline,
    /// Raw JSON.
    Json,
}

impl OutputFormat {
    /// Resolve `Auto` to a concrete format based on TTY state.
    pub fn resolve(self) -> OutputFormat {
        match self {
            OutputFormat::Auto => {
                if std::io::stdout().is_terminal() {
                    OutputFormat::Table
                } else {
                    OutputFormat::Json
                }
            }
            other => other,
        }
    }

    /// Auto-detect from CLI flags (§5.1):
    /// 1. Explicit --format flag
    /// 2. --quiet → JSON
    /// 3. Non-TTY → JSON
    /// 4. Interactive → Auto
    pub fn detect(explicit: Option<&str>, quiet: bool) -> Self {
        if let Some(fmt) = explicit {
            return fmt.parse().unwrap_or(OutputFormat::Table);
        }
        if quiet || !std::io::stdout().is_terminal() {
            return OutputFormat::Json;
        }
        OutputFormat::Auto
    }

    pub fn is_json(self) -> bool {
        matches!(self, OutputFormat::Json)
    }

    pub fn is_tui(self) -> bool {
        !self.is_json()
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(OutputFormat::Auto),
            "table" => Ok(OutputFormat::Table),
            "card" => Ok(OutputFormat::Card),
            "inline" => Ok(OutputFormat::Inline),
            "json" => Ok(OutputFormat::Json),
            other => Err(format!(
                "unknown format '{other}'; expected auto, table, card, inline, or json"
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Color support
// ---------------------------------------------------------------------------

/// Global color configuration. Set once at startup from `--no-color` / `NO_COLOR`.
#[derive(Debug, Clone, Copy)]
pub struct ColorConfig {
    pub enabled: bool,
}

impl ColorConfig {
    /// Determine color support from flags and environment.
    ///
    /// Color is disabled when:
    /// - `--no-color` flag is set, OR
    /// - `NO_COLOR` env var is set (any value), OR
    /// - stdout is not a TTY.
    pub fn new(no_color_flag: bool) -> Self {
        let enabled = !no_color_flag
            && std::env::var("NO_COLOR").is_err()
            && std::io::stdout().is_terminal();
        ColorConfig { enabled }
    }

    /// Wrap `text` in the given ANSI 256 color code, or return `text` unchanged.
    pub fn paint(&self, text: &str, ansi_code: u8) -> String {
        if self.enabled {
            format!("\x1b[38;5;{ansi_code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: &str) -> String {
        if self.enabled {
            format!("\x1b[1m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// ANSI 256 color codes (design brief color palette)
// ---------------------------------------------------------------------------

pub mod colors {
    pub const URGENT: u8 = 196;  // #EF4444 red
    pub const HIGH: u8 = 214;    // #F59E0B amber
    pub const MEDIUM: u8 = 35;   // #10B981 green
    pub const LOW: u8 = 247;     // #94A3B8 gray
    pub const MUTED: u8 = 247;   // #94A3B8 — IDs, timestamps
    pub const ERROR: u8 = 196;   // red
    pub const SUCCESS: u8 = 35;  // green
}

// ---------------------------------------------------------------------------
// Terminal width
// ---------------------------------------------------------------------------

/// Returns the current terminal column width, or 80 as a safe fallback.
pub fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Em-dash used for missing values (design brief: "never blank").
pub const EM_DASH: &str = "—";

/// Truncate a string to `max_chars`, appending "…" if truncated.
pub fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else if max_chars == 0 {
        String::new()
    } else {
        let truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{truncated}…")
    }
}

/// Format a priority value as `P:High`, `P:Med`, etc.
pub fn format_priority(priority: &str) -> String {
    let lower = priority.to_ascii_lowercase();
    let abbrev = match lower.as_str() {
        "urgent" | "critical" => "Urgent",
        "high" => "High",
        "medium" | "med" => "Med",
        "low" => "Low",
        "none" | "" => return EM_DASH.to_string(),
        other => other,
    };
    format!("P:{abbrev}")
}

/// Colorize a priority string using ANSI codes.
pub fn color_priority(priority: &str, color: &ColorConfig) -> String {
    let lower = priority.to_ascii_lowercase();
    let code = match lower.as_str() {
        "urgent" | "critical" => colors::URGENT,
        "high" => colors::HIGH,
        "medium" | "med" => colors::MEDIUM,
        "low" => colors::LOW,
        _ => colors::MUTED,
    };
    color.paint(&format_priority(priority), code)
}

/// Colorize a status string: active states green, terminal states gray.
pub fn color_status(status: &str, color: &ColorConfig) -> String {
    let lower = status.to_ascii_lowercase();
    let code = match lower.as_str() {
        "in progress" | "in review" | "active" => colors::SUCCESS,
        "done" | "closed" | "cancelled" | "archived" => colors::MUTED,
        _ => return status.to_string(),
    };
    color.paint(status, code)
}

// ---------------------------------------------------------------------------
// Display data types (used by commands to pass structured data to formatters)
// ---------------------------------------------------------------------------

/// Compact task data for list/search tables.
pub struct TaskSummary {
    pub key: String,
    pub status: String,
    pub title: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub sprint: Option<String>,
    pub due: Option<String>,
}

/// Full task data for detail card view.
pub struct TaskDetail {
    pub key: String,
    pub title: String,
    pub status: String,
    pub task_type: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub sprint: Option<String>,
    pub due_date: Option<String>,
    pub labels: Option<String>,
    pub estimate: Option<String>,
    pub reporter: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub description: Option<String>,
    pub comments_count: u32,
    pub comments_preview: Vec<(String, String)>, // (author, body)
    pub blocked_by: Vec<(String, String)>,         // (key, title)
    pub blocks: Vec<(String, String)>,             // (key, title)
    pub activity: Vec<(String, String, String)>,   // (timestamp, actor, description)
}

// ---------------------------------------------------------------------------
// High-level print functions (used by all command handlers)
// ---------------------------------------------------------------------------

/// Print a list of tasks.
///
/// TUI → table with fixed-width columns.
/// JSON → serialized as `{"items": [...], "total": N}`.
pub fn print_task_list<T: serde::Serialize>(
    tasks: &[TaskSummary],
    json_value: Option<&T>,
    format: OutputFormat,
    color: &ColorConfig,
) {
    match format.resolve() {
        OutputFormat::Json => {
            if let Some(v) = json_value {
                print_json(v);
            }
        }
        _ => {
            if tasks.is_empty() {
                println!("No tasks.");
                return;
            }
            let rows: Vec<table::TaskRow> = tasks
                .iter()
                .map(|t| table::TaskRow {
                    key: t.key.clone(),
                    status: t.status.clone(),
                    title: t.title.clone(),
                    assignee: t.assignee.clone(),
                    priority: t.priority.clone(),
                    sprint: t.sprint.clone(),
                    due: t.due.clone(),
                })
                .collect();
            print!("{}", table::render_task_list(&rows, color));
        }
    }
}

/// Print a task detail card.
pub fn print_task_card<T: serde::Serialize>(
    task: &TaskDetail,
    json_value: Option<&T>,
    format: OutputFormat,
    color: &ColorConfig,
) {
    match format.resolve() {
        OutputFormat::Json => {
            if let Some(v) = json_value {
                print_json(v);
            }
        }
        _ => {
            let mut sections = Vec::new();

            // Primary meta fields (two-column layout per design brief).
            let fields = vec![
                card::CardField::required("Status", color_status(&task.status, color)),
                card::CardField::new(
                    "Priority",
                    Some(
                        task.priority
                            .as_deref()
                            .map(|p| color_priority(p, color))
                            .unwrap_or_else(|| EM_DASH.to_string()),
                    ),
                ),
                card::CardField::new("Type", task.task_type.clone()),
                card::CardField::new(
                    "Assignee",
                    task.assignee.as_ref().map(|a| format!("@{a}")),
                ),
                card::CardField::new("Sprint", task.sprint.clone()),
                card::CardField::new("Due", task.due_date.clone()),
                card::CardField::new("Labels", task.labels.clone()),
                card::CardField::new("Estimate", task.estimate.clone()),
                card::CardField::new("Reporter", task.reporter.clone()),
            ];
            sections.push(card::CardSection::Fields(fields));

            // Description.
            if let Some(desc) = &task.description {
                if !desc.is_empty() {
                    sections.push(card::CardSection::Prose {
                        heading: "Description".to_string(),
                        body: desc.clone(),
                    });
                }
            }

            // Dependencies.
            if !task.blocked_by.is_empty() || !task.blocks.is_empty() {
                let mut items = Vec::new();
                for (key, title) in &task.blocked_by {
                    items.push(format!("blocked by  {key}  {title}"));
                }
                for (key, title) in &task.blocks {
                    items.push(format!("blocks      {key}  {title}"));
                }
                sections.push(card::CardSection::List {
                    heading: "Dependencies".to_string(),
                    items,
                });
            }

            // Activity.
            if !task.activity.is_empty() {
                let items: Vec<String> = task
                    .activity
                    .iter()
                    .map(|(ts, actor, desc)| format!("{ts}  {actor}  {desc}"))
                    .collect();
                sections.push(card::CardSection::List {
                    heading: format!("Activity (latest {})", task.activity.len()),
                    items,
                });
            }

            // Comments.
            if task.comments_count > 0 {
                let items: Vec<String> = task
                    .comments_preview
                    .iter()
                    .map(|(author, body)| {
                        let truncated =
                            truncate(body, terminal_width().saturating_sub(12));
                        format!("@{author}: {truncated}")
                    })
                    .collect();
                sections.push(card::CardSection::List {
                    heading: format!("Comments ({})", task.comments_count),
                    items,
                });
            }

            print!("{}", card::render_card(&task.key, &task.title, &sections, color));
        }
    }
}

/// Print raw JSON to stdout.
///
/// Compact (no whitespace) for non-TTY per §8.1; pretty-printed otherwise.
pub fn print_json<T: serde::Serialize>(value: &T) {
    let pretty = std::io::stdout().is_terminal();
    let result = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    };
    match result {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("Error: serialization failed: {e}"),
    }
}

/// Print a single-line mutation confirmation.
///
/// Design brief §3.3: no "Successfully" prefix; arrow (→) for state transitions.
pub fn print_inline(msg: &str) {
    println!("{msg}");
}

/// Print a pagination footer when there are more results.
///
/// "N more results — use --cursor <TOKEN> to continue"
pub fn print_pagination_footer(remaining: u64, cursor: &str, color: &ColorConfig) {
    let msg = color.paint(
        &format!("{remaining} more results — use --cursor {cursor} to continue"),
        colors::MUTED,
    );
    println!("{msg}");
}
