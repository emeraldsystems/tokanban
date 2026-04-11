use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::{MutationResponse, PaginatedResponse};
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors, EM_DASH};
use crate::format::table::{render_table, Column};

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentItem {
    pub id: String,
    pub author: String,
    pub body: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CommentCommand {
    /// Add a comment to a task (body from arg or stdin)
    Add {
        /// Task key (e.g., PLAT-42)
        task_key: String,
        /// Comment body (omit to read from stdin)
        body: Option<String>,
    },
    /// List comments on a task
    List {
        /// Task key
        task_key: String,
    },
    /// Delete a comment
    Delete {
        /// Comment ID
        id: String,
    },
    /// Edit a comment
    Edit {
        /// Comment ID
        id: String,
        /// New body
        body: String,
    },
}

pub async fn handle(cmd: &CommentCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        CommentCommand::Add { task_key, body } => {
            handle_add(ctx, task_key, body.as_deref()).await
        }
        CommentCommand::List { task_key } => handle_list(ctx, task_key).await,
        CommentCommand::Delete { id } => handle_delete(ctx, id).await,
        CommentCommand::Edit { id, body } => handle_edit(ctx, id, body).await,
    }
}

async fn handle_add(ctx: &Ctx, task_key: &str, body: Option<&str>) -> Result<()> {
    // Read from stdin if body not provided (§8.3).
    let body_text = if let Some(b) = body {
        b.to_string()
    } else {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf.trim().to_string()
    };

    let req_body = json!({ "body": body_text });
    let resp: MutationResponse = ctx.api
        .post(&format!("/v1/tasks/{task_key}/comments"), &req_body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} Comment added to {task_key}"));
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, task_key: &str) -> Result<()> {
    let url = format!("/v1/tasks/{task_key}/comments");
    let resp: PaginatedResponse<CommentItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No comments.");
            return Ok(());
        }
        let columns = [
            Column::new("ID", 20),
            Column::new("Author", 12),
            Column::new("Body", 40).flexible(),
            Column::new("Created", 10),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|c| vec![
                Some(ctx.color.paint(&c.id, colors::MUTED)),
                Some(format!("@{}", c.author)),
                Some(c.body.clone()),
                Some(c.created_at.clone().unwrap_or_else(|| EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_delete(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: MutationResponse = ctx.api.delete(&format!("/v1/comments/{id}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_deleted("comment", id, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_edit(ctx: &Ctx, id: &str, body: &str) -> Result<()> {
    let req_body = json!({ "body": body });
    let resp: MutationResponse = ctx.api.patch(&format!("/v1/comments/{id}"), &req_body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} Comment {id} updated"));
    }
    Ok(())
}
