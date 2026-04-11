use clap::Subcommand;
use serde_json::json;

use crate::api::{MutationResponse, PaginatedResponse};
use crate::config::save_config;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors};
use crate::format::card::{CardField, CardSection, render_card};
use crate::format::table::{render_table, Column};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceItem {
    pub id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub member_count: Option<u64>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Create a new workspace
    Create {
        /// Workspace name
        name: String,
    },
    /// List all accessible workspaces
    List,
    /// Set default workspace in config
    Set {
        /// Workspace slug
        slug: String,
    },
    /// Display current workspace details
    Current,
}

pub async fn handle(cmd: &WorkspaceCommand, ctx: &mut Ctx) -> Result<()> {
    match cmd {
        WorkspaceCommand::Create { name } => handle_create(ctx, name).await,
        WorkspaceCommand::List => handle_list(ctx).await,
        WorkspaceCommand::Set { slug } => handle_set(ctx, slug),
        WorkspaceCommand::Current => handle_current(ctx).await,
    }
}

async fn handle_create(ctx: &Ctx, name: &str) -> Result<()> {
    let body = json!({ "name": name });
    let resp: MutationResponse = ctx.api.post("/v1/workspaces", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let slug = resp.key.as_deref().unwrap_or(&resp.id);
        let msg = format::inline::mutation_created("workspace", slug, Some(name), &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx) -> Result<()> {
    let resp: PaginatedResponse<WorkspaceItem> = ctx.api.get("/v1/workspaces").await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No workspaces.");
            return Ok(());
        }
        let columns = [
            Column::new("Slug", 12),
            Column::new("Name", 24).flexible(),
            Column::new("Members", 7).right(),
            Column::new("Created", 10),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|w| vec![
                Some(ctx.color.paint(&w.slug, colors::MUTED)),
                Some(w.name.clone()),
                Some(w.member_count.map(|c| c.to_string()).unwrap_or_else(|| format::EM_DASH.to_string())),
                Some(w.created_at.clone().unwrap_or_else(|| format::EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

fn handle_set(ctx: &mut Ctx, slug: &str) -> Result<()> {
    ctx.config.defaults.workspace = Some(slug.to_string());
    save_config(&ctx.config, ctx.config_path.as_ref())?;
    if !ctx.quiet {
        println!("Default workspace set to {slug}");
    }
    Ok(())
}

async fn handle_current(ctx: &Ctx) -> Result<()> {
    let slug = ctx.workspace_slug(None)?;
    let resp: WorkspaceItem = ctx.api.get(&format!("/v1/workspaces/{slug}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let fields = vec![
            CardField::required("Slug", resp.slug.clone()),
            CardField::required("Name", resp.name.clone()),
            CardField::new("Members", resp.member_count.map(|c| c.to_string())),
            CardField::new("Created", resp.created_at.clone()),
        ];
        print!("{}", render_card(&resp.slug, &resp.name, &[CardSection::Fields(fields)], &ctx.color));
    }
    Ok(())
}
