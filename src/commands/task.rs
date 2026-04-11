use clap::Subcommand;
use serde_json::json;

use crate::api::{
    MutationResponse, PaginatedResponse, TaskDetailResponse, TaskItem,
};
use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format::{self, TaskDetail, TaskSummary};

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Create a new task
    Create {
        /// Task title
        title: String,
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Priority: urgent, high, medium, low, none (case-insensitive)
        #[arg(long)]
        priority: Option<String>,
        /// Assignee username or ID
        #[arg(long)]
        assignee: Option<String>,
        /// Sprint ID
        #[arg(long)]
        sprint: Option<String>,
        /// Task description
        #[arg(long)]
        description: Option<String>,
    },
    /// List tasks with optional filters
    List {
        /// Filter by project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by assignee
        #[arg(long)]
        assignee: Option<String>,
        /// Filter by sprint ID
        #[arg(long)]
        sprint: Option<String>,
        /// Filter by priority (case-insensitive)
        #[arg(long)]
        priority: Option<String>,
        /// Filter by due date (ISO 8601)
        #[arg(long)]
        due: Option<String>,
        /// Cursor for pagination
        #[arg(long)]
        cursor: Option<String>,
        /// Page size (default 25, max 100)
        #[arg(long, default_value = "25")]
        limit: u32,
    },
    /// View full task details
    View {
        /// Task key (e.g., PLAT-42)
        key: String,
    },
    /// Update a task's fields
    Update {
        /// Task key
        key: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New status
        #[arg(long)]
        status: Option<String>,
        /// New assignee
        #[arg(long)]
        assignee: Option<String>,
        /// New priority (case-insensitive)
        #[arg(long)]
        priority: Option<String>,
        /// New sprint ID
        #[arg(long)]
        sprint: Option<String>,
    },
    /// Search tasks by free-text query
    Search {
        /// Search query
        query: String,
        /// Filter by project key
        #[arg(long)]
        project: Option<String>,
        /// Maximum results (default 20)
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Close a task (transitions status to Closed)
    Close {
        /// Task key
        key: String,
        /// Reason for closing
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reopen a closed task
    Reopen {
        /// Task key
        key: String,
    },
}

pub async fn handle(cmd: &TaskCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        TaskCommand::Create { title, project, priority, assignee, sprint, description } => {
            handle_create(ctx, title, project.clone(), priority.as_deref(), assignee.as_deref(), sprint.as_deref(), description.as_deref()).await
        }
        TaskCommand::List { project, status, assignee, sprint, priority, due, cursor, limit } => {
            handle_list(ctx, project.clone(), status.as_deref(), assignee.as_deref(), sprint.as_deref(), priority.as_deref(), due.as_deref(), cursor.as_deref(), *limit).await
        }
        TaskCommand::View { key } => handle_view(ctx, key).await,
        TaskCommand::Update { key, title, status, assignee, priority, sprint } => {
            handle_update(ctx, key, title.as_deref(), status.as_deref(), assignee.as_deref(), priority.as_deref(), sprint.as_deref()).await
        }
        TaskCommand::Search { query, project, limit } => {
            handle_search(ctx, query, project.clone(), *limit).await
        }
        TaskCommand::Close { key, reason } => handle_close(ctx, key, reason.as_deref()).await,
        TaskCommand::Reopen { key } => handle_reopen(ctx, key).await,
    }
}

async fn handle_create(
    ctx: &Ctx,
    title: &str,
    project: Option<String>,
    priority: Option<&str>,
    assignee: Option<&str>,
    sprint: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;

    let mut body = json!({ "title": title });
    if let Some(p) = priority { body["priority"] = json!(normalize_priority(p)?); }
    if let Some(a) = assignee { body["assignee_id"] = json!(a); }
    if let Some(s) = sprint { body["sprint_id"] = json!(s); }
    if let Some(d) = description { body["description"] = json!(d); }

    let resp: MutationResponse = ctx.api
        .post(&format!("/v1/projects/{project_id}/tasks"), &body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let key = resp.key.as_deref().unwrap_or(&resp.id);
        let msg = crate::format::inline::mutation_created("task", key, Some(title), &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(
    ctx: &Ctx,
    project: Option<String>,
    status: Option<&str>,
    assignee: Option<&str>,
    sprint: Option<&str>,
    priority: Option<&str>,
    due: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;

    let mut url = format!("/v1/projects/{project_id}/tasks?limit={limit}");
    if let Some(s) = status {
        let normalized = normalize_status(s);
        url.push_str(&format!("&status={}", enc(&normalized)));
    }
    if let Some(a) = assignee { url.push_str(&format!("&assignee={}", enc(a))); }
    if let Some(s) = sprint { url.push_str(&format!("&sprint_id={}", enc(s))); }
    if let Some(p) = priority {
        let normalized = normalize_priority(p)?;
        url.push_str(&format!("&priority={}", enc(&normalized)));
    }
    if let Some(d) = due { url.push_str(&format!("&due={}", enc(d))); }
    if let Some(c) = cursor { url.push_str(&format!("&cursor={}", enc(c))); }

    let resp: PaginatedResponse<TaskItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let summaries: Vec<TaskSummary> = resp.items.iter().map(task_item_to_summary).collect();
        format::print_task_list(
            &summaries,
            Option::<&PaginatedResponse<TaskItem>>::None,
            ctx.format,
            &ctx.color,
        );
        if let Some(next_cursor) = &resp.cursor {
            let remaining = resp.total.saturating_sub(resp.items.len() as u64);
            if remaining > 0 {
                format::print_pagination_footer(remaining, next_cursor, &ctx.color);
            }
        }
    }
    Ok(())
}

async fn handle_view(ctx: &Ctx, key: &str) -> Result<()> {
    let resp: TaskDetailResponse = ctx.api.get(&format!("/v1/tasks/{key}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let detail = task_detail_to_display(&resp);
        format::print_task_card(
            &detail,
            Option::<&TaskDetailResponse>::None,
            ctx.format,
            &ctx.color,
        );
    }
    Ok(())
}

async fn handle_update(
    ctx: &Ctx,
    key: &str,
    title: Option<&str>,
    status: Option<&str>,
    assignee: Option<&str>,
    priority: Option<&str>,
    sprint: Option<&str>,
) -> Result<()> {
    let normalized_status = status.map(normalize_status);
    let normalized_priority = match priority {
        Some(value) => Some(normalize_priority(value)?),
        None => None,
    };

    let mut body = json!({});
    if let Some(t) = title { body["title"] = json!(t); }
    if let Some(s) = &normalized_status { body["status"] = json!(s); }
    if let Some(a) = assignee { body["assignee_id"] = json!(a); }
    if let Some(p) = &normalized_priority { body["priority"] = json!(p); }
    if let Some(s) = sprint { body["sprint_id"] = json!(s); }

    let resp: MutationResponse = ctx.api.patch(&format!("/v1/tasks/{key}"), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        // Build change list for the confirmation message.
        let mut changes: Vec<(&str, &str, &str)> = Vec::new();
        if let Some(s) = normalized_status.as_deref() { changes.push(("status", "—", s)); }
        if let Some(p) = normalized_priority.as_deref() { changes.push(("priority", "—", p)); }
        let msg = crate::format::inline::mutation_updated("task", key, &changes, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_search(ctx: &Ctx, query: &str, project: Option<String>, limit: u32) -> Result<()> {
    let mut url = format!("/v1/search/tasks?q={}&limit={limit}", enc(query));
    if let Some(p) = project {
        let project = ctx.resolve_project(&p).await?;
        url.push_str(&format!("&project_id={}", enc(&project.id)));
    }

    let resp: PaginatedResponse<TaskItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let summaries: Vec<TaskSummary> = resp.items.iter().map(task_item_to_summary).collect();
        format::print_task_list(
            &summaries,
            Option::<&PaginatedResponse<TaskItem>>::None,
            ctx.format,
            &ctx.color,
        );
    }
    Ok(())
}

async fn handle_close(ctx: &Ctx, key: &str, reason: Option<&str>) -> Result<()> {
    let mut body = json!({ "status": "done" });
    if let Some(r) = reason { body["close_reason"] = json!(r); }

    let resp: MutationResponse = ctx.api.patch(&format!("/v1/tasks/{key}"), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = crate::format::inline::mutation_closed("task", key, reason, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_reopen(ctx: &Ctx, key: &str) -> Result<()> {
    let body = json!({ "status": "todo" });
    let resp: MutationResponse = ctx.api.patch(&format!("/v1/tasks/{key}"), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = crate::format::inline::mutation_reopened("task", key, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn task_item_to_summary(t: &TaskItem) -> TaskSummary {
    TaskSummary {
        key: t.key.clone(),
        status: t.status.clone(),
        title: t.title.clone(),
        assignee: t.assignee.as_ref().map(|a| a.name.clone()),
        priority: t.priority.clone(),
        sprint: t.sprint.as_ref().map(|s| s.name.clone()),
        due: t.due_date.clone(),
    }
}

fn task_detail_to_display(t: &TaskDetailResponse) -> TaskDetail {
    TaskDetail {
        key: t.key.clone(),
        title: t.title.clone(),
        status: t.status.clone(),
        task_type: t.task_type.clone(),
        priority: t.priority.clone(),
        assignee: t.assignee.as_ref().map(|a| a.name.clone()),
        sprint: t.sprint.as_ref().map(|s| s.name.clone()),
        due_date: t.due_date.clone(),
        labels: t.labels.as_ref().map(|v| v.join(", ")),
        estimate: t.estimate.map(|e| format!("{e} pts")),
        reporter: t.reporter.as_ref().map(|r| format!("@{}", r.name)),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        description: t.description.clone(),
        comments_count: t.comments_count,
        comments_preview: t.comments.iter()
            .take(3)
            .map(|c| (c.author.name.clone(), c.body.clone()))
            .collect(),
        blocked_by: t.blocked_by.iter()
            .map(|r| (r.key.clone(), r.title.clone()))
            .collect(),
        blocks: t.blocks.iter()
            .map(|r| (r.key.clone(), r.title.clone()))
            .collect(),
        activity: t.activity.iter()
            .take(3)
            .map(|a| (a.timestamp.clone(), a.actor.clone(), a.description.clone()))
            .collect(),
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn normalize_priority(priority: &str) -> Result<String> {
    let normalized = priority.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let canonical = match normalized.as_str() {
        "urgent" | "critical" | "p0" => "urgent",
        "high" | "p1" => "high",
        "medium" | "med" | "normal" | "default" | "p2" => "medium",
        "low" | "p3" => "low",
        "none" | "no_priority" | "no-priority" | "unset" => "none",
        _ => {
            return Err(CliError::InvalidInput(format!(
                "Invalid priority '{priority}'. Use one of: urgent, high, medium, low, none."
            )))
        }
    };

    Ok(canonical.to_string())
}

fn normalize_status(status: &str) -> String {
    let trimmed = status.trim();
    let normalized = trimmed.to_ascii_lowercase().replace([' ', '-'], "_");

    match normalized.as_str() {
        "backlog" => "backlog".to_string(),
        "todo" | "to_do" => "todo".to_string(),
        "in_progress" => "in_progress".to_string(),
        "in_review" => "in_review".to_string(),
        "done" | "closed" | "complete" | "completed" => "done".to_string(),
        "cancelled" | "canceled" => "cancelled".to_string(),
        _ => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_priority, normalize_status};
    use crate::error::CliError;

    #[test]
    fn normalize_priority_accepts_common_aliases() {
        assert_eq!(normalize_priority("High").unwrap(), "high");
        assert_eq!(normalize_priority("P0").unwrap(), "urgent");
        assert_eq!(normalize_priority("normal").unwrap(), "medium");
        assert_eq!(normalize_priority("no priority").unwrap(), "none");
    }

    #[test]
    fn normalize_priority_rejects_unknown_value() {
        let err = normalize_priority("super-important").unwrap_err();
        assert!(matches!(err, CliError::InvalidInput(_)));
        assert!(err.to_string().contains("urgent, high, medium, low, none"));
    }

    #[test]
    fn normalize_status_maps_default_workflow_aliases() {
        assert_eq!(normalize_status("To Do"), "todo");
        assert_eq!(normalize_status("In Progress"), "in_progress");
        assert_eq!(normalize_status("Closed"), "done");
        assert_eq!(normalize_status("Canceled"), "cancelled");
    }

    #[test]
    fn normalize_status_preserves_custom_values() {
        assert_eq!(normalize_status("Needs QA"), "Needs QA");
        assert_eq!(normalize_status("blocked_external"), "blocked_external");
    }
}
