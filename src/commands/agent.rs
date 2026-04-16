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
pub struct AgentItem {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "type")]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentCreateResponse {
    pub id: String,
    pub name: String,
    /// The API key — only shown once at creation time.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentRotateResponse {
    pub id: String,
    pub api_key: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Create a new agent token
    Create {
        /// Agent name
        name: String,
        /// Agent type tag (e.g., "summarizer", "reviewer")
        #[arg(long, rename_all = "verbatim")]
        r#type: String,
        /// Comma-separated scopes (e.g., "tasks:read,tasks:write" or "tasks:read,tasks:write,projects:read,memory:read,memory:write")
        #[arg(long)]
        scopes: String,
        /// Override default workspace
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List agents in workspace
    List {
        /// Override default workspace
        #[arg(long)]
        workspace: Option<String>,
    },
    /// View agent details
    View {
        /// Agent ID
        id: String,
    },
    /// Rotate agent key (revoke current, generate new with 1h grace)
    Rotate {
        /// Agent ID
        id: String,
    },
    /// Revoke an agent permanently
    Revoke {
        /// Agent ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        no_confirm: bool,
    },
    /// List agent scopes
    Scopes {
        /// Agent ID
        id: String,
    },
}

pub async fn handle(cmd: &AgentCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        AgentCommand::Create { name, r#type, scopes, workspace } => {
            handle_create(ctx, name, r#type, scopes, workspace.clone()).await
        }
        AgentCommand::List { workspace } => handle_list(ctx, workspace.clone()).await,
        AgentCommand::View { id } => handle_view(ctx, id).await,
        AgentCommand::Rotate { id } => handle_rotate(ctx, id).await,
        AgentCommand::Revoke { id, no_confirm: _ } => handle_revoke(ctx, id).await,
        AgentCommand::Scopes { id } => handle_scopes(ctx, id).await,
    }
}

async fn handle_create(
    ctx: &Ctx,
    name: &str,
    agent_type: &str,
    scopes: &str,
    workspace: Option<String>,
) -> Result<()> {
    let ws = ctx.workspace_slug(workspace)?;
    let scope_list: Vec<&str> = scopes.split(',').map(|s| s.trim()).collect();
    let body = json!({
        "name": name,
        "type_tag": agent_type,
        "scopes": scope_list,
        "workspace_id": ws,
    });
    let resp: AgentCreateResponse = ctx.api.post("/v1/agent-keys", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!("{check} Agent {name} created (id: {})", resp.id));
        if let Some(key) = &resp.api_key {
            // Show the key once — it won't be shown again.
            println!();
            println!("API key (save this now — it will not be shown again):");
            println!("  {key}");
        }
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, workspace: Option<String>) -> Result<()> {
    let ws = ctx.workspace_slug(workspace)?;
    let url = format!("/v1/agent-keys?workspace_id={ws}");
    let resp: PaginatedResponse<AgentItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        if resp.items.is_empty() {
            println!("No agents.");
            return Ok(());
        }
        let columns = [
            Column::new("ID", 20),
            Column::new("Name", 20).flexible(),
            Column::new("Type", 14),
            Column::new("Created", 10),
            Column::new("Last used", 10),
        ];
        let rows: Vec<Vec<Option<String>>> = resp.items
            .iter()
            .map(|a| vec![
                Some(ctx.color.paint(&a.id, colors::MUTED)),
                Some(a.name.clone()),
                Some(a.agent_type.clone().unwrap_or_else(|| EM_DASH.to_string())),
                Some(a.created_at.clone().unwrap_or_else(|| EM_DASH.to_string())),
                Some(a.last_used_at.clone().unwrap_or_else(|| EM_DASH.to_string())),
            ])
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

async fn handle_view(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: AgentItem = ctx.api.get(&format!("/v1/agent-keys/{id}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let fields = vec![
            CardField::required("ID", resp.id.clone()),
            CardField::new("Type", resp.agent_type.clone()),
            CardField::new("Created", resp.created_at.clone()),
            CardField::new("Last used", resp.last_used_at.clone()),
        ];
        let mut sections = vec![CardSection::Fields(fields)];
        if let Some(scopes) = &resp.scopes {
            let items: Vec<String> = scopes.iter().map(|s| s.clone()).collect();
            sections.push(CardSection::List { heading: "Scopes".to_string(), items });
        }
        print!("{}", render_card(id, &resp.name, &sections, &ctx.color));
    }
    Ok(())
}

async fn handle_rotate(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: AgentRotateResponse = ctx.api
        .post(&format!("/v1/agent-keys/{id}/rotate"), &json!({}))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let valid_until = resp.expires_at.as_deref();
        let msg = format::inline::mutation_rotated("agent", id, valid_until, &ctx.color);
        format::print_inline(&msg);
        println!();
        println!("New API key (save this now — it will not be shown again):");
        println!("  {}", resp.api_key);
    }
    Ok(())
}

async fn handle_revoke(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: MutationResponse = ctx.api.delete(&format!("/v1/agent-keys/{id}")).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_revoked("agent", id, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_scopes(ctx: &Ctx, id: &str) -> Result<()> {
    let resp: AgentItem = ctx.api.get(&format!("/v1/agent-keys/{id}")).await?;

    if ctx.format.is_json() {
        if let Some(scopes) = &resp.scopes {
            format::print_json(scopes);
        }
    } else {
        let scopes = resp.scopes.unwrap_or_default();
        if scopes.is_empty() {
            println!("No scopes assigned.");
        } else {
            for scope in &scopes {
                println!("{scope}");
            }
        }
    }
    Ok(())
}
