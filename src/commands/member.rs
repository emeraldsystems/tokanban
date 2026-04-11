use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::{MutationResponse, PaginatedResponse};
use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::{self, colors, EM_DASH};
use crate::format::table::{render_table, Column};

#[derive(Debug, Serialize, Deserialize)]
pub struct MemberItem {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: String,
    #[serde(default)]
    pub joined_at: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MemberCommand {
    /// Invite a member to the workspace
    Invite {
        /// Email address
        email: String,
        /// Role: admin, editor, viewer
        #[arg(long)]
        role: String,
        /// Override default workspace
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List workspace members
    List {
        /// Override default workspace
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Update a member's role
    Update {
        /// User ID
        user_id: String,
        /// New role: admin, editor, viewer
        #[arg(long)]
        role: String,
    },
    /// Revoke a member's access
    Revoke {
        /// User ID
        user_id: String,
        /// Override default workspace
        #[arg(long)]
        workspace: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        no_confirm: bool,
    },
}

pub async fn handle(cmd: &MemberCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        MemberCommand::Invite { email, role, workspace } => {
            handle_invite(ctx, email, role, workspace.clone()).await
        }
        MemberCommand::List { workspace } => handle_list(ctx, workspace.clone()).await,
        MemberCommand::Update { user_id, role } => handle_update(ctx, user_id, role).await,
        MemberCommand::Revoke { user_id, workspace, no_confirm: _ } => {
            handle_revoke(ctx, user_id, workspace.clone()).await
        }
    }
}

async fn handle_invite(ctx: &Ctx, email: &str, role: &str, workspace: Option<String>) -> Result<()> {
    let ws = ctx.workspace_slug(workspace)?;
    let body = json!({
        "email": email,
        "role": role,
        "workspace_id": ws,
    });
    let resp: MutationResponse = ctx.api.post("/v1/members/invite", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_invited(email, role, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, workspace: Option<String>) -> Result<()> {
    let ws = ctx.workspace_slug(workspace)?;
    let url = format!("/v1/members?workspace_id={ws}");
    let resp: PaginatedResponse<MemberItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No members.");
            return Ok(());
        }
        let columns = [
            Column::new("ID", 16),
            Column::new("Name", 20).flexible(),
            Column::new("Email", 28),
            Column::new("Role", 8),
            Column::new("Joined", 10),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|m| vec![
                Some(ctx.color.paint(&m.id, colors::MUTED)),
                Some(m.name.clone()),
                Some(m.email.clone()),
                Some(m.role.clone()),
                Some(m.joined_at.clone().unwrap_or_else(|| EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_update(ctx: &Ctx, user_id: &str, role: &str) -> Result<()> {
    let body = json!({ "role": role });
    let resp: MutationResponse = ctx.api.patch(&format!("/v1/members/{user_id}"), &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} Member {user_id} role updated to {role}"));
    }
    Ok(())
}

async fn handle_revoke(ctx: &Ctx, user_id: &str, workspace: Option<String>) -> Result<()> {
    let _ws = ctx.workspace_slug(workspace)?;
    let resp: MutationResponse = ctx.api.delete(&format!("/v1/members/{user_id}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_revoked("member", user_id, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}
