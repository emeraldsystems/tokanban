use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format;

#[derive(Debug, Args)]
pub struct ReviewFile {
    /// Private follow-up ID
    pub id: String,
    /// Reviewed JSON body, including expected_revision; '-' reads stdin
    #[arg(long)]
    pub file: PathBuf,
}
#[derive(Debug, Args)]
pub struct SourceFile {
    /// Reviewed JSON request; '-' reads stdin
    #[arg(long)]
    pub file: PathBuf,
}
#[derive(Debug, Subcommand)]
pub enum FollowupCommand {
    /// List private suggestions across projects; never records an inbox visit
    List {
        #[arg(long)]
        state: Option<String>,
        #[arg(long, default_value = "current", value_parser = ["current", "historical", "all"])]
        historical: String,
        #[arg(long, value_parser = ["self", "other", "none", "unknown"])]
        waiting_on: Option<String>,
        /// Canonical project ID; omitted means all projects, regardless of config defaults
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        repository_id: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=100))]
        limit: u32,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100000))]
        offset: u32,
    },
    /// Show canonical review state, original sources, and any ordinary task link
    Get { id: String },
    /// Save a task review template, or the exact reserved acceptance for recovery
    Plan {
        id: String,
        /// New JSON file to review; existing files are not overwritten
        #[arg(long)]
        output: PathBuf,
    },
    /// Edit private fields from a reviewed JSON file
    Edit(ReviewFile),
    /// Create one ordinary task from explicitly reviewed JSON fields; retries preserve exact values
    Accept(ReviewFile),
    /// Link an existing task without changing its ownership or status
    Link(ReviewFile),
    /// Merge two proposed suggestions using both reviewed revisions
    Merge(ReviewFile),
    /// Dismiss with an explicit reason
    Dismiss(ReviewFile),
    /// Mark obsolete with an explicit reason
    Obsolete(ReviewFile),
    /// Resolve after terminal task evidence or with an explicit review reason
    Resolve(ReviewFile),
    /// Reopen the private suggestion, preserving any existing task association
    Reopen(ReviewFile),
    /// Preview saved historical sources; no writes or implicit imports
    BackfillPreview {
        #[arg(long)]
        file: PathBuf,
        /// Save the exact selection/fingerprint as a new apply plan for explicit review
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Apply the exact reviewed historical selection/fingerprint/key; never creates tasks
    BackfillApply(SourceFile),
    /// Source, review-state, completion and explicit inbox-visit metrics; reads record no visits
    Metrics {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        repository_id: Option<String>,
        #[arg(long)]
        observed_from: Option<u64>,
        #[arg(long)]
        observed_to: Option<u64>,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=3650))]
        stale_after_days: u32,
    },
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn object_from_reader(mut reader: impl Read) -> Result<Value> {
    let mut bytes = Vec::new();
    reader.by_ref().take(128_001).read_to_end(&mut bytes)?;
    if bytes.len() > 128_000 {
        return Err(CliError::InvalidInput(
            "Reviewed JSON must be at most 128 KB.".into(),
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if !value.is_object() {
        return Err(CliError::InvalidInput(
            "Provide one JSON request object, without a response data wrapper.".into(),
        ));
    }
    Ok(value)
}
fn read_object(path: &Path) -> Result<Value> {
    if path == Path::new("-") {
        object_from_reader(std::io::stdin().lock())
    } else {
        object_from_reader(std::fs::File::open(path)?)
    }
}
fn validate_review(action: &str, value: &Value) -> Result<()> {
    if value
        .get("expected_revision")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .is_none()
    {
        return Err(CliError::InvalidInput(
            "Include the reviewed expected_revision from followup get.".into(),
        ));
    }
    if action == "accept" {
        for key in ["project_id", "title", "description"] {
            if value.get(key).and_then(Value::as_str).is_none() {
                return Err(CliError::InvalidInput(format!(
                    "Explicitly review the string field {key}."
                )));
            }
        }
        for key in ["assignee_id", "due_date"] {
            if !value.get(key).is_some_and(|v| v.is_null() || v.is_string()) {
                return Err(CliError::InvalidInput(format!(
                    "Explicitly review {key}: use a string or null."
                )));
            }
        }
    }
    Ok(())
}
fn write_plan(path: &Path, value: &Value) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}
fn acceptance_plan(detail: &Value) -> Result<Value> {
    let revision = detail
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| CliError::InvalidInput("The follow-up has no current revision.".into()))?;
    if let Some(reserved) = detail.get("acceptance_review").filter(|v| v.is_object()) {
        let mut plan = reserved.clone();
        plan["expected_revision"] = json!(revision);
        return Ok(plan);
    }
    if detail["state"] != "proposed" || detail.get("merge_target_id").is_some_and(|v| !v.is_null())
    {
        return Err(CliError::InvalidInput("Only a proposed canonical suggestion can prepare a new acceptance. Open its canonical follow-up or existing task.".into()));
    }
    Ok(
        json!({ "expected_revision": revision, "project_id": detail["proposed_project_id"],
        "title": detail["title"], "description": "", "assignee_id": null, "due_date": null, "share_source_links": false }),
    )
}
fn query(values: &[(&str, Option<String>)]) -> String {
    let mut encoded = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in values {
        if let Some(value) = value {
            encoded.append_pair(key, value);
        }
    }
    encoded.finish()
}
fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "—".into(),
    }
}
fn display(value: &Value, ctx: &Ctx) {
    if ctx.quiet {
        return;
    }
    if ctx.format.is_json() {
        format::print_json(value);
        return;
    }
    if let Some(items) = value.get("items").and_then(Value::as_array) {
        if items.is_empty() {
            println!("No follow-ups match these filters.");
        }
        for item in items {
            println!(
                "{}  {}  {}  waiting:{}  origin:{}",
                text(item, "id"),
                text(item, "state"),
                text(item, "title"),
                text(item, "waiting_on"),
                if item["historical"] == true {
                    "historical"
                } else {
                    "current/reviewed"
                }
            );
            if let Some(task) = item.get("task_id").and_then(Value::as_str) {
                println!("  Task: {task}  project:{}", text(item, "task_project_id"));
            }
            if let Some(canonical) = item.get("merge_target_id").and_then(Value::as_str) {
                println!("  Canonical follow-up: {canonical}");
            }
        }
        if !value["next_offset"].is_null() {
            println!("Next page: --offset {}", value["next_offset"]);
        }
    } else if value.get("completion").is_some() {
        println!(
            "{} source occurrences; {} exact suggestions (not distinct obligations)",
            value["sources"]["total"], value["totals"]["distinct_suggestions"]
        );
        println!(
            "Linked tasks: {} completed, {} nonterminal, {} unknown",
            value["completion"]["completed"],
            value["completion"]["nonterminal"],
            value["completion"]["unknown"]
        );
        println!(
            "{} recorded inbox visits across {} UTC days",
            value["inbox"]["visits"], value["inbox"]["distinct_active_days"]
        );
        println!("Use --format json for state breakdown, filters and evidence. Accepted does not mean completed.");
    } else if value.get("unique_suggestions").is_some() {
        println!(
            "{} saved entries; {} source occurrences; {} exact suggestions. Apply available: {}",
            value["source_entries"],
            value["source_occurrences"],
            value["distinct_suggestions"],
            value["can_apply"]
        );
        if let Some(items) = value["unique_suggestions"].as_array() {
            for item in items {
                println!("  {}: {}", text(item, "id"), text(item, "description"));
            }
        }
        if let Some(sessions) = value["sessions"].as_array() {
            for session in sessions {
                println!(
                    "  Handoff {}: directory {} | repository {} | branch {}",
                    text(session, "id"),
                    text(&session["context"], "workingDirectory"),
                    text(&session["context"], "repositoryId"),
                    text(&session["context"], "branch")
                );
            }
        }
        if let Some(warnings) = value["warnings"].as_array() {
            for warning in warnings {
                println!("  {}", text(warning, "message"));
            }
        }
        println!("Review saved context with --format json before applying the saved plan. No tasks were created.");
    } else if value.get("run_id").is_some() {
        println!("Historical import {}: {} new sources, {} new suggestions; replayed: {}. No tasks created.", text(value, "run_id"), value["new_sources"], value["new_suggestions"], value["replayed"]);
    } else {
        println!("{} — {}", text(value, "id"), text(value, "title"));
        println!(
            "State: {}  revision: {}  waiting: {}",
            text(value, "state"),
            text(value, "revision"),
            text(value, "waiting_on")
        );
        println!("{}", text(value, "description"));
        if let Some(canonical) = value.get("canonical_id").and_then(Value::as_str) {
            println!(
                "Canonical: {canonical} ({})",
                text(value, "canonical_state")
            );
        }
        if let Some(task) = value
            .get("canonical_task_id")
            .or_else(|| value.get("task_id"))
            .and_then(Value::as_str)
        {
            println!("Linked task: {task}; completion must be checked separately.");
        }
        if let Some(sources) = value["sources"].as_array() {
            for source in sources {
                println!(
                    "Source handoff: https://app.tokanban.com/dashboard/memory/sessions/{}",
                    text(source, "session_id")
                );
            }
        }
        if value["state"] == "accepting" {
            println!("Creation is reserved. Use followup plan to save its exact values, then retry accept with that file.");
        }
        println!("Use --format json for the complete source and review history.");
    }
}
async fn review(ctx: &Ctx, action: &str, args: &ReviewFile) -> Result<()> {
    let body = read_object(&args.file)?;
    validate_review(action, &body)?;
    let path = format!(
        "/v1/followups/{}{}",
        encode(&args.id),
        if action == "edit" {
            String::new()
        } else {
            format!("/{action}")
        }
    );
    // Preserve the exact reviewed payload; retries must not fill defaults or change nullable fields.
    let response: Value = if action == "edit" {
        ctx.api.patch(&path, &body).await?
    } else {
        ctx.api.post(&path, &body).await?
    };
    display(&response["data"], ctx);
    Ok(())
}
pub async fn handle(command: &FollowupCommand, ctx: &Ctx) -> Result<()> {
    match command {
        FollowupCommand::List {
            state,
            historical,
            waiting_on,
            project_id,
            repository_id,
            session_id,
            limit,
            offset,
        } => {
            let filters = query(&[
                ("state", state.clone()),
                ("historical", Some(historical.clone())),
                ("waiting_on", waiting_on.clone()),
                ("project_id", project_id.clone()),
                ("repository_id", repository_id.clone()),
                ("session_id", session_id.clone()),
                ("limit", Some(limit.to_string())),
                ("offset", Some(offset.to_string())),
            ]);
            let response: Value = ctx.api.get(&format!("/v1/followups?{filters}")).await?;
            display(&response, ctx);
        }
        FollowupCommand::Get { id } => {
            let response: Value = ctx
                .api
                .get(&format!("/v1/followups/{}", encode(id)))
                .await?;
            display(&response["data"], ctx);
        }
        FollowupCommand::Plan { id, output } => {
            let response: Value = ctx
                .api
                .get(&format!("/v1/followups/{}", encode(id)))
                .await?;
            let plan = acceptance_plan(&response["data"])?;
            write_plan(output, &plan)?;
            if !ctx.quiet {
                if ctx.format.is_json() {
                    format::print_json(
                        &json!({ "output": output, "plan": plan, "tasks_created": 0 }),
                    );
                } else {
                    println!("Saved {}. Review project, title, shared description, assignment and due date before accepting this follow-up with --file. No task created.", output.display());
                }
            }
        }
        FollowupCommand::Edit(args) => review(ctx, "edit", args).await?,
        FollowupCommand::Accept(args) => review(ctx, "accept", args).await?,
        FollowupCommand::Link(args) => review(ctx, "link", args).await?,
        FollowupCommand::Merge(args) => review(ctx, "merge", args).await?,
        FollowupCommand::Dismiss(args) => review(ctx, "dismiss", args).await?,
        FollowupCommand::Obsolete(args) => review(ctx, "obsolete", args).await?,
        FollowupCommand::Resolve(args) => review(ctx, "resolve", args).await?,
        FollowupCommand::Reopen(args) => review(ctx, "reopen", args).await?,
        FollowupCommand::BackfillPreview { file, output } => {
            let response: Value = ctx
                .api
                .post("/v1/followups/backfill/preview", &read_object(file)?)
                .await?;
            let preview = &response["data"];
            if let Some(path) = output {
                if preview["can_apply"] != true || !preview["fingerprint"].is_string() {
                    return Err(CliError::InvalidInput("This preview cannot be applied. Inspect its warnings before selecting a smaller or valid set.".into()));
                }
                write_plan(
                    path,
                    &json!({ "session_ids": preview["selection"]["session_ids"], "fingerprint": preview["fingerprint"], "idempotency_key": format!("{:032x}", rand::random::<u128>()) }),
                )?;
            }
            display(preview, ctx);
        }
        FollowupCommand::BackfillApply(args) => {
            let body = read_object(&args.file)?;
            for key in ["session_ids", "fingerprint", "idempotency_key"] {
                if body.get(key).is_none() {
                    return Err(CliError::InvalidInput(format!(
                        "The reviewed import plan requires {key}."
                    )));
                }
            }
            let response: Value = ctx.api.post("/v1/followups/backfill/apply", &body).await?;
            display(&response["data"], ctx);
        }
        FollowupCommand::Metrics {
            project_id,
            repository_id,
            observed_from,
            observed_to,
            stale_after_days,
        } => {
            let filters = query(&[
                ("project_id", project_id.clone()),
                ("repository_id", repository_id.clone()),
                ("observed_from", observed_from.map(|n| n.to_string())),
                ("observed_to", observed_to.map(|n| n.to_string())),
                ("stale_after_days", Some(stale_after_days.to_string())),
            ]);
            let response: Value = ctx
                .api
                .get(&format!("/v1/followups/metrics?{filters}"))
                .await?;
            display(&response["data"], ctx);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use crate::config::AppConfig;
    use crate::format::OutputFormat;
    use clap::Parser;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    fn context(server: &MockServer) -> Ctx {
        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("fixture-token".into());
        Ctx::new(config, None, true, false, OutputFormat::Json, true).unwrap()
    }
    #[test]
    fn parses_review_commands_filters_and_bounds() {
        assert!(matches!(
            Cli::try_parse_from([
                "tokanban",
                "followup",
                "accept",
                "f-one",
                "--file",
                "review.json"
            ])
            .unwrap()
            .command,
            Command::Followup(FollowupCommand::Accept(_))
        ));
        assert!(Cli::try_parse_from([
            "tokanban",
            "followup",
            "list",
            "--waiting-on",
            "self",
            "--historical",
            "all"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["tokanban", "followup", "list", "--limit", "101"]).is_err());
        assert!(Cli::try_parse_from([
            "tokanban",
            "followup",
            "metrics",
            "--stale-after-days",
            "0"
        ])
        .is_err());
    }
    #[test]
    fn review_template_never_copies_private_description_and_preserves_exact_reserved_values() {
        let detail = json!({ "revision": 1, "state": "proposed", "title": "Review title", "description": "Private source context", "proposed_project_id": "project", "merge_target_id": null });
        let plan = acceptance_plan(&detail).unwrap();
        assert_eq!(plan["description"], "");
        assert!(plan["assignee_id"].is_null());
        assert!(plan["due_date"].is_null());
        let reserved = json!({ "project_id": "p", "title": "Reviewed", "description": "Shared", "assignee_id": null, "due_date": null, "labels": [], "estimate": 0, "status": "todo", "share_source_links": false });
        let recovered =
            acceptance_plan(&json!({ "revision": 9, "acceptance_review": reserved })).unwrap();
        let mut expected = reserved;
        expected["expected_revision"] = json!(9);
        assert_eq!(recovered, expected);
        assert!(acceptance_plan(
            &json!({ "revision": 2, "state": "merged", "merge_target_id": "canonical" })
        )
        .is_err());
    }
    #[test]
    fn requires_explicit_nulls_and_bounds_json_input() {
        let mut review = json!({ "expected_revision": 1, "project_id": "p", "title": "T", "description": "D", "assignee_id": null, "due_date": null });
        assert!(validate_review("accept", &review).is_ok());
        review.as_object_mut().unwrap().remove("assignee_id");
        assert!(validate_review("accept", &review).is_err());
        assert!(object_from_reader("[]".as_bytes()).is_err());
        assert!(object_from_reader(vec![b' '; 128001].as_slice()).is_err());
    }
    #[tokio::test]
    async fn sends_exact_reviewed_acceptance_and_does_not_automatically_retry_conflicts() {
        let server = MockServer::start().await;
        let ctx = context(&server);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("review.json");
        let body = json!({ "expected_revision": 7, "project_id": "p", "title": "Reviewed", "description": "Only shared text", "assignee_id": null, "due_date": null, "labels": [], "estimate": 0, "share_source_links": false });
        std::fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        Mock::given(method("POST"))
            .and(path("/v1/followups/f-one/accept"))
            .and(body_json(&body))
            .respond_with(ResponseTemplate::new(409).set_body_json(
                json!({"error":{"code":"FOLLOWUP_REVISION_CONFLICT","message":"Changed"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        assert!(handle(
            &FollowupCommand::Accept(ReviewFile {
                id: "f-one".into(),
                file
            }),
            &ctx
        )
        .await
        .is_err());
        server.verify().await;
    }
    #[tokio::test]
    async fn lists_across_projects_and_never_records_a_visit() {
        let server = MockServer::start().await;
        let mut ctx = context(&server);
        ctx.config.defaults.project = Some("unrelated-default".into());
        Mock::given(method("GET"))
            .and(path("/v1/followups"))
            .and(query_param("waiting_on", "self"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"items":[],"next_offset":null})),
            )
            .expect(1)
            .mount(&server)
            .await;
        handle(
            &FollowupCommand::List {
                state: None,
                historical: "current".into(),
                waiting_on: Some("self".into()),
                project_id: None,
                repository_id: None,
                session_id: None,
                limit: 50,
                offset: 0,
            },
            &ctx,
        )
        .await
        .unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].url.query().unwrap().contains("project_id"));
    }
    #[tokio::test]
    async fn historical_preview_saves_exact_reviewed_selection_and_apply_reuses_the_key() {
        let server = MockServer::start().await;
        let ctx = context(&server);
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("selection.json");
        let output = dir.path().join("apply.json");
        std::fs::write(&input, r#"{"session_ids":["s-one"]}"#).unwrap();
        Mock::given(method("POST")).and(path("/v1/followups/backfill/preview")).and(body_json(json!({"session_ids":["s-one"]}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{"selection":{"session_ids":["s-one"]},"fingerprint":"a".repeat(64),"can_apply":true}}))).expect(1).mount(&server).await;
        handle(
            &FollowupCommand::BackfillPreview {
                file: input,
                output: Some(output.clone()),
            },
            &ctx,
        )
        .await
        .unwrap();
        let plan = read_object(&output).unwrap();
        assert_eq!(plan["session_ids"], json!(["s-one"]));
        assert_eq!(plan["fingerprint"], "a".repeat(64));
        assert_eq!(plan["idempotency_key"].as_str().unwrap().len(), 32);
        Mock::given(method("POST"))
            .and(path("/v1/followups/backfill/apply"))
            .and(body_json(&plan))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data":{"run_id":"run","tasks_created":0}})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let command = FollowupCommand::BackfillApply(SourceFile { file: output });
        handle(&command, &ctx).await.unwrap();
        handle(&command, &ctx).await.unwrap();
        server.verify().await;
    }
    #[test]
    fn creates_private_plan_files_without_overwriting_existing_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.json");
        write_plan(&path, &json!({"review":1})).unwrap();
        assert!(write_plan(&path, &json!({"review":2})).is_err());
        assert_eq!(read_object(&path).unwrap(), json!({"review":1}));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
