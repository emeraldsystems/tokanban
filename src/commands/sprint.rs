use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::{MutationResponse, PaginatedResponse};
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors, EM_DASH};
use crate::format::card::{CardField, CardSection, render_card};
use crate::format::table::{render_table, Column};

#[derive(Debug, Serialize, Deserialize)]
pub struct SprintItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub task_count: Option<u64>,
    #[serde(default)]
    pub points_done: Option<u64>,
    #[serde(default)]
    pub points_total: Option<u64>,
}

#[derive(Debug, Subcommand)]
pub enum SprintCommand {
    /// Create a new sprint
    Create {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Sprint name
        #[arg(long)]
        name: String,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: String,
    },
    /// List sprints in a project
    List {
        /// Filter by project key
        #[arg(long)]
        project: Option<String>,
    },
    /// View sprint details
    View {
        /// Sprint ID
        id: String,
    },
    /// Update sprint settings
    Update {
        /// Sprint ID
        id: String,
        /// New name
        #[arg(long)]
        name: Option<String>,
        /// New start date
        #[arg(long)]
        start: Option<String>,
        /// New end date
        #[arg(long)]
        end: Option<String>,
    },
    /// Activate a sprint
    Activate {
        /// Sprint ID
        id: String,
    },
    /// Close a sprint
    Close {
        /// Sprint ID
        id: String,
    },
}

pub async fn handle(cmd: &SprintCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        SprintCommand::Create { project, name, start, end } => {
            handle_create(ctx, project.clone(), name, start, end).await
        }
        SprintCommand::List { project } => handle_list(ctx, project.clone()).await,
        SprintCommand::View { id } => handle_view(ctx, id).await,
        SprintCommand::Update { id, name, start, end } => {
            handle_update(ctx, id, name.as_deref(), start.as_deref(), end.as_deref()).await
        }
        SprintCommand::Activate { id } => handle_state(ctx, id, "active").await,
        SprintCommand::Close { id } => handle_state(ctx, id, "closed").await,
    }
}

async fn handle_create(
    ctx: &Ctx,
    project: Option<String>,
    name: &str,
    start: &str,
    end: &str,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let body = json!({
        "project_id": project_id,
        "name": name,
        "start_date": start,
        "end_date": end,
    });
    let resp: MutationResponse = ctx.api.post("/v1/sprints", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let id = resp.key.as_deref().unwrap_or(&resp.id);
        let msg = format::inline::mutation_created("sprint", id, Some(name), &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, project: Option<String>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let url = format!("/v1/sprints?project_id={project_id}");
    let resp: PaginatedResponse<SprintItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No sprints.");
            return Ok(());
        }
        let columns = [
            Column::new("ID", 16),
            Column::new("Name", 18).flexible(),
            Column::new("State", 10),
            Column::new("Start", 10),
            Column::new("End", 10),
            Column::new("Tasks", 5).right(),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|s| vec![
                Some(ctx.color.paint(&s.id, colors::MUTED)),
                Some(s.name.clone()),
                Some(s.state.clone().unwrap_or_else(|| "Planned".to_string())),
                Some(s.start_date.clone().unwrap_or_else(|| EM_DASH.to_string())),
                Some(s.end_date.clone().unwrap_or_else(|| EM_DASH.to_string())),
                Some(s.task_count.map(|c| c.to_string()).unwrap_or_else(|| EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_view(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: SprintItem = ctx.api.get(&format!("/v1/sprints/{id}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let progress = match (resp.points_done, resp.points_total) {
            (Some(done), Some(total)) if total > 0 => Some(format!("{done}/{total} pts")),
            _ => None,
        };
        let fields = vec![
            CardField::required("State", resp.state.clone().unwrap_or_else(|| "Planned".to_string())),
            CardField::new("Start", resp.start_date.clone()),
            CardField::new("End", resp.end_date.clone()),
            CardField::new("Tasks", resp.task_count.map(|c| c.to_string())),
            CardField::new("Progress", progress),
        ];
        print!("{}", render_card(id, &resp.name, &[CardSection::Fields(fields)], &ctx.color));
    }
    Ok(())
}

async fn handle_update(
    ctx: &Ctx,
    id: &str,
    name: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<()> {
    let mut body = json!({});
    if let Some(n) = name { body["name"] = json!(n); }
    if let Some(s) = start { body["start_date"] = json!(s); }
    if let Some(e) = end { body["end_date"] = json!(e); }

    let resp: MutationResponse = ctx.api.patch(&format!("/v1/sprints/{id}"), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_updated("sprint", id, &[], &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_state(ctx: &Ctx, id: &str, state: &str) -> Result<()> {
    let body = json!({ "state": state });
    let _resp: MutationResponse = ctx.api.patch(&format!("/v1/sprints/{id}"), &body).await?;

    if !ctx.quiet {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        let action = if state == "active" { "activated" } else { "closed" };
        format::print_inline(&format!("{check} Sprint {id} {action}"));
    }
    Ok(())
}
