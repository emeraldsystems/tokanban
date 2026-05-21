use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use chrono::Utc;
use clap::Subcommand;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config;
use crate::error::{CliError, Result};
use crate::format;

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// Score a memory candidate using the local richness gate
    Score {
        /// Raw JSON request
        #[arg(long, conflicts_with = "input_file")]
        input: Option<String>,

        /// Read JSON request from a file path, or `-` for stdin
        #[arg(long, value_name = "PATH", conflicts_with = "input")]
        input_file: Option<PathBuf>,
    },

    /// Persist deferred candidates across interrupted sessions
    #[command(subcommand)]
    Candidate(CandidateCommand),
}

#[derive(Debug, Subcommand)]
pub enum CandidateCommand {
    /// Add a deferred candidate to the local store
    Add {
        /// Raw JSON request
        #[arg(long, conflicts_with = "input_file")]
        input: Option<String>,

        /// Read JSON request from a file path, or `-` for stdin
        #[arg(long, value_name = "PATH", conflicts_with = "input")]
        input_file: Option<PathBuf>,

        /// Scope the candidate to a Tokanban project
        #[arg(long)]
        project_id: Option<String>,

        /// Scope the candidate to a working directory
        #[arg(long)]
        working_directory: Option<String>,

        /// Optional task scope
        #[arg(long)]
        task_id: Option<String>,

        /// Optional module scope
        #[arg(long)]
        module: Option<String>,

        /// Optional short note
        #[arg(long)]
        note: Option<String>,
    },

    /// List deferred candidates
    List {
        /// Filter by Tokanban project
        #[arg(long)]
        project_id: Option<String>,

        /// Filter by working directory
        #[arg(long)]
        working_directory: Option<String>,

        /// Filter by task
        #[arg(long)]
        task_id: Option<String>,

        /// Filter by module
        #[arg(long)]
        module: Option<String>,
    },

    /// Re-evaluate deferred candidates for session-end promotion
    Review {
        /// Filter by Tokanban project
        #[arg(long)]
        project_id: Option<String>,

        /// Filter by working directory
        #[arg(long)]
        working_directory: Option<String>,

        /// Filter by task
        #[arg(long)]
        task_id: Option<String>,

        /// Filter by module
        #[arg(long)]
        module: Option<String>,
    },

    /// Clear deferred candidates
    Clear {
        /// Clear every stored candidate
        #[arg(long)]
        all: bool,

        /// Clear specific candidate ids
        #[arg(long = "id", value_name = "CANDIDATE_ID")]
        ids: Vec<String>,

        /// Filter by Tokanban project
        #[arg(long)]
        project_id: Option<String>,

        /// Filter by working directory
        #[arg(long)]
        working_directory: Option<String>,

        /// Filter by task
        #[arg(long)]
        task_id: Option<String>,

        /// Filter by module
        #[arg(long)]
        module: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateKind {
    Fact,
    Decision,
    Scratch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    WriteNow,
    Defer,
    Drop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScoreRequest {
    pub kind: MemoryCandidateKind,
    pub content: String,
    pub confidence: Option<f32>,
    #[serde(default)]
    pub explicit_user_request: bool,
    pub durability: Option<f32>,
    pub reuse_breadth: Option<f32>,
    pub rediscovery_cost: Option<f32>,
    pub impact_if_forgotten: Option<f32>,
    pub evidence_quality: Option<f32>,
    pub atomicity: Option<f32>,
    pub evidence_completeness: Option<f32>,
    pub stability: Option<f32>,
    pub wording_readiness: Option<f32>,
    pub scope_binding: Option<f32>,
    pub session_timing: Option<f32>,
    pub decision_finality: Option<f32>,
    #[serde(default)]
    pub project_known: bool,
    #[serde(default)]
    pub workdir_known: bool,
    #[serde(default)]
    pub task_known: bool,
    #[serde(default)]
    pub module_known: bool,
    #[serde(default)]
    pub at_session_end: bool,
    #[serde(default)]
    pub volatile: bool,
    #[serde(default)]
    pub obvious_from_code: bool,
    #[serde(default)]
    pub duplicate: bool,
    #[serde(default)]
    pub open_hypothesis: bool,
    #[serde(default)]
    pub likely_resolved_this_session: bool,
    #[serde(default)]
    pub weak_support: bool,
    pub supporting_fact_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScoreResponse {
    pub action: MemoryAction,
    pub bypassed: bool,
    pub richness: f32,
    pub promotion_readiness: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateScope {
    pub project_id: Option<String>,
    pub working_directory: Option<String>,
    pub task_id: Option<String>,
    pub module: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredCandidateRecord {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub scope: CandidateScope,
    pub note: Option<String>,
    pub request: MemoryScoreRequest,
    pub score: MemoryScoreResponse,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CandidateStore {
    version: u32,
    candidates: Vec<DeferredCandidateRecord>,
}

#[derive(Debug, Serialize)]
struct CandidateAddResponse {
    stored: bool,
    updated_existing: bool,
    record: DeferredCandidateRecord,
}

#[derive(Debug, Serialize)]
struct CandidateListResponse {
    items: Vec<DeferredCandidateRecord>,
    total: usize,
}

#[derive(Debug, Serialize)]
struct CandidateClearResponse {
    removed: usize,
    remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SessionEndDisposition {
    Learned,
    DecisionsMade,
    Remaining,
    Drop,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewedCandidate {
    id: String,
    kind: MemoryCandidateKind,
    content: String,
    confidence: Option<f32>,
    scope: CandidateScope,
    note: Option<String>,
    disposition: SessionEndDisposition,
    previous_score: MemoryScoreResponse,
    session_end_score: MemoryScoreResponse,
}

#[derive(Debug, Serialize)]
struct CandidateReviewResponse {
    total: usize,
    promote_now: Vec<ReviewedCandidate>,
    keep_deferred: Vec<ReviewedCandidate>,
    drop: Vec<ReviewedCandidate>,
    session_end_contract: SessionEndContract,
    clear_after_session_end_ids: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct SessionEndContract {
    learned: Vec<SessionEndLearnedItem>,
    decisions_made: Vec<SessionEndDecisionItem>,
    remaining: Vec<SessionEndRemainingItem>,
}

#[derive(Debug, Serialize)]
struct SessionEndLearnedItem {
    fact: String,
    confidence: Option<f32>,
    task_id: Option<String>,
    module: Option<String>,
}

#[derive(Debug, Serialize)]
struct SessionEndDecisionItem {
    decision: String,
    task_id: Option<String>,
    module: Option<String>,
}

#[derive(Debug, Serialize)]
struct SessionEndRemainingItem {
    description: String,
    task_id: Option<String>,
    module: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct CandidateDefaults {
    durability: f32,
    reuse_breadth: f32,
    rediscovery_cost: f32,
    impact_if_forgotten: f32,
    atomicity: f32,
    decision_finality: f32,
}

#[derive(Debug, Clone)]
struct ScoredSignals {
    durability: f32,
    reuse_breadth: f32,
    rediscovery_cost: f32,
    impact_if_forgotten: f32,
    evidence_quality: f32,
    atomicity: f32,
    evidence_completeness: f32,
    stability: f32,
    wording_readiness: f32,
    scope_binding: f32,
    session_timing: f32,
    decision_finality: f32,
    volatile: bool,
    obvious_from_code: bool,
    duplicate: bool,
    open_hypothesis: bool,
    likely_resolved_this_session: bool,
    weak_support: bool,
}

pub async fn handle(cmd: &MemoryCommand) -> Result<()> {
    match cmd {
        MemoryCommand::Score { input, input_file } => {
            handle_score(input.as_deref(), input_file.as_ref())
        }
        MemoryCommand::Candidate(cmd) => handle_candidate(cmd),
    }
}

fn handle_score(input: Option<&str>, input_file: Option<&PathBuf>) -> Result<()> {
    let request: MemoryScoreRequest = read_json_input(input, input_file)?;
    let response = score_candidate(&request);
    format::print_json(&response);
    Ok(())
}

fn handle_candidate(cmd: &CandidateCommand) -> Result<()> {
    match cmd {
        CandidateCommand::Add {
            input,
            input_file,
            project_id,
            working_directory,
            task_id,
            module,
            note,
        } => handle_candidate_add(
            input.as_deref(),
            input_file.as_ref(),
            CandidateScope {
                project_id: project_id.clone(),
                working_directory: working_directory.clone(),
                task_id: task_id.clone(),
                module: module.clone(),
            },
            note.clone(),
        ),
        CandidateCommand::List {
            project_id,
            working_directory,
            task_id,
            module,
        } => handle_candidate_list(CandidateScope {
            project_id: project_id.clone(),
            working_directory: working_directory.clone(),
            task_id: task_id.clone(),
            module: module.clone(),
        }),
        CandidateCommand::Review {
            project_id,
            working_directory,
            task_id,
            module,
        } => handle_candidate_review(CandidateScope {
            project_id: project_id.clone(),
            working_directory: working_directory.clone(),
            task_id: task_id.clone(),
            module: module.clone(),
        }),
        CandidateCommand::Clear {
            all,
            ids,
            project_id,
            working_directory,
            task_id,
            module,
        } => handle_candidate_clear(
            *all,
            ids,
            CandidateScope {
                project_id: project_id.clone(),
                working_directory: working_directory.clone(),
                task_id: task_id.clone(),
                module: module.clone(),
            },
        ),
    }
}

fn handle_candidate_add(
    input: Option<&str>,
    input_file: Option<&PathBuf>,
    scope: CandidateScope,
    note: Option<String>,
) -> Result<()> {
    if scope.project_id.is_none() && scope.working_directory.is_none() {
        return Err(CliError::InvalidInput(
            "Deferred candidates must be scoped to at least --project-id or --working-directory."
                .to_string(),
        ));
    }

    let request: MemoryScoreRequest = read_json_input(input, input_file)?;
    let score = score_candidate(&request);
    if score.action != MemoryAction::Defer {
        return Err(CliError::InvalidInput(format!(
            "Candidate scored as {:?}; only deferred candidates should be added to the local store.",
            score.action
        )));
    }

    let now = Utc::now().to_rfc3339();
    let record = DeferredCandidateRecord {
        id: candidate_id(&request, &scope),
        created_at: now.clone(),
        updated_at: now,
        scope,
        note,
        request,
        score,
    };

    let path = candidate_store_path()?;
    let mut store = load_candidate_store(&path)?;
    let (record, updated_existing) = upsert_candidate(&mut store, record);
    save_candidate_store(&path, &store)?;

    format::print_json(&CandidateAddResponse {
        stored: true,
        updated_existing,
        record,
    });
    Ok(())
}

fn handle_candidate_list(scope: CandidateScope) -> Result<()> {
    let path = candidate_store_path()?;
    let store = load_candidate_store(&path)?;
    let mut items: Vec<DeferredCandidateRecord> = store
        .candidates
        .into_iter()
        .filter(|candidate| scope_matches(&candidate.scope, &scope))
        .collect();
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });

    format::print_json(&CandidateListResponse {
        total: items.len(),
        items,
    });
    Ok(())
}

fn handle_candidate_review(scope: CandidateScope) -> Result<()> {
    let path = candidate_store_path()?;
    let store = load_candidate_store(&path)?;
    let mut promote_now = Vec::new();
    let mut keep_deferred = Vec::new();
    let mut drop = Vec::new();
    let mut session_end_contract = SessionEndContract::default();
    let mut clear_after_session_end_ids = Vec::new();

    for candidate in store
        .candidates
        .into_iter()
        .filter(|candidate| scope_matches(&candidate.scope, &scope))
    {
        let reviewed = review_candidate(candidate);
        match reviewed.disposition {
            SessionEndDisposition::Learned => {
                session_end_contract.learned.push(SessionEndLearnedItem {
                    fact: reviewed.content.clone(),
                    confidence: reviewed.confidence,
                    task_id: reviewed.scope.task_id.clone(),
                    module: reviewed.scope.module.clone(),
                });
                clear_after_session_end_ids.push(reviewed.id.clone());
                promote_now.push(reviewed);
            }
            SessionEndDisposition::DecisionsMade => {
                session_end_contract
                    .decisions_made
                    .push(SessionEndDecisionItem {
                        decision: reviewed.content.clone(),
                        task_id: reviewed.scope.task_id.clone(),
                        module: reviewed.scope.module.clone(),
                    });
                clear_after_session_end_ids.push(reviewed.id.clone());
                promote_now.push(reviewed);
            }
            SessionEndDisposition::Remaining => {
                session_end_contract
                    .remaining
                    .push(SessionEndRemainingItem {
                        description: reviewed.content.clone(),
                        task_id: reviewed.scope.task_id.clone(),
                        module: reviewed.scope.module.clone(),
                    });
                keep_deferred.push(reviewed);
            }
            SessionEndDisposition::Drop => {
                clear_after_session_end_ids.push(reviewed.id.clone());
                drop.push(reviewed);
            }
        }
    }

    let total = promote_now.len() + keep_deferred.len() + drop.len();
    format::print_json(&CandidateReviewResponse {
        total,
        promote_now,
        keep_deferred,
        drop,
        session_end_contract,
        clear_after_session_end_ids,
    });
    Ok(())
}

fn handle_candidate_clear(all: bool, ids: &[String], scope: CandidateScope) -> Result<()> {
    if !all && ids.is_empty() && scope == CandidateScope::default() {
        return Err(CliError::InvalidInput(
            "Refusing to clear every candidate without --all, --id, or at least one scope filter."
                .to_string(),
        ));
    }

    let path = candidate_store_path()?;
    let mut store = load_candidate_store(&path)?;
    let before = store.candidates.len();
    if all {
        store.candidates.clear();
    } else {
        store
            .candidates
            .retain(|candidate| !candidate_matches(candidate, ids, &scope));
    }
    let removed = before - store.candidates.len();
    save_candidate_store(&path, &store)?;

    format::print_json(&CandidateClearResponse {
        removed,
        remaining: store.candidates.len(),
    });
    Ok(())
}

fn read_json_input<T: DeserializeOwned>(
    input: Option<&str>,
    input_file: Option<&PathBuf>,
) -> Result<T> {
    if let Some(raw) = input {
        return parse_json(raw, "--input");
    }

    if let Some(path) = input_file {
        if path.as_os_str() == "-" {
            return parse_json(&read_stdin()?, "stdin");
        }
        let raw = fs::read_to_string(path)?;
        return parse_json(&raw, &path.display().to_string());
    }

    if !io::stdin().is_terminal() {
        return parse_json(&read_stdin()?, "stdin");
    }

    Err(CliError::InvalidInput(
        "Provide JSON input via --input, --input-file, or stdin.".to_string(),
    ))
}

fn read_stdin() -> Result<String> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    Ok(raw)
}

fn parse_json<T: DeserializeOwned>(raw: &str, source: &str) -> Result<T> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliError::InvalidInput(format!(
            "No JSON input received from {source}."
        )));
    }
    serde_json::from_str(trimmed).map_err(|err| {
        CliError::InvalidInput(format!("Invalid memory score JSON from {source}: {err}"))
    })
}

pub fn score_candidate(request: &MemoryScoreRequest) -> MemoryScoreResponse {
    let signals = derive_signals(request);
    let richness = round_score(score_richness(&signals));
    let promotion_readiness = round_score(score_promotion_readiness(&signals));
    let action = if request.explicit_user_request {
        MemoryAction::WriteNow
    } else if richness >= 0.70 && promotion_readiness >= 0.65 {
        MemoryAction::WriteNow
    } else if richness >= 0.55 {
        MemoryAction::Defer
    } else {
        MemoryAction::Drop
    };

    MemoryScoreResponse {
        action,
        bypassed: request.explicit_user_request,
        richness,
        promotion_readiness,
        reasons: build_reasons(request, &signals, richness, promotion_readiness, action),
    }
}

fn derive_signals(request: &MemoryScoreRequest) -> ScoredSignals {
    let confidence = clamp01(request.confidence.unwrap_or(0.5));
    let support_boost = (request.supporting_fact_count.unwrap_or(0).min(3) as f32) * 0.10;
    let inferred_hypothesis = infer_hypothesis(&request.content);
    let open_hypothesis = request.open_hypothesis || inferred_hypothesis;
    let likely_resolved =
        request.likely_resolved_this_session || infer_short_lived(&request.content);
    let defaults = defaults_for_request(request, open_hypothesis, likely_resolved);

    let atomicity = resolve_signal(
        request.atomicity,
        atomicity_hint(&request.content).max(defaults.atomicity),
    );

    let evidence_quality = resolve_signal(
        request.evidence_quality,
        (confidence + support_boost).clamp(0.0, 1.0),
    );
    let evidence_completeness = resolve_signal(
        request.evidence_completeness,
        (confidence * 0.85 + support_boost).clamp(0.0, 1.0),
    );
    let stability = resolve_signal(
        request.stability,
        if open_hypothesis {
            0.25
        } else {
            confidence.max(match request.kind {
                MemoryCandidateKind::Scratch => 0.30,
                _ => 0.55,
            })
        },
    );
    let wording_readiness = resolve_signal(
        request.wording_readiness,
        wording_hint(&request.content).max(atomicity * 0.85),
    );
    let scope_binding = resolve_signal(request.scope_binding, scope_binding_hint(request));
    let session_timing = resolve_signal(
        request.session_timing,
        if request.at_session_end { 0.85 } else { 0.40 },
    );
    let decision_finality = resolve_signal(
        request.decision_finality,
        if open_hypothesis {
            0.35
        } else if request.supporting_fact_count.unwrap_or(0) > 0 {
            defaults.decision_finality.max(0.75)
        } else {
            defaults.decision_finality
        },
    );

    ScoredSignals {
        durability: resolve_signal(request.durability, defaults.durability),
        reuse_breadth: resolve_signal(request.reuse_breadth, defaults.reuse_breadth),
        rediscovery_cost: resolve_signal(request.rediscovery_cost, defaults.rediscovery_cost),
        impact_if_forgotten: resolve_signal(
            request.impact_if_forgotten,
            defaults.impact_if_forgotten,
        ),
        evidence_quality,
        atomicity,
        evidence_completeness,
        stability,
        wording_readiness,
        scope_binding,
        session_timing,
        decision_finality,
        volatile: request.volatile,
        obvious_from_code: request.obvious_from_code,
        duplicate: request.duplicate,
        open_hypothesis,
        likely_resolved_this_session: likely_resolved,
        weak_support: request.weak_support,
    }
}

fn score_richness(signals: &ScoredSignals) -> f32 {
    let mut score = 0.25 * signals.durability
        + 0.20 * signals.reuse_breadth
        + 0.20 * signals.rediscovery_cost
        + 0.15 * signals.impact_if_forgotten
        + 0.10 * signals.evidence_quality
        + 0.10 * signals.atomicity;

    if signals.volatile {
        score -= 0.20;
    }
    if signals.obvious_from_code {
        score -= 0.15;
    }
    if signals.duplicate {
        score -= 0.15;
    }

    clamp01(score)
}

fn score_promotion_readiness(signals: &ScoredSignals) -> f32 {
    let mut score = 0.30 * signals.evidence_completeness
        + 0.25 * signals.stability
        + 0.15 * signals.wording_readiness
        + 0.10 * signals.scope_binding
        + 0.10 * signals.session_timing
        + 0.10 * signals.decision_finality;

    if signals.open_hypothesis {
        score -= 0.35;
    }
    if signals.likely_resolved_this_session {
        score -= 0.25;
    }
    if signals.weak_support {
        score -= 0.15;
    }

    clamp01(score)
}

fn build_reasons(
    request: &MemoryScoreRequest,
    signals: &ScoredSignals,
    richness: f32,
    promotion_readiness: f32,
    action: MemoryAction,
) -> Vec<String> {
    let mut ranked: Vec<(f32, String)> = Vec::new();

    if request.explicit_user_request {
        ranked.push((1.0, "explicit user request bypasses the gate".to_string()));
    }
    if signals.durability >= 0.75 {
        ranked.push((
            signals.durability,
            "strong durability beyond this session".to_string(),
        ));
    }
    if signals.reuse_breadth >= 0.75 {
        ranked.push((
            signals.reuse_breadth,
            "likely to help future sessions or agents".to_string(),
        ));
    }
    if signals.rediscovery_cost >= 0.75 {
        ranked.push((
            signals.rediscovery_cost,
            "costly to rediscover from code or logs".to_string(),
        ));
    }
    if signals.evidence_quality >= 0.75 || signals.evidence_completeness >= 0.75 {
        ranked.push((
            signals.evidence_quality.max(signals.evidence_completeness),
            "well-supported rather than speculative".to_string(),
        ));
    }
    if signals.scope_binding >= 0.75 {
        ranked.push((
            signals.scope_binding,
            "cleanly scoped to the active project/workdir context".to_string(),
        ));
    }
    if signals.open_hypothesis {
        ranked.push((0.95, "still an open hypothesis".to_string()));
    }
    if signals.likely_resolved_this_session {
        ranked.push((0.85, "likely to change before the session ends".to_string()));
    }
    if signals.volatile {
        ranked.push((
            0.80,
            "too volatile for durable memory right now".to_string(),
        ));
    }
    if signals.obvious_from_code {
        ranked.push((0.75, "cheap to rediscover directly from code".to_string()));
    }
    if signals.duplicate {
        ranked.push((0.75, "duplicates existing durable memory".to_string()));
    }
    if signals.weak_support {
        ranked.push((0.70, "supporting evidence is still weak".to_string()));
    }

    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut reasons: Vec<String> = ranked
        .into_iter()
        .map(|(_, reason)| reason)
        .take(3)
        .collect();

    if reasons.is_empty() {
        reasons.push(match action {
            MemoryAction::WriteNow => format!(
                "richness {:.2} and promotion readiness {:.2} both clear the write thresholds",
                richness, promotion_readiness
            ),
            MemoryAction::Defer => format!(
                "richness {:.2} is high enough to keep, but promotion readiness {:.2} is still too low",
                richness, promotion_readiness
            ),
            MemoryAction::Drop => format!(
                "richness {:.2} is below the durable-memory threshold",
                richness
            ),
        });
    }

    reasons
}

fn defaults_for_request(
    request: &MemoryScoreRequest,
    open_hypothesis: bool,
    likely_resolved: bool,
) -> CandidateDefaults {
    match request.kind {
        MemoryCandidateKind::Fact if infer_rule_like(&request.content) => CandidateDefaults {
            durability: 0.82,
            reuse_breadth: 0.76,
            rediscovery_cost: 0.78,
            impact_if_forgotten: 0.72,
            atomicity: 0.65,
            decision_finality: 0.60,
        },
        MemoryCandidateKind::Fact => CandidateDefaults {
            durability: 0.65,
            reuse_breadth: 0.60,
            rediscovery_cost: 0.55,
            impact_if_forgotten: 0.55,
            atomicity: 0.60,
            decision_finality: 0.55,
        },
        MemoryCandidateKind::Decision if !open_hypothesis && !likely_resolved => {
            CandidateDefaults {
                durability: 0.85,
                reuse_breadth: 0.80,
                rediscovery_cost: 0.72,
                impact_if_forgotten: 0.78,
                atomicity: 0.62,
                decision_finality: 0.82,
            }
        }
        MemoryCandidateKind::Decision => CandidateDefaults {
            durability: 0.78,
            reuse_breadth: 0.75,
            rediscovery_cost: 0.60,
            impact_if_forgotten: 0.72,
            atomicity: 0.60,
            decision_finality: 0.72,
        },
        MemoryCandidateKind::Scratch => CandidateDefaults {
            durability: 0.20,
            reuse_breadth: 0.15,
            rediscovery_cost: 0.25,
            impact_if_forgotten: 0.20,
            atomicity: 0.45,
            decision_finality: 0.25,
        },
    }
}

fn scope_binding_hint(request: &MemoryScoreRequest) -> f32 {
    let root_count = request.project_known as u8 + request.workdir_known as u8;
    let deep_scope = request.task_known || request.module_known;
    match (root_count, deep_scope) {
        (2, true) => 0.90,
        (2, false) => 0.78,
        (1, true) => 0.75,
        (1, false) => 0.60,
        (0, true) => 0.50,
        _ => 0.35,
    }
}

fn atomicity_hint(content: &str) -> f32 {
    let words = content.split_whitespace().count();
    match words {
        0..=4 => 0.35,
        5..=24 => 0.75,
        25..=40 => 0.60,
        _ => 0.45,
    }
}

fn wording_hint(content: &str) -> f32 {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return 0.20_f32;
    }
    let ends_cleanly = trimmed.ends_with('.') || !trimmed.ends_with(',');
    let has_pathological_markers = infer_hypothesis(content);
    let base: f32 = if ends_cleanly { 0.70_f32 } else { 0.55_f32 };
    if has_pathological_markers {
        (base - 0.20_f32).max(0.20_f32)
    } else {
        base
    }
}

fn infer_hypothesis(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "maybe ",
        "might ",
        "probably ",
        "suspect",
        "need to ",
        "check if",
        "verify ",
        "investigate ",
        "likely ",
        "appears to ",
        "seems ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn infer_short_lived(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "next",
        "for now",
        "this session",
        "follow up",
        "inspect",
        "open ",
        "look at ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn infer_rule_like(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "must ",
        "requires ",
        "require ",
        "do not ",
        "don't ",
        "always ",
        "never ",
        "workflow",
        "convention",
        "rule",
        "only ",
        "cannot ",
        "can't ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn resolve_signal(explicit: Option<f32>, default: f32) -> f32 {
    clamp01(explicit.unwrap_or(default))
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn round_score(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

fn candidate_store_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join("memory-candidates.json"))
}

fn load_candidate_store(path: &PathBuf) -> Result<CandidateStore> {
    if !path.exists() {
        return Ok(CandidateStore {
            version: 1,
            candidates: Vec::new(),
        });
    }

    let raw = fs::read_to_string(path)?;
    let mut store: CandidateStore = serde_json::from_str(&raw).map_err(|err| {
        CliError::Config(format!(
            "Could not parse deferred candidate store at {}: {err}",
            path.display()
        ))
    })?;
    if store.version == 0 {
        store.version = 1;
    }
    Ok(store)
}

fn save_candidate_store(path: &PathBuf, store: &CandidateStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = serde_json::to_string_pretty(store)?;
    fs::write(path, raw)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

fn upsert_candidate(
    store: &mut CandidateStore,
    record: DeferredCandidateRecord,
) -> (DeferredCandidateRecord, bool) {
    if let Some(existing) = store
        .candidates
        .iter_mut()
        .find(|candidate| candidate.id == record.id)
    {
        existing.updated_at = record.updated_at.clone();
        existing.scope = record.scope.clone();
        existing.note = record.note.clone();
        existing.request = record.request.clone();
        existing.score = record.score.clone();
        return (existing.clone(), true);
    }

    store.candidates.push(record.clone());
    (record, false)
}

fn review_candidate(record: DeferredCandidateRecord) -> ReviewedCandidate {
    let DeferredCandidateRecord {
        id,
        scope,
        note,
        request,
        score,
        ..
    } = record;

    let mut session_end_request = request.clone();
    session_end_request.at_session_end = true;
    let session_end_score = score_candidate(&session_end_request);

    let disposition = match session_end_score.action {
        MemoryAction::WriteNow => match request.kind {
            MemoryCandidateKind::Decision => SessionEndDisposition::DecisionsMade,
            MemoryCandidateKind::Fact | MemoryCandidateKind::Scratch => {
                SessionEndDisposition::Learned
            }
        },
        MemoryAction::Defer => SessionEndDisposition::Remaining,
        MemoryAction::Drop => SessionEndDisposition::Drop,
    };

    ReviewedCandidate {
        id,
        kind: request.kind,
        content: request.content,
        confidence: request.confidence,
        scope,
        note,
        disposition,
        previous_score: score,
        session_end_score,
    }
}

fn scope_matches(candidate: &CandidateScope, filter: &CandidateScope) -> bool {
    matches_filter_field(&candidate.project_id, &filter.project_id)
        && matches_filter_field(&candidate.working_directory, &filter.working_directory)
        && matches_filter_field(&candidate.task_id, &filter.task_id)
        && matches_filter_field(&candidate.module, &filter.module)
}

fn candidate_matches(
    candidate: &DeferredCandidateRecord,
    ids: &[String],
    scope: &CandidateScope,
) -> bool {
    let id_matches = ids.is_empty() || ids.iter().any(|id| id == &candidate.id);
    id_matches && scope_matches(&candidate.scope, scope)
}

fn matches_filter_field(candidate: &Option<String>, filter: &Option<String>) -> bool {
    match filter {
        Some(expected) => candidate.as_ref() == Some(expected),
        None => true,
    }
}

fn candidate_id(request: &MemoryScoreRequest, scope: &CandidateScope) -> String {
    let mut hasher = Sha256::new();
    hasher.update(match request.kind {
        MemoryCandidateKind::Fact => "fact",
        MemoryCandidateKind::Decision => "decision",
        MemoryCandidateKind::Scratch => "scratch",
    });
    hasher.update(request.content.trim().as_bytes());
    for value in [
        scope.project_id.as_deref(),
        scope.working_directory.as_deref(),
        scope.task_id.as_deref(),
        scope.module.as_deref(),
    ] {
        hasher.update(b"|");
        hasher.update(value.unwrap_or(""));
    }
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    format!("cand_{}", &hex[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: MemoryCandidateKind, content: &str) -> MemoryScoreRequest {
        MemoryScoreRequest {
            kind,
            content: content.to_string(),
            confidence: Some(0.8),
            explicit_user_request: false,
            durability: None,
            reuse_breadth: None,
            rediscovery_cost: None,
            impact_if_forgotten: None,
            evidence_quality: None,
            atomicity: None,
            evidence_completeness: None,
            stability: None,
            wording_readiness: None,
            scope_binding: None,
            session_timing: None,
            decision_finality: None,
            project_known: true,
            workdir_known: true,
            task_known: false,
            module_known: false,
            at_session_end: false,
            volatile: false,
            obvious_from_code: false,
            duplicate: false,
            open_hypothesis: false,
            likely_resolved_this_session: false,
            weak_support: false,
            supporting_fact_count: Some(2),
        }
    }

    #[test]
    fn explicit_user_requests_bypass_the_gate() {
        let mut input = request(
            MemoryCandidateKind::Scratch,
            "Remember this: the user wants this kept even if it looks temporary.",
        );
        input.explicit_user_request = true;

        let result = score_candidate(&input);
        assert_eq!(result.action, MemoryAction::WriteNow);
        assert!(result.bypassed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("bypasses")));
    }

    #[test]
    fn unresolved_high_value_items_defer() {
        let mut input = request(
            MemoryCandidateKind::Decision,
            "JWT is probably the right choice, but threat-model review is still pending.",
        );
        input.at_session_end = false;
        input.supporting_fact_count = Some(1);

        let result = score_candidate(&input);
        assert_eq!(result.action, MemoryAction::Defer);
        assert!(result.richness >= 0.55);
        assert!(result.promotion_readiness < 0.65);
    }

    #[test]
    fn short_lived_navigation_notes_drop() {
        let mut input = request(
            MemoryCandidateKind::Scratch,
            "Need to inspect src/mcp/server.ts next.",
        );
        input.confidence = Some(0.4);
        input.at_session_end = false;

        let result = score_candidate(&input);
        assert_eq!(result.action, MemoryAction::Drop);
        assert!(result.richness < 0.55);
    }

    #[test]
    fn session_end_review_promotes_ready_candidates() {
        let mut input = request(
            MemoryCandidateKind::Decision,
            "Adopt the deferred candidate review helper as the shared wrap-up path.",
        );
        input.confidence = Some(0.7);
        input.evidence_completeness = Some(0.62);
        input.stability = Some(0.65);
        input.wording_readiness = Some(0.62);
        input.supporting_fact_count = None;

        let initial_score = score_candidate(&input);
        assert_eq!(initial_score.action, MemoryAction::Defer);

        let record = DeferredCandidateRecord {
            id: "cand_test123".to_string(),
            created_at: "2026-04-21T00:00:00Z".to_string(),
            updated_at: "2026-04-21T00:00:00Z".to_string(),
            scope: CandidateScope {
                project_id: Some("proj-123".to_string()),
                working_directory: Some("/tmp/work".to_string()),
                task_id: Some("TKB-79".to_string()),
                module: Some("memory-gate".to_string()),
            },
            note: Some("promote at wrap-up".to_string()),
            request: input,
            score: initial_score,
        };

        let reviewed = review_candidate(record);
        assert_eq!(reviewed.disposition, SessionEndDisposition::DecisionsMade);
        assert_eq!(reviewed.session_end_score.action, MemoryAction::WriteNow);
        assert!(reviewed.session_end_score.promotion_readiness >= 0.65);
    }

    #[test]
    fn session_end_review_keeps_open_hypotheses_in_remaining() {
        let mut input = request(
            MemoryCandidateKind::Decision,
            "JWT is probably the right choice, but threat-model review is still pending.",
        );
        input.supporting_fact_count = Some(1);

        let initial_score = score_candidate(&input);
        assert_eq!(initial_score.action, MemoryAction::Defer);

        let record = DeferredCandidateRecord {
            id: "cand_remaining".to_string(),
            created_at: "2026-04-21T00:00:00Z".to_string(),
            updated_at: "2026-04-21T00:00:00Z".to_string(),
            scope: CandidateScope {
                project_id: Some("proj-123".to_string()),
                working_directory: Some("/tmp/work".to_string()),
                task_id: Some("TKB-76".to_string()),
                module: Some("memory-gate".to_string()),
            },
            note: Some("still pending review".to_string()),
            request: input,
            score: initial_score,
        };

        let reviewed = review_candidate(record);
        assert_eq!(reviewed.disposition, SessionEndDisposition::Remaining);
        assert_eq!(reviewed.session_end_score.action, MemoryAction::Defer);
        assert!(reviewed.session_end_score.promotion_readiness < 0.65);
    }

    #[test]
    fn candidate_matches_honors_ids_and_scope_filters() {
        let candidate = DeferredCandidateRecord {
            id: "cand_match".to_string(),
            created_at: "2026-04-21T00:00:00Z".to_string(),
            updated_at: "2026-04-21T00:00:00Z".to_string(),
            scope: CandidateScope {
                project_id: Some("proj-123".to_string()),
                working_directory: Some("/tmp/work".to_string()),
                task_id: None,
                module: Some("memory-gate".to_string()),
            },
            note: None,
            request: request(MemoryCandidateKind::Fact, "Keep deploy rules durable."),
            score: MemoryScoreResponse {
                action: MemoryAction::Defer,
                bypassed: false,
                richness: 0.7,
                promotion_readiness: 0.6,
                reasons: vec!["high richness".to_string()],
            },
        };

        assert!(candidate_matches(
            &candidate,
            &["cand_match".to_string()],
            &CandidateScope {
                project_id: Some("proj-123".to_string()),
                working_directory: None,
                task_id: None,
                module: None,
            }
        ));
        assert!(!candidate_matches(
            &candidate,
            &["cand_other".to_string()],
            &CandidateScope::default()
        ));
        assert!(!candidate_matches(
            &candidate,
            &["cand_match".to_string()],
            &CandidateScope {
                project_id: Some("proj-999".to_string()),
                working_directory: None,
                task_id: None,
                module: None,
            }
        ));
    }
}
