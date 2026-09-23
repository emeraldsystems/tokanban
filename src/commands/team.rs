use clap::Subcommand;
use serde_json::json;

use crate::api::{TeamItem, TeamListResponse};
use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format::table::{render_table, Column};
use crate::format::{self, colors, EM_DASH};

#[derive(Debug, Subcommand)]
pub enum TeamCommand {
    /// List teams in the current workspace
    List,
    /// Create a team
    Create { name: String },
    /// View a team and its human/AI membership
    View { id: String },
    /// Rename a team
    Update {
        id: String,
        #[arg(long)]
        name: String,
    },
    /// Delete a team
    Delete { id: String },
    /// Add a human or AI teammate to a team
    AddMember {
        id: String,
        #[arg(long = "type")]
        member_type: String,
        #[arg(long)]
        member_id: String,
    },
    /// Remove a human or AI teammate from a team
    RemoveMember {
        id: String,
        #[arg(long = "type")]
        member_type: String,
        #[arg(long)]
        member_id: String,
    },
}

pub async fn handle(cmd: &TeamCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        TeamCommand::List => {
            let resp: TeamListResponse = ctx.api.get("/v1/teams").await?;
            print_teams(ctx, &resp.items);
        }
        TeamCommand::Create { name } => {
            let team: TeamItem = ctx.api.post("/v1/teams", &json!({ "name": name })).await?;
            print_team(ctx, &team);
        }
        TeamCommand::View { id } => {
            let team: TeamItem = ctx.api.get(&format!("/v1/teams/{}", enc(id))).await?;
            print_team(ctx, &team);
        }
        TeamCommand::Update { id, name } => {
            let team: TeamItem = ctx
                .api
                .patch(&format!("/v1/teams/{}", enc(id)), &json!({ "name": name }))
                .await?;
            print_team(ctx, &team);
        }
        TeamCommand::Delete { id } => {
            let _: () = ctx.api.delete(&format!("/v1/teams/{}", enc(id))).await?;
            if ctx.format.is_json() {
                format::print_json(&json!({ "deleted": true, "id": id }));
            } else {
                format::print_inline(&format!("✓ Deleted team {id}"));
            }
        }
        TeamCommand::AddMember {
            id,
            member_type,
            member_id,
        } => {
            let member_type = normalize_member_type(member_type)?;
            let team: TeamItem = ctx
                .api
                .post(
                    &format!("/v1/teams/{}/members", enc(id)),
                    &json!({ "member_type": member_type, "member_id": member_id }),
                )
                .await?;
            print_team(ctx, &team);
        }
        TeamCommand::RemoveMember {
            id,
            member_type,
            member_id,
        } => {
            let member_type = normalize_member_type(member_type)?;
            let _: () = ctx
                .api
                .delete(&format!(
                    "/v1/teams/{}/members/{}/{}",
                    enc(id),
                    member_type,
                    enc(member_id)
                ))
                .await?;
            if ctx.format.is_json() {
                format::print_json(&json!({
                    "removed": true,
                    "team_id": id,
                    "member_type": member_type,
                    "member_id": member_id
                }));
            } else {
                format::print_inline(&format!("✓ Removed {member_type} {member_id} from {id}"));
            }
        }
    }
    Ok(())
}

fn print_teams(ctx: &Ctx, teams: &[TeamItem]) {
    if ctx.format.is_json() {
        format::print_json(&TeamListResponse {
            items: teams.to_vec(),
        });
        return;
    }
    if teams.is_empty() {
        println!("No teams.");
        return;
    }
    let columns = [
        Column::new("ID", 18),
        Column::new("Name", 28).flexible(),
        Column::new("Members", 7).right(),
        Column::new("Updated", 12),
    ];
    let rows = teams
        .iter()
        .map(|team| {
            vec![
                Some(ctx.color.paint(&team.id, colors::MUTED)),
                Some(team.name.clone()),
                Some(team.members.len().to_string()),
                Some(
                    team.updated_at
                        .clone()
                        .unwrap_or_else(|| EM_DASH.to_string()),
                ),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn print_team(ctx: &Ctx, team: &TeamItem) {
    if ctx.format.is_json() {
        format::print_json(team);
        return;
    }
    println!("{} ({})", team.name, team.id);
    if team.members.is_empty() {
        println!("No members.");
        return;
    }
    let columns = [
        Column::new("Type", 8),
        Column::new("ID", 18),
        Column::new("Name", 28).flexible(),
    ];
    let rows = team
        .members
        .iter()
        .map(|member| {
            vec![
                Some(member.member_type.clone()),
                Some(ctx.color.paint(&member.member_id, colors::MUTED)),
                Some(member.name.clone()),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn normalize_member_type(value: &str) -> Result<&str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "human" => Ok("human"),
        "ai" => Ok("ai"),
        _ => Err(CliError::InvalidInput(
            "Member type must be 'human' or 'ai'.".to_string(),
        )),
    }
}

fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
