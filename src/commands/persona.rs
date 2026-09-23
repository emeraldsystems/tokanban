use clap::Subcommand;
use serde_json::json;

use crate::api::{
    AiTeammateItem, AiTeammateListResponse, PaginatedResponse, PersonaContextCoverage,
    PersonaContextResponse, PersonaListResponse, ProjectDetailResponse, ProjectEntityItem,
    TaskItem,
};
use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format::table::{render_table, Column};
use crate::format::{self, colors, EM_DASH};

pub const PERSONA_KEYS: [&str; 5] = ["pm", "architect", "engineer", "reviewer", "researcher"];

#[derive(Debug, Subcommand)]
pub enum PersonaCommand {
    /// List built-in personas and project activation state
    List {
        #[arg(long)]
        project: Option<String>,
    },
    /// Enable or disable project personas (activation is shared)
    Configure {
        #[arg(long)]
        project: Option<String>,
        /// Persona to enable; repeat for multiple roles
        #[arg(long = "enable")]
        enable: Vec<String>,
        /// Persona to disable; repeat for multiple roles
        #[arg(long = "disable")]
        disable: Vec<String>,
    },
    /// List the project's persistent assignable AI teammates
    Teammates {
        #[arg(long)]
        project: Option<String>,
    },
    /// List tasks queued for a persona's default AI teammate
    Assignments {
        persona: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Load live project context for an active persona
    Context {
        persona: String,
        #[arg(long)]
        project: Option<String>,
        /// Active Claude/coding session identifier for provenance guidance
        #[arg(long)]
        session: Option<String>,
        /// Bound context size for lifecycle hook injection
        #[arg(long)]
        compact: bool,
    },
}

pub async fn handle(cmd: &PersonaCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        PersonaCommand::List { project } => handle_list(ctx, project.clone()).await,
        PersonaCommand::Configure {
            project,
            enable,
            disable,
        } => handle_configure(ctx, project.clone(), enable, disable).await,
        PersonaCommand::Teammates { project } => handle_teammates(ctx, project.clone()).await,
        PersonaCommand::Assignments {
            persona,
            project,
            limit,
        } => handle_assignments(ctx, persona, project.clone(), *limit).await,
        PersonaCommand::Context {
            persona,
            project,
            session,
            compact,
        } => handle_context(ctx, persona, project.clone(), session.clone(), *compact).await,
    }
}

async fn handle_list(ctx: &Ctx, project: Option<String>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let resp: PersonaListResponse = ctx
        .api
        .get(&format!("/v1/projects/{project_id}/personas"))
        .await?;
    print_personas(ctx, &resp);
    Ok(())
}

async fn handle_configure(
    ctx: &Ctx,
    project: Option<String>,
    enable: &[String],
    disable: &[String],
) -> Result<()> {
    if enable.is_empty() && disable.is_empty() {
        return Err(CliError::InvalidInput(
            "Provide at least one --enable or --disable persona.".to_string(),
        ));
    }
    let enable = normalize_persona_keys(enable)?;
    let disable = normalize_persona_keys(disable)?;
    if let Some(key) = enable.iter().find(|key| disable.contains(key)) {
        return Err(CliError::InvalidInput(format!(
            "Persona '{key}' cannot be enabled and disabled in the same request."
        )));
    }

    let project_id = ctx.project_id(project).await?;
    let path = format!("/v1/projects/{project_id}/personas");
    let current: PersonaListResponse = ctx.api.get(&path).await?;
    let mut enabled: Vec<String> = current
        .personas
        .iter()
        .filter(|item| item.enabled)
        .map(|item| item.key.clone())
        .collect();
    for key in enable {
        if !enabled.contains(&key) {
            enabled.push(key);
        }
    }
    enabled.retain(|key| !disable.contains(key));
    enabled.sort_by_key(|key| {
        PERSONA_KEYS
            .iter()
            .position(|candidate| candidate == key)
            .unwrap_or(usize::MAX)
    });

    let resp: PersonaListResponse = ctx.api.put(&path, &json!({ "enabled": enabled })).await?;
    print_personas(ctx, &resp);
    Ok(())
}

async fn handle_teammates(ctx: &Ctx, project: Option<String>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let resp: AiTeammateListResponse = ctx
        .api
        .get(&format!("/v1/projects/{project_id}/teammates"))
        .await?;
    print_teammates(ctx, &resp.items);
    Ok(())
}

async fn handle_assignments(
    ctx: &Ctx,
    persona: &str,
    project: Option<String>,
    limit: u32,
) -> Result<()> {
    let persona = normalize_persona_key(persona)?;
    let project_id = ctx.project_id(project).await?;
    let teammates: AiTeammateListResponse = ctx
        .api
        .get(&format!("/v1/projects/{project_id}/teammates"))
        .await?;
    let teammate = teammate_for(&teammates.items, &persona).ok_or_else(|| {
        CliError::InvalidInput(format!(
            "No default AI teammate exists for persona '{persona}' in this project."
        ))
    })?;
    let tasks: PaginatedResponse<TaskItem> = ctx
        .api
        .get(&format!(
            "/v1/projects/{project_id}/tasks?assignee_id={}&limit={}",
            enc(&teammate.id),
            limit.clamp(1, 100)
        ))
        .await?;
    print_tasks(ctx, &tasks.items, Some(&teammate.name));
    Ok(())
}

async fn handle_context(
    ctx: &Ctx,
    persona: &str,
    project: Option<String>,
    session_id: Option<String>,
    compact: bool,
) -> Result<()> {
    let persona_key = normalize_persona_key(persona)?;
    let resolved = ctx.project(project).await?;
    let project_path = format!("/v1/projects/{}", resolved.id);
    let persona_path = format!("/v1/projects/{}/personas", resolved.id);
    let teammate_path = format!("/v1/projects/{}/teammates", resolved.id);

    let (project, personas, teammates): (
        ProjectDetailResponse,
        PersonaListResponse,
        AiTeammateListResponse,
    ) = tokio::try_join!(
        ctx.api.get(&project_path),
        ctx.api.get(&persona_path),
        ctx.api.get(&teammate_path)
    )?;

    let persona = personas
        .personas
        .iter()
        .find(|item| item.key == persona_key)
        .cloned()
        .ok_or_else(|| {
            CliError::InvalidInput(format!(
                "Backend did not return the built-in persona '{persona_key}'."
            ))
        })?;
    let teammate = teammate_for(&teammates.items, &persona_key).cloned();

    if !persona.enabled {
        if ctx.format.is_json() {
            format::print_json(&json!({
                "active": false,
                "persona": persona,
                "project": project,
                "teammate": teammate,
                "session_id": session_id,
                "reason": "persona_disabled"
            }));
        } else {
            println!(
                "Persona '{}' is disabled for {}. Enable it with `tokanban persona configure --enable {}`.",
                persona_key, resolved.key, persona_key
            );
        }
        return Ok(());
    }

    let active_teammate = teammate.as_ref().ok_or_else(|| {
        CliError::InvalidInput(format!(
            "Persona '{persona_key}' is enabled but its default AI teammate is missing. Retry persona activation or ask a project admin to repair it."
        ))
    })?;
    if !active_teammate.enabled
        || persona.teammate_id.as_deref() != Some(active_teammate.id.as_str())
    {
        return Err(CliError::InvalidInput(format!(
            "Persona '{persona_key}' activation and teammate identity are inconsistent. Retry persona activation before starting work."
        )));
    }

    // ProjectDO pagination is not guaranteed to put active work on the first
    // page. Scan bounded pages and disclose coverage rather than presenting a
    // first-page sample as the full board. Assignments use their own filtered
    // query so an AI teammate's queue cannot disappear behind other tasks.
    let tasks_base = format!("/v1/projects/{}/tasks?limit=100", resolved.id);
    let entities_base = format!("/v1/projects/{}/entities?limit=100", resolved.id);
    let assignments_base = teammate.as_ref().map(|item| {
        format!(
            "/v1/projects/{}/tasks?limit=100&assignee_id={}",
            resolved.id,
            enc(&item.id)
        )
    });
    let (tasks_load, entities_load, assignments_load) = tokio::try_join!(
        fetch_task_pages(ctx, &tasks_base, 10),
        fetch_entity_pages(ctx, &entities_base, 10),
        fetch_optional_task_pages(ctx, assignments_base.as_deref(), 10)
    )?;

    let task_scan_count = tasks_load.items.len();
    let task_total = tasks_load.total;
    let task_next_cursor = tasks_load.next_cursor;
    let mut all_tasks = tasks_load.items;
    all_tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    if compact {
        let mut selected = all_tasks
            .iter()
            .filter(|task| !is_terminal_status(&task.status))
            .take(75)
            .cloned()
            .collect::<Vec<_>>();
        for task in all_tasks
            .iter()
            .filter(|task| is_terminal_status(&task.status))
            .take(10)
        {
            if !selected.iter().any(|selected| selected.id == task.id) {
                selected.push(task.clone());
            }
        }
        all_tasks = selected;
    }
    let task_coverage = coverage(
        task_scan_count,
        all_tasks.len(),
        task_total,
        task_next_cursor,
    );

    let assignment_scan_count = assignments_load.items.len();
    let assignment_total = assignments_load.total;
    let assignment_next_cursor = assignments_load.next_cursor;
    let mut assigned_tasks = assignments_load.items;
    assigned_tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    if compact {
        assigned_tasks.truncate(100);
    }
    let assignment_coverage = coverage(
        assignment_scan_count,
        assigned_tasks.len(),
        assignment_total,
        assignment_next_cursor,
    );

    let entity_scan_count = entities_load.items.len();
    let entity_total = entities_load.total;
    let entity_next_cursor = entities_load.next_cursor;
    let mut project_entities = entities_load.items;
    project_entities.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    if compact {
        project_entities.truncate(100);
    }
    let entity_coverage = coverage(
        entity_scan_count,
        project_entities.len(),
        entity_total,
        entity_next_cursor,
    );
    let enabled_specialists = personas
        .personas
        .iter()
        .filter(|item| item.enabled && item.key != "pm")
        .map(|item| item.key.clone())
        .collect();

    let response = PersonaContextResponse {
        active: true,
        persona,
        project,
        teammate,
        assigned_tasks,
        tasks: all_tasks,
        entities: project_entities,
        enabled_specialists,
        session_id,
        task_coverage,
        assignment_coverage,
        entity_coverage,
    };
    if ctx.format.is_json() {
        format::print_json(&response);
    } else {
        print_context(ctx, &response);
    }
    Ok(())
}

fn print_personas(ctx: &Ctx, resp: &PersonaListResponse) {
    if ctx.format.is_json() {
        format::print_json(resp);
        return;
    }
    let columns = [
        Column::new("Persona", 14),
        Column::new("Enabled", 7),
        Column::new("AI teammate", 20).flexible(),
        Column::new("Description", 32).flexible(),
    ];
    let rows = resp
        .personas
        .iter()
        .map(|item| {
            vec![
                Some(item.name.clone()),
                Some(if item.enabled { "yes" } else { "no" }.to_string()),
                Some(
                    item.teammate_id
                        .clone()
                        .unwrap_or_else(|| EM_DASH.to_string()),
                ),
                Some(item.description.clone()),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn print_teammates(ctx: &Ctx, items: &[AiTeammateItem]) {
    if ctx.format.is_json() {
        format::print_json(&AiTeammateListResponse {
            items: items.to_vec(),
        });
        return;
    }
    if items.is_empty() {
        println!("No AI teammates.");
        return;
    }
    let columns = [
        Column::new("ID", 18),
        Column::new("Name", 24).flexible(),
        Column::new("Persona", 12),
        Column::new("Enabled", 7),
    ];
    let rows = items
        .iter()
        .map(|item| {
            vec![
                Some(ctx.color.paint(&item.id, colors::MUTED)),
                Some(item.name.clone()),
                Some(item.persona_key.clone()),
                Some(if item.enabled { "yes" } else { "no" }.to_string()),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn print_tasks(ctx: &Ctx, tasks: &[TaskItem], teammate_name: Option<&str>) {
    if ctx.format.is_json() {
        format::print_json(&json!({
            "assignee": teammate_name,
            "items": tasks,
            "total": tasks.len()
        }));
        return;
    }
    if tasks.is_empty() {
        println!("No queued assignments.");
        return;
    }
    let columns = [
        Column::new("Key", 10),
        Column::new("Status", 12),
        Column::new("Priority", 9),
        Column::new("Title", 32).flexible(),
        Column::new("Claim", 10),
    ];
    let rows = tasks
        .iter()
        .map(|task| {
            vec![
                Some(ctx.color.paint(&task.key, colors::MUTED)),
                Some(task.status.clone()),
                Some(task.priority.clone().unwrap_or_else(|| "none".to_string())),
                Some(task.title.clone()),
                Some(
                    task.ownership
                        .as_ref()
                        .map(|owner| owner.actor_name.clone())
                        .unwrap_or_else(|| EM_DASH.to_string()),
                ),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn print_context(ctx: &Ctx, response: &PersonaContextResponse) {
    println!(
        "{} active for {} ({}). {} tasks, {} durable records.",
        response.persona.name,
        response.project.name,
        response.project.key,
        response.tasks.len(),
        response.entities.len()
    );
    if !response.enabled_specialists.is_empty() {
        println!(
            "Enabled specialists: {}",
            response.enabled_specialists.join(", ")
        );
    }
    for (label, coverage) in [
        ("tasks", &response.task_coverage),
        ("assignments", &response.assignment_coverage),
        ("entities", &response.entity_coverage),
    ] {
        if !coverage.scan_complete || coverage.selection_truncated {
            println!(
                "Context coverage ({label}): returned {} of {} scanned (reported total {}); scan_complete={}",
                coverage.returned, coverage.scanned, coverage.total, coverage.scan_complete
            );
        }
    }
    print_tasks(
        ctx,
        &response.assigned_tasks,
        response.teammate.as_ref().map(|item| item.name.as_str()),
    );
}

pub fn normalize_persona_keys(keys: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    for key in keys {
        let key = normalize_persona_key(key)?;
        if !normalized.contains(&key) {
            normalized.push(key);
        }
    }
    Ok(normalized)
}

fn normalize_persona_key(key: &str) -> Result<String> {
    let normalized = key.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let normalized = match normalized.as_str() {
        "project_manager" | "manager" => "pm",
        value => value,
    };
    if PERSONA_KEYS.contains(&normalized) {
        Ok(normalized.to_string())
    } else {
        Err(CliError::InvalidInput(format!(
            "Unknown persona '{key}'. Use pm, architect, engineer, reviewer, or researcher."
        )))
    }
}

fn teammate_for<'a>(items: &'a [AiTeammateItem], persona: &str) -> Option<&'a AiTeammateItem> {
    items.iter().find(|item| item.persona_key == persona)
}

struct PageLoad<T> {
    items: Vec<T>,
    total: u64,
    next_cursor: Option<String>,
}

async fn fetch_task_pages(ctx: &Ctx, base: &str, max_pages: usize) -> Result<PageLoad<TaskItem>> {
    let mut items = Vec::new();
    let mut total = 0;
    let mut cursor: Option<String> = None;
    for _ in 0..max_pages {
        let url = page_url(base, cursor.as_deref());
        let page: PaginatedResponse<TaskItem> = ctx.api.get(&url).await?;
        total = total.max(page.total);
        items.extend(page.items);
        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(PageLoad {
        items,
        total,
        next_cursor: cursor,
    })
}

async fn fetch_optional_task_pages(
    ctx: &Ctx,
    base: Option<&str>,
    max_pages: usize,
) -> Result<PageLoad<TaskItem>> {
    match base {
        Some(path) => fetch_task_pages(ctx, path, max_pages).await,
        None => Ok(PageLoad {
            items: Vec::new(),
            total: 0,
            next_cursor: None,
        }),
    }
}

async fn fetch_entity_pages(
    ctx: &Ctx,
    base: &str,
    max_pages: usize,
) -> Result<PageLoad<ProjectEntityItem>> {
    let mut items = Vec::new();
    let mut total = 0;
    let mut cursor: Option<String> = None;
    for _ in 0..max_pages {
        let url = page_url(base, cursor.as_deref());
        let page: PaginatedResponse<ProjectEntityItem> = ctx.api.get(&url).await?;
        total = total.max(page.total);
        items.extend(page.items);
        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(PageLoad {
        items,
        total,
        next_cursor: cursor,
    })
}

fn page_url(base: &str, cursor: Option<&str>) -> String {
    match cursor {
        Some(cursor) => format!("{base}&cursor={}", enc(cursor)),
        None => base.to_string(),
    }
}

fn coverage(
    scanned: usize,
    returned: usize,
    total: u64,
    next_cursor: Option<String>,
) -> PersonaContextCoverage {
    PersonaContextCoverage {
        scanned,
        returned,
        total,
        scan_complete: next_cursor.is_none() && scanned as u64 >= total,
        selection_truncated: returned < scanned,
        next_cursor,
    }
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str(),
        "done" | "closed" | "resolved" | "cancelled" | "canceled" | "archived"
    )
}

fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::{normalize_persona_key, normalize_persona_keys};

    #[test]
    fn normalizes_persona_alias_and_deduplicates() {
        assert_eq!(normalize_persona_key("Project Manager").unwrap(), "pm");
        assert_eq!(
            normalize_persona_keys(&[
                "Engineer".to_string(),
                "engineer".to_string(),
                "reviewer".to_string(),
            ])
            .unwrap(),
            vec!["engineer", "reviewer"]
        );
    }

    #[test]
    fn rejects_custom_personas_in_v1() {
        assert!(normalize_persona_key("custom").is_err());
    }
}
