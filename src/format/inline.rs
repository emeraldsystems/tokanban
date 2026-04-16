use super::{ColorConfig, colors};

/// Format a mutation confirmation message.
///
/// Rules (design brief §3.3):
/// - Single-line for single mutations.
/// - Arrow (→) for state transitions.
/// - No "Successfully" prefix.
pub fn mutation_created(resource: &str, key: &str, title: Option<&str>, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    if let Some(t) = title {
        format!("{check} {key} {resource} created — {t}")
    } else {
        format!("{check} {key} {resource} created")
    }
}

pub fn mutation_updated(_resource: &str, key: &str, changes: &[(&str, &str, &str)], color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    if changes.len() == 1 {
        let (field, from, to) = changes[0];
        format!("{check} Updated {key}  {field}: {from} {} {to}", arrow(color))
    } else {
        let parts: Vec<String> = changes
            .iter()
            .map(|(field, from, to)| format!("{field}: {from} {} {to}", arrow(color)))
            .collect();
        format!("{check} Updated {key}  {}", parts.join(", "))
    }
}

pub fn mutation_deleted(resource: &str, key: &str, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    format!("{check} {resource} {key} deleted")
}

pub fn mutation_archived(resource: &str, key: &str, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    format!("{check} {resource} {key} archived")
}

pub fn mutation_closed(resource: &str, key: &str, reason: Option<&str>, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    if let Some(r) = reason {
        format!("{check} {resource} {key} closed — {r}")
    } else {
        format!("{check} {resource} {key} closed")
    }
}

pub fn mutation_reopened(resource: &str, key: &str, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    format!("{check} {resource} {key} reopened")
}

pub fn mutation_invited(email: &str, role: &str, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    format!("{check} Invited {email} as {role}")
}

pub fn mutation_revoked(resource: &str, key: &str, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    format!("{check} {resource} {key} revoked")
}

pub fn mutation_rotated(resource: &str, key: &str, valid_until: Option<&str>, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    if let Some(until) = valid_until {
        format!("{check} {resource} key rotated for {key} (valid until {until})")
    } else {
        format!("{check} {resource} key rotated for {key}")
    }
}

/// Compact card format: `✓ PLAT-42  Fix auth token refresh logic  →  In Progress  @bob`
pub fn compact_card(key: &str, title: &str, status: Option<&str>, assignee: Option<&str>, color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    let mut parts = vec![format!("{check} {key}"), title.to_string()];
    if let Some(s) = status {
        parts.push(format!("{} {s}", arrow(color)));
    }
    if let Some(a) = assignee {
        parts.push(format!("@{a}"));
    }
    parts.join("  ")
}

/// Bulk operation summary: `Moved 3 tasks to Sprint 13: PLAT-50, PLAT-51, PLAT-52`
pub fn bulk_summary(verb: &str, count: usize, resource: &str, context: &str, keys: &[String], color: &ColorConfig) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    let key_list = if keys.len() <= 5 {
        keys.join(", ")
    } else {
        let shown: Vec<&String> = keys[..5].iter().collect();
        format!("{}, … ({} more)", shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), keys.len() - 5)
    };
    format!("{check} {verb} {count} {resource}s {context}: {key_list}")
}

fn arrow(color: &ColorConfig) -> String {
    color.paint("→", colors::MUTED)
}
