use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors};

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportResponse {
    pub id: String,
    #[serde(default)]
    pub tasks_created: Option<u64>,
    #[serde(default)]
    pub tasks_skipped: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ImportCommand {
    /// Import tasks from a Jira JSON export file
    Jira {
        /// Path to Jira export file
        file: String,
        /// Target project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
    },
    /// Import tasks from a CSV file
    Csv {
        /// Path to CSV file
        file: String,
        /// Target project key (overrides config default)
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn handle(cmd: &ImportCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        ImportCommand::Jira { file, project } => {
            handle_import(ctx, file, "jira", project.clone()).await
        }
        ImportCommand::Csv { file, project } => {
            handle_import(ctx, file, "csv", project.clone()).await
        }
    }
}

async fn handle_import(ctx: &Ctx, file: &str, format_type: &str, project: Option<String>) -> Result<()> {
    let project_id = ctx.project_id(project).await?;

    // Read file bytes for multipart upload.
    let bytes = std::fs::read(file)?;
    let filename = std::path::Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);

    // Show progress indicator.
    if !ctx.quiet && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        eprint!("Uploading {filename}…");
    }

    // Build multipart form body via reqwest.
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| crate::error::CliError::Config(e.to_string()))?;

    let form = reqwest::multipart::Form::new()
        .text("project_id", project_id)
        .part("file", part);

    let resp: ImportResponse = ctx.api
        .post_multipart(&format!("/v1/import/{format_type}"), form)
        .await?;

    if !ctx.quiet {
        eprintln!(); // Clear progress line.
    }

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        let created = resp.tasks_created.unwrap_or(0);
        let skipped = resp.tasks_skipped.unwrap_or(0);
        format::print_inline(&format!(
            "{check} Import complete — {created} tasks created, {skipped} skipped"
        ));
    }
    Ok(())
}
