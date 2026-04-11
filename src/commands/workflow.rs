use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::MutationResponse;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors};
use crate::format::table::{render_table, Column};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowResponse {
    pub project_id: String,
    pub statuses: Vec<WorkflowStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowStatus {
    pub name: String,
    #[serde(default)]
    pub is_terminal: Option<bool>,
    #[serde(default)]
    pub allowed_transitions: Option<Vec<String>>,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    /// Show the current workflow for a project
    Show {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
    },
    /// Update workflow configuration
    Update {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Add a new status to the workflow
        #[arg(long)]
        add_status: Option<String>,
        /// Remove a status (must not have open tasks)
        #[arg(long)]
        remove_status: Option<String>,
        /// Migrate tasks from one status to another (FROM:TO)
        #[arg(long)]
        migrate: Option<String>,
    },
}

pub async fn handle(cmd: &WorkflowCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        WorkflowCommand::Show { project } => handle_show(ctx, project.clone()).await,
        WorkflowCommand::Update { project, add_status, remove_status, migrate } => {
            handle_update(ctx, project.clone(), add_status.as_deref(), remove_status.as_deref(), migrate.as_deref()).await
        }
    }
}

async fn handle_show(ctx: &Ctx, project: Option<String>) -> Result<()> {
    let project = ctx.project(project).await?;
    let url = format!("/v1/projects/{}/workflow", project.id);
    let resp: WorkflowResponse = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let columns = [
            Column::new("Status", 20).flexible(),
            Column::new("Terminal", 8),
            Column::new("Allowed transitions", 40),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.statuses
            .iter()
            .map(|s| vec![
                Some(ctx.color.paint(&s.name, colors::MUTED)),
                Some(s.is_terminal.map(|t| if t { "Yes" } else { "No" }.to_string()).unwrap_or_else(|| "No".to_string())),
                Some(s.allowed_transitions.as_ref().map(|v| v.join(", ")).unwrap_or_else(|| "any".to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_update(
    ctx: &Ctx,
    project: Option<String>,
    add_status: Option<&str>,
    remove_status: Option<&str>,
    migrate: Option<&str>,
) -> Result<()> {
    let project = ctx.project(project).await?;

    let mut changes = json!({});
    if let Some(s) = add_status { changes["add_status"] = json!(s); }
    if let Some(s) = remove_status { changes["remove_status"] = json!(s); }

    // Parse FROM:TO migration spec.
    let mut body = json!({ "changes": changes });
    if let Some(m) = migrate {
        let parts: Vec<&str> = m.splitn(2, ':').collect();
        if parts.len() == 2 {
            body["migration_map"] = json!({ parts[0]: parts[1] });
        }
    }

    let resp: MutationResponse = ctx.api
        .patch(&format!("/v1/projects/{}/workflow", project.id), &body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} Workflow updated for {}", project.key));
    }
    Ok(())
}
