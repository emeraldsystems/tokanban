use super::{ColorConfig, EM_DASH, terminal_width, truncate};

/// A single column definition for a table.
pub struct Column {
    pub header: &'static str,
    /// Minimum width (header length is the floor).
    pub min_width: usize,
    /// Right-align numeric/priority columns.
    pub right_align: bool,
    /// This column gets any extra space when the terminal is wide.
    pub flexible: bool,
}

impl Column {
    pub fn new(header: &'static str, min_width: usize) -> Self {
        Column { header, min_width, right_align: false, flexible: false }
    }

    pub fn right(mut self) -> Self {
        self.right_align = true;
        self
    }

    pub fn flexible(mut self) -> Self {
        self.flexible = true;
        self
    }
}

/// Render a table given column definitions and rows.
///
/// Each row is a `Vec<Option<String>>` — `None` renders as em-dash.
pub fn render_table(
    columns: &[Column],
    rows: &[Vec<Option<String>>],
    color: &ColorConfig,
) -> String {
    let term_width = terminal_width();

    // Compute column widths: max of header length, min_width, and all data widths.
    let mut widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let data_max = rows
                .iter()
                .filter_map(|row| row.get(i).and_then(|v| v.as_deref()))
                // Strip ANSI codes for width calculation.
                .map(|s| visible_width(s))
                .max()
                .unwrap_or(0);
            col.min_width.max(col.header.len()).max(data_max)
        })
        .collect();

    // Distribute extra terminal space to the flexible column, if any.
    let total_fixed: usize = widths.iter().sum::<usize>() + (widths.len() * 2).saturating_sub(2);
    if let Some(flex_idx) = columns.iter().position(|c| c.flexible) {
        if total_fixed < term_width {
            let extra = term_width.saturating_sub(total_fixed);
            widths[flex_idx] += extra;
        }
    }

    let mut out = String::new();

    // Header row.
    let header_line: String = columns
        .iter()
        .enumerate()
        .map(|(i, col)| pad(col.header, widths[i], col.right_align))
        .collect::<Vec<_>>()
        .join("  ");
    out.push_str(&color.bold(&header_line));
    out.push('\n');

    // Separator.
    let sep: String = widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("  ");
    out.push_str(&sep);
    out.push('\n');

    // Data rows.
    for row in rows {
        let line: String = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                let raw = row
                    .get(i)
                    .and_then(|v| v.as_deref())
                    .unwrap_or(EM_DASH);
                // Truncate at column width (using visible chars).
                let truncated_raw = if col.flexible {
                    let raw_vis = visible_width(raw);
                    if raw_vis > widths[i] {
                        // Need to truncate, but raw may have ANSI codes.
                        // Simple approach: truncate on visible chars of the plain string.
                        let plain = strip_ansi(raw);
                        truncate(&plain, widths[i])
                    } else {
                        raw.to_string()
                    }
                } else {
                    raw.to_string()
                };
                pad_with_ansi(&truncated_raw, widths[i], col.right_align)
            })
            .collect::<Vec<_>>()
            .join("  ");
        out.push_str(&line);
        out.push('\n');
    }

    out
}

/// Left- or right-pad a plain (no ANSI) string to `width`.
fn pad(s: &str, width: usize, right: bool) -> String {
    let len = s.chars().count();
    let padding = width.saturating_sub(len);
    if right {
        format!("{}{s}", " ".repeat(padding))
    } else {
        format!("{s}{}", " ".repeat(padding))
    }
}

/// Pad a string that may contain ANSI escape codes. We measure visible width
/// (stripping codes) to determine how much padding to add.
fn pad_with_ansi(s: &str, width: usize, right: bool) -> String {
    let vis = visible_width(s);
    let padding = width.saturating_sub(vis);
    if right {
        format!("{}{s}", " ".repeat(padding))
    } else {
        format!("{s}{}", " ".repeat(padding))
    }
}

/// Count printable (non-ANSI) characters.
pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Remove ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Consume until we hit a letter (final byte of the sequence).
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Pre-built task list table
// ---------------------------------------------------------------------------

/// Data for a single task row in a task list table.
pub struct TaskRow {
    pub key: String,
    pub status: String,
    pub title: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub sprint: Option<String>,
    pub due: Option<String>,
}

/// Render the standard task list table.
pub fn render_task_list(tasks: &[TaskRow], color: &ColorConfig) -> String {
    use super::{color_priority, color_status};

    let columns = [
        Column::new("Key", 7),
        Column::new("Status", 11),
        Column::new("Title", 20).flexible(),
        Column::new("Assignee", 8),
        Column::new("Priority", 8).right(),
        Column::new("Sprint", 8),
    ];

    let rows: Vec<Vec<Option<String>>> = tasks
        .iter()
        .map(|t| {
            vec![
                Some(color.paint(&t.key, super::colors::MUTED)),
                Some(color_status(&t.status, color)),
                Some(t.title.clone()),
                t.assignee.as_deref().map(|a| format!("@{a}")),
                t.priority.as_deref().map(|p| color_priority(p, color)),
                t.sprint.clone(),
            ]
        })
        .collect();

    render_table(&columns, &rows, color)
}

/// Render a one-line task summary (used in search results, sprint boards).
///
/// Format: `PLAT-42  In Progress  Fix auth token refresh logic  @bob  P:High  Sprint 12`
pub fn render_task_summary(task: &TaskRow, color: &ColorConfig) -> String {
    use super::{color_priority, color_status};

    let key = color.paint(&task.key, super::colors::MUTED);
    let status = color_status(&task.status, color);
    let assignee = task
        .assignee
        .as_deref()
        .map(|a| format!("@{a}"))
        .unwrap_or_else(|| EM_DASH.to_string());
    let priority = task
        .priority
        .as_deref()
        .map(|p| color_priority(p, color))
        .unwrap_or_else(|| EM_DASH.to_string());
    let sprint = task.sprint.as_deref().unwrap_or(EM_DASH);

    format!("{key}  {status}  {}  {assignee}  {priority}  {sprint}", task.title)
}
