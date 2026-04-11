use clap::Subcommand;
use serde_json::json;

use crate::api::{MutationResponse, PaginatedResponse, ProjectDetailResponse, ProjectItem};
use crate::config::save_config;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors, EM_DASH};
use crate::format::card::{CardField, CardSection, render_card};
use crate::format::table::{render_table, Column};

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Create a new project
    Create {
        /// Project name
        name: String,
        /// Key prefix for task IDs (e.g., PLAT)
        #[arg(long)]
        key_prefix: String,
    },
    /// List projects in workspace
    List {
        /// Optional workspace override for compatibility with older configs
        #[arg(long)]
        workspace: Option<String>,
    },
    /// View project details
    View {
        /// Project key, name, or ID
        key: String,
    },
    /// Update project settings
    Update {
        /// Project key, name, or ID
        key: String,
        /// New project name
        #[arg(long)]
        name: Option<String>,
        /// New key prefix
        #[arg(long)]
        key_prefix: Option<String>,
    },
    /// Archive a project
    Archive {
        /// Project key, name, or ID
        key: String,
    },
    /// Set default project in config
    Set {
        /// Project key, name, or ID
        key: String,
    },
}

pub async fn handle(cmd: &ProjectCommand, ctx: &mut Ctx) -> Result<()> {
    match cmd {
        ProjectCommand::Create { name, key_prefix } => {
            handle_create(ctx, name, key_prefix).await
        }
        ProjectCommand::List { workspace } => {
            handle_list(ctx, workspace.clone()).await
        }
        ProjectCommand::View { key } => handle_view(ctx, key).await,
        ProjectCommand::Update { key, name, key_prefix } => {
            handle_update(ctx, key, name.as_deref(), key_prefix.as_deref()).await
        }
        ProjectCommand::Archive { key } => handle_archive(ctx, key).await,
        ProjectCommand::Set { key } => handle_set(ctx, key).await,
    }
}

async fn handle_create(ctx: &Ctx, name: &str, key_prefix: &str) -> Result<()> {
    let body = json!({
        "name": name,
        "key_prefix": key_prefix,
    });

    let resp: ProjectDetailResponse = ctx.api.post("/v1/projects", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let key = project_key_or_prefix_detail(&resp);
        let msg = crate::format::inline::mutation_created("project", key, Some(&resp.name), &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, _workspace: Option<String>) -> Result<()> {
    let resp: PaginatedResponse<ProjectItem> = ctx.api.get("/v1/projects").await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No projects.");
            return Ok(());
        }
        let columns = [
            Column::new("Key", 6),
            Column::new("Name", 20).flexible(),
            Column::new("Prefix", 6),
            Column::new("Status", 10),
            Column::new("Tasks", 5).right(),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|p| vec![
                Some(ctx.color.paint(project_key_or_prefix_item(p), colors::MUTED)),
                Some(p.name.clone()),
                Some(p.key_prefix.clone()),
                Some(p.status.clone().unwrap_or_else(|| "Active".to_string())),
                Some(p.task_count.map(|c| c.to_string()).unwrap_or_else(|| EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_view(ctx: &Ctx, key: &str) -> Result<()> {
    let project = ctx.resolve_project(key).await?;
    let resp: ProjectDetailResponse = ctx.api.get(&format!("/v1/projects/{}", project.id)).await?;
    let display_key = project_key_or_prefix_detail(&resp);

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let fields = vec![
            CardField::required("Key", display_key.to_string()),
            CardField::required("Prefix", resp.key_prefix.clone()),
            CardField::new("Status", Some(resp.status.clone().unwrap_or_else(|| "Active".to_string()))),
            CardField::new("Tasks", resp.task_count.map(|c| c.to_string())),
            CardField::new("Members", resp.member_count.map(|c| c.to_string())),
            CardField::new("Created", resp.created_at.clone()),
            CardField::new("Updated", resp.updated_at.clone()),
        ];
        let mut sections = vec![CardSection::Fields(fields)];
        if let Some(desc) = &resp.description {
            if !desc.is_empty() {
                sections.push(CardSection::Prose {
                    heading: "Description".to_string(),
                    body: desc.clone(),
                });
            }
        }
        print!("{}", render_card(display_key, &resp.name, &sections, &ctx.color));
    }
    Ok(())
}

async fn handle_update(ctx: &Ctx, key: &str, name: Option<&str>, key_prefix: Option<&str>) -> Result<()> {
    let project = ctx.resolve_project(key).await?;
    let mut body = json!({});
    if let Some(n) = name { body["name"] = json!(n); }
    if let Some(k) = key_prefix { body["key_prefix"] = json!(k); }

    let resp: MutationResponse = ctx.api.patch(&format!("/v1/projects/{}", project.id), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = crate::format::inline::mutation_updated("project", &project.key, &[], &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_archive(ctx: &Ctx, key: &str) -> Result<()> {
    let project = ctx.resolve_project(key).await?;
    let body = json!({ "status": "archived" });
    let resp: MutationResponse = ctx.api.patch(&format!("/v1/projects/{}", project.id), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = crate::format::inline::mutation_archived("project", &project.key, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_set(ctx: &mut Ctx, key: &str) -> Result<()> {
    let project = ctx.resolve_project(key).await?;
    ctx.config.defaults.project = Some(project.id.clone());
    save_config(&ctx.config, ctx.config_path.as_ref())?;
    if !ctx.quiet {
        println!("Default project set to {} ({})", project.name, project.key);
    }
    Ok(())
}

fn project_key_or_prefix_item(project: &ProjectItem) -> &str {
    if project.key.is_empty() {
        &project.key_prefix
    } else {
        &project.key
    }
}

fn project_key_or_prefix_detail(project: &ProjectDetailResponse) -> &str {
    if project.key.is_empty() {
        &project.key_prefix
    } else {
        &project.key
    }
}
