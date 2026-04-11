use super::{ColorConfig, EM_DASH, terminal_width};

/// Minimum and maximum card widths.
const MIN_WIDTH: usize = 50;
const MAX_WIDTH: usize = 80;

/// A key-value pair displayed inside a card.
pub struct CardField {
    pub label: &'static str,
    pub value: Option<String>,
}

impl CardField {
    pub fn new(label: &'static str, value: Option<String>) -> Self {
        CardField { label, value }
    }

    pub fn required(label: &'static str, value: String) -> Self {
        CardField { label, value: Some(value) }
    }
}

/// Section inside a card (e.g. Description, Comments, Dependencies).
pub enum CardSection {
    /// Two-column key-value pairs laid out in a grid.
    Fields(Vec<CardField>),
    /// A heading followed by flowing prose text.
    Prose { heading: String, body: String },
    /// A heading followed by list items.
    List { heading: String, items: Vec<String> },
}

/// Render a card using box-drawing characters.
///
/// ```text
/// ┌─ PLAT-42 ────────────────────────────────────┐
/// │ Fix auth token refresh logic                 │
/// ├───────────────────────────────────────────────┤
/// │ Status     In Progress    Priority   High    │
/// │ Assignee   @sven          Sprint     12      │
/// ├───────────────────────────────────────────────┤
/// │ Description                                   │
/// │                                               │
/// │ Token refresh fails silently …               │
/// └───────────────────────────────────────────────┘
/// ```
pub fn render_card(
    title_key: &str,
    title_text: &str,
    sections: &[CardSection],
    color: &ColorConfig,
) -> String {
    // Card width: capped at terminal width minus 2 for side borders.
    let inner_width = (terminal_width().saturating_sub(2))
        .min(MAX_WIDTH)
        .max(MIN_WIDTH);

    let mut out = String::new();

    // Top border with key embedded.
    let key_part = format!("─ {title_key} ");
    let remaining = inner_width.saturating_sub(key_part.chars().count());
    let top = format!("┌{key_part}{:─<width$}┐", "", width = remaining);
    out.push_str(&top);
    out.push('\n');

    // Title line.
    out.push_str(&box_line(title_text, inner_width, color));
    out.push('\n');

    // Sections.
    for section in sections {
        // Section separator.
        out.push_str(&format!("├{:─<width$}┤", "", width = inner_width));
        out.push('\n');

        match section {
            CardSection::Fields(fields) => {
                render_fields_section(&mut out, fields, inner_width, color);
            }
            CardSection::Prose { heading, body } => {
                render_prose_section(&mut out, heading.as_str(), body, inner_width, color);
            }
            CardSection::List { heading, items } => {
                render_list_section(&mut out, heading.as_str(), items, inner_width, color);
            }
        }
    }

    // Bottom border.
    out.push_str(&format!("└{:─<width$}┘", "", width = inner_width));
    out.push('\n');

    out
}

/// Render a two-column key-value field section.
///
/// Fields are paired: [0,1] on one row, [2,3] on next row, etc.
fn render_fields_section(
    out: &mut String,
    fields: &[CardField],
    inner_width: usize,
    color: &ColorConfig,
) {
    // Label column width (max label length + 2 for padding).
    let label_w = fields.iter().map(|f| f.label.len()).max().unwrap_or(8) + 1;
    // Half the inner content width for two-column layout.
    let half = (inner_width.saturating_sub(2)) / 2;

    let mut i = 0;
    while i < fields.len() {
        let left = &fields[i];
        let right = fields.get(i + 1);

        let left_val = left.value.as_deref().unwrap_or(EM_DASH);
        let left_label = color.bold(left.label);
        let left_cell = format!("{left_label:<label_w$} {left_val}");
        let left_cell_vis = left_label.len()
            - (left_label.len().saturating_sub(left.label.len())) // strip ANSI overhead? simpler:
            + 1
            + left_val.len();
        let _ = left_cell_vis; // We'll use fixed padding instead.

        let right_cell = if let Some(r) = right {
            let rval = r.value.as_deref().unwrap_or(EM_DASH);
            format!("{:<label_w$} {rval}", color.bold(r.label))
        } else {
            String::new()
        };

        // Build the full inner line padded to inner_width.
        let content = format!("{left_cell:<half$}  {right_cell}");
        out.push_str(&box_content_line(&content, inner_width));
        out.push('\n');

        i += 2;
    }
}

/// Render a prose section (heading + wrapped body text).
fn render_prose_section(
    out: &mut String,
    heading: &str,
    body: &str,
    inner_width: usize,
    color: &ColorConfig,
) {
    out.push_str(&box_line(&color.bold(heading), inner_width, color));
    out.push('\n');
    out.push_str(&box_line("", inner_width, color));
    out.push('\n');

    let wrap_width = inner_width.saturating_sub(2); // 1 space each side
    for line in wrap_text(body, wrap_width) {
        out.push_str(&box_line(&line, inner_width, color));
        out.push('\n');
    }
}

/// Render a list section (heading + indented items).
fn render_list_section(
    out: &mut String,
    heading: &str,
    items: &[String],
    inner_width: usize,
    color: &ColorConfig,
) {
    out.push_str(&box_line(&color.bold(heading), inner_width, color));
    out.push('\n');
    for item in items {
        let indented = format!("  {item}");
        out.push_str(&box_line(&indented, inner_width, color));
        out.push('\n');
    }
}

/// Wrap `text` to lines of at most `width` characters.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.clone());
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

/// Format `│ {content padded to inner_width} │`.
fn box_line(content: &str, inner_width: usize, _color: &ColorConfig) -> String {
    box_content_line(content, inner_width)
}

/// Build a box line with content padded to inner_width (content is plain or
/// may contain ANSI codes — we measure visible width for padding).
fn box_content_line(content: &str, inner_width: usize) -> String {
    use crate::format::table::visible_width;
    let vis = visible_width(content);
    // inner_width includes 1 leading space + content + trailing spaces
    // Layout: │ {content}{padding} │
    let available = inner_width.saturating_sub(2); // subtract the two spaces for margins
    let padding = available.saturating_sub(vis);
    format!("│ {content}{} │", " ".repeat(padding))
}
