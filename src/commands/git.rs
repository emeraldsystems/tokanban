use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format::card::{render_card, CardField, CardSection};
use crate::format::table::{render_table, Column};
use crate::format::{self, colors, EM_DASH};

#[derive(Debug, Subcommand)]
pub enum GitCommand {
    /// Start connecting a GitHub repository to this project (opens the browser for consent)
    Connect {
        /// Project key, name, or ID (overrides config default)
        #[arg(long)]
        project: Option<String>,
        /// Print the installation URL instead of opening a browser
        #[arg(long)]
        no_browser: bool,
    },
    /// Show connected Git repositories, installation state, and sync health
    Status {
        /// Project key, name, or ID (overrides config default)
        #[arg(long)]
        project: Option<String>,
    },
    /// Inspect a repository policy or apply an explicitly reviewed JSON policy document
    Policy {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        repository: String,
        /// JSON document containing expected_revision, binding_generation, and policy
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// List pull requests linked to a task
    ListLinks {
        /// Task key (e.g. PLAT-42)
        key: String,
    },
    /// Inspect your private session sources and their historical/current Git evidence
    Provenance {
        /// Task key or canonical task ID
        key: String,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=50))]
        limit: u32,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=10000))]
        offset: u32,
    },
    /// Manually link a pull request to a task
    Link {
        /// Task key (e.g. PLAT-42)
        key: String,
        /// Tokanban repository ID (see `tokanban git status`)
        #[arg(long)]
        repository: Option<String>,
        /// Pull request URL (https://github.com/<owner>/<repo>/pull/<number>)
        #[arg(long)]
        url: Option<String>,
        /// Pull request number (required together with --repository)
        #[arg(long)]
        number: Option<u64>,
        /// related (default, display-only) or completes (required evidence for completion; never changes status by itself)
        #[arg(long, default_value = "related")]
        relation: String,
        /// Live task claim when working as a claimed agent
        #[arg(long)]
        claim_id: Option<String>,
    },
    /// Remove a linked pull request (records an exclusion so it is not silently relinked)
    Unlink {
        /// Task key (e.g. PLAT-42)
        key: String,
        /// Link ID from `tokanban git list-links`
        link_id: String,
        /// Live task claim when working as a claimed agent
        #[arg(long)]
        claim_id: Option<String>,
    },
}

pub async fn handle(cmd: &GitCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        GitCommand::Policy {
            project,
            repository,
            file,
        } => handle_policy(ctx, project.clone(), repository, file.as_deref()).await,
        GitCommand::Connect {
            project,
            no_browser,
        } => handle_connect(ctx, project.clone(), *no_browser).await,
        GitCommand::Status { project } => handle_status(ctx, project.clone()).await,
        GitCommand::ListLinks { key } => handle_list_links(ctx, key).await,
        GitCommand::Provenance { key, limit, offset } => {
            handle_provenance(ctx, key, *limit, *offset).await
        }
        GitCommand::Link {
            key,
            repository,
            url,
            number,
            relation,
            claim_id,
        } => {
            handle_link(
                ctx,
                key,
                repository.as_deref(),
                url.as_deref(),
                *number,
                relation,
                claim_id.as_deref(),
            )
            .await
        }
        GitCommand::Unlink {
            key,
            link_id,
            claim_id,
        } => handle_unlink(ctx, key, link_id, claim_id.as_deref()).await,
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct InstallationStart {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    installation_url: String,
}

async fn handle_connect(ctx: &Ctx, project: Option<String>, no_browser: bool) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let resp: InstallationStart = ctx
        .api
        .post(
            &format!("/v1/projects/{project_id}/git/installations/start"),
            &json!({}),
        )
        .await?;

    validate_installation_url(&resp.installation_url)?;
    if ctx.format.is_json() {
        format::print_json(&resp);
        return Ok(());
    }

    if no_browser {
        println!("Open this URL to connect GitHub: {}", resp.installation_url);
        return Ok(());
    }

    match open::that(&resp.installation_url) {
        Ok(()) => {
            let check = ctx.color.paint("✓", colors::SUCCESS);
            format::print_inline(&format!(
                "{check} Opened GitHub installation in your browser"
            ));
        }
        Err(_) => {
            println!("Could not open a browser automatically. Open this URL to connect GitHub:");
            println!("{}", resp.installation_url);
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct GitInstallationRow {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    id: String,
    account_login: String,
    state: String,
    #[serde(default)]
    last_error_code: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GitRepositoryRow {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    id: String,
    full_name: String,
    enabled: i64,
    installation_state: String,
    #[serde(default)]
    bound_project_id: Option<String>,
    #[serde(default)]
    last_error_code: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GitStatusResponse {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    configured: bool,
    #[serde(default)]
    installations: Vec<GitInstallationRow>,
    #[serde(default)]
    repositories: Vec<GitRepositoryRow>,
    #[serde(default)]
    stale_pull_requests: Option<u64>,
}

async fn handle_status(ctx: &Ctx, project: Option<String>) -> Result<()> {
    let project = ctx.project(project).await?;
    let resp: GitStatusResponse = ctx
        .api
        .get(&format!("/v1/projects/{}/git/health", project.id))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
        return Ok(());
    }

    if !resp.configured {
        println!("GitHub integration requires server configuration by a Tokanban administrator.");
        return Ok(());
    }

    println!(
        "{} pull request(s) need a refresh.",
        resp.stale_pull_requests.unwrap_or(0)
    );

    if resp.installations.is_empty() {
        println!("No GitHub installations connected. Run `tokanban git connect` to start.");
    } else {
        let columns = [
            Column::new("Installation", 12),
            Column::new("Account", 20).flexible(),
            Column::new("State", 12),
            Column::new("Issue", 20),
        ];
        let rows: Vec<Vec<Option<String>>> = resp
            .installations
            .iter()
            .map(|i| {
                vec![
                    Some(ctx.color.paint(&i.id, colors::MUTED)),
                    Some(i.account_login.clone()),
                    Some(i.state.clone()),
                    Some(
                        i.last_error_code
                            .clone()
                            .unwrap_or_else(|| EM_DASH.to_string()),
                    ),
                ]
            })
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }

    if resp.repositories.is_empty() {
        println!("No repositories verified yet.");
    } else {
        let columns = [
            Column::new("Repository ID", 12),
            Column::new("Full name", 24).flexible(),
            Column::new("Bound here", 10),
            Column::new("Status", 12),
        ];
        let rows: Vec<Vec<Option<String>>> = resp
            .repositories
            .iter()
            .map(|r| {
                let bound = if r.bound_project_id.as_deref() == Some(project.id.as_str()) {
                    "yes"
                } else if r.bound_project_id.is_some() {
                    "other project"
                } else {
                    "no"
                };
                let status = if r.enabled == 1 && r.installation_state == "active" {
                    "available".to_string()
                } else {
                    r.last_error_code
                        .clone()
                        .unwrap_or_else(|| "unavailable".to_string())
                };
                vec![
                    Some(ctx.color.paint(&r.id, colors::MUTED)),
                    Some(r.full_name.clone()),
                    Some(bound.to_string()),
                    Some(status),
                ]
            })
            .collect();
        print!("{}", render_table(&columns, &rows, &ctx.color));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct GitLinkEvidence {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    draft: Option<bool>,
    #[serde(default)]
    check_summary: Option<String>,
    #[serde(default)]
    review_summary: Option<String>,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    head_ref: Option<String>,
    #[serde(default)]
    base_ref: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct GitLinkItem {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    id: String,
    full_name: String,
    number: u64,
    relation: String,
    evidence_state: String,
    #[serde(default)]
    last_error_code: Option<String>,
    #[serde(default)]
    fetched_at: Option<i64>,
    url: String,
    #[serde(default)]
    evidence: Option<GitLinkEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GitLinksResponse {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    data: Vec<GitLinkItem>,
}

fn print_git_links_table(items: &[GitLinkItem], ctx: &Ctx) {
    if items.is_empty() {
        println!("No pull requests linked.");
        return;
    }
    let columns = [
        Column::new("Link ID", 12),
        Column::new("Pull request", 24).flexible(),
        Column::new("Relation", 10),
        Column::new("State", 10),
        Column::new("Checks", 10),
        Column::new("Review", 12),
        Column::new("Evidence", 12),
    ];
    let rows: Vec<Vec<Option<String>>> = items
        .iter()
        .map(|item| {
            let pr = format!("{}#{}", item.full_name, item.number);
            let evidence = item.evidence.as_ref();
            let state = evidence
                .and_then(|e| e.state.clone())
                .map(|s| {
                    if evidence.and_then(|e| e.draft) == Some(true) {
                        format!("{s} (draft)")
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| EM_DASH.to_string());
            vec![
                Some(ctx.color.paint(&item.id, colors::MUTED)),
                Some(pr),
                Some(item.relation.clone()),
                Some(state),
                Some(
                    evidence
                        .and_then(|e| e.check_summary.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                ),
                Some(
                    evidence
                        .and_then(|e| e.review_summary.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                ),
                Some(item.evidence_state.clone()),
            ]
        })
        .collect();
    print!("{}", render_table(&columns, &rows, &ctx.color));
    for item in items {
        if item.evidence_state == "stale" || item.evidence_state == "unavailable" {
            println!(
                "  {} {}#{}: evidence is {}{}",
                ctx.color.paint("!", colors::HIGH),
                item.full_name,
                item.number,
                item.evidence_state,
                item.last_error_code
                    .as_ref()
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default()
            );
        }
    }
}

async fn handle_list_links(ctx: &Ctx, key: &str) -> Result<()> {
    let resp: GitLinksResponse = ctx
        .api
        .get(&format!("/v1/tasks/{}/git-links", enc(key)))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        print_git_links_table(&resp.data, ctx);
    }
    Ok(())
}

async fn handle_link(
    ctx: &Ctx,
    key: &str,
    repository: Option<&str>,
    url: Option<&str>,
    number: Option<u64>,
    relation: &str,
    claim_id: Option<&str>,
) -> Result<()> {
    let relation = match relation.to_ascii_lowercase().as_str() {
        "related" => "related",
        "completes" => "completes",
        other => {
            return Err(CliError::InvalidInput(format!(
                "Invalid relation '{other}'. Use 'related' or 'completes'."
            )))
        }
    };
    if url.is_none() && (repository.is_none() || number.is_none()) {
        return Err(CliError::InvalidInput(
            "Provide --repository with --number, or --url.".to_string(),
        ));
    }

    let mut body = json!({ "relation": relation });
    if let Some(claim) = claim_id {
        body["claim_id"] = json!(claim);
    }
    if let Some(u) = url {
        body["url"] = json!(u);
    }
    if let Some(r) = repository {
        body["repository_id"] = json!(r);
    }
    if let Some(n) = number {
        body["number"] = json!(n);
    }

    let resp: GitLinkItem = ctx
        .api
        .post(&format!("/v1/tasks/{}/git-links", enc(key)), &body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let fields = vec![
            CardField::required("Link ID", resp.id.clone()),
            CardField::required(
                "Pull request",
                format!("{}#{}", resp.full_name, resp.number),
            ),
            CardField::required("Relation", resp.relation.clone()),
            CardField::new("Evidence", Some(resp.evidence_state.clone())),
        ];
        print!(
            "{}",
            render_card(key, &resp.url, &[CardSection::Fields(fields)], &ctx.color)
        );
    }
    Ok(())
}

async fn handle_unlink(ctx: &Ctx, key: &str, link_id: &str, claim_id: Option<&str>) -> Result<()> {
    let resp: serde_json::Value = ctx
        .api
        .delete(&format!(
            "/v1/tasks/{}/git-links/{}{}",
            enc(key),
            enc(link_id),
            claim_id
                .map(|c| format!("?claim_id={}", enc(c)))
                .unwrap_or_default()
        ))
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        format::print_inline(&format!(
            "{check} Unlinked pull request from {key} (excluded from automatic relinking)"
        ));
    }
    Ok(())
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn validate_installation_url(value: &str) -> Result<()> {
    let valid = url::Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with("/apps/")
            && url.path().ends_with("/installations/new")
    });
    if valid {
        Ok(())
    } else {
        Err(CliError::InvalidInput(
            "The server returned an invalid GitHub installation URL.".into(),
        ))
    }
}

async fn handle_policy(
    ctx: &Ctx,
    project: Option<String>,
    repository: &str,
    file: Option<&std::path::Path>,
) -> Result<()> {
    let project_id = ctx.project_id(project).await?;
    let result: serde_json::Value = if let Some(file) = file {
        if std::fs::metadata(file)?.len() > 65536 {
            return Err(CliError::InvalidInput(
                "Policy document must be at most 64 KiB.".into(),
            ));
        }
        let body: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)
            .map_err(|_| CliError::InvalidInput("Policy document must be valid JSON.".into()))?;
        if body
            .get("expected_revision")
            .and_then(|v| v.as_u64())
            .is_none()
            || body
                .get("binding_generation")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                == 0
            || !body.get("policy").is_some_and(|v| v.is_object())
        {
            return Err(CliError::InvalidInput("Policy document requires expected_revision, positive binding_generation, and a policy object. Inspect the current policy first.".into()));
        }
        ctx.api
            .patch(
                &format!(
                    "/v1/projects/{}/git/repositories/{}/policy",
                    enc(&project_id),
                    enc(repository)
                ),
                &body,
            )
            .await?
    } else {
        let status: serde_json::Value = ctx
            .api
            .get(&format!("/v1/projects/{}/git/health", enc(&project_id)))
            .await?;
        let row = status
            .get("repositories")
            .and_then(|v| v.as_array())
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row.get("id").and_then(|v| v.as_str()) == Some(repository))
            })
            .ok_or_else(|| {
                CliError::InvalidInput(
                    "Repository was not found in this project's Git settings.".into(),
                )
            })?;
        json!({ "expected_revision": row.get("policy_revision"), "binding_generation": row.get("generation"), "policy": row.get("policy") })
    };
    format::print_json(&result);
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct TaskProvenanceResponse {
    data: TaskProvenance,
}

#[derive(Debug, Deserialize, Serialize)]
struct TaskProvenance {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    task_id: String,
    private_to_current_user: bool,
    namespace: String,
    items: Vec<SessionProvenance>,
    next_offset: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SessionProvenance {
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
    session_id: String,
    source_harness: Option<String>,
    status: String,
    association: String,
    git_provenance: Option<serde_json::Value>,
}

async fn handle_provenance(ctx: &Ctx, key: &str, limit: u32, offset: u32) -> Result<()> {
    if key.trim().is_empty() || !(1..=50).contains(&limit) || offset > 10000 {
        return Err(CliError::InvalidInput(
            "Provide a task, a limit from 1 to 50, and an offset from 0 to 10000.".into(),
        ));
    }
    let response: TaskProvenanceResponse = ctx
        .api
        .get(&format!(
            "/v1/tasks/{}/session-provenance?limit={limit}&offset={offset}",
            enc(key)
        ))
        .await?;
    if ctx.format.is_json() {
        format::print_json(&response);
        return Ok(());
    }
    let data = &response.data;
    if data.private_to_current_user {
        println!(
            "Session sources are private to your current user ({} namespace).",
            data.namespace
        );
    }
    if data.items.is_empty() {
        println!("No session sources recorded for this task.");
        return Ok(());
    }
    let columns = [
        Column::new("Session", 16).flexible(),
        Column::new("Harness", 12),
        Column::new("Status", 12),
        Column::new("Association", 18),
        Column::new("Git snapshot", 12),
    ];
    let rows = data
        .items
        .iter()
        .map(|item| {
            vec![
                Some(item.session_id.clone()),
                item.source_harness.clone(),
                Some(item.status.clone()),
                Some(item.association.clone()),
                Some(
                    if item.git_provenance.is_some() {
                        "recorded"
                    } else {
                        "none"
                    }
                    .into(),
                ),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", render_table(&columns, &rows, &ctx.color));
    println!("Use --format json for historical snapshots and separately evaluated current PR evidence. Deployment status remains unknown.");
    if let Some(next) = data.next_offset {
        println!("More sources are available with --offset {next}.");
    }
    Ok(())
}
