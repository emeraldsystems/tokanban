use clap::Args;
use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::error::Result;
use crate::format::table::{render_table, Column};
use crate::format::{self, colors};

/// `tokanban usage` — workspace token usage and estimated cost
/// (GET /v1/usage/summary + /v1/usage/breakdown).
#[derive(Debug, Args)]
pub struct UsageArgs {
    /// Time range: 7d, 30d, or 90d
    #[arg(long, default_value = "30d")]
    pub range: String,
    /// Project key (limits results to one project)
    #[arg(long)]
    pub project: Option<String>,
    /// Agent identity filter (sessions.created_by)
    #[arg(long)]
    pub agent: Option<String>,
    /// Optional session owner user ID
    #[arg(long)]
    pub user: Option<String>,
    /// Model filter, e.g. claude-opus-4-8
    #[arg(long)]
    pub model: Option<String>,
    /// Group breakdown by: project, agent, or model
    #[arg(long, default_value = "project")]
    pub by: String,
    /// Also fetch and show session lifecycle counts (raw starts, run kinds,
    /// structured handoffs, timeout reasons) — spec/SESSION_LIFECYCLE.md.
    #[arg(long)]
    pub lifecycle: bool,
}

/// TKB-139: telemetry/cost coverage, independent of the token/cost sums.
/// A session with no accepted usage report is "unmeasured". Optional coverage
/// keeps responses from older servers readable without inventing coverage.
#[derive(Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub struct UsageCoverage {
    #[serde(default)]
    pub sessions_total: u64,
    #[serde(default)]
    pub sessions_token_measured: u64,
    #[serde(default)]
    pub sessions_token_unmeasured: u64,
    #[serde(default)]
    pub sessions_cost_known: u64,
    #[serde(default)]
    pub sessions_cost_unknown: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageSummary {
    pub sessions: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
    pub active_now: u64,
    #[serde(default)]
    pub coverage: Option<UsageCoverage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageGroup {
    pub key: Option<String>,
    pub sessions: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub completed_tasks: Option<u64>,
    #[serde(default)]
    pub cost_per_completed_task: Option<f64>,
    #[serde(default)]
    pub sessions_token_measured: Option<u64>,
    #[serde(default)]
    pub sessions_token_unmeasured: Option<u64>,
    #[serde(default)]
    pub sessions_cost_known: Option<u64>,
    #[serde(default)]
    pub sessions_cost_unknown: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageBreakdown {
    pub by: String,
    pub groups: Vec<UsageGroup>,
}

/// TKB-127: session lifecycle analytics contract (spec/SESSION_LIFECYCLE.md).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RunKindCounts {
    #[serde(default)]
    pub human: u64,
    #[serde(default)]
    pub subagent: u64,
    #[serde(default)]
    pub evaluation: u64,
    #[serde(default)]
    pub test: u64,
    #[serde(default)]
    pub unknown: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeoutReasonCount {
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionLifecycle {
    pub raw_starts: u64,
    pub session_records: u64,
    pub identified_harness_sessions: u64,
    pub unidentified_sessions: u64,
    #[serde(default)]
    pub run_kind: RunKindCounts,
    pub structured_handoffs: u64,
    #[serde(default)]
    pub timeout_reasons: Vec<TimeoutReasonCount>,
    #[serde(default)]
    pub automatic_chronicles: u64,
    #[serde(default)]
    pub noise_automatic_chronicles: u64,
    #[serde(default)]
    pub duplicate_automatic_chronicles: u64,
}

#[derive(Debug, Serialize)]
struct UsageReport<'a> {
    summary: &'a UsageSummary,
    breakdown: &'a UsageBreakdown,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_lifecycle: Option<&'a SessionLifecycle>,
}

fn fmt_tokens(value: Option<u64>) -> String {
    let Some(n) = value else {
        return "unmeasured".to_string();
    };
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_cost(v: Option<f64>) -> String {
    match v {
        Some(c) => format!("${c:.2}"),
        None => "unknown".to_string(),
    }
}

/// "4/10 measured" — honest coverage, never collapsed into the token/cost totals.
fn fmt_coverage(measured: u64, total: u64) -> String {
    format!("{measured}/{total} measured")
}

/// Group-level coverage falls back to `EM_DASH` for servers/fixtures that
/// predate the coverage columns (fields absent -> None), rather than lying
/// with a fabricated 0/N.
fn fmt_group_coverage(measured: Option<u64>, total: u64) -> String {
    match measured {
        Some(m) => fmt_coverage(m, total),
        None => "unknown".to_string(),
    }
}

// Older servers coalesced missing values to zero. Without coverage that zero
// is ambiguous, so keep it unknown. Positive reported subtotals remain visible.
fn normalize_missing_coverage(summary: &mut UsageSummary, breakdown: &mut UsageBreakdown) {
    let unmeasured = summary
        .coverage
        .map(|c| c.sessions_token_measured == 0)
        .unwrap_or(summary.total_tokens == Some(0));
    if unmeasured {
        summary.input_tokens = None;
        summary.output_tokens = None;
        summary.cache_read_tokens = None;
        summary.cache_write_tokens = None;
        summary.total_tokens = None;
    }
    if summary
        .coverage
        .map(|c| c.sessions_cost_known == 0)
        .unwrap_or(summary.estimated_cost_usd == Some(0.0))
    {
        summary.estimated_cost_usd = None;
    }
    for group in &mut breakdown.groups {
        let total = group.input_tokens.unwrap_or(0)
            + group.output_tokens.unwrap_or(0)
            + group.cache_read_tokens.unwrap_or(0)
            + group.cache_write_tokens.unwrap_or(0);
        if group
            .sessions_token_measured
            .map(|n| n == 0)
            .unwrap_or(total == 0)
        {
            group.input_tokens = None;
            group.output_tokens = None;
            group.cache_read_tokens = None;
            group.cache_write_tokens = None;
        }
        if group
            .sessions_cost_known
            .map(|n| n == 0)
            .unwrap_or(group.estimated_cost_usd == Some(0.0))
        {
            group.estimated_cost_usd = None;
        }
    }
}

pub async fn handle(args: &UsageArgs, ctx: &Ctx) -> Result<()> {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    params.append_pair("range", &args.range);
    if let Some(project) = args.project.clone() {
        let project_id = ctx.project_id(Some(project)).await?;
        params.append_pair("project_id", &project_id);
    }
    if let Some(agent) = &args.agent {
        params.append_pair("agent", agent);
    }
    if let Some(user) = &args.user {
        params.append_pair("user_id", user);
    }
    if let Some(model) = &args.model {
        params.append_pair("model", model);
    }

    let filters = params.finish();
    let mut summary: UsageSummary = ctx.api.get(&format!("/v1/usage/summary?{filters}")).await?;
    let group_by: String = url::form_urlencoded::byte_serialize(args.by.as_bytes()).collect();
    let mut breakdown: UsageBreakdown = ctx
        .api
        .get(&format!("/v1/usage/breakdown?by={}&{filters}", group_by))
        .await?;
    normalize_missing_coverage(&mut summary, &mut breakdown);
    let session_lifecycle: Option<SessionLifecycle> = if args.lifecycle {
        Some(
            ctx.api
                .get(&format!("/v1/usage/sessions/lifecycle?{filters}"))
                .await?,
        )
    } else {
        None
    };

    if ctx.format.is_json() {
        format::print_json(&UsageReport {
            summary: &summary,
            breakdown: &breakdown,
            session_lifecycle: session_lifecycle.as_ref(),
        });
        return Ok(());
    }

    println!(
        "Usage last {}: {} session records ({} active now), token subtotal {}, known est. cost {}",
        args.range,
        summary.sessions,
        summary.active_now,
        fmt_tokens(summary.total_tokens),
        fmt_cost(summary.estimated_cost_usd),
    );
    println!(
        "  input {} / output {} / cache read {} / cache write {}",
        fmt_tokens(summary.input_tokens),
        fmt_tokens(summary.output_tokens),
        fmt_tokens(summary.cache_read_tokens),
        fmt_tokens(summary.cache_write_tokens),
    );
    if let Some(coverage) = summary.coverage {
        println!(
            "  coverage: {} tokens, {} cost; totals are partial when coverage is incomplete",
            fmt_coverage(coverage.sessions_token_measured, coverage.sessions_total),
            fmt_coverage(coverage.sessions_cost_known, coverage.sessions_total)
        );
    } else {
        println!("  coverage: unknown (server did not provide coverage)");
    }
    println!("  Cohort: sessions created in this range; cumulative totals as of now.");

    if let Some(lc) = &session_lifecycle {
        println!();
        println!(
            "Session lifecycle: {} raw starts, {} session records ({} identified, {} unidentified)",
            lc.raw_starts,
            lc.session_records,
            lc.identified_harness_sessions,
            lc.unidentified_sessions
        );
        println!(
            "  run kind: human {} / subagent {} / evaluation {} / test {} / unknown {}",
            lc.run_kind.human,
            lc.run_kind.subagent,
            lc.run_kind.evaluation,
            lc.run_kind.test,
            lc.run_kind.unknown
        );
        println!(
            "  structured handoffs: {} (coverage proxy)",
            lc.structured_handoffs
        );
        println!("  active automatic chronicles: {} / known empty: {} / excess per session and lifecycle: {}",
            lc.automatic_chronicles, lc.noise_automatic_chronicles, lc.duplicate_automatic_chronicles);
        if lc.timeout_reasons.is_empty() {
            println!("  timeout reasons: none currently incomplete");
        } else {
            let reasons: Vec<String> = lc
                .timeout_reasons
                .iter()
                .map(|r| format!("{} x{}", r.reason, r.count))
                .collect();
            println!(
                "  timeout reasons (current incomplete): {}",
                reasons.join(", ")
            );
        }
    }

    if breakdown.groups.is_empty() {
        println!();
        println!("No usage recorded in this range.");
        return Ok(());
    }

    println!();
    println!("By {}:", breakdown.by);
    let columns = [
        Column::new("Group", 24).flexible(),
        Column::new("Sessions", 8),
        Column::new("Tokens", 10),
        Column::new("Est. cost", 10),
        Column::new("Token / cost coverage", 32),
    ];
    let rows: Vec<Vec<Option<String>>> = breakdown
        .groups
        .iter()
        .map(|g| {
            let tokens = match (g.input_tokens, g.output_tokens) {
                (Some(input), Some(output)) => Some(
                    input
                        + output
                        + g.cache_read_tokens.unwrap_or(0)
                        + g.cache_write_tokens.unwrap_or(0),
                ),
                _ => None,
            };
            vec![
                Some(
                    g.key
                        .clone()
                        .unwrap_or_else(|| ctx.color.paint("(none)", colors::MUTED)),
                ),
                Some(g.sessions.to_string()),
                Some(fmt_tokens(tokens)),
                Some(fmt_cost(g.estimated_cost_usd)),
                Some(format!(
                    "{} / {}",
                    fmt_group_coverage(g.sessions_token_measured, g.sessions),
                    fmt_group_coverage(g.sessions_cost_known, g.sessions)
                )),
            ]
        })
        .collect();
    print!("{}", render_table(&columns, &rows, &ctx.color));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::format::OutputFormat;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_ctx(server_uri: String, format: OutputFormat) -> Ctx {
        let mut config = AppConfig::default();
        config.api.url = server_uri;
        config.auth.access_token = Some("tk_test".to_string());
        Ctx::new(config, None, false, false, format, true).unwrap()
    }

    // ── Serde backward compatibility (TKB-139 / TKB-127) ──────────────────

    #[test]
    fn usage_summary_defaults_coverage_when_server_omits_it() {
        let raw = json!({
            "sessions": 3, "input_tokens": 100, "output_tokens": 50,
            "cache_read_tokens": 0, "cache_write_tokens": 0,
            "total_tokens": 150, "estimated_cost_usd": 1.5, "active_now": 1
        });
        let summary: UsageSummary = serde_json::from_value(raw).unwrap();
        assert!(summary.coverage.is_none());
    }

    #[test]
    fn usage_summary_reads_explicit_coverage_honestly() {
        let raw = json!({
            "sessions": 3, "input_tokens": 100, "output_tokens": 50,
            "cache_read_tokens": 0, "cache_write_tokens": 0,
            "total_tokens": 150, "estimated_cost_usd": 1.5, "active_now": 1,
            "coverage": {
                "sessions_total": 3, "sessions_token_measured": 1, "sessions_token_unmeasured": 2,
                "sessions_cost_known": 0, "sessions_cost_unknown": 3
            }
        });
        let summary: UsageSummary = serde_json::from_value(raw).unwrap();
        assert_eq!(summary.coverage.unwrap().sessions_token_measured, 1);
        assert_eq!(summary.coverage.unwrap().sessions_token_unmeasured, 2);
        assert_eq!(summary.coverage.unwrap().sessions_cost_known, 0);
    }

    #[test]
    fn usage_group_coverage_fields_are_optional_for_old_servers() {
        let raw = json!({ "key": "proj_a", "sessions": 5, "input_tokens": 10, "output_tokens": 0 });
        let group: UsageGroup = serde_json::from_value(raw).unwrap();
        assert_eq!(group.sessions_token_measured, None);
        assert_eq!(group.cache_read_tokens, None);
    }

    #[test]
    fn session_lifecycle_deserializes_run_kind_and_timeout_reasons() {
        let raw = json!({
            "raw_starts": 8, "session_records": 7,
            "identified_harness_sessions": 7, "unidentified_sessions": 0,
            "run_kind": { "human": 1, "subagent": 5, "evaluation": 1, "test": 0, "unknown": 0 },
            "structured_handoffs": 2,
            "timeout_reasons": [{ "reason": "timeout_no_memory_activity", "count": 2 }]
        });
        let lc: SessionLifecycle = serde_json::from_value(raw).unwrap();
        assert_eq!(lc.raw_starts, 8);
        assert!(lc.raw_starts > lc.session_records);
        assert_eq!(lc.run_kind.human, 1);
        assert_eq!(lc.run_kind.subagent, 5);
        assert_eq!(lc.timeout_reasons[0].count, 2);
    }

    // ── Honest formatting ──────────────────────────────────────────────────

    #[test]
    fn fmt_coverage_never_hides_zero_measured() {
        assert_eq!(fmt_coverage(0, 5), "0/5 measured");
        assert_eq!(fmt_coverage(5, 5), "5/5 measured");
    }

    #[test]
    fn fmt_group_coverage_falls_back_to_dash_when_absent() {
        assert_eq!(fmt_group_coverage(None, 5), "unknown");
        assert_eq!(fmt_group_coverage(Some(0), 5), "0/5 measured");
    }

    #[test]
    fn missing_zero_is_unknown_but_measured_zero_remains_zero_in_json() {
        let raw = json!({"sessions": 1, "input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0,
            "cache_write_tokens": 0, "total_tokens": 0, "estimated_cost_usd": 0, "active_now": 0});
        let mut summary: UsageSummary = serde_json::from_value(raw.clone()).unwrap();
        let mut groups = UsageBreakdown {
            by: "model".into(),
            groups: vec![],
        };
        normalize_missing_coverage(&mut summary, &mut groups);
        assert_eq!(summary.total_tokens, None);
        assert_eq!(
            serde_json::to_value(&summary).unwrap()["estimated_cost_usd"],
            serde_json::Value::Null
        );
        let mut measured = raw;
        measured["coverage"] =
            json!({"sessions_total": 1, "sessions_token_measured": 1, "sessions_cost_known": 1});
        let mut summary: UsageSummary = serde_json::from_value(measured).unwrap();
        normalize_missing_coverage(&mut summary, &mut groups);
        assert_eq!(summary.total_tokens, Some(0));
        assert_eq!(summary.estimated_cost_usd, Some(0.0));
    }

    #[test]
    fn nullable_totals_from_new_server_deserialize_as_unknown() {
        let summary: UsageSummary =
            serde_json::from_value(json!({"sessions": 2, "input_tokens": null,
            "output_tokens": null, "cache_read_tokens": null, "cache_write_tokens": null,
            "total_tokens": null, "estimated_cost_usd": null, "active_now": 0}))
            .unwrap();
        assert_eq!(fmt_tokens(summary.total_tokens), "unmeasured");
        assert_eq!(fmt_cost(summary.estimated_cost_usd), "unknown");
    }

    // ── handle() request wiring ──────────────────────────────────────────

    #[tokio::test]
    async fn handle_without_lifecycle_flag_does_not_call_lifecycle_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/usage/summary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sessions": 0, "input_tokens": 0, "output_tokens": 0,
                "cache_read_tokens": 0, "cache_write_tokens": 0,
                "total_tokens": 0, "estimated_cost_usd": 0.0, "active_now": 0
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/usage/breakdown"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "by": "project", "groups": [] })),
            )
            .mount(&server)
            .await;

        let ctx = test_ctx(server.uri(), OutputFormat::Json);
        let args = UsageArgs {
            range: "7d".to_string(),
            project: None,
            agent: None,
            model: None,
            user: None,
            by: "project".to_string(),
            lifecycle: false,
        };
        handle(&args, &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| !r.url.path().contains("lifecycle")));
    }

    #[tokio::test]
    async fn handle_with_lifecycle_flag_fetches_the_lifecycle_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/usage/summary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sessions": 7, "input_tokens": 0, "output_tokens": 0,
                "cache_read_tokens": 0, "cache_write_tokens": 0,
                "total_tokens": 0, "estimated_cost_usd": 0.0, "active_now": 0
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/usage/breakdown"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "by": "project", "groups": [] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/usage/sessions/lifecycle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "raw_starts": 8, "session_records": 7,
                "identified_harness_sessions": 7, "unidentified_sessions": 0,
                "run_kind": { "human": 1, "subagent": 5, "evaluation": 1, "test": 0, "unknown": 0 },
                "structured_handoffs": 0,
                "timeout_reasons": []
            })))
            .mount(&server)
            .await;

        let ctx = test_ctx(server.uri(), OutputFormat::Json);
        let args = UsageArgs {
            range: "7d".to_string(),
            project: None,
            agent: None,
            model: None,
            user: None,
            by: "project".to_string(),
            lifecycle: true,
        };
        handle(&args, &ctx).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert!(requests
            .iter()
            .any(|r| r.url.path() == "/v1/usage/sessions/lifecycle"));
    }
}
