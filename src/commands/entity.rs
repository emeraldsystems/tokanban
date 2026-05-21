use clap::Subcommand;
use serde_json::{json, Value};

use crate::api::{MutationResponse, PaginatedResponse, ProjectEntityItem};
use crate::ctx::{Ctx, ResolvedProject};
use crate::error::{CliError, Result};
use crate::format::card::{render_card, CardField, CardSection};
use crate::format::table::{render_table, Column};
use crate::format::{self, color_status, colors, truncate, EM_DASH};

#[derive(Debug, Subcommand)]
pub enum EntityCommand {
    /// Create a decision, finding, or requirement
    Create {
        /// Kind: decision/DEC, finding/FND, or requirement/REQ
        kind: String,
        /// Short title
        title: String,
        /// Project key/name/ID (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Markdown body or detailed context
        #[arg(long)]
        content: Option<String>,
        /// Status (default: active)
        #[arg(long)]
        status: Option<String>,
        /// Tokanban memory ID/fact ID to cross-reference
        #[arg(long = "memory-ref", alias = "memory")]
        memory_refs: Vec<String>,
        /// Related task/entity key
        #[arg(long = "related", alias = "relates-to")]
        related_keys: Vec<String>,
        /// JSON object metadata
        #[arg(long)]
        metadata: Option<String>,
    },
    /// List project decisions, findings, and requirements
    List {
        /// Project key/name/ID (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Filter by kind: decision/DEC, finding/FND, requirement/REQ
        #[arg(long)]
        kind: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Search title/content
        #[arg(long, alias = "search")]
        query: Option<String>,
        /// Cursor for pagination
        #[arg(long)]
        cursor: Option<String>,
        /// Page size (default 25, max 100)
        #[arg(long, default_value = "25")]
        limit: u32,
    },
    /// View one project entity by key or UUID
    View {
        /// Entity key (e.g., TKB-DEC-1) or UUID
        key: String,
        /// Project key/name/ID (only required for UUIDs or ambiguous keys)
        #[arg(long)]
        project: Option<String>,
    },
    /// Update a project entity
    Update {
        /// Entity key (e.g., TKB-FND-1) or UUID
        key: String,
        /// Project key/name/ID (only required for UUIDs or ambiguous keys)
        #[arg(long)]
        project: Option<String>,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New Markdown body/content
        #[arg(long)]
        content: Option<String>,
        /// New status
        #[arg(long)]
        status: Option<String>,
        /// Replace memory refs with these values
        #[arg(long = "memory-ref", alias = "memory")]
        memory_refs: Vec<String>,
        /// Clear all memory refs
        #[arg(long)]
        clear_memory_refs: bool,
        /// Replace related keys with these values
        #[arg(long = "related", alias = "relates-to")]
        related_keys: Vec<String>,
        /// Clear all related keys
        #[arg(long)]
        clear_related: bool,
        /// Replace metadata with this JSON object
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Delete a project entity
    Delete {
        /// Entity key (e.g., TKB-REQ-1) or UUID
        key: String,
        /// Project key/name/ID (only required for UUIDs or ambiguous keys)
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn handle(cmd: &EntityCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        EntityCommand::Create {
            kind,
            title,
            project,
            content,
            status,
            memory_refs,
            related_keys,
            metadata,
        } => {
            handle_create(
                ctx,
                kind,
                title,
                project.clone(),
                content.as_deref(),
                status.as_deref(),
                memory_refs,
                related_keys,
                metadata.as_deref(),
            )
            .await
        }
        EntityCommand::List {
            project,
            kind,
            status,
            query,
            cursor,
            limit,
        } => {
            handle_list(
                ctx,
                project.clone(),
                kind.as_deref(),
                status.as_deref(),
                query.as_deref(),
                cursor.as_deref(),
                *limit,
            )
            .await
        }
        EntityCommand::View { key, project } => handle_view(ctx, key, project.clone()).await,
        EntityCommand::Update {
            key,
            project,
            title,
            content,
            status,
            memory_refs,
            clear_memory_refs,
            related_keys,
            clear_related,
            metadata,
        } => {
            handle_update(
                ctx,
                key,
                project.clone(),
                title.as_deref(),
                content.as_deref(),
                status.as_deref(),
                memory_refs,
                *clear_memory_refs,
                related_keys,
                *clear_related,
                metadata.as_deref(),
            )
            .await
        }
        EntityCommand::Delete { key, project } => handle_delete(ctx, key, project.clone()).await,
    }
}

async fn handle_create(
    ctx: &Ctx,
    kind: &str,
    title: &str,
    project: Option<String>,
    content: Option<&str>,
    status: Option<&str>,
    memory_refs: &[String],
    related_keys: &[String],
    metadata: Option<&str>,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let mut body = json!({
        "kind": normalize_kind(kind)?,
        "title": title,
    });

    if let Some(value) = content {
        body["content"] = json!(value);
    }
    if let Some(value) = status {
        body["status"] = json!(normalize_status(value));
    }
    if !memory_refs.is_empty() {
        body["memory_refs"] = json!(memory_refs);
    }
    if !related_keys.is_empty() {
        body["related_keys"] = json!(related_keys);
    }
    if let Some(value) = metadata {
        body["metadata"] = parse_metadata(value)?;
    }

    let resp: ProjectEntityItem = ctx
        .api
        .post(&format!("/v1/projects/{project_id}/entities"), &body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg = format::inline::mutation_created(
            entity_label(&resp.kind),
            &resp.key,
            Some(&resp.title),
            &ctx.color,
        );
        format::print_inline(&msg);
    }

    Ok(())
}

async fn handle_list(
    ctx: &Ctx,
    project: Option<String>,
    kind: Option<&str>,
    status: Option<&str>,
    query: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let mut url = format!("/v1/projects/{project_id}/entities?limit={limit}");
    if let Some(value) = kind {
        url.push_str(&format!("&kind={}", enc(&normalize_kind(value)?)));
    }
    if let Some(value) = status {
        url.push_str(&format!("&status={}", enc(&normalize_status(value))));
    }
    if let Some(value) = query {
        url.push_str(&format!("&q={}", enc(value)));
    }
    if let Some(value) = cursor {
        url.push_str(&format!("&cursor={}", enc(value)));
    }

    let resp: PaginatedResponse<ProjectEntityItem> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        print_entity_list(&resp.items, ctx);
        if let Some(next_cursor) = &resp.cursor {
            let remaining = resp.total.saturating_sub(resp.items.len() as u64);
            if remaining > 0 {
                format::print_pagination_footer(remaining, next_cursor, &ctx.color);
            }
        }
    }

    Ok(())
}

async fn handle_view(ctx: &Ctx, key: &str, project: Option<String>) -> Result<()> {
    let project = resolve_entity_project(ctx, key, project).await?;
    let entity: ProjectEntityItem = ctx
        .api
        .get(&format!(
            "/v1/projects/{}/entities/{}",
            project.id,
            enc(key)
        ))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&entity);
    } else {
        print_entity_card(&entity, ctx);
    }

    Ok(())
}

async fn handle_update(
    ctx: &Ctx,
    key: &str,
    project: Option<String>,
    title: Option<&str>,
    content: Option<&str>,
    status: Option<&str>,
    memory_refs: &[String],
    clear_memory_refs: bool,
    related_keys: &[String],
    clear_related: bool,
    metadata: Option<&str>,
) -> Result<()> {
    let project = resolve_entity_project(ctx, key, project).await?;
    let normalized_status = status.map(normalize_status);
    let mut body = json!({});

    if let Some(value) = title {
        body["title"] = json!(value);
    }
    if let Some(value) = content {
        body["content"] = json!(value);
    }
    if let Some(value) = &normalized_status {
        body["status"] = json!(value);
    }
    if clear_memory_refs {
        body["memory_refs"] = json!([]);
    } else if !memory_refs.is_empty() {
        body["memory_refs"] = json!(memory_refs);
    }
    if clear_related {
        body["related_keys"] = json!([]);
    } else if !related_keys.is_empty() {
        body["related_keys"] = json!(related_keys);
    }
    if let Some(value) = metadata {
        body["metadata"] = parse_metadata(value)?;
    }

    if body.as_object().map(|obj| obj.is_empty()).unwrap_or(true) {
        return Err(CliError::InvalidInput(
            "Provide at least one field to update.".to_string(),
        ));
    }

    let resp: ProjectEntityItem = ctx
        .api
        .patch(
            &format!("/v1/projects/{}/entities/{}", project.id, enc(key)),
            &body,
        )
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let mut changes: Vec<(&str, &str, &str)> = Vec::new();
        if let Some(value) = normalized_status.as_deref() {
            changes.push(("status", EM_DASH, value));
        }
        let msg = if changes.is_empty() {
            let check = ctx.color.paint("✓", colors::SUCCESS);
            format!("{check} Updated {}", resp.key)
        } else {
            format::inline::mutation_updated("entity", &resp.key, &changes, &ctx.color)
        };
        format::print_inline(&msg);
    }

    Ok(())
}

async fn handle_delete(ctx: &Ctx, key: &str, project: Option<String>) -> Result<()> {
    let project = resolve_entity_project(ctx, key, project).await?;
    let resp: MutationResponse = ctx
        .api
        .delete(&format!(
            "/v1/projects/{}/entities/{}",
            project.id,
            enc(key)
        ))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let display_key = resp.key.as_deref().unwrap_or(key);
        let msg = format::inline::mutation_deleted("entity", display_key, &ctx.color);
        format::print_inline(&msg);
    }

    Ok(())
}

fn print_entity_list(entities: &[ProjectEntityItem], ctx: &Ctx) {
    if entities.is_empty() {
        println!("No project entities.");
        return;
    }

    let columns = [
        Column::new("Key", 12),
        Column::new("Kind", 11),
        Column::new("Status", 10),
        Column::new("Title", 30).flexible(),
        Column::new("Memory", 6).right(),
    ];
    let rows: Vec<Vec<Option<String>>> = entities
        .iter()
        .map(|entity| {
            vec![
                Some(ctx.color.paint(&entity.key, colors::MUTED)),
                Some(entity_label(&entity.kind).to_string()),
                Some(color_status(&entity.status, &ctx.color)),
                Some(entity.title.clone()),
                Some(entity.memory_refs.len().to_string()),
            ]
        })
        .collect();

    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn print_entity_card(entity: &ProjectEntityItem, ctx: &Ctx) {
    let memory_refs = if entity.memory_refs.is_empty() {
        None
    } else {
        Some(entity.memory_refs.join(", "))
    };
    let related_keys = if entity.related_keys.is_empty() {
        None
    } else {
        Some(entity.related_keys.join(", "))
    };
    let metadata = if entity.metadata.is_object()
        && entity
            .metadata
            .as_object()
            .map(|obj| obj.is_empty())
            .unwrap_or(true)
    {
        None
    } else {
        Some(entity.metadata.to_string())
    };

    let mut sections = vec![CardSection::Fields(vec![
        CardField::required("Kind", entity_label(&entity.kind).to_string()),
        CardField::required("Status", color_status(&entity.status, &ctx.color)),
        CardField::new("Memory", memory_refs),
        CardField::new("Related", related_keys),
        CardField::new("Created", entity.created_at.clone()),
        CardField::new("Updated", entity.updated_at.clone()),
    ])];

    if !entity.content.is_empty() {
        sections.push(CardSection::Prose {
            heading: "Content".to_string(),
            body: entity.content.clone(),
        });
    }

    if let Some(value) = metadata {
        sections.push(CardSection::List {
            heading: "Metadata".to_string(),
            items: vec![truncate(&value, 400)],
        });
    }

    print!(
        "{}",
        render_card(&entity.key, &entity.title, &sections, &ctx.color)
    );
}

async fn resolve_entity_project(
    ctx: &Ctx,
    key: &str,
    project: Option<String>,
) -> Result<ResolvedProject> {
    if let Some(project_ref) = project {
        return ctx.project(Some(project_ref)).await;
    }

    if let Some(prefix) = project_prefix_from_entity_key(key) {
        return ctx.resolve_project(&prefix).await;
    }

    ctx.project(None).await
}

fn project_prefix_from_entity_key(key: &str) -> Option<String> {
    let upper = key.to_ascii_uppercase();
    for marker in ["-DEC-", "-FND-", "-REQ-"] {
        if let Some(index) = upper.find(marker) {
            let prefix = &key[..index];
            if !prefix.is_empty() {
                return Some(prefix.to_string());
            }
        }
    }
    None
}

fn normalize_kind(kind: &str) -> Result<String> {
    let normalized = kind.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "decision" | "dec" => Ok("decision".to_string()),
        "finding" | "findings" | "fnd" => Ok("finding".to_string()),
        "requirement" | "requirements" | "req" => Ok("requirement".to_string()),
        _ => Err(CliError::InvalidInput(format!(
            "Invalid entity kind '{kind}'. Use decision/DEC, finding/FND, or requirement/REQ."
        ))),
    }
}

fn entity_label(kind: &str) -> &str {
    match kind {
        "decision" | "DEC" => "decision",
        "finding" | "FND" => "finding",
        "requirement" | "REQ" => "requirement",
        _ => "entity",
    }
}

fn normalize_status(status: &str) -> String {
    status.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn parse_metadata(raw: &str) -> Result<Value> {
    let value = serde_json::from_str::<Value>(raw)
        .map_err(|err| CliError::InvalidInput(format!("metadata must be a JSON object: {err}")))?;
    if !value.is_object() {
        return Err(CliError::InvalidInput(
            "metadata must be a JSON object.".to_string(),
        ));
    }
    Ok(value)
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::{normalize_kind, project_prefix_from_entity_key};

    #[test]
    fn normalize_kind_accepts_codes_and_names() {
        assert_eq!(normalize_kind("DEC").unwrap(), "decision");
        assert_eq!(normalize_kind("finding").unwrap(), "finding");
        assert_eq!(normalize_kind("REQ").unwrap(), "requirement");
    }

    #[test]
    fn project_prefix_from_entity_key_extracts_prefix() {
        assert_eq!(
            project_prefix_from_entity_key("TKB-DEC-1").as_deref(),
            Some("TKB")
        );
        assert_eq!(
            project_prefix_from_entity_key("ABC-FND-22").as_deref(),
            Some("ABC")
        );
        assert_eq!(project_prefix_from_entity_key("not-a-key"), None);
    }
}
