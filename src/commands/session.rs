use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::config::{self, AppConfig};
use crate::error::Result;

/// Individual token counts must be non-negative integers within this bound
/// (2^53 - 1) to be trusted as measured usage; this also matches the range
/// JSON numbers can round-trip losslessly through JS-based tooling.
const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Bounded recursion depth for `$VAR`/`${VAR}` token interpolation, so a
/// cyclic env var (e.g. `FOO=$FOO`) can't recurse forever.
const MAX_TOKEN_ENV_HOPS: u8 = 4;

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Report Claude Code token usage for the current session
    #[command(name = "report-usage", hide = true)]
    ReportUsage(ReportUsageArgs),
    /// Establish a stable Claude session before its first tool call
    #[command(name = "start-hook", hide = true)]
    StartHook(ReportUsageArgs),
}

#[derive(Debug, Args)]
pub struct ReportUsageArgs {
    /// Override throttle state directory
    #[arg(long, hide = true)]
    pub state_dir: Option<PathBuf>,

    /// Override Claude Code config path for MCP auth discovery
    #[arg(long, hide = true)]
    pub claude_config: Option<PathBuf>,

    /// Override MCP endpoint URL
    #[arg(long, hide = true)]
    pub mcp_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl UsageTotals {
    /// Componentwise maximum. Used both to enforce the compaction-safe floor
    /// (never report less than a prior known total for the same canonical
    /// session) and to combine repeated per-message-id usage observations
    /// (a later, partial/corrupted duplicate must not silently lower an
    /// already-observed field).
    fn clamp_at_least(self, floor: UsageTotals) -> UsageTotals {
        UsageTotals {
            input_tokens: self.input_tokens.max(floor.input_tokens),
            output_tokens: self.output_tokens.max(floor.output_tokens),
            cache_read_tokens: self.cache_read_tokens.max(floor.cache_read_tokens),
            cache_write_tokens: self.cache_write_tokens.max(floor.cache_write_tokens),
        }
    }

    /// Overflow-safe field-wise addition (saturates instead of panicking or
    /// silently wrapping).
    fn is_safe(self) -> bool {
        [
            self.input_tokens,
            self.output_tokens,
            self.cache_read_tokens,
            self.cache_write_tokens,
        ]
        .iter()
        .all(|n| *n <= JS_MAX_SAFE_INTEGER)
    }

    fn saturating_add(self, other: UsageTotals) -> UsageTotals {
        UsageTotals {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_add(other.cache_read_tokens),
            cache_write_tokens: self
                .cache_write_tokens
                .saturating_add(other.cache_write_tokens),
        }
    }
}

/// Parse a `usage` object into a validated, measured `UsageTotals`. Returns
/// `None` (rather than defaulting missing/invalid fields to zero) if
/// `input_tokens`/`output_tokens` are absent, or if any present field is
/// negative, non-integer, null, or exceeds `JS_MAX_SAFE_INTEGER` — a
/// malformed usage object must never be silently counted as zero usage.
fn parse_measured_usage(usage: &Value) -> Option<UsageTotals> {
    Some(UsageTotals {
        input_tokens: parse_required_count(usage, "input_tokens")?,
        output_tokens: parse_required_count(usage, "output_tokens")?,
        cache_read_tokens: parse_optional_count(usage, "cache_read_input_tokens")?,
        cache_write_tokens: parse_optional_count(usage, "cache_creation_input_tokens")?,
    })
}

fn parse_required_count(usage: &Value, key: &str) -> Option<u64> {
    usage
        .get(key)?
        .as_u64()
        .filter(|&n| n <= JS_MAX_SAFE_INTEGER)
}

/// Absent optional fields default to 0 (legitimately not present). A field
/// that *is* present (including explicit `null`) must still be a valid
/// bounded non-negative integer, or the whole object is rejected.
fn parse_optional_count(usage: &Value, key: &str) -> Option<u64> {
    match usage.get(key) {
        None => Some(0),
        Some(value) => value.as_u64().filter(|&n| n <= JS_MAX_SAFE_INTEGER),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptUsageReport {
    pub session_id: String,
    pub usage: UsageTotals,
    /// Whether at least one validly measured usage object was observed
    /// (even if all-zero). Distinct from "no usage object seen at all" —
    /// only the latter should fall back to a heartbeat.
    pub measured_usage: bool,
    pub model: Option<String>,
    pub saw_session_end: bool,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    transcript_path: Option<PathBuf>,
    hook_event_name: Option<String>,
    session_id: Option<String>,
    /// Claude Code's current working directory for this session, used to
    /// resolve project-scoped MCP credentials without guessing across
    /// unrelated projects.
    cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookEvent {
    Stop,
    SessionEnd,
}

/// Bounded, non-secret outcome codes for a single report attempt. Never store
/// raw error text, URLs, headers or response bodies alongside these: doctor
/// diagnostics and local state files must stay safe to share/paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatusCode {
    /// Usage was reported and accepted.
    Success,
    /// Startup established a canonical mapping; no usage was reported.
    SessionStarted,
    /// No usage has been observed for this session yet; a heartbeat
    /// (session_update with usage omitted) was sent instead of fabricating
    /// a zero-usage sample.
    Heartbeat,
    /// The hook payload did not include a transcript path.
    NoTranscriptPath,
    /// The transcript file could not be read.
    TranscriptUnreadable,
    /// The transcript did not contain any recognizable message lines.
    TranscriptUnsupported,
    /// The transcript parsed but no Tokanban session_start/session_id was found.
    NoSessionMapping,
    /// No Tokanban credentials could be resolved for the configured account.
    CredentialsMissing,
    /// The configured/overridden MCP endpoint is not a safe target (not
    /// HTTPS except loopback, or carries userinfo/query/fragment).
    UnsafeEndpoint,
    /// The server rejected the configured credentials.
    AuthRejected,
    /// The server rejected the usage payload itself (e.g. validation error).
    UsageRejected,
    /// The server rate-limited the request.
    RateLimited,
    /// The server returned an error.
    ServerError,
    /// The request could not reach the server (connection/timeout).
    NetworkError,
    /// The server response could not be parsed.
    MalformedResponse,
}

impl ReportStatusCode {
    /// Short, human-readable label (also used for doctor rendering).
    pub fn label(self) -> &'static str {
        match self {
            ReportStatusCode::Success => "success",
            ReportStatusCode::SessionStarted => "session mapped (usage not reported yet)",
            ReportStatusCode::Heartbeat => "heartbeat (no usage yet)",
            ReportStatusCode::NoTranscriptPath => "no transcript path",
            ReportStatusCode::TranscriptUnreadable => "transcript unreadable",
            ReportStatusCode::TranscriptUnsupported => "transcript unsupported",
            ReportStatusCode::NoSessionMapping => "no session mapping",
            ReportStatusCode::CredentialsMissing => "credentials missing",
            ReportStatusCode::UnsafeEndpoint => "unsafe endpoint",
            ReportStatusCode::AuthRejected => "auth rejected",
            ReportStatusCode::UsageRejected => "usage rejected",
            ReportStatusCode::RateLimited => "rate limited",
            ReportStatusCode::ServerError => "server error",
            ReportStatusCode::NetworkError => "network error",
            ReportStatusCode::MalformedResponse => "malformed response",
        }
    }

    /// Actionable next step for this outcome, safe to print (no secrets).
    pub fn next_step(self) -> &'static str {
        match self {
            ReportStatusCode::Success => "No action needed.",
            ReportStatusCode::SessionStarted => "The startup hook established a session. Usage is reported by subsequent Stop and SessionEnd hooks.",
            ReportStatusCode::Heartbeat => {
                "No token usage recorded yet for this session; this is expected early in a session and the reporter will keep sending heartbeats."
            }
            ReportStatusCode::NoTranscriptPath => {
                "Claude Code did not provide a transcript path for this hook event; usage cannot be reported for it."
            }
            ReportStatusCode::TranscriptUnreadable => {
                "The transcript file could not be read. Check that Claude Code can write it and that the hook runs in the same environment."
            }
            ReportStatusCode::TranscriptUnsupported => {
                "The transcript format was not recognized by this CLI version. Update the tokanban CLI."
            }
            ReportStatusCode::NoSessionMapping => {
                "No saved startup mapping or successful Tokanban session_start was found. Run `tokanban init claude --yes` to install the session startup and usage hooks."
            }
            ReportStatusCode::CredentialsMissing => {
                "No Tokanban credentials were found for the configured Claude account/project. Run `tokanban auth login`, or check CLAUDE_CONFIG_DIR / --claude-config, the current project's .mcp.json, and the tokanban MCP server entry for that account."
            }
            ReportStatusCode::UnsafeEndpoint => {
                "The configured MCP endpoint is not safe to send credentials to (must be HTTPS, or plain HTTP only for loopback, with no embedded credentials/query/fragment). Check the API URL configuration."
            }
            ReportStatusCode::AuthRejected => {
                "The server rejected the configured credentials. Run `tokanban auth login` to re-authenticate."
            }
            ReportStatusCode::UsageRejected => {
                "The server rejected the usage payload. This may indicate a CLI/server version mismatch."
            }
            ReportStatusCode::RateLimited => {
                "The server rate-limited usage reporting. It will be retried on the next report."
            }
            ReportStatusCode::ServerError => {
                "The server returned an error. It will be retried on the next report."
            }
            ReportStatusCode::NetworkError => {
                "Could not reach the Tokanban API. Check network connectivity and the configured API URL."
            }
            ReportStatusCode::MalformedResponse => "The server response could not be parsed.",
        }
    }
}

/// Record of a single report attempt (success or bounded failure code).
/// Deliberately excludes raw error text, URLs, headers and response bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub at_unix: u64,
    pub status: ReportStatusCode,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UsageState {
    last_report_unix: u64,
    last_session_id: Option<String>,
    #[serde(default)]
    stable_mapping: bool,
    last_totals: UsageTotals,
    /// Whether `last_totals` reflects at least one real measured observation
    /// (as opposed to only ever having sent heartbeats).
    #[serde(default)]
    usage_measured: bool,
    /// The harness-provided session_id this state file was last keyed by,
    /// distinct from `last_session_id` (the Tokanban session id it maps to).
    #[serde(default)]
    harness_session_id: Option<String>,
    #[serde(default)]
    last_attempt: Option<AttemptRecord>,
    /// Outcome of the reporter's own auto-triggered `session_end` call,
    /// tracked separately from `last_attempt` so a failed close-out never
    /// erases/overwrites the primary successful usage report.
    #[serde(default)]
    session_end: Option<AttemptRecord>,
}

pub async fn handle(cmd: &SessionCommand, config: &AppConfig) -> Result<()> {
    let mut input = String::new();
    let _ = io::stdin().take(1024 * 1024).read_to_string(&mut input);
    match cmd {
        SessionCommand::ReportUsage(args) => {
            let _ = report_usage_from_hook_input(args, config, &input, now_unix()).await;
        }
        SessionCommand::StartHook(args) => {
            if let Some(output) = start_from_hook_input(args, config, &input, now_unix()).await {
                println!("{output}");
            }
        }
    }
    Ok(())
}

async fn start_from_hook_input(
    args: &ReportUsageArgs,
    config: &AppConfig,
    input: &str,
    now: u64,
) -> Option<Value> {
    let hook: HookInput = serde_json::from_str(input).ok()?;
    if hook.hook_event_name.as_deref() != Some("SessionStart") {
        return None;
    }
    let harness_id = hook.session_id.as_deref()?.trim();
    if !valid_session_id(harness_id) {
        return None;
    }
    let harness = Some(harness_id.to_string());
    let mcp_url = args
        .mcp_url
        .clone()
        .unwrap_or_else(|| mcp_url_from_api_base(&config.api.url));
    let claude_path = resolve_claude_config_path(args.claude_config.as_deref());
    let endpoint_status = validate_mcp_url(&mcp_url);
    let api_key = if endpoint_status.is_none() {
        resolve_api_key(
            config,
            claude_path.as_deref(),
            hook.cwd.as_deref(),
            &mcp_url,
        )
    } else {
        None
    };
    let account = account_identity_hash(&mcp_url, api_key.as_deref(), claude_path.as_deref());
    let path = state_path(args.state_dir.as_deref(), &account, harness_id).ok()?;
    if let Some(status) = endpoint_status {
        persist_attempt(&path, now, &harness, status);
        return None;
    }
    let Some(api_key) = api_key else {
        persist_attempt(&path, now, &harness, ReportStatusCode::CredentialsMissing);
        return None;
    };
    let mut arguments = json!({"source_harness": "claude-code", "harness_session_id": harness_id});
    if let Some(cwd) = hook.cwd.as_ref().filter(|path| path.is_dir()) {
        arguments["working_directory"] = json!(cwd.to_string_lossy());
    }
    if let Ok(parent) = std::env::var("TOKANBAN_PARENT_SESSION_ID") {
        if valid_session_id(&parent) {
            arguments["parent_session_id"] = json!(parent);
        }
    }
    if let Ok(kind) = std::env::var("TOKANBAN_SESSION_KIND") {
        if matches!(
            kind.as_str(),
            "human" | "subagent" | "evaluation" | "test" | "unknown"
        ) {
            arguments["session_kind"] = json!(kind);
        }
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(config.api.timeout_secs.clamp(1, 10)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let data = match call_mcp_tool(&client, &mcp_url, &api_key, 1, "session_start", arguments).await
    {
        McpCallOutcome::Accepted(data) => data,
        McpCallOutcome::Rejected(status) => {
            persist_attempt(&path, now, &harness, status);
            return None;
        }
    };
    let session_id = data.get("session_id")?.as_str()?.to_string();
    let _ = update_state(&path, |mut current| {
        if current.last_session_id.as_deref() != Some(&session_id) {
            current.last_totals = UsageTotals::default();
            current.usage_measured = false;
            current.last_report_unix = 0;
            current.session_end = None;
        }
        current.last_session_id = Some(session_id.clone());
        current.stable_mapping = true;
        current.harness_session_id = harness.clone();
        if current
            .last_attempt
            .as_ref()
            .map(|attempt| attempt.at_unix < now)
            .unwrap_or(true)
        {
            current.last_attempt = Some(AttemptRecord {
                at_unix: now,
                status: ReportStatusCode::SessionStarted,
            });
        }
        current
    });
    Some(
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": format!(
        "Tokanban canonical session: {session_id}. Reuse this session_id for session_update and session_end; startup already called session_start with your stable harness identity. Call memory_relevant_now with this session_id and your working directory, and write a structured handoff when finished. A completed handoff is preserved on resume.")}}),
    )
}

async fn report_usage_from_hook_input(
    args: &ReportUsageArgs,
    config: &AppConfig,
    input: &str,
    now: u64,
) -> Result<()> {
    let hook: HookInput = match serde_json::from_str(input) {
        Ok(hook) => hook,
        Err(_) => return Ok(()),
    };
    let event = match hook.hook_event_name.as_deref().and_then(parse_hook_event) {
        Some(event) => event,
        None => return Ok(()),
    };

    let harness_session_id = hook
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let cwd = hook.cwd.clone();

    let mcp_url = args
        .mcp_url
        .clone()
        .unwrap_or_else(|| mcp_url_from_api_base(&config.api.url));
    let claude_config_path = resolve_claude_config_path(args.claude_config.as_deref());

    let session_key = harness_session_id.clone().unwrap_or_else(|| {
        hook.transcript_path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // The endpoint itself must be safe to send credentials to before we even
    // try to resolve which credentials to use for it.
    if let Some(status) = validate_mcp_url(&mcp_url) {
        let account_hash = account_identity_hash(&mcp_url, None, claude_config_path.as_deref());
        if let Ok(state_path) = state_path(args.state_dir.as_deref(), &account_hash, &session_key) {
            persist_attempt(&state_path, now, &harness_session_id, status);
        }
        return Ok(());
    }

    // Resolve the account identity (auth + endpoint) before touching any
    // per-session state: this must match the actual harness account/config
    // location (and be paired to this exact endpoint) so we never silently
    // namespace state under, or report usage to, a different account.
    let api_key = resolve_api_key(
        config,
        claude_config_path.as_deref(),
        cwd.as_deref(),
        &mcp_url,
    );
    let account_hash =
        account_identity_hash(&mcp_url, api_key.as_deref(), claude_config_path.as_deref());
    let state_path = match state_path(args.state_dir.as_deref(), &account_hash, &session_key) {
        Ok(path) => path,
        Err(_) => return Ok(()),
    };

    // An initial, unlocked peek: only used to decide what to send and
    // whether to throttle. The authoritative write later re-reads under a
    // lock and merges instead of overwriting, so staleness here is harmless.
    let peek = read_state(&state_path);

    let transcript_path = match hook.transcript_path {
        Some(path) => path,
        None => {
            persist_attempt(
                &state_path,
                now,
                &harness_session_id,
                ReportStatusCode::NoTranscriptPath,
            );
            return Ok(());
        }
    };
    let transcript = match fs::read_to_string(&transcript_path) {
        Ok(transcript) => transcript,
        Err(_) => {
            persist_attempt(
                &state_path,
                now,
                &harness_session_id,
                ReportStatusCode::TranscriptUnreadable,
            );
            return Ok(());
        }
    };
    let report = match parse_transcript_with_mapping(
        &transcript,
        if peek.stable_mapping {
            peek.last_session_id.as_deref()
        } else {
            None
        },
    )
    .or_else(|| parse_transcript_with_mapping(&transcript, peek.last_session_id.as_deref()))
    {
        Some(report) => report,
        None => {
            persist_attempt(
                &state_path,
                now,
                &harness_session_id,
                transcript_diagnostic_when_no_report(&transcript),
            );
            return Ok(());
        }
    };

    let same_session = peek.last_session_id.as_deref() == Some(report.session_id.as_str());
    let prior_totals = if same_session {
        peek.last_totals
    } else {
        UsageTotals::default()
    };
    let prior_measured = same_session && peek.usage_measured;
    let usage = report.usage.clamp_at_least(prior_totals);
    let usage_known = (report.measured_usage || prior_measured) && usage.is_safe();

    if event == HookEvent::Stop && !should_report_stop(now, &peek) {
        return Ok(());
    }

    let api_key = match api_key {
        Some(key) => key,
        None => {
            persist_attempt(
                &state_path,
                now,
                &harness_session_id,
                ReportStatusCode::CredentialsMissing,
            );
            return Ok(());
        }
    };
    let client = match Client::builder()
        .timeout(Duration::from_secs(config.api.timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            persist_attempt(
                &state_path,
                now,
                &harness_session_id,
                ReportStatusCode::NetworkError,
            );
            return Ok(());
        }
    };

    // Never fabricate a zero-usage sample: only send `usage` once we actually
    // know it (measured this time, or carried over from a prior measurement
    // for this same canonical session); otherwise omit it as a heartbeat.
    let usage_payload = if usage_known { Some(usage) } else { None };

    let primary_status = match call_mcp_tool(
        &client,
        &mcp_url,
        &api_key,
        1,
        "session_update",
        session_update_arguments(&report.session_id, usage_payload, report.model.as_deref()),
    )
    .await
    {
        McpCallOutcome::Rejected(status) => {
            persist_attempt(&state_path, now, &harness_session_id, status);
            return Ok(());
        }
        McpCallOutcome::Accepted(_) => {
            if usage_payload.is_none() {
                ReportStatusCode::Heartbeat
            } else {
                ReportStatusCode::Success
            }
        }
    };

    let session_end_status = if event == HookEvent::SessionEnd && !report.saw_session_end {
        Some(
            match call_mcp_tool(
                &client,
                &mcp_url,
                &api_key,
                2,
                "session_end",
                session_end_arguments(&report.session_id, usage_payload, report.model.as_deref()),
            )
            .await
            {
                McpCallOutcome::Accepted(_) => ReportStatusCode::Success,
                McpCallOutcome::Rejected(status) => status,
            },
        )
    } else {
        None
    };

    let session_id = report.session_id;
    let _ = update_state(&state_path, |mut current| {
        if current.stable_mapping && current.last_session_id.as_deref() != Some(session_id.as_str())
        {
            return current;
        }
        let same_session = current.last_session_id.as_deref() == Some(session_id.as_str());
        let merged_totals = if same_session {
            usage.clamp_at_least(current.last_totals)
        } else {
            usage
        };
        let merged_known = usage_known || (same_session && current.usage_measured);
        current.harness_session_id = harness_session_id.clone();
        current.last_attempt = Some(AttemptRecord {
            at_unix: now,
            status: primary_status,
        });
        current.last_report_unix = current.last_report_unix.max(now);
        current.last_session_id = Some(session_id.clone());
        current.last_totals = merged_totals;
        current.usage_measured = merged_known;
        if let Some(status) = session_end_status {
            current.session_end = Some(AttemptRecord {
                at_unix: now,
                status,
            });
        }
        current
    });
    Ok(())
}

fn persist_attempt(
    state_path: &Path,
    now: u64,
    harness_session_id: &Option<String>,
    status: ReportStatusCode,
) {
    let _ = update_state(state_path, |mut current| {
        current.harness_session_id = harness_session_id.clone();
        current.last_attempt = Some(AttemptRecord {
            at_unix: now,
            status,
        });
        current
    });
}

/// Reject endpoints that aren't safe to send credentials to: HTTPS is
/// required except for loopback (used by local test fixtures), and no
/// userinfo/query/fragment is allowed (any of those could carry a secret in
/// the URL itself, and none is expected for a legitimate MCP endpoint).
fn validate_mcp_url(raw: &str) -> Option<ReportStatusCode> {
    let Ok(parsed) = url::Url::parse(raw) else {
        return Some(ReportStatusCode::UnsafeEndpoint);
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Some(ReportStatusCode::UnsafeEndpoint);
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Some(ReportStatusCode::UnsafeEndpoint);
    }
    let is_loopback = match parsed.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    match parsed.scheme() {
        "https" => None,
        "http" if is_loopback => None,
        _ => Some(ReportStatusCode::UnsafeEndpoint),
    }
}

pub fn parse_transcript_usage_report(transcript: &str) -> Option<TranscriptUsageReport> {
    parse_transcript_with_mapping(transcript, None)
}

fn parse_transcript_with_mapping(
    transcript: &str,
    stable_session_id: Option<&str>,
) -> Option<TranscriptUsageReport> {
    let mut usage_by_message_id: HashMap<String, UsageTotals> = HashMap::new();
    let mut usage_without_id = UsageTotals::default();
    let mut measured_usage = false;
    let mut model = None;
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut tool_results: HashMap<String, Vec<Value>> = HashMap::new();
    for line in transcript
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let message = value.get("message").unwrap_or(&value);
        if let Some(entry_usage) = message.get("usage").and_then(parse_measured_usage) {
            measured_usage = true;
            match message.get("id").and_then(Value::as_str) {
                Some(id) => {
                    usage_by_message_id
                        .entry(id.to_string())
                        .and_modify(|existing| *existing = existing.clamp_at_least(entry_usage))
                        .or_insert(entry_usage);
                }
                None => {
                    usage_without_id = usage_without_id.saturating_add(entry_usage);
                }
            }
        }
        if let Some(name) = message.get("model").and_then(Value::as_str) {
            model = Some(name.to_string());
        }
        for item in content_items(message) {
            match item.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let (Some(name), Some(id)) = (
                        item.get("name").and_then(Value::as_str),
                        item.get("id").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if is_session_start_tool(name) {
                        starts.push(id.to_string());
                    }
                    if is_session_end_tool(name) {
                        if let Some(session) =
                            item.pointer("/input/session_id").and_then(Value::as_str)
                        {
                            ends.push((id.to_string(), session.to_string()));
                        }
                    }
                }
                Some("tool_result")
                    if item.get("is_error").and_then(Value::as_bool) != Some(true) =>
                {
                    let Some(id) = item.get("tool_use_id").and_then(Value::as_str) else {
                        continue;
                    };
                    for text in tool_result_texts(item) {
                        if let Some(data) = transcript_tool_payload(&text) {
                            tool_results.entry(id.to_string()).or_default().push(data);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // A mapping returned by the startup hook is authoritative for the harness
    // identity. A later manual start must not move its usage to another row.
    let session_id = stable_session_id
        .filter(|id| valid_session_id(id))
        .map(str::to_string)
        .or_else(|| {
            starts.iter().rev().find_map(|id| {
                tool_results.get(id)?.iter().find_map(|data| {
                    data.get("session_id")
                        .and_then(Value::as_str)
                        .filter(|id| valid_session_id(id))
                        .map(str::to_string)
                })
            })
        })?;
    let saw_session_end = ends.iter().any(|(id, session)| {
        session == &session_id
            && tool_results
                .get(id)
                .map(|items| {
                    items.iter().any(|data| {
                        data.get("session_id").and_then(Value::as_str) == Some(session_id.as_str())
                            && data.get("status").and_then(Value::as_str) == Some("completed")
                    })
                })
                .unwrap_or(false)
    });
    let mut usage = usage_without_id;
    for totals in usage_by_message_id.values() {
        usage = usage.saturating_add(*totals);
    }
    if !usage.is_safe() {
        // No fabricated cap or rounded JSON integer. Preserve the last accepted
        // floor in the caller; without one this attempt is a heartbeat.
        usage = UsageTotals::default();
        measured_usage = false;
    }
    Some(TranscriptUsageReport {
        session_id,
        usage,
        measured_usage,
        model,
        saw_session_end,
    })
}

fn valid_session_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}

fn transcript_tool_payload(text: &str) -> Option<Value> {
    let root: Value = serde_json::from_str(text).ok()?;
    if root.get("isError").and_then(Value::as_bool) == Some(true) || root.get("error").is_some() {
        return None;
    }
    let data = root
        .get("structuredContent")
        .or_else(|| root.get("data"))
        .unwrap_or(&root);
    if data.get("error").is_some()
        || data.get("accepted").and_then(Value::as_bool) == Some(false)
        || matches!(
            data.get("status").and_then(Value::as_str),
            Some("error" | "failed" | "rejected" | "invalid")
        )
    {
        return None;
    }
    data.get("session_id")
        .and_then(Value::as_str)
        .filter(|id| valid_session_id(id))?;
    Some(data.clone())
}

/// When `parse_transcript_usage_report` finds no report, distinguish a
/// transcript format this CLI doesn't recognize at all (no valid JSON lines)
/// from one that parses fine but never contained a Tokanban session mapping.
fn transcript_diagnostic_when_no_report(transcript: &str) -> ReportStatusCode {
    let saw_any_json_line = transcript
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(|line| serde_json::from_str::<Value>(line).is_ok());
    if saw_any_json_line {
        ReportStatusCode::NoSessionMapping
    } else {
        ReportStatusCode::TranscriptUnsupported
    }
}

pub fn build_mcp_tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
        },
    })
}

/// `usage: None` omits the usage field entirely, sending a heartbeat rather
/// than a fabricated zero-usage sample (the server treats omitted usage on
/// `session_update` as a liveness signal).
fn session_update_arguments(
    session_id: &str,
    usage: Option<UsageTotals>,
    model: Option<&str>,
) -> Value {
    let mut args = Map::new();
    args.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    if let Some(usage) = usage {
        args.insert("usage".to_string(), usage_value(usage));
    }
    if let Some(model) = model {
        args.insert("model".to_string(), Value::String(model.to_string()));
    }
    Value::Object(args)
}

fn session_end_arguments(
    session_id: &str,
    usage: Option<UsageTotals>,
    model: Option<&str>,
) -> Value {
    let mut args = match session_update_arguments(session_id, usage, model) {
        Value::Object(args) => args,
        _ => Map::new(),
    };
    // Tells the server this session_end was auto-triggered by the usage
    // reporter (not a genuine agent-driven completion).
    args.insert(
        "completion_kind".to_string(),
        Value::String("reporter".to_string()),
    );
    args.insert(
        "continuation_prompt".to_string(),
        Value::String(
            "Usage reporter closed the session because no successful session_end handoff was present in the transcript."
                .to_string(),
        ),
    );
    args.insert("completed".to_string(), Value::Array(vec![]));
    args.insert("remaining".to_string(), Value::Array(vec![]));
    args.insert("learned".to_string(), Value::Array(vec![]));
    args.insert("decisions_made".to_string(), Value::Array(vec![]));
    args.insert("files_touched".to_string(), Value::Array(vec![]));
    Value::Object(args)
}

fn usage_value(usage: UsageTotals) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_read_tokens": usage.cache_read_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
    })
}

/// Outcome of an MCP tool call, classified into a bounded status. The raw
/// response `Value` is only ever kept in-memory transiently (for `Accepted`,
/// which is otherwise discarded); nothing here is persisted to local state.
#[derive(Debug)]
enum McpCallOutcome {
    Accepted(Value),
    Rejected(ReportStatusCode),
}

async fn call_mcp_tool(
    client: &Client,
    mcp_url: &str,
    api_key: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> McpCallOutcome {
    let expected_session = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let expected_harness = arguments
        .get("harness_session_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let has_usage = arguments.get("usage").is_some();
    let response = match client
        .post(mcp_url)
        .bearer_auth(api_key)
        .header("MCP-Protocol-Version", "2024-11-05")
        .json(&build_mcp_tool_call(id, name, arguments))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return McpCallOutcome::Rejected(ReportStatusCode::NetworkError),
    };

    let status = response.status();
    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => return McpCallOutcome::Rejected(ReportStatusCode::NetworkError),
    };

    if !status.is_success() {
        return McpCallOutcome::Rejected(classify_http_status(status));
    }

    let value: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(_) => return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse),
    };

    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse);
    }

    if let Some(error) = value.get("error") {
        return McpCallOutcome::Rejected(classify_jsonrpc_error(error));
    }

    let Some(result) = value.get("result") else {
        return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse);
    };

    match value.get("id") {
        Some(Value::Number(n)) if n.as_u64() == Some(id) => {}
        _ => return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse),
    }

    if !result.is_object()
        || result
            .get("isError")
            .is_some_and(|value| !value.is_boolean())
    {
        return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse);
    }
    let payloads = tool_payloads(result);
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return McpCallOutcome::Rejected(
            payloads
                .iter()
                .find_map(application_code)
                .map(classify_code_str)
                .unwrap_or(ReportStatusCode::UsageRejected),
        );
    }
    for data in &payloads {
        if data.get("accepted").and_then(Value::as_bool) == Some(false)
            || data.get("error").is_some()
            || data
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| matches!(s, "error" | "failed" | "rejected" | "invalid"))
        {
            return McpCallOutcome::Rejected(
                application_code(data)
                    .map(classify_code_str)
                    .unwrap_or(ReportStatusCode::UsageRejected),
            );
        }
    }
    let Some(data) = payloads.first() else {
        return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse);
    };
    let session_id = data
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256);
    let consistent = session_id.is_some()
        && payloads
            .iter()
            .all(|candidate| candidate.get("session_id").and_then(Value::as_str) == session_id);
    let status = data.get("status").and_then(Value::as_str);
    let valid = consistent
        && match name {
            "session_start" => {
                matches!(status, Some("active" | "incomplete" | "completed"))
                    && data.get("harness_session_id").and_then(Value::as_str)
                        == expected_harness.as_deref()
            }
            "session_update" => {
                session_id == expected_session.as_deref()
                    && matches!(status, Some("active" | "incomplete" | "completed"))
                    && if has_usage {
                        data.get("accepted").and_then(Value::as_bool) == Some(true)
                    } else {
                        data.get("heartbeat").is_some_and(Value::is_boolean)
                    }
            }
            "session_end" => {
                session_id == expected_session.as_deref() && status == Some("completed")
            }
            _ => false,
        };
    if !valid {
        return McpCallOutcome::Rejected(ReportStatusCode::MalformedResponse);
    }
    McpCallOutcome::Accepted(data.clone())
}

// Standard MCP structured content is the data object itself. Legacy Tokanban
// data envelopes and text-only tool results remain supported explicitly.
fn tool_payloads(result: &Value) -> Vec<Value> {
    fn unwrap_data(value: Value) -> Value {
        if value.get("session_id").is_none() {
            value.get("data").cloned().unwrap_or(value)
        } else {
            value
        }
    }
    let mut payloads = Vec::new();
    for key in ["structuredContent", "data"] {
        if let Some(value) = result.get(key).filter(|v| v.is_object()) {
            payloads.push(unwrap_data(value.clone()));
        }
    }
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for item in content {
            if item.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(value) = item
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .filter(|v| v.is_object())
                {
                    payloads.push(unwrap_data(value));
                }
            }
        }
    }
    payloads
}

fn application_code(value: &Value) -> Option<&str> {
    [
        "/error/code",
        "/data/error/code",
        "/data/code",
        "/code",
        "/status",
    ]
    .iter()
    .find_map(|path| value.pointer(path).and_then(Value::as_str))
}

fn classify_http_status(status: reqwest::StatusCode) -> ReportStatusCode {
    match status.as_u16() {
        401 | 403 => ReportStatusCode::AuthRejected,
        429 => ReportStatusCode::RateLimited,
        500..=599 => ReportStatusCode::ServerError,
        _ => ReportStatusCode::UsageRejected,
    }
}

fn classify_jsonrpc_error(error: &Value) -> ReportStatusCode {
    if let Some(code) = application_code(error) {
        return classify_code_str(code);
    }
    match error.get("code").and_then(Value::as_i64) {
        Some(-32700 | -32600 | -32601 | -32602) => ReportStatusCode::MalformedResponse,
        Some(-32603) => ReportStatusCode::ServerError,
        _ => ReportStatusCode::UsageRejected,
    }
}

/// Map a known application error code (any casing/naming convention) into a
/// bounded diagnostic. Falls back to `UsageRejected` for anything else the
/// server understood but rejected.
fn classify_code_str(code: &str) -> ReportStatusCode {
    let upper = code.to_ascii_uppercase();
    if upper.contains("RATE_LIMIT") || upper.contains("RATE-LIMIT") || upper.contains("RATELIMIT") {
        ReportStatusCode::RateLimited
    } else if upper.starts_with("AUTH")
        || upper.contains("UNAUTHENTICATED")
        || upper.contains("UNAUTHORIZED")
    {
        ReportStatusCode::AuthRejected
    } else if upper.starts_with("FORBIDDEN") || upper.contains("PERMISSION") {
        ReportStatusCode::AuthRejected
    } else if upper.contains("VALIDATION") || upper.contains("INVALID") {
        ReportStatusCode::UsageRejected
    } else if upper.parse::<u16>().is_ok_and(|n| (500..600).contains(&n)) {
        ReportStatusCode::ServerError
    } else {
        ReportStatusCode::UsageRejected
    }
}

fn parse_hook_event(value: &str) -> Option<HookEvent> {
    match value {
        "Stop" | "stop" => Some(HookEvent::Stop),
        "SessionEnd" | "session_end" | "sessionend" => Some(HookEvent::SessionEnd),
        _ => None,
    }
}

fn should_report_stop(now: u64, state: &UsageState) -> bool {
    let heartbeat_secs = std::env::var("TOKANBAN_USAGE_HEARTBEAT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60);
    state.last_report_unix == 0 || now.saturating_sub(state.last_report_unix) >= heartbeat_secs
}

/// State files are namespaced by a non-secret hash of the account identity
/// (endpoint + resolved auth) so that identical harness session IDs seen
/// across different Claude/Tokanban accounts on the same machine never share
/// throttle/cumulative-usage state. The session key itself is also hashed
/// (fixed-width hex) rather than lossily sanitized, so distinct keys that
/// would collide under character substitution (e.g. "a/b" and "a_b") can
/// never map to the same file; the original harness session id is preserved
/// inside the state file's `harness_session_id` field for humans/doctor.
fn state_path(state_dir: Option<&Path>, account_hash: &str, session_key: &str) -> Result<PathBuf> {
    let dir = match state_dir {
        Some(path) => path.to_path_buf(),
        None => config::config_dir()?.join("usage-state"),
    };
    Ok(dir.join(format!(
        "{account_hash}__{}.json",
        hash_session_key(session_key)
    )))
}

fn hash_session_key(session_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Hash the account's endpoint + resolved credential (if any) + the config
/// location it came from into a short, non-secret, non-reversible key. This
/// is a local partitioning key only, never sent over the network.
fn account_identity_hash(
    mcp_url: &str,
    api_key: Option<&str>,
    claude_config_path: Option<&Path>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(mcp_url.as_bytes());
    hasher.update(b"\0");
    hasher.update(
        claude_config_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string())
            .as_bytes(),
    );
    hasher.update(b"\0");
    hasher.update(api_key.unwrap_or("").as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Corrupt/unreadable/unparseable state is never treated as a successful
/// prior record — it's simply treated as "no prior state" (defaults), never
/// silently accepted as valid metrics.
fn read_state(path: &Path) -> UsageState {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<UsageState>(&contents).ok())
        .unwrap_or_default()
}

/// Atomically read-modify-write the state file: acquires a short-lived
/// advisory lock, re-reads the *current* on-disk state (which may be newer
/// than anything the caller peeked at earlier), applies `update`, and writes
/// via tempfile+rename so concurrent Stop/SessionEnd invocations for the
/// same session can't interleave a torn write or silently lose an update.
/// `update` should merge against the freshly-read state (e.g. via
/// `clamp_at_least`) rather than blindly overwriting it.
fn update_state<F>(path: &Path, update: F) -> io::Result<()>
where
    F: FnOnce(UsageState) -> UsageState,
{
    let _lock = StateLock::acquire(path)?;
    let updated = update(read_state(path));
    write_state_atomic(path, &updated)
}

fn write_state_atomic(path: &Path, state: &UsageState) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    let nonce: u64 = rand::rng().random();
    let temporary = PathBuf::from(format!(
        "{}.tmp-{}-{nonce:016x}",
        path.display(),
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(&contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

const MAX_LOCK_ATTEMPTS: u32 = 50;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);

// Keep a stable inode: unlinking a lock can give competing processes locks on
// different files. The OS releases this lock on drop or process exit.
struct StateLock {
    _file: fs::File,
}
impl StateLock {
    fn acquire(path: &Path) -> io::Result<Self> {
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        for _ in 0..MAX_LOCK_ATTEMPTS {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_RETRY_DELAY),
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Usage state is busy; retry on the next hook.",
        ))
    }
}

/// Resolve the exact Claude Code MCP config location the running harness
/// would use: an explicit `--claude-config` always wins; otherwise
/// `CLAUDE_CONFIG_DIR` (if set) is authoritative, since a harness that sets it
/// is telling us which account's config to use and we must not silently fall
/// back to a different account's home config. Only when neither is set do we
/// default to `~/.claude.json`.
fn resolve_claude_config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join(".claude.json"));
    }
    dirs::home_dir().map(|home| home.join(".claude.json"))
}

/// Read-only connection selection shared with doctor. Deliberately not Debug/Serialize:
/// the resolved credential must never enter diagnostic output or local state.
pub(crate) struct ReporterConnection {
    pub endpoint: String,
    pub credential: Option<String>,
    pub account_fingerprint: String,
    pub endpoint_status: Option<ReportStatusCode>,
}

pub(crate) fn resolve_reporter_connection(
    config: &AppConfig,
    claude_config: Option<&Path>,
    cwd: Option<&Path>,
    endpoint_override: Option<&str>,
) -> ReporterConnection {
    let endpoint = endpoint_override
        .map(str::to_owned)
        .unwrap_or_else(|| mcp_url_from_api_base(&config.api.url));
    let claude_path = resolve_claude_config_path(claude_config);
    let endpoint_status = validate_mcp_url(&endpoint);
    let credential = if endpoint_status.is_none() {
        resolve_api_key(config, claude_path.as_deref(), cwd, &endpoint)
    } else {
        None
    };
    let account_fingerprint =
        account_identity_hash(&endpoint, credential.as_deref(), claude_path.as_deref());
    ReporterConnection {
        endpoint,
        credential,
        account_fingerprint,
        endpoint_status,
    }
}

/// Auth resolution order: an explicit env override always wins (caller-owned,
/// trusted for any endpoint); then `.claude.json`'s local per-project entry
/// for the exact hook cwd, project `.mcp.json`, and `.claude.json`'s global
/// entry, each paired to
/// `mcp_url` so a token registered for one endpoint is never sent to
/// another; then the CLI's own logged-in credential (also caller-owned).
fn resolve_api_key(
    config: &AppConfig,
    claude_config_path: Option<&Path>,
    cwd: Option<&Path>,
    mcp_url: &str,
) -> Option<String> {
    // An explicitly selected but unusable account is a failure, never a
    // reason to try a different account's token.
    if let Ok(value) = std::env::var("TOKANBAN_API_KEY") {
        return normalize_token(&value);
    }
    let root = match read_mcp_config(claude_config_path) {
        Ok(value) => value,
        Err(()) => return None,
    };
    if let CredentialSelection::Selected(token) = local_claude_token(&root, cwd, mcp_url) {
        return token;
    }
    let project_path = cwd.map(|path| path.join(".mcp.json"));
    let project = match read_mcp_config(project_path.as_deref()) {
        Ok(value) => value,
        Err(()) => return None,
    };
    if let CredentialSelection::Selected(token) = server_tokens(&project, mcp_url) {
        return token;
    }
    if let CredentialSelection::Selected(token) = server_tokens(&root, mcp_url) {
        return token;
    }
    if std::env::var_os("CLAUDE_CONFIG_DIR").is_some()
        || !mcp_endpoint_matches(&mcp_url_from_api_base(&config.api.url), mcp_url)
    {
        return None;
    }
    config
        .auth
        .access_token
        .as_deref()
        .and_then(normalize_token)
}

#[derive(Debug)]
enum CredentialSelection {
    Absent,
    Selected(Option<String>),
}

fn read_mcp_config(path: Option<&Path>) -> std::result::Result<Value, ()> {
    let Some(path) = path else {
        return Ok(Value::Null);
    };
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(_) => return Err(()),
    };
    let root: Value = serde_json::from_str(&contents).map_err(|_| ())?;
    if !root.is_object() {
        return Err(());
    }
    Ok(root)
}

fn server_tokens(root: &Value, mcp_url: &str) -> CredentialSelection {
    match root.get("mcpServers") {
        None => CredentialSelection::Absent,
        Some(Value::Object(servers)) => select_tokanban_server_token(servers, mcp_url),
        Some(_) => CredentialSelection::Selected(None),
    }
}

fn local_claude_token(root: &Value, cwd: Option<&Path>, mcp_url: &str) -> CredentialSelection {
    let Some(cwd) = cwd else {
        return CredentialSelection::Absent;
    };
    let key = cwd.to_string_lossy();
    match root
        .get("projects")
        .and_then(|projects| projects.get(key.as_ref()))
    {
        Some(project) => server_tokens(project, mcp_url),
        None => CredentialSelection::Absent,
    }
}

// Claude's documented per-name precedence is local > project > user.
// Preserve the difference between "absent" and "selected but invalid".
fn select_tokanban_server_token(
    servers: &Map<String, Value>,
    mcp_url: &str,
) -> CredentialSelection {
    if let Some(server) = servers.get("tokanban") {
        return CredentialSelection::Selected(api_key_from_mcp_server(server, mcp_url));
    }
    let matching: Vec<&Value> = servers
        .values()
        .filter(|server| {
            server
                .get("url")
                .and_then(Value::as_str)
                .map(|url| mcp_endpoint_matches(url, mcp_url))
                .unwrap_or(false)
        })
        .collect();
    if matching.is_empty() {
        return CredentialSelection::Absent;
    }
    let Some(first) = api_key_from_mcp_server(matching[0], mcp_url) else {
        return CredentialSelection::Selected(None);
    };
    if matching
        .iter()
        .any(|server| api_key_from_mcp_server(server, mcp_url).as_deref() != Some(first.as_str()))
    {
        return CredentialSelection::Selected(None);
    }
    CredentialSelection::Selected(Some(first))
}

/// A server entry's token is only usable for the endpoint it was actually
/// registered for — never reused against a different (e.g. `--mcp-url`
/// overridden) endpoint.
fn api_key_from_mcp_server(server: &Value, mcp_url: &str) -> Option<String> {
    let url = server.get("url").and_then(Value::as_str)?;
    if !mcp_endpoint_matches(url, mcp_url) {
        return None;
    }
    let auth = server
        .get("headers")
        .and_then(|headers| headers.get("Authorization"))
        .and_then(Value::as_str)?;
    normalize_token(auth)
}

fn mcp_endpoint_matches(configured: &str, effective: &str) -> bool {
    configured.trim_end_matches('/') == effective.trim_end_matches('/')
}

fn normalize_token(value: &str) -> Option<String> {
    normalize_token_bounded(value, MAX_TOKEN_ENV_HOPS)
}

/// Resolves `$VAR`/`${VAR}` indirection up to `remaining_hops` times so a
/// cyclic env var (e.g. `FOO=$FOO`) terminates instead of recursing forever.
fn normalize_token_bounded(value: &str, remaining_hops: u8) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_bearer = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();
    let env_name = if without_bearer.starts_with("${") && without_bearer.ends_with('}') {
        Some(&without_bearer[2..without_bearer.len() - 1])
    } else {
        without_bearer.strip_prefix('$')
    };
    if let Some(env_name) = env_name {
        if remaining_hops == 0 {
            return None;
        }
        return std::env::var(env_name)
            .ok()
            .and_then(|value| normalize_token_bounded(&value, remaining_hops - 1));
    }
    Some(without_bearer.to_string())
}

fn mcp_url_from_api_base(api_url: &str) -> String {
    let trimmed = api_url.trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn content_items(message: &Value) -> Vec<&Value> {
    match message.get("content") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

fn tool_result_texts(item: &Value) -> Vec<String> {
    match item.get("content") {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| match item {
                        Value::String(text) => Some(text.clone()),
                        _ => None,
                    })
            })
            .collect(),
        Some(Value::Object(_)) => item
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(|text| vec![text.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn is_session_start_tool(name: &str) -> bool {
    name == "session_start" || name.ends_with("__session_start")
}

fn is_session_end_tool(name: &str) -> bool {
    name == "session_end" || name.ends_with("__session_end")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    /// Tests that mutate process-global env vars (TOKANBAN_API_KEY,
    /// TOKANBAN_USAGE_HEARTBEAT_SECS) must serialize against each other,
    /// since `cargo test` runs tests in parallel within one process.
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct BodyContains(&'static str);
    impl wiremock::Match for BodyContains {
        fn matches(&self, request: &Request) -> bool {
            String::from_utf8_lossy(&request.body).contains(self.0)
        }
    }

    fn only_state_file(dir: &Path) -> PathBuf {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "expected exactly one state file, found {entries:?}"
        );
        entries.remove(0)
    }

    fn read_attempt_status(path: &Path) -> ReportStatusCode {
        let contents = fs::read_to_string(path).unwrap();
        let value: Value = serde_json::from_str(&contents).unwrap();
        serde_json::from_value(value["last_attempt"]["status"].clone()).unwrap()
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            env::set_var(key, value);
        } else {
            env::remove_var(key);
        }
    }

    fn sample_transcript() -> String {
        [
            json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "model": "claude-opus-4-8",
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": 20,
                        "cache_read_input_tokens": 300,
                        "cache_creation_input_tokens": 40
                    },
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_start_1",
                        "name": "mcp__tokanban__session_start",
                        "input": {}
                    }]
                }
            })
            .to_string(),
            json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_start_1",
                        "content": [{
                            "type": "text",
                            "text": "{\"data\":{\"session_id\":\"sess_tok_123\"}}"
                        }]
                    }]
                }
            })
            .to_string(),
            json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "model": "claude-opus-4-8",
                    "usage": {
                        "input_tokens": 15,
                        "output_tokens": 5
                    },
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_end_1",
                        "name": "mcp__tokanban__session_end",
                        "input": {"session_id": "sess_tok_123"}
                    }]
                }
            })
            .to_string(),
        ]
        .join("\n")
    }

    fn sample_transcript_b() -> String {
        [
            json!({
                "message": {
                    "model": "claude-opus-4-8",
                    "usage": {"input_tokens": 7, "output_tokens": 3},
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_start_b",
                        "name": "mcp__tokanban__session_start",
                        "input": {}
                    }]
                }
            })
            .to_string(),
            json!({
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_start_b",
                        "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_tok_b\"}}"}]
                    }]
                }
            })
            .to_string(),
        ]
        .join("\n")
    }

    fn transcript_with_session_no_usage() -> String {
        [
            json!({
                "message": {
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_start_hb",
                        "name": "mcp__tokanban__session_start",
                        "input": {}
                    }]
                }
            })
            .to_string(),
            json!({
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_start_hb",
                        "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_hb\"}}"}]
                    }]
                }
            })
            .to_string(),
        ]
        .join("\n")
    }

    fn transcript_without_session_end() -> String {
        sample_transcript()
            .lines()
            .take(2)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn transcript_parser_extracts_session_id_usage_and_model() {
        let report = parse_transcript_usage_report(&sample_transcript()).unwrap();
        assert_eq!(report.session_id, "sess_tok_123");
        assert_eq!(report.model.as_deref(), Some("claude-opus-4-8"));
        assert!(
            !report.saw_session_end,
            "a tool call without a successful result is not a completed handoff"
        );
        assert_eq!(
            report.usage,
            UsageTotals {
                input_tokens: 115,
                output_tokens: 25,
                cache_read_tokens: 300,
                cache_write_tokens: 40,
            }
        );
    }

    #[test]
    fn transcript_parser_returns_none_without_session_start() {
        let transcript = json!({
            "message": {
                "usage": {"input_tokens": 1, "output_tokens": 2},
                "content": []
            }
        })
        .to_string();
        assert!(parse_transcript_usage_report(&transcript).is_none());
    }

    #[test]
    fn transcript_diagnostic_distinguishes_unsupported_from_no_mapping() {
        assert_eq!(
            transcript_diagnostic_when_no_report("not json at all\nstill not json"),
            ReportStatusCode::TranscriptUnsupported
        );
        let transcript = json!({"message": {"content": []}}).to_string();
        assert_eq!(
            transcript_diagnostic_when_no_report(&transcript),
            ReportStatusCode::NoSessionMapping
        );
    }

    #[test]
    fn transcript_parser_deduplicates_repeated_streaming_message_ids() {
        let transcript = [
            json!({
                "message": {
                    "id": "msg_1",
                    "model": "claude-opus-4-8",
                    "usage": {"input_tokens": 50, "output_tokens": 5},
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_start_1",
                        "name": "mcp__tokanban__session_start",
                        "input": {}
                    }]
                }
            })
            .to_string(),
            // Same message id repeats with a larger, final usage number as the
            // stream completes; must replace, not add to, the first entry.
            json!({
                "message": {
                    "id": "msg_1",
                    "model": "claude-opus-4-8",
                    "usage": {"input_tokens": 100, "output_tokens": 20},
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_start_1",
                        "name": "mcp__tokanban__session_start",
                        "input": {}
                    }]
                }
            })
            .to_string(),
            json!({
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_start_1",
                        "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_dedup\"}}"}]
                    }]
                }
            })
            .to_string(),
        ]
        .join("\n");

        let report = parse_transcript_usage_report(&transcript).unwrap();
        assert_eq!(report.session_id, "sess_dedup");
        assert_eq!(
            report.usage,
            UsageTotals {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            }
        );
    }

    #[test]
    fn usage_totals_clamp_after_compaction() {
        let computed = UsageTotals {
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_write_tokens: 2,
        };
        let prior = UsageTotals {
            input_tokens: 100,
            output_tokens: 15,
            cache_read_tokens: 50,
            cache_write_tokens: 1,
        };
        assert_eq!(
            computed.clamp_at_least(prior),
            UsageTotals {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: 50,
                cache_write_tokens: 2,
            }
        );
    }

    #[test]
    fn stop_throttle_respects_env_window() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_USAGE_HEARTBEAT_SECS");
        env::set_var("TOKANBAN_USAGE_HEARTBEAT_SECS", "30");
        let state = UsageState {
            last_report_unix: 100,
            ..UsageState::default()
        };
        assert!(!should_report_stop(120, &state));
        assert!(should_report_stop(130, &state));
        restore_env("TOKANBAN_USAGE_HEARTBEAT_SECS", prior);
    }

    #[test]
    fn mcp_payload_builder_uses_tools_call_shape() {
        let payload = build_mcp_tool_call(
            9,
            "session_update",
            session_update_arguments(
                "sess_1",
                Some(UsageTotals {
                    input_tokens: 1,
                    output_tokens: 2,
                    cache_read_tokens: 3,
                    cache_write_tokens: 4,
                }),
                Some("claude-opus-4-8"),
            ),
        );
        assert_eq!(payload["jsonrpc"], "2.0");
        assert_eq!(payload["method"], "tools/call");
        assert_eq!(payload["params"]["name"], "session_update");
        assert_eq!(payload["params"]["arguments"]["session_id"], "sess_1");
        assert_eq!(
            payload["params"]["arguments"]["usage"]["cache_write_tokens"],
            4
        );
        assert_eq!(payload["params"]["arguments"]["model"], "claude-opus-4-8");
    }

    #[test]
    fn mcp_payload_builder_omits_usage_for_heartbeat() {
        let args = session_update_arguments("sess_1", None, Some("claude-opus-4-8"));
        assert!(args.get("usage").is_none());
        assert_eq!(args["session_id"], "sess_1");
    }

    const TOKANBAN_URL: &str = "https://api.tokanban.com/mcp";

    #[test]
    fn auth_resolution_prefers_env_then_claude_then_cli_config() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        let temp = tempfile::tempdir().unwrap();
        let claude_path = temp.path().join("claude.json");
        fs::write(
            &claude_path,
            json!({
                "mcpServers": {
                    "tokanban": {
                        "type": "url",
                        "url": TOKANBAN_URL,
                        "headers": {"Authorization": "Bearer tk_claude"}
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let mut config = AppConfig::default();
        config.auth.access_token = Some("tk_cli".to_string());

        env::set_var("TOKANBAN_API_KEY", "tk_env");
        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), None, TOKANBAN_URL).as_deref(),
            Some("tk_env")
        );
        env::remove_var("TOKANBAN_API_KEY");
        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), None, TOKANBAN_URL).as_deref(),
            Some("tk_claude")
        );
        fs::write(&claude_path, "{}").unwrap();
        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), None, TOKANBAN_URL).as_deref(),
            Some("tk_cli")
        );
        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[test]
    fn claude_json_token_is_not_reused_for_a_mismatched_override_endpoint() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let temp = tempfile::tempdir().unwrap();
        let claude_path = temp.path().join("claude.json");
        fs::write(
            &claude_path,
            json!({
                "mcpServers": {
                    "tokanban": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_claude"}}
                }
            })
            .to_string(),
        )
        .unwrap();
        let config = AppConfig::default();

        assert_eq!(
            resolve_api_key(
                &config,
                Some(&claude_path),
                None,
                "http://127.0.0.1:9999/mcp"
            ),
            None
        );
        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), None, TOKANBAN_URL).as_deref(),
            Some("tk_claude")
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[test]
    fn ambiguous_non_tokanban_keyed_servers_are_rejected_not_guessed() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let temp = tempfile::tempdir().unwrap();
        let claude_path = temp.path().join("claude.json");
        fs::write(
            &claude_path,
            json!({
                "mcpServers": {
                    "staging": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_one"}},
                    "prod": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_two"}}
                }
            })
            .to_string(),
        )
        .unwrap();
        let mut config = AppConfig::default();
        config.auth.access_token = Some("tk_cli_fallback".to_string());

        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), None, TOKANBAN_URL).as_deref(),
            None
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[test]
    fn claude_json_project_scope_picks_the_hooks_cwd_not_another_project() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let temp = tempfile::tempdir().unwrap();
        let claude_path = temp.path().join("claude.json");
        let project_a = temp.path().join("aaa-project");
        let project_b = temp.path().join("zzz-project");
        let project_a_key = project_a.to_string_lossy().into_owned();
        let project_b_key = project_b.to_string_lossy().into_owned();
        fs::write(
            &claude_path,
            json!({
                "projects": {
                    project_a_key: {
                        "mcpServers": {"tokanban": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_wrong_project"}}}
                    },
                    project_b_key: {
                        "mcpServers": {"tokanban": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_right_project"}}}
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let config = AppConfig::default();

        assert_eq!(
            resolve_api_key(&config, Some(&claude_path), Some(&project_b), TOKANBAN_URL).as_deref(),
            Some("tk_right_project")
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[test]
    fn mcp_json_project_override_is_used_when_present() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(
            project_dir.join(".mcp.json"),
            json!({
                "mcpServers": {"tokanban": {"url": TOKANBAN_URL, "headers": {"Authorization": "Bearer tk_project_override"}}}
            })
            .to_string(),
        )
        .unwrap();
        let config = AppConfig::default();
        let missing_claude = temp.path().join("missing-claude.json");

        assert_eq!(
            resolve_api_key(
                &config,
                Some(&missing_claude),
                Some(&project_dir),
                TOKANBAN_URL
            )
            .as_deref(),
            Some("tk_project_override")
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[test]
    fn normalize_token_bounded_recursion_handles_cyclic_env_vars() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_TEST_CYCLE");
        env::set_var("TOKANBAN_TEST_CYCLE", "$TOKANBAN_TEST_CYCLE");
        assert_eq!(normalize_token("$TOKANBAN_TEST_CYCLE"), None);
        restore_env("TOKANBAN_TEST_CYCLE", prior);
    }

    #[test]
    fn validate_mcp_url_allows_https_and_loopback_http_only() {
        assert!(validate_mcp_url(TOKANBAN_URL).is_none());
        assert!(validate_mcp_url("http://127.0.0.1:8080/mcp").is_none());
        assert!(validate_mcp_url("http://[::1]:8080/mcp").is_none());
        assert!(validate_mcp_url("http://127.attacker.example/mcp").is_some());
        assert!(validate_mcp_url("http://api.tokanban.com/mcp").is_some());
        assert!(validate_mcp_url("https://user:pass@api.tokanban.com/mcp").is_some());
        assert!(validate_mcp_url("https://api.tokanban.com/mcp?token=secret").is_some());
        assert!(validate_mcp_url("https://api.tokanban.com/mcp#fragment").is_some());
    }

    #[tokio::test]
    async fn report_usage_rejects_unsafe_override_endpoint_without_network_call() {
        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, sample_transcript()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "unsafe-endpoint-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            // Reserved, non-routable test-net address: even if the safety
            // check failed, this must never receive real traffic.
            mcp_url: Some("http://192.0.2.1/mcp".to_string()),
        };

        report_usage_from_hook_input(&args, &config, &hook, 10_000)
            .await
            .unwrap();

        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::UnsafeEndpoint
        );
    }

    #[test]
    fn state_key_hashing_avoids_naive_sanitization_collisions() {
        assert_ne!(hash_session_key("a/b"), hash_session_key("a_b"));
    }

    #[test]
    fn classify_code_str_maps_known_prefixes_to_bounded_codes() {
        assert_eq!(
            classify_code_str("AUTH_INVALID_TOKEN"),
            ReportStatusCode::AuthRejected
        );
        assert_eq!(
            classify_code_str("FORBIDDEN_WORKSPACE"),
            ReportStatusCode::AuthRejected
        );
        assert_eq!(
            classify_code_str("RATE_LIMITED"),
            ReportStatusCode::RateLimited
        );
        assert_eq!(
            classify_code_str("VALIDATION_SESSION_NOT_FOUND"),
            ReportStatusCode::UsageRejected
        );
        assert_eq!(
            classify_code_str("auth.unauthenticated"),
            ReportStatusCode::AuthRejected
        );
        assert_eq!(
            classify_code_str("something_unexpected"),
            ReportStatusCode::UsageRejected
        );
    }

    #[test]
    fn usage_totals_saturating_add_does_not_panic_on_overflow() {
        let a = UsageTotals {
            input_tokens: u64::MAX - 1,
            ..Default::default()
        };
        let b = UsageTotals {
            input_tokens: 5,
            ..Default::default()
        };
        assert_eq!(a.saturating_add(b).input_tokens, u64::MAX);
    }

    #[test]
    fn explicit_zero_usage_is_measured_not_unknown() {
        let transcript = [
            json!({"message": {"usage": {"input_tokens": 0, "output_tokens": 0}, "content": [{"type": "tool_use", "id": "t1", "name": "mcp__tokanban__session_start", "input": {}}]}}).to_string(),
            json!({"message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_zero\"}}"}]}]}}).to_string(),
        ]
        .join("\n");
        let report = parse_transcript_usage_report(&transcript).unwrap();
        assert!(report.measured_usage);
        assert_eq!(report.usage, UsageTotals::default());
    }

    #[test]
    fn missing_usage_object_is_unmeasured() {
        let transcript = [
            json!({"message": {"content": [{"type": "tool_use", "id": "t1", "name": "mcp__tokanban__session_start", "input": {}}]}}).to_string(),
            json!({"message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_none\"}}"}]}]}}).to_string(),
        ]
        .join("\n");
        let report = parse_transcript_usage_report(&transcript).unwrap();
        assert!(!report.measured_usage);
    }

    #[test]
    fn invalid_usage_fields_are_rejected_not_zeroed() {
        let transcript = [
            json!({"message": {"usage": {"input_tokens": -5, "output_tokens": 2}, "content": [{"type": "tool_use", "id": "t1", "name": "mcp__tokanban__session_start", "input": {}}]}}).to_string(),
            json!({"message": {"usage": {"input_tokens": 3.5, "output_tokens": 2}}}).to_string(),
            json!({"message": {"usage": {"input_tokens": 1, "output_tokens": null}}}).to_string(),
            json!({"message": {"usage": {"output_tokens": 2}}}).to_string(),
            json!({"message": {"usage": {"input_tokens": 9007199254740992u64, "output_tokens": 1}}}).to_string(),
            json!({"message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_invalid\"}}"}]}]}}).to_string(),
        ]
        .join("\n");
        let report = parse_transcript_usage_report(&transcript).unwrap();
        assert!(!report.measured_usage);
        assert_eq!(report.usage, UsageTotals::default());
    }

    #[test]
    fn repeated_message_id_combines_via_componentwise_maximum_not_overwrite() {
        let transcript = [
            json!({"message": {"id": "msg_1", "usage": {"input_tokens": 100, "output_tokens": 10, "cache_read_input_tokens": 5}, "content": [{"type": "tool_use", "id": "t1", "name": "mcp__tokanban__session_start", "input": {}}]}}).to_string(),
            json!({"message": {"id": "msg_1", "usage": {"input_tokens": 40, "output_tokens": 10, "cache_read_input_tokens": 20}}}).to_string(),
            json!({"message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "{\"data\":{\"session_id\":\"sess_max\"}}"}]}]}}).to_string(),
        ]
        .join("\n");
        let report = parse_transcript_usage_report(&transcript).unwrap();
        assert_eq!(
            report.usage,
            UsageTotals {
                input_tokens: 100,
                output_tokens: 10,
                cache_read_tokens: 20,
                cache_write_tokens: 0,
            }
        );
    }

    #[test]
    fn concurrent_updates_never_lose_the_larger_recorded_total() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");

        update_state(&path, |mut s| {
            s.last_session_id = Some("sess_a".to_string());
            s.last_totals = UsageTotals {
                input_tokens: 500,
                ..Default::default()
            };
            s.usage_measured = true;
            s
        })
        .unwrap();

        // A "slow" writer that started from a stale, smaller snapshot must
        // merge against the freshest on-disk value, not overwrite it.
        update_state(&path, |mut s| {
            let stale_usage = UsageTotals {
                input_tokens: 200,
                ..Default::default()
            };
            s.last_totals = stale_usage.clamp_at_least(s.last_totals);
            s.last_session_id = Some("sess_a".to_string());
            s
        })
        .unwrap();

        let final_state = read_state(&path);
        assert_eq!(final_state.last_totals.input_tokens, 500);
    }

    #[test]
    fn concurrent_state_updates_are_serialized_and_not_lost() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        std::thread::scope(|scope| {
            for _ in 0..20 {
                let path = path.clone();
                scope.spawn(move || {
                    update_state(&path, |mut s| {
                        s.last_totals.input_tokens = s.last_totals.input_tokens.saturating_add(1);
                        s
                    })
                    .unwrap();
                });
            }
        });
        let final_state = read_state(&path);
        assert_eq!(final_state.last_totals.input_tokens, 20);
    }

    #[tokio::test]
    async fn report_usage_posts_update_and_writes_state() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"structuredContent": {"session_id": "sess_tok_123", "status": "active", "accepted": true}}
            })))
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, sample_transcript()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "claude-session-1"
        })
        .to_string();
        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 1_000)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: Value = requests[0].body_json().unwrap();
        assert_eq!(body["method"], "tools/call");
        assert_eq!(body["params"]["name"], "session_update");
        assert_eq!(body["params"]["arguments"]["session_id"], "sess_tok_123");
        assert_eq!(body["params"]["arguments"]["usage"]["input_tokens"], 115);

        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(read_attempt_status(&state_file), ReportStatusCode::Success);
    }

    #[tokio::test]
    async fn report_usage_missing_transcript_path_records_diagnostic() {
        let server = MockServer::start().await;
        let hook = json!({
            "hook_event_name": "Stop",
            "session_id": "no-transcript-session"
        })
        .to_string();

        let temp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 8_000)
            .await
            .unwrap();

        assert!(server.received_requests().await.unwrap().is_empty());
        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::NoTranscriptPath
        );
    }

    #[tokio::test]
    async fn report_usage_unsupported_transcript_records_diagnostic_without_network_call() {
        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("garbage.jsonl");
        fs::write(&transcript_path, "not json\nalso not json\n").unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "garbage-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 3_000)
            .await
            .unwrap();

        assert!(server.received_requests().await.unwrap().is_empty());
        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::TranscriptUnsupported
        );
    }

    #[tokio::test]
    async fn report_usage_no_session_mapping_records_diagnostic() {
        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("no-session.jsonl");
        let transcript = json!({
            "message": {
                "model": "claude-opus-4-8",
                "usage": {"input_tokens": 5, "output_tokens": 1},
                "content": []
            }
        })
        .to_string();
        fs::write(&transcript_path, transcript).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "no-mapping-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 4_000)
            .await
            .unwrap();

        assert!(server.received_requests().await.unwrap().is_empty());
        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::NoSessionMapping
        );
    }

    #[tokio::test]
    async fn report_usage_sends_heartbeat_instead_of_fabricating_zero_usage() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"structuredContent": {"session_id": "sess_hb", "status": "active", "heartbeat": true}}
            })))
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("heartbeat.jsonl");
        fs::write(&transcript_path, transcript_with_session_no_usage()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "heartbeat-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 5_000)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: Value = requests[0].body_json().unwrap();
        assert_eq!(body["params"]["arguments"]["session_id"], "sess_hb");
        assert!(body["params"]["arguments"].get("usage").is_none());

        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::Heartbeat
        );
        let contents = fs::read_to_string(&state_file).unwrap();
        let value: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(value["last_totals"]["input_tokens"], 0);
        assert_eq!(value["last_report_unix"], 5000);
    }

    #[tokio::test]
    async fn report_usage_credentials_missing_skips_network_call_and_records_diagnostic() {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, sample_transcript()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "no-creds-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 2_000)
            .await
            .unwrap();

        assert!(server.received_requests().await.unwrap().is_empty());
        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::CredentialsMissing
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[tokio::test]
    async fn report_usage_records_auth_rejected_and_never_persists_response_body() {
        let server = MockServer::start().await;
        let decoy_secret = "decoy-secret-should-never-be-persisted";
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(401).set_body_string(format!(
                "{{\"error\":\"unauthorized: token {decoy_secret}\"}}"
            )))
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, sample_transcript()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "auth-rejected-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 6_000)
            .await
            .unwrap();

        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::AuthRejected
        );
        let contents = fs::read_to_string(&state_file).unwrap();
        assert!(!contents.contains(decoy_secret));
        assert!(!contents.contains("unauthorized"));
        assert!(!contents.contains(server.uri().as_str()));
        assert!(!contents.contains("tk_cli"));
    }

    #[tokio::test]
    async fn report_usage_records_server_error_diagnostic_without_leaking_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(
                ResponseTemplate::new(500).set_body_string("internal error, password=hunter2"),
            )
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, sample_transcript()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "Stop",
            "session_id": "server-error-session"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 7_000)
            .await
            .unwrap();

        let state_file = only_state_file(&temp.path().join("state"));
        assert_eq!(
            read_attempt_status(&state_file),
            ReportStatusCode::ServerError
        );
        let contents = fs::read_to_string(&state_file).unwrap();
        assert!(!contents.contains("hunter2"));
    }

    #[tokio::test]
    async fn report_usage_namespaces_state_by_account_identity_so_same_harness_session_id_does_not_mix(
    ) {
        let _guard = env_lock();
        let prior = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");

        let server = MockServer::start().await;
        for session in ["sess_tok_123", "sess_tok_b"] {
            Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(wiremock::matchers::body_partial_json(json!({"params":{"arguments":{"session_id":session}}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"structuredContent": {"session_id": session, "status": "active", "accepted": true}}
            })))
            .mount(&server)
            .await;
        }

        let temp = tempfile::tempdir().unwrap();
        let transcript_a = temp.path().join("a.jsonl");
        fs::write(&transcript_a, sample_transcript()).unwrap();
        let transcript_b = temp.path().join("b.jsonl");
        fs::write(&transcript_b, sample_transcript_b()).unwrap();

        let hook_a = json!({
            "transcript_path": transcript_a,
            "hook_event_name": "Stop",
            "session_id": "shared-harness-id"
        })
        .to_string();
        let hook_b = json!({
            "transcript_path": transcript_b,
            "hook_event_name": "Stop",
            "session_id": "shared-harness-id"
        })
        .to_string();

        let state_dir = temp.path().join("state");
        let missing_claude = temp.path().join("missing-claude.json");

        let mut config_a = AppConfig::default();
        config_a.api.url = server.uri();
        config_a.auth.access_token = Some("tk_account_a".to_string());
        let args_a = ReportUsageArgs {
            state_dir: Some(state_dir.clone()),
            claude_config: Some(missing_claude.clone()),
            mcp_url: None,
        };

        let mut config_b = AppConfig::default();
        config_b.api.url = server.uri();
        config_b.auth.access_token = Some("tk_account_b".to_string());
        let args_b = ReportUsageArgs {
            state_dir: Some(state_dir.clone()),
            claude_config: Some(missing_claude),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args_a, &config_a, &hook_a, 1_000)
            .await
            .unwrap();
        report_usage_from_hook_input(&args_b, &config_b, &hook_b, 1_000)
            .await
            .unwrap();

        let entries: Vec<_> = fs::read_dir(&state_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        assert_eq!(
            entries.len(),
            2,
            "expected one namespaced state file per account, found: {:?}",
            entries.iter().map(|e| e.path()).collect::<Vec<_>>()
        );

        let mut totals_seen: Vec<(String, u64)> = entries
            .iter()
            .map(|entry| {
                let contents = fs::read_to_string(entry.path()).unwrap();
                let value: Value = serde_json::from_str(&contents).unwrap();
                (
                    value["last_session_id"].as_str().unwrap().to_string(),
                    value["last_totals"]["input_tokens"].as_u64().unwrap(),
                )
            })
            .collect();
        totals_seen.sort();
        assert_eq!(
            totals_seen,
            vec![
                ("sess_tok_123".to_string(), 115),
                ("sess_tok_b".to_string(), 7),
            ]
        );

        restore_env("TOKANBAN_API_KEY", prior);
    }

    #[tokio::test]
    async fn call_mcp_tool_rejects_tool_level_error_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0", "id": 1,
                "result": {
                    "content": [{"type": "text", "text": "Auth failed"}],
                    "isError": true,
                    "structuredContent": {"data": {"code": "AUTH_INVALID_TOKEN"}},
                    "data": {"code": "AUTH_INVALID_TOKEN"}
                }
            })))
            .mount(&server)
            .await;
        let client = Client::new();
        let outcome = call_mcp_tool(
            &client,
            &format!("{}/mcp", server.uri()),
            "tk",
            1,
            "session_update",
            json!({}),
        )
        .await;
        match outcome {
            McpCallOutcome::Rejected(status) => assert_eq!(status, ReportStatusCode::AuthRejected),
            McpCallOutcome::Accepted(_) => panic!("expected rejection"),
        }
    }

    #[tokio::test]
    async fn call_mcp_tool_rejects_accepted_false_even_with_http_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0", "id": 1,
                "result": {
                    "content": [], "isError": false,
                    "structuredContent": {"data": {"accepted": false, "code": "VALIDATION_SESSION_NOT_FOUND"}},
                    "data": {"accepted": false, "code": "VALIDATION_SESSION_NOT_FOUND"}
                }
            })))
            .mount(&server)
            .await;
        let client = Client::new();
        let outcome = call_mcp_tool(
            &client,
            &format!("{}/mcp", server.uri()),
            "tk",
            1,
            "session_update",
            json!({}),
        )
        .await;
        match outcome {
            McpCallOutcome::Rejected(status) => assert_eq!(status, ReportStatusCode::UsageRejected),
            McpCallOutcome::Accepted(_) => panic!("expected rejection"),
        }
    }

    #[tokio::test]
    async fn call_mcp_tool_rejects_malformed_success_shapes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/missing-result"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"jsonrpc": "2.0", "id": 1})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/wrong-version"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jsonrpc": "1.0", "id": 1, "result": {}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/wrong-id"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jsonrpc": "2.0", "id": 999, "result": {}})),
            )
            .mount(&server)
            .await;

        let client = Client::new();
        for suffix in ["missing-result", "wrong-version", "wrong-id"] {
            let outcome = call_mcp_tool(
                &client,
                &format!("{}/{suffix}", server.uri()),
                "tk",
                1,
                "session_update",
                json!({}),
            )
            .await;
            match outcome {
                McpCallOutcome::Rejected(status) => {
                    assert_eq!(status, ReportStatusCode::MalformedResponse, "case {suffix}")
                }
                McpCallOutcome::Accepted(_) => panic!("expected rejection for {suffix}"),
            }
        }
    }

    #[tokio::test]
    async fn session_end_failure_does_not_erase_prior_successful_session_update() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(BodyContains("session_update"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jsonrpc": "2.0", "id": 1,
                "result": {"structuredContent": {"session_id": "sess_tok_123", "status": "active", "accepted": true}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(BodyContains("session_end"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let temp = tempfile::tempdir().unwrap();
        let transcript_path = temp.path().join("session.jsonl");
        fs::write(&transcript_path, transcript_without_session_end()).unwrap();
        let hook = json!({
            "transcript_path": transcript_path,
            "hook_event_name": "SessionEnd",
            "session_id": "session-end-fails"
        })
        .to_string();

        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_cli".to_string());
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing-claude.json")),
            mcp_url: None,
        };

        report_usage_from_hook_input(&args, &config, &hook, 9_000)
            .await
            .unwrap();

        let state_file = only_state_file(&temp.path().join("state"));
        let contents = fs::read_to_string(&state_file).unwrap();
        let value: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(value["last_attempt"]["status"], "success");
        assert_eq!(value["last_totals"]["input_tokens"], 100);
        assert_eq!(value["session_end"]["status"], "server_error");

        let requests = server.received_requests().await.unwrap();
        let end_request = requests
            .iter()
            .find(|r| String::from_utf8_lossy(&r.body).contains("session_end"))
            .unwrap();
        let body: Value = end_request.body_json().unwrap();
        assert_eq!(body["params"]["arguments"]["completion_kind"], "reporter");
    }

    fn real_tool_result(id: u64, data: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"result":{
            "content":[{"type":"text","text":data.to_string()}], "structuredContent":data,"data":data
        }})
    }

    #[tokio::test]
    async fn startup_reuses_harness_identity_and_reports_without_manual_start() {
        let _guard = env_lock();
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(BodyContains("session_start"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_tool_result(1,
                json!({"session_id":"canonical-hook","harness_session_id":"harness-1","status":"active"}))))
            .expect(2).mount(&server).await;
        Mock::given(method("POST"))
            .and(BodyContains("session_update"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_tool_result(
                1,
                json!({"session_id":"canonical-hook","status":"active","accepted":true}),
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(BodyContains("session_end"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_tool_result(
                2,
                json!({"session_id":"canonical-hook","status":"completed"}),
            )))
            .expect(1)
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let args = ReportUsageArgs {
            state_dir: Some(temp.path().join("state")),
            claude_config: Some(temp.path().join("missing")),
            mcp_url: None,
        };
        let mut config = AppConfig::default();
        config.api.url = server.uri();
        config.auth.access_token = Some("tk_test".into());
        let start =
            json!({"hook_event_name":"SessionStart","session_id":"harness-1","cwd":temp.path()})
                .to_string();
        for now in [1000, 1001] {
            let output = start_from_hook_input(&args, &config, &start, now)
                .await
                .unwrap();
            assert_eq!(
                output["hookSpecificOutput"]["hookEventName"],
                "SessionStart"
            );
            assert!(output["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("canonical-hook"));
            assert!(!output.to_string().contains("tk_test"));
        }
        let state_file = only_state_file(&temp.path().join("state"));
        let mapped = read_state(&state_file);
        assert!(mapped.stable_mapping);
        assert!(!mapped.usage_measured);
        assert_eq!(mapped.last_report_unix, 0);
        let transcript = temp.path().join("session.jsonl");
        fs::write(&transcript,json!({"message":{"id":"m1","model":"test-model","usage":{"input_tokens":71,"output_tokens":13}}}).to_string()).unwrap();
        let stop=json!({"hook_event_name":"SessionEnd","session_id":"harness-1","transcript_path":transcript}).to_string();
        report_usage_from_hook_input(&args, &config, &stop, 1100)
            .await
            .unwrap();
        let state = read_state(&state_file);
        assert_eq!(state.last_session_id.as_deref(), Some("canonical-hook"));
        assert_eq!(state.last_totals.input_tokens, 71);
        assert!(state.usage_measured);
        assert_eq!(state.session_end.unwrap().status, ReportStatusCode::Success);
        let requests = server.received_requests().await.unwrap();
        for request in requests.iter().take(2) {
            let body: Value = request.body_json().unwrap();
            let args = &body["params"]["arguments"];
            assert_eq!(args["harness_session_id"], "harness-1");
            assert_eq!(args["source_harness"], "claude-code");
            assert!(
                args.get("session_kind").is_none(),
                "no inference that a run is human"
            );
            assert!(args.get("usage").is_none());
        }
        let update: Value = requests[2].body_json().unwrap();
        assert_eq!(update["params"]["arguments"]["usage"]["input_tokens"], 71);
    }

    #[tokio::test]
    async fn mcp_empty_mismatched_and_conflicting_success_envelopes_are_rejected() {
        let server = MockServer::start().await;
        let cases = [
            json!({"jsonrpc":"2.0","id":1,"result":{"content":[],"structuredContent":{},"data":{}}}),
            real_tool_result(
                1,
                json!({"session_id":"different","status":"active","accepted":true}),
            ),
            json!({"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"session_id":"s","status":"active","accepted":true},"data":{"session_id":"s","accepted":false}}}),
            real_tool_result(1, json!({"session_id":"s","status":"active"})),
        ];
        for (n, body) in cases.into_iter().enumerate() {
            let suffix = format!("/{n}");
            Mock::given(method("POST"))
                .and(path(suffix.as_str()))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            let outcome = call_mcp_tool(
                &Client::new(),
                &format!("{}{suffix}", server.uri()),
                "test",
                1,
                "session_update",
                json!({"session_id":"s","usage":{}}),
            )
            .await;
            assert!(matches!(outcome, McpCallOutcome::Rejected(_)), "case {n}");
        }
        assert_eq!(
            classify_jsonrpc_error(&json!({"code":-32603,"data":{"code":"UNAUTHORIZED"}})),
            ReportStatusCode::AuthRejected
        );
        assert_eq!(
            classify_jsonrpc_error(&json!({"code":-32603,"data":{"code":"RATE_LIMITED"}})),
            ReportStatusCode::RateLimited
        );
    }

    #[test]
    fn failed_or_other_session_end_does_not_suppress_reporter_close() {
        let mut lines = vec![sample_transcript()];
        for (id, session, error) in [
            ("toolu_end_1", "sess_tok_123", true),
            ("other-end", "different", false),
        ] {
            lines.push(json!({"message":{"content":[{"type":"tool_use","id":id,"name":"session_end","input":{"session_id":session}}]}}).to_string());
            lines.push(json!({"message":{"content":[{"type":"tool_result","tool_use_id":id,"is_error":error,"content":[{"type":"text","text":json!({"session_id":session,"status":"completed"}).to_string()}]}]}}).to_string());
        }
        assert!(
            !parse_transcript_usage_report(&lines.join("\n"))
                .unwrap()
                .saw_session_end
        );
        lines.push(json!({"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_end_1","content":[{"type":"text","text":json!({"data":{"session_id":"sess_tok_123","status":"completed"}}).to_string()}]}]}}).to_string());
        assert!(
            parse_transcript_usage_report(&lines.join("\n"))
                .unwrap()
                .saw_session_end
        );
        let mapped =
            parse_transcript_with_mapping(&lines.join("\n"), Some("stable-other")).unwrap();
        assert_eq!(mapped.session_id, "stable-other");
        assert!(!mapped.saw_session_end);
    }

    #[test]
    fn cumulative_safe_integer_overflow_does_not_emit_a_fabricated_measurement() {
        let transcript=[json!({"message":{"id":"m1","usage":{"input_tokens":JS_MAX_SAFE_INTEGER,"output_tokens":0}}}).to_string(),
            json!({"message":{"id":"m2","usage":{"input_tokens":1,"output_tokens":0}}}).to_string()].join("\n");
        let report = parse_transcript_with_mapping(&transcript, Some("s")).unwrap();
        assert!(!report.measured_usage);
        assert_eq!(report.usage, UsageTotals::default());
    }

    #[test]
    fn busy_state_lock_never_writes_unlocked_and_recovers_after_owner_drops() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        update_state(&path, |mut s| {
            s.last_totals.input_tokens = 9;
            s
        })
        .unwrap();
        let guard = StateLock::acquire(&path).unwrap();
        let result = update_state(&path, |mut s| {
            s.last_totals.input_tokens = 99;
            s
        });
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(read_state(&path).last_totals.input_tokens, 9);
        drop(guard);
        update_state(&path, |mut s| {
            s.last_totals.input_tokens = 10;
            s
        })
        .unwrap();
        assert_eq!(read_state(&path).last_totals.input_tokens, 10);
    }

    #[test]
    fn selected_invalid_account_never_falls_back_and_local_beats_project() {
        let _guard = env_lock();
        let before = env::var_os("TOKANBAN_API_KEY");
        env::remove_var("TOKANBAN_API_KEY");
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("repo");
        fs::create_dir(&cwd).unwrap();
        let path = temp.path().join("claude.json");
        let mut config = AppConfig::default();
        config.auth.access_token = Some("cli-other".into());
        let server = |token: &str| json!({"url":TOKANBAN_URL,"headers":{"Authorization":token}});
        fs::write(
            cwd.join(".mcp.json"),
            json!({"mcpServers":{"tokanban":server("project")}}).to_string(),
        )
        .unwrap();
        fs::write(&path,json!({"projects":{cwd.to_string_lossy().as_ref():{"mcpServers":{"tokanban":server("local")}}},"mcpServers":{"tokanban":server("global")}}).to_string()).unwrap();
        assert_eq!(
            resolve_api_key(&config, Some(&path), Some(&cwd), TOKANBAN_URL).as_deref(),
            Some("local")
        );
        fs::write(&path,json!({"projects":{cwd.to_string_lossy().as_ref():{"mcpServers":{"tokanban":server("${UNSET_TOKANBAN_REPORTER_FIXTURE}")}}},"mcpServers":{"tokanban":server("global")}}).to_string()).unwrap();
        assert!(resolve_api_key(&config, Some(&path), Some(&cwd), TOKANBAN_URL).is_none());
        fs::write(&path, "malformed selected config").unwrap();
        assert!(resolve_api_key(&config, Some(&path), Some(&cwd), TOKANBAN_URL).is_none());
        fs::write(&path, "{}").unwrap();
        fs::write(cwd.join(".mcp.json"), "malformed project config").unwrap();
        assert!(resolve_api_key(&config, Some(&path), Some(&cwd), TOKANBAN_URL).is_none());
        fs::remove_file(cwd.join(".mcp.json")).unwrap();
        assert!(resolve_api_key(
            &config,
            Some(&path),
            Some(&cwd),
            "https://different.example/mcp"
        )
        .is_none());
        let prior_dir = env::var_os("CLAUDE_CONFIG_DIR");
        env::set_var("CLAUDE_CONFIG_DIR", temp.path());
        assert!(resolve_api_key(&config, Some(&path), Some(&cwd), TOKANBAN_URL).is_none());
        restore_env("CLAUDE_CONFIG_DIR", prior_dir);
        restore_env("TOKANBAN_API_KEY", before);
    }
}
