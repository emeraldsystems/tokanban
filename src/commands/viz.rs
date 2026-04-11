use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors};

/// A visualization result — HTML content or a URL to open.
#[derive(Debug, Serialize, Deserialize)]
pub struct VizResponse {
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Subcommand)]
pub enum VizCommand {
    /// Display a Kanban board (opens HTML in browser or saves to file)
    Kanban {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Save to file instead of opening in browser
        #[arg(long)]
        output: Option<String>,
    },
    /// Display a burndown chart for a sprint
    Burndown {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Sprint ID
        #[arg(long)]
        sprint: String,
        /// Save to file instead of opening in browser
        #[arg(long)]
        output: Option<String>,
    },
    /// Display a timeline view for a project
    Timeline {
        /// Project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Save to file instead of opening in browser
        #[arg(long)]
        output: Option<String>,
    },
}

pub async fn handle(cmd: &VizCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        VizCommand::Kanban { project, output } => {
            handle_kanban(ctx, project.clone(), output.as_deref()).await
        }
        VizCommand::Burndown { project, sprint, output } => {
            handle_burndown(ctx, project.clone(), sprint, output.as_deref()).await
        }
        VizCommand::Timeline { project, output } => {
            handle_timeline(ctx, project.clone(), output.as_deref()).await
        }
    }
}

async fn handle_kanban(ctx: &Ctx, project: Option<String>, output: Option<&str>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let url = format!("/v1/visualizations/kanban?project_id={project_id}");
    deliver_viz(ctx, &url, "kanban", output).await
}

async fn handle_burndown(ctx: &Ctx, project: Option<String>, sprint: &str, output: Option<&str>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let url = format!("/v1/visualizations/burndown?project_id={project_id}&sprint_id={sprint}");
    deliver_viz(ctx, &url, "burndown", output).await
}

async fn handle_timeline(ctx: &Ctx, project: Option<String>, output: Option<&str>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let url = format!("/v1/visualizations/timeline?project_id={project_id}");
    deliver_viz(ctx, &url, "timeline", output).await
}

async fn deliver_viz(ctx: &Ctx, api_url: &str, viz_type: &str, output: Option<&str>) -> Result<()> {
    let resp: VizResponse = ctx.api.get(api_url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
        return Ok(());
    }

    // Prefer HTML content, then URL redirect.
    if let Some(html) = &resp.html {
        if let Some(path) = output {
            // Save to file.
            std::fs::write(path, html)?;
            let check = ctx.color.paint("✓", colors::SUCCESS);
            format::print_inline(&format!("{check} {viz_type} saved to {path}"));
        } else {
            // Write to temp file and open in browser.
            let tmp = write_temp_html(html, viz_type)?;
            open::that(&tmp).map_err(|e| {
                crate::error::CliError::Config(format!("Could not open browser: {e}"))
            })?;
            let check = ctx.color.paint("✓", colors::SUCCESS);
            format::print_inline(&format!("{check} {viz_type} opened in browser"));
        }
    } else if let Some(url) = &resp.url {
        open::that(url).map_err(|e| {
            crate::error::CliError::Config(format!("Could not open browser: {e}"))
        })?;
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} {viz_type} opened in browser"));
    } else {
        eprintln!("No visualization content returned.");
    }
    Ok(())
}

fn write_temp_html(html: &str, prefix: &str) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("tokanban-{prefix}.html"));
    std::fs::write(&path, html)?;
    Ok(path)
}
