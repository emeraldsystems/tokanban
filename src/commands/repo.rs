//! `tokanban repo` — local Git checkout/worktree discovery and explicit
//! repository-memory binding, per `spec/REPOSITORY_MEMORY.md` (TKB-129).
//!
//! `repo inspect` is read-only and offline by default: it never touches the
//! network or requires credentials, only read-only `git` plumbing commands
//! run through `std::process::Command` (no shell interpolation, no config or
//! checkout mutation). Passing `--binding` opts into an authenticated lookup
//! of the current repository-memory binding for the inspected directory.
//! `create`, `list`, `bind`, and `unbind` are explicit REST actions against
//! `/v1/memory/repositories` and `/v1/memory/checkouts` and always require
//! normal CLI auth/config. Nothing here ever infers a binding from a matching
//! folder name or remote URL — identity is always explicit (repository ID).
//!
//! `scope preview/apply/restore` (TKB-130 historical association, CLI surface
//! authorized under TKB-131) call `/v1/memory/scopes/*`, matching
//! `src/shared/memory-association.ts` exactly. Preview never writes. Apply
//! only ever resends a plan file previously produced by `scope preview
//! --format json`, so it can never silently re-preview and apply in the same
//! step — the reviewed selection and fingerprint are always what gets sent.
//!
//! All revision-like counters (`expected_revision`, checkout/repository
//! `revision`) are strictly typed as non-negative integers end to end, so an
//! invalid revision is rejected by argument parsing before any network call.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ctx::Ctx;
use crate::error::{CliError, Result};
use crate::format::table::{render_table, Column};
use crate::format::{self, colors, ColorConfig, OutputFormat, EM_DASH};

// ---------------------------------------------------------------------------
// CLI surface
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum RepoCommand {
    /// Inspect local Git checkout metadata (read-only, offline by default)
    Inspect {
        /// Directory to inspect (defaults to the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Also fetch the current repository-memory binding for this directory (requires auth)
        #[arg(long)]
        binding: bool,
    },
    /// Create a new memory repository identity
    Create {
        /// Repository name
        name: String,
        /// Canonical remote URL to associate (credentials/query/fragment are stripped)
        #[arg(long)]
        remote: Option<String>,
        /// Associate with a Tokanban project (key, name, or ID)
        #[arg(long)]
        project: Option<String>,
    },
    /// List memory repositories visible to the caller
    List {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Bind (or rebind) a working directory to a memory repository
    Bind {
        /// Repository ID to bind
        repository_id: String,
        /// Working directory to bind (defaults to the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Branch to record for this checkout (defaults to the discovered branch, if any)
        #[arg(long)]
        branch: Option<String>,
        /// Checkout kind: main, worktree, clone, or unknown (defaults to the discovered kind)
        #[arg(long)]
        kind: Option<String>,
        /// Required current revision (non-negative integer); omit (or 0) for the first bind
        #[arg(long = "expected-revision", value_parser = parse_revision)]
        expected_revision: Option<u64>,
    },
    /// Deactivate the binding for a working directory
    Unbind {
        /// Working directory to unbind (defaults to the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Required current revision of the active binding (non-negative integer)
        #[arg(long = "expected-revision", value_parser = parse_positive_revision)]
        expected_revision: u64,
    },
    /// Show checkout binding history for a working directory
    History {
        /// Checkout ID (looked up from --path when omitted)
        #[arg(long)]
        checkout_id: Option<String>,
        /// Working directory to resolve a checkout ID from (defaults to the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Compare local discovery against existing repositories for manual review
    ///
    /// Never binds automatically: matches by name or remote are shown for the
    /// human/agent to confirm with an explicit `repo bind`.
    Aliases {
        /// Directory to compare (defaults to the current directory)
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Repository-shared memory scope operations (historical promotion)
    #[command(subcommand)]
    Scope(ScopeCommand),
}

/// `repo scope preview/apply/restore` — explicit, reviewable promotion of
/// existing facts/decisions into repository-shared (or branch/workdir/
/// experiment) scope. Matches `src/shared/memory-association.ts` exactly.
#[derive(Debug, Subcommand)]
pub enum ScopeCommand {
    /// Preview a repository-scope change for explicit memory IDs (read-only, makes no writes)
    Preview {
        /// Repository ID that will own the shared scope
        #[arg(long)]
        repository_id: String,
        /// Fact/decision memory ID to include; repeat for multiple (1-50 total)
        #[arg(long = "memory-id", required = true)]
        memory_ids: Vec<String>,
        /// Destination scope: repository, branch, workdir, or experiment (default: repository)
        #[arg(long)]
        scope: Option<String>,
        /// Branch name (used by --scope branch)
        #[arg(long)]
        branch: Option<String>,
        /// Experiment name (used by --scope experiment)
        #[arg(long)]
        experiment: Option<String>,
    },
    /// Apply a previously reviewed scope change
    ///
    /// Never previews implicitly: requires a plan file produced by
    /// `repo scope preview --format json`, so the applied selection and
    /// fingerprint are always exactly what was already shown to the user.
    Apply {
        /// Plan file written from `repo scope preview --format json`
        #[arg(long)]
        plan: PathBuf,
    },
    /// Restore the scopes from before a prior scope operation
    Restore {
        /// Scope operation ID to restore
        operation_id: String,
    },
}

pub async fn handle(cmd: &RepoCommand, ctx: &Ctx) -> Result<()> {
    match cmd {
        RepoCommand::Inspect { path, binding } => handle_inspect(ctx, path.clone(), *binding).await,
        RepoCommand::Create {
            name,
            remote,
            project,
        } => handle_create(ctx, name, remote.as_deref(), project.clone()).await,
        RepoCommand::List { limit, offset } => handle_list(ctx, *limit, *offset).await,
        RepoCommand::Bind {
            repository_id,
            path,
            branch,
            kind,
            expected_revision,
        } => {
            handle_bind(
                ctx,
                repository_id,
                path.clone(),
                branch.as_deref(),
                kind.as_deref(),
                *expected_revision,
            )
            .await
        }
        RepoCommand::Unbind {
            path,
            expected_revision,
        } => handle_unbind(ctx, path.clone(), *expected_revision).await,
        RepoCommand::History {
            checkout_id,
            path,
            limit,
            offset,
        } => handle_history(ctx, checkout_id.clone(), path.clone(), *limit, *offset).await,
        RepoCommand::Aliases { path } => handle_aliases(ctx, path.clone()).await,
        RepoCommand::Scope(cmd) => handle_scope(ctx, cmd).await,
    }
}

async fn handle_scope(ctx: &Ctx, cmd: &ScopeCommand) -> Result<()> {
    match cmd {
        ScopeCommand::Preview {
            repository_id,
            memory_ids,
            scope,
            branch,
            experiment,
        } => {
            handle_scope_preview(
                ctx,
                repository_id,
                memory_ids,
                scope.as_deref(),
                branch.as_deref(),
                experiment.as_deref(),
            )
            .await
        }
        ScopeCommand::Apply { plan } => handle_scope_apply(ctx, plan).await,
        ScopeCommand::Restore { operation_id } => handle_scope_restore(ctx, operation_id).await,
    }
}

// ---------------------------------------------------------------------------
// Offline entry point (used by main.rs before any config/auth is loaded)
// ---------------------------------------------------------------------------

/// Fully offline `repo inspect` (no `--binding`): local Git discovery only.
/// Never touches the network or requires a config/token, mirroring `doctor`.
pub fn handle_inspect_offline(
    path: Option<PathBuf>,
    format: OutputFormat,
    no_color: bool,
) -> Result<()> {
    let report = build_offline_report(path)?;
    let color = ColorConfig::new(no_color);
    print_inspection(&report, format, &color);
    Ok(())
}

fn build_offline_report(path: Option<PathBuf>) -> Result<RepoInspectionReport> {
    let target = resolve_target_dir(path)?;
    let git = inspect_git(&target, &SystemGitRunner);
    Ok(RepoInspectionReport {
        path: target.display().to_string(),
        git,
        binding: None,
    })
}

/// Authenticated `repo inspect` dispatch: always does the same offline Git
/// discovery, and only reaches the network when `binding` is explicitly set.
async fn handle_inspect(ctx: &Ctx, path: Option<PathBuf>, binding: bool) -> Result<()> {
    let mut report = build_offline_report(path)?;
    if binding {
        let checkout = lookup_checkout(ctx, &report.path, false).await?;
        report.binding = Some(match checkout {
            Some(c) => BindingLookup::Bound(c),
            None => BindingLookup::None,
        });
    }
    print_inspection(&report, ctx.format, &ctx.color);
    Ok(())
}

// ---------------------------------------------------------------------------
// Git discovery (read-only, injectable runner for tests)
// ---------------------------------------------------------------------------

/// Output of a single read-only `git` invocation.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Abstraction over running a read-only `git` subcommand, so tests can
/// simulate "git not installed" without touching the process `PATH`.
pub trait GitRunner {
    fn run(&self, cwd: &Path, args: &[&str]) -> std::io::Result<GitOutput>;
}

/// Runs real `git` via `std::process::Command` with explicit argv (no shell,
/// no interpolation). Only ever invoked with read-only plumbing commands.
pub struct SystemGitRunner;

impl GitRunner for SystemGitRunner {
    fn run(&self, cwd: &Path, args: &[&str]) -> std::io::Result<GitOutput> {
        let output = ProcessCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .output()?;
        Ok(GitOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Discovered Git checkout metadata for a directory.
#[derive(Debug, Clone, Serialize)]
pub struct GitCheckoutInfo {
    /// Working tree root (`git rev-parse --show-toplevel`), canonical absolute.
    pub repo_root: String,
    /// This checkout's git dir (`git rev-parse --git-dir`), canonical absolute.
    pub git_dir: String,
    /// The shared git dir for the repo family (`git rev-parse --git-common-dir`).
    pub common_git_dir: String,
    /// Best-effort classification: "main" or "worktree" (derived from
    /// whether `git_dir` and `common_git_dir` differ). Never "clone" or
    /// "unknown" — those are only assigned by explicit `--kind` on bind.
    pub checkout_kind: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub head_commit: Option<String>,
    pub remote_name: Option<String>,
    /// Normalized for safe display: credentials, query, and fragment stripped.
    pub remote_url: Option<String>,
}

/// Result of local Git discovery for a directory.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GitReport {
    /// The `git` executable could not be run at all.
    NotAvailable { detail: String },
    /// `git` ran but the directory is not inside a Git working tree.
    NotARepository { detail: String },
    /// Directory is inside a Git working tree.
    Repository(GitCheckoutInfo),
}

/// Run read-only `git` plumbing to discover checkout metadata for `cwd`.
/// Never mutates Git config, never checks out a branch, never fetches.
pub fn inspect_git(cwd: &Path, runner: &dyn GitRunner) -> GitReport {
    let probe = match runner.run(cwd, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(output) => output,
        Err(_) => {
            return GitReport::NotAvailable {
                detail: "git executable was not found on PATH.".to_string(),
            }
        }
    };
    if !probe.success || probe.stdout.trim() != "true" {
        return GitReport::NotARepository {
            detail: "Directory is not inside a Git working tree.".to_string(),
        };
    }

    let repo_root = run_ok(runner, cwd, &["rev-parse", "--show-toplevel"])
        .map(|s| resolve_git_path(cwd, &s))
        .unwrap_or_else(|| cwd.display().to_string());

    let git_dir = run_ok(runner, cwd, &["rev-parse", "--git-dir"])
        .map(|s| resolve_git_path(cwd, &s))
        .unwrap_or_default();
    let common_git_dir = run_ok(runner, cwd, &["rev-parse", "--git-common-dir"])
        .map(|s| resolve_git_path(cwd, &s))
        .unwrap_or_else(|| git_dir.clone());

    let checkout_kind =
        if !git_dir.is_empty() && !common_git_dir.is_empty() && git_dir != common_git_dir {
            "worktree"
        } else {
            "main"
        }
        .to_string();

    let (branch, detached) = match runner.run(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])
    {
        Ok(out) if out.success && !out.stdout.trim().is_empty() => {
            (Some(out.stdout.trim().to_string()), false)
        }
        _ => (None, true),
    };

    let head_commit = run_ok(runner, cwd, &["rev-parse", "HEAD"]);

    let remotes: Vec<String> = run_ok(runner, cwd, &["remote"])
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let remote_name = remotes
        .iter()
        .find(|r| r.as_str() == "origin")
        .cloned()
        .or_else(|| remotes.first().cloned());
    let remote_url = match &remote_name {
        Some(name) => {
            run_ok(runner, cwd, &["remote", "get-url", name]).map(|raw| normalize_remote(&raw))
        }
        None => None,
    };

    GitReport::Repository(GitCheckoutInfo {
        repo_root,
        git_dir,
        common_git_dir,
        checkout_kind,
        branch,
        detached,
        head_commit,
        remote_name,
        remote_url,
    })
}

fn run_ok(runner: &dyn GitRunner, cwd: &Path, args: &[&str]) -> Option<String> {
    match runner.run(cwd, args) {
        Ok(out) if out.success => {
            let trimmed = out.stdout.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

/// Resolve a path returned by `git rev-parse` (which may be relative to
/// `cwd`, e.g. plain `.git`) to a canonical absolute string. No assumption is
/// made about repository layout beyond what `git` itself reported.
fn resolve_git_path(cwd: &Path, raw: &str) -> String {
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    std::fs::canonicalize(&joined)
        .unwrap_or(joined)
        .display()
        .to_string()
}

/// Resolve `--path` (or the current directory) to a canonical absolute
/// directory. Errors clearly if the path does not exist, rather than
/// silently misreporting Git as unavailable.
fn resolve_target_dir(path: Option<PathBuf>) -> Result<PathBuf> {
    let raw = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    let absolute = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()?.join(raw)
    };
    if !absolute.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "Path '{}' is not an existing directory.",
            absolute.display()
        )));
    }
    Ok(std::fs::canonicalize(&absolute).unwrap_or(absolute))
}

/// Strip credentials and display-only query/fragment metadata from a remote
/// URL for safe display. Supports SSH/HTTP(S)/Git URLs and scp-like SSH
/// syntax (`user@host:path`). Never resolves or fetches the remote.
pub fn normalize_remote(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    if trimmed.contains("://") {
        if let Ok(mut url) = url::Url::parse(trimmed) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            if !["https", "http", "ssh", "git", "file"].contains(&url.scheme()) {
                return "<unsupported remote>".to_string();
            }
            let path = url.path().trim_end_matches('/');
            let path = path.strip_suffix(".git").unwrap_or(path).to_string();
            url.set_path(&path);
            return url.to_string();
        }
        return "<unrecognized remote>".to_string();
    }

    // scp-like syntax: [user@]host:path (e.g. git@github.com:acme/widgets.git).
    if let Some(colon_idx) = trimmed.find(':') {
        let (host_part, rest) = trimmed.split_at(colon_idx);
        let path_part = &rest[1..];
        if !host_part.is_empty() && !path_part.is_empty() && !host_part.contains('/') {
            let host = host_part.rsplit('@').next().unwrap_or(host_part);
            return normalize_remote(&format!("ssh://{host}/{path_part}"));
        }
    }

    trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_string()
}

fn parse_revision(raw: &str) -> std::result::Result<u64, String> {
    match raw.parse::<u64>() {
        Ok(value) if value <= 9_007_199_254_740_991 => Ok(value),
        _ => Err("Revision must be a non-negative safe integer.".to_string()),
    }
}
fn parse_positive_revision(raw: &str) -> std::result::Result<u64, String> {
    let value = parse_revision(raw)?;
    if value == 0 {
        Err("Unbind requires a positive current revision.".to_string())
    } else {
        Ok(value)
    }
}

fn normalize_checkout_kind(raw: &str) -> Result<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "main" => Ok("main".to_string()),
        "worktree" => Ok("worktree".to_string()),
        "clone" => Ok("clone".to_string()),
        "unknown" => Ok("unknown".to_string()),
        other => Err(CliError::InvalidInput(format!(
            "Invalid checkout kind '{other}'. Use main, worktree, clone, or unknown."
        ))),
    }
}

const MAX_MEMORY_IDS: usize = 50;
const MAX_PLAN_FILE_BYTES: u64 = 1_000_000;
const KNOWN_MEMORY_SCOPES: [&str; 4] = ["repository", "branch", "workdir", "experiment"];

fn normalize_memory_scope(raw: Option<&str>) -> Result<String> {
    match raw.map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok("repository".to_string()),
        Some(s) if s.is_empty() => Ok("repository".to_string()),
        Some(s) if KNOWN_MEMORY_SCOPES.contains(&s.as_str()) => Ok(s),
        Some(other) => Err(CliError::InvalidInput(format!(
            "Invalid --scope '{other}'. Use repository, branch, workdir, or experiment."
        ))),
    }
}

fn validate_repository_id(repository_id: &str) -> Result<()> {
    if repository_id.trim().is_empty() {
        return Err(CliError::InvalidInput(
            "--repository-id must not be empty.".to_string(),
        ));
    }
    Ok(())
}

fn validate_memory_ids(memory_ids: &[String]) -> Result<()> {
    if memory_ids.is_empty() {
        return Err(CliError::InvalidInput(
            "Provide at least one --memory-id (1-50 allowed).".to_string(),
        ));
    }
    if memory_ids.len() > MAX_MEMORY_IDS {
        return Err(CliError::InvalidInput(format!(
            "Too many memory IDs ({}). Scope preview accepts 1-{MAX_MEMORY_IDS} per request.",
            memory_ids.len()
        )));
    }
    let mut seen = HashSet::new();
    for id in memory_ids {
        if id.trim().is_empty() {
            return Err(CliError::InvalidInput(
                "--memory-id values must not be empty.".to_string(),
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(CliError::InvalidInput(format!(
                "Duplicate --memory-id '{id}'. Each memory ID must be unique."
            )));
        }
    }
    Ok(())
}

fn validate_optional_text(value: Option<&str>, flag: &str) -> Result<()> {
    if let Some(v) = value {
        if v.trim().is_empty() {
            return Err(CliError::InvalidInput(format!(
                "{flag} must not be empty when provided."
            )));
        }
    }
    Ok(())
}

fn validate_fingerprint(fingerprint: &str) -> Result<()> {
    let valid = fingerprint.len() == 64
        && fingerprint
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if valid {
        Ok(())
    } else {
        Err(CliError::InvalidInput(
            "Plan file has an invalid preview fingerprint (expected 64 lowercase hex characters)."
                .to_string(),
        ))
    }
}

/// Read and structurally validate a scope plan file. Never echoes file
/// contents or underlying parse errors back to the user — only a bounded,
/// generic message — so a corrupt, huge, or unexpected file can't leak
/// content into CLI output.
fn read_scope_plan(path: &Path) -> Result<ScopePlan> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|_| {
        CliError::InvalidInput(format!("Could not read plan file '{}'.", path.display()))
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| CliError::InvalidInput("Could not inspect plan file.".to_string()))?;
    if !metadata.is_file() {
        return Err(CliError::InvalidInput(
            "Plan must be a regular file.".to_string(),
        ));
    }
    if metadata.len() > MAX_PLAN_FILE_BYTES {
        return Err(CliError::InvalidInput(format!(
            "Plan file '{}' is too large ({} bytes, max {MAX_PLAN_FILE_BYTES}). Use the JSON output of `repo scope preview`, not an arbitrary file.",
            path.display(),
            metadata.len()
        )));
    }
    let mut contents = String::new();
    file.take(MAX_PLAN_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|_| {
            CliError::InvalidInput("Could not read plan file as UTF-8 JSON.".to_string())
        })?;
    if contents.len() as u64 > MAX_PLAN_FILE_BYTES {
        return Err(CliError::InvalidInput(
            "Plan file is too large.".to_string(),
        ));
    }
    let plan: ScopePlan = serde_json::from_str(&contents).map_err(|_| {
        CliError::InvalidInput(format!(
            "Plan file '{}' is not a valid scope preview plan (expected JSON from `repo scope preview --format json`).",
            path.display()
        ))
    })?;

    validate_repository_id(&plan.request.repository_id)?;
    validate_memory_ids(&plan.request.memory_ids)?;
    validate_fingerprint(&plan.response.preview_fingerprint)?;
    normalize_memory_scope(Some(&plan.request.memory_scope))?;
    let selected: HashSet<_> = plan.request.memory_ids.iter().collect();
    let shown: HashSet<_> = plan
        .response
        .items
        .iter()
        .map(|item| &item.memory_id)
        .collect();
    if selected != shown
        || plan.response.count as usize != selected.len()
        || plan.response.items.len() != selected.len()
        || plan.response.items.iter().any(|item| {
            item.after.repository_id.as_deref() != Some(plan.request.repository_id.as_str())
                || item.after.memory_scope != plan.request.memory_scope
                || item.after.branch != plan.request.branch
                || item.after.experiment != plan.request.experiment
        })
    {
        return Err(CliError::InvalidInput(
            "Plan selection or destination does not match its preview.".to_string(),
        ));
    }
    if plan.request.repository_id != plan.response.repository_id {
        return Err(CliError::InvalidInput(format!(
            "Plan file '{}' is inconsistent (request and preview repository IDs differ).",
            path.display()
        )));
    }

    Ok(plan)
}

// ---------------------------------------------------------------------------
// REST wire types (spec/REPOSITORY_MEMORY.md, TKB-128 binding contract)
// ---------------------------------------------------------------------------

/// `POST/GET /v1/memory/repositories` item shape. `revision` is a
/// non-negative integer counter starting at 1 (0 is reserved for "no prior
/// revision" on `expected_revision`, never a real object revision).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub canonical_remote: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub git_repository_id: Option<String>,
    pub revision: u64,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

/// `POST/GET /v1/memory/checkouts` item shape (a binding). `revision` is a
/// non-negative integer counter, matching `RepositoryRecord::revision`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutRecord {
    pub id: String,
    pub repository_id: String,
    pub revision: u64,
    #[serde(default)]
    pub working_directory_label: Option<String>,
    #[serde(default)]
    pub working_directory_key: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default = "default_checkout_kind")]
    pub checkout_kind: String,
    #[serde(default, deserialize_with = "deserialize_active")]
    pub active: bool,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

fn default_checkout_kind() -> String {
    "unknown".to_string()
}

fn deserialize_active<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(false),
        serde_json::Value::String(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    })
}

/// `{items, next_offset, namespace?}` list wrapper used by `/v1/memory/*`.
/// `next_offset` is a non-negative integer offset for the next page.
#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct MemoryListResponse<T> {
    #[serde(default)]
    pub items: Vec<T>,
    #[serde(default)]
    pub next_offset: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub namespace: Option<String>,
}

/// Full inspection payload combining offline Git discovery with an optional
/// (opt-in, authenticated) current binding lookup.
#[derive(Debug, Clone, Serialize)]
pub struct RepoInspectionReport {
    pub path: String,
    pub git: GitReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<BindingLookup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BindingLookup {
    None,
    Bound(CheckoutRecord),
}

// ---------------------------------------------------------------------------
// Scope wire types (spec/REPOSITORY_MEMORY.md TKB-130, matches
// src/shared/memory-association.ts exactly)
// ---------------------------------------------------------------------------

/// The exact request previously sent to `/v1/memory/scopes/preview`. Kept
/// alongside the response in a `ScopePlan` so `apply` can resend it
/// unchanged rather than re-deriving it from the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeRequest {
    pub repository_id: String,
    pub memory_ids: Vec<String>,
    pub memory_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<String>,
}

/// `MemoryScopeFields` shape shared by `before`/`after` in preview items.
/// `scope_revision` is a non-negative integer counter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScopeFields {
    #[serde(default)]
    pub repository_id: Option<String>,
    pub memory_scope: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub experiment: Option<String>,
    pub scope_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopePreviewItem {
    pub memory_id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub original_working_directory: Option<String>,
    pub before: MemoryScopeFields,
    pub after: MemoryScopeFields,
}

/// `POST /v1/memory/scopes/preview` response shape. Read-only: making this
/// call never writes anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopePreviewResponse {
    pub repository_id: String,
    #[serde(default)]
    pub namespace: Option<String>,
    pub count: u32,
    pub items: Vec<ScopePreviewItem>,
    pub preview_fingerprint: String,
}

/// Self-contained plan combining the exact request that was previewed with
/// the server's response. `repo scope apply --plan FILE` reads this back so
/// apply always resends the identical selection and fingerprint that was
/// already shown to the user — never a freshly re-derived one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopePlan {
    pub request: ScopeRequest,
    pub response: ScopePreviewResponse,
}

/// `POST /v1/memory/scopes/apply` and `POST /v1/memory/scopes/:id/restore`
/// response shape (a `memory_scope_operations` row). `count` is only present
/// when the operation newly committed, not on an idempotent replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeOperationResponse {
    pub id: String,
    pub repository_id: String,
    pub kind: String,
    #[serde(default)]
    pub restores_operation_id: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub replayed: bool,
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

async fn handle_create(
    ctx: &Ctx,
    name: &str,
    remote: Option<&str>,
    project: Option<String>,
) -> Result<()> {
    let mut body = json!({ "name": name });
    if let Some(raw) = remote {
        body["canonical_remote"] = json!(normalize_remote(raw));
    }
    if let Some(project_ref) = project {
        let resolved = ctx.resolve_project(&project_ref).await?;
        body["project_id"] = json!(resolved.id);
    }

    let resp: RepositoryRecord = ctx.api.post("/v1/memory/repositories", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        // Identity (id) and name are shown distinctly: names may collide
        // across forks or re-created repositories, the id never does.
        let msg =
            format::inline::mutation_created("repository", &resp.id, Some(&resp.name), &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_list(ctx: &Ctx, limit: u32, offset: u32) -> Result<()> {
    let url = format!("/v1/memory/repositories?limit={limit}&offset={offset}");
    let resp: MemoryListResponse<RepositoryRecord> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        print_repository_table(&resp.items, ctx);
        if let Some(next) = resp.next_offset {
            let msg = ctx.color.paint(
                &format!("more results — use --offset {next} to continue"),
                colors::MUTED,
            );
            println!("{msg}");
        }
    }
    Ok(())
}

async fn handle_bind(
    ctx: &Ctx,
    repository_id: &str,
    path: Option<PathBuf>,
    branch: Option<&str>,
    kind: Option<&str>,
    expected_revision: Option<u64>,
) -> Result<()> {
    let target = resolve_target_dir(path)?;
    let working_directory = target.display().to_string();
    let discovered = inspect_git(&target, &SystemGitRunner);

    let mut body = json!({
        "repository_id": repository_id,
        "working_directory": working_directory,
    });

    match branch {
        Some(b) => {
            body["branch"] = json!(b);
        }
        None => {
            if let GitReport::Repository(info) = &discovered {
                if let Some(b) = &info.branch {
                    body["branch"] = json!(b);
                }
            }
        }
    }

    let resolved_kind = match kind {
        Some(k) => Some(normalize_checkout_kind(k)?),
        None => match &discovered {
            GitReport::Repository(info) => Some(info.checkout_kind.clone()),
            _ => None,
        },
    };
    if let Some(k) = resolved_kind {
        body["checkout_kind"] = json!(k);
    }

    if let Some(rev) = expected_revision {
        body["expected_revision"] = json!(rev);
    }

    let resp: CheckoutRecord = ctx.api.post("/v1/memory/checkouts", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let check = ctx.color.paint("✓", colors::SUCCESS);
        println!(
            "{check} Bound {} → repository {} (revision {})",
            working_directory, resp.repository_id, resp.revision
        );
    }
    Ok(())
}

async fn handle_unbind(ctx: &Ctx, path: Option<PathBuf>, expected_revision: u64) -> Result<()> {
    let target = resolve_target_dir(path)?;
    let working_directory = target.display().to_string();
    let body = json!({
        "working_directory": working_directory,
        "expected_revision": expected_revision,
    });

    // Unbind response shape isn't fixed by the contract beyond the request
    // body; decode as a raw value so unexpected/extra fields never break it.
    let resp: serde_json::Value = ctx
        .api
        .delete_with_body("/v1/memory/checkouts", &body)
        .await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        let msg =
            format::inline::mutation_deleted("checkout binding", &working_directory, &ctx.color);
        format::print_inline(&msg);
    }
    Ok(())
}

async fn handle_history(
    ctx: &Ctx,
    checkout_id: Option<String>,
    path: Option<PathBuf>,
    limit: u32,
    offset: u32,
) -> Result<()> {
    let id = match checkout_id {
        Some(id) => id,
        None => {
            let target = resolve_target_dir(path)?;
            let working_directory = target.display().to_string();
            let checkout = lookup_checkout(ctx, &working_directory, true).await?;
            checkout
                .ok_or_else(|| {
                    CliError::InvalidInput(format!(
                        "No checkout binding found for '{working_directory}'. Pass --checkout-id explicitly."
                    ))
                })?
                .id
        }
    };

    let url = format!(
        "/v1/memory/checkouts/{}/history?limit={limit}&offset={offset}",
        enc(&id)
    );
    let resp: MemoryListResponse<serde_json::Value> = ctx.api.get(&url).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else if resp.items.is_empty() {
        println!("No history entries.");
    } else {
        for entry in &resp.items {
            println!("{entry}");
        }
        if let Some(next) = resp.next_offset {
            let msg = ctx.color.paint(
                &format!("more results — use --offset {next} to continue"),
                colors::MUTED,
            );
            println!("{msg}");
        }
    }
    Ok(())
}

async fn handle_aliases(ctx: &Ctx, path: Option<PathBuf>) -> Result<()> {
    let target = resolve_target_dir(path)?;
    let git = inspect_git(&target, &SystemGitRunner);
    let dir_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let discovered_remote = match &git {
        GitReport::Repository(info) => info.remote_url.clone(),
        _ => None,
    };

    let mut candidates: Vec<RepositoryRecord> = Vec::new();
    let mut offset: u64 = 0;
    for _ in 0..20 {
        let url = format!("/v1/memory/repositories?limit=50&offset={offset}");
        let resp: MemoryListResponse<RepositoryRecord> = ctx.api.get(&url).await?;
        let fetched = resp.items.len();
        candidates.extend(resp.items);
        match resp.next_offset {
            Some(next) if fetched > 0 && next != offset => offset = next,
            _ => break,
        }
    }

    let matches = filter_alias_candidates(&dir_name, discovered_remote.as_deref(), candidates);

    if ctx.format.is_json() {
        format::print_json(&json!({ "candidates": matches }));
    } else if matches.is_empty() {
        println!("No potential aliases found for this directory. Use `tokanban repo create` or `tokanban repo bind` explicitly — nothing is bound automatically.");
    } else {
        println!(
            "Potential aliases found — review before binding; nothing here is bound automatically:"
        );
        print_repository_table(&matches, ctx);
    }
    Ok(())
}

/// Compare candidates by name/remote for user review only. Never merges or
/// dedupes: forks and same-named repositories are always kept as distinct
/// identities in the returned list, exactly as the server returned them.
pub fn filter_alias_candidates(
    dir_name: &str,
    discovered_remote: Option<&str>,
    candidates: Vec<RepositoryRecord>,
) -> Vec<RepositoryRecord> {
    candidates
        .into_iter()
        .filter(|r| {
            let name_match = !dir_name.is_empty() && r.name.eq_ignore_ascii_case(dir_name);
            let remote_match = discovered_remote
                .zip(r.canonical_remote.as_deref())
                .map(|(a, b)| normalize_remote(a) == normalize_remote(b))
                .unwrap_or(false);
            name_match || remote_match
        })
        .collect()
}

async fn lookup_checkout(
    ctx: &Ctx,
    working_directory: &str,
    include_inactive: bool,
) -> Result<Option<CheckoutRecord>> {
    let url = format!(
        "/v1/memory/checkouts?working_directory={}&include_inactive={}&limit=50&offset=0",
        enc(working_directory),
        include_inactive
    );
    let resp: MemoryListResponse<CheckoutRecord> = ctx.api.get(&url).await?;
    Ok(resp.items.into_iter().next())
}

// ---------------------------------------------------------------------------
// Scope command handlers (TKB-130/TKB-131)
// ---------------------------------------------------------------------------

/// Read-only: builds and sends the preview request, but never writes.
async fn handle_scope_preview(
    ctx: &Ctx,
    repository_id: &str,
    memory_ids: &[String],
    scope: Option<&str>,
    branch: Option<&str>,
    experiment: Option<&str>,
) -> Result<()> {
    validate_repository_id(repository_id)?;
    validate_memory_ids(memory_ids)?;
    validate_optional_text(branch, "--branch")?;
    validate_optional_text(experiment, "--experiment")?;
    let memory_scope = normalize_memory_scope(scope)?;

    let request = ScopeRequest {
        repository_id: repository_id.to_string(),
        memory_ids: memory_ids.to_vec(),
        memory_scope,
        branch: branch.map(|s| s.to_string()),
        experiment: experiment.map(|s| s.to_string()),
    };

    let mut body = json!({
        "repository_id": request.repository_id,
        "memory_ids": request.memory_ids,
        "memory_scope": request.memory_scope,
    });
    if let Some(b) = &request.branch {
        body["branch"] = json!(b);
    }
    if let Some(e) = &request.experiment {
        body["experiment"] = json!(e);
    }

    let response: ScopePreviewResponse = ctx.api.post("/v1/memory/scopes/preview", &body).await?;
    let plan = ScopePlan { request, response };

    if ctx.format.is_json() {
        format::print_json(&plan);
    } else {
        print!("{}", render_scope_plan_human(&plan, &ctx.color));
    }
    Ok(())
}

/// Never previews implicitly: only ever resends a plan file that was already
/// written and shown to the user by a prior `repo scope preview` run.
async fn handle_scope_apply(ctx: &Ctx, plan_path: &Path) -> Result<()> {
    let plan = read_scope_plan(plan_path)?;

    let mut body = json!({
        "repository_id": plan.request.repository_id,
        "memory_ids": plan.request.memory_ids,
        "memory_scope": plan.request.memory_scope,
        "preview_fingerprint": plan.response.preview_fingerprint,
    });
    if let Some(b) = &plan.request.branch {
        body["branch"] = json!(b);
    }
    if let Some(e) = &plan.request.experiment {
        body["experiment"] = json!(e);
    }

    let resp: ScopeOperationResponse = ctx.api.post("/v1/memory/scopes/apply", &body).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        print!(
            "{}",
            render_scope_operation_human("Applied", &resp, &ctx.color)
        );
    }
    Ok(())
}

async fn handle_scope_restore(ctx: &Ctx, operation_id: &str) -> Result<()> {
    if operation_id.trim().is_empty() {
        return Err(CliError::InvalidInput(
            "operation_id must not be empty.".to_string(),
        ));
    }
    let url = format!("/v1/memory/scopes/{}/restore", enc(operation_id));
    let resp: ScopeOperationResponse = ctx.api.post(&url, &json!({})).await?;

    if ctx.format.is_json() {
        format::print_json(&resp);
    } else {
        print!(
            "{}",
            render_scope_operation_human("Restored", &resp, &ctx.color)
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn print_inspection(
    report: &RepoInspectionReport,
    output_format: OutputFormat,
    color: &ColorConfig,
) {
    match output_format.resolve() {
        OutputFormat::Json => format::print_json(report),
        _ => print!("{}", render_inspect_human(report, color)),
    }
}

fn render_inspect_human(report: &RepoInspectionReport, color: &ColorConfig) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", color.bold("Repository inspection")));
    out.push_str(&format!("  Path:  {}\n", report.path));

    match &report.git {
        GitReport::NotAvailable { detail } => {
            out.push_str(&format!(
                "  Git:   {}\n",
                color.paint("not available", colors::MUTED)
            ));
            out.push_str(&format!("         {detail}\n"));
        }
        GitReport::NotARepository { detail } => {
            out.push_str(&format!(
                "  Git:   {}\n",
                color.paint("not a repository", colors::MUTED)
            ));
            out.push_str(&format!("         {detail}\n"));
        }
        GitReport::Repository(info) => {
            out.push_str(&format!(
                "  Git:            {}\n",
                color.paint("detected", colors::SUCCESS)
            ));
            out.push_str(&format!("  Root:           {}\n", info.repo_root));
            out.push_str(&format!("  Git dir:        {}\n", info.git_dir));
            out.push_str(&format!("  Common git dir: {}\n", info.common_git_dir));
            out.push_str(&format!("  Checkout kind:  {}\n", info.checkout_kind));
            if info.detached {
                out.push_str(&format!("  Branch:         {} (detached HEAD)\n", EM_DASH));
            } else {
                out.push_str(&format!(
                    "  Branch:         {}\n",
                    info.branch.as_deref().unwrap_or(EM_DASH)
                ));
            }
            out.push_str(&format!(
                "  HEAD commit:    {}\n",
                info.head_commit.as_deref().unwrap_or(EM_DASH)
            ));
            out.push_str(&format!(
                "  Remote:         {} {}\n",
                info.remote_name.as_deref().unwrap_or(EM_DASH),
                info.remote_url.as_deref().unwrap_or("")
            ));
        }
    }

    match &report.binding {
        None => {}
        Some(BindingLookup::None) => {
            out.push_str(&format!(
                "  Binding:        {}\n",
                color.paint("none", colors::MUTED)
            ));
        }
        Some(BindingLookup::Bound(checkout)) => {
            out.push_str(&format!(
                "  Binding:        {}\n",
                color.paint("bound", colors::SUCCESS)
            ));
            out.push_str(&format!("    Repository ID: {}\n", checkout.repository_id));
            out.push_str(&format!("    Checkout ID:   {}\n", checkout.id));
            out.push_str(&format!(
                "    Branch:        {}\n",
                checkout.branch.as_deref().unwrap_or(EM_DASH)
            ));
            out.push_str(&format!("    Kind:          {}\n", checkout.checkout_kind));
            out.push_str(&format!("    Revision:      {}\n", checkout.revision));
        }
    }

    out
}

fn print_repository_table(items: &[RepositoryRecord], ctx: &Ctx) {
    if items.is_empty() {
        println!("No repositories.");
        return;
    }
    let columns = [
        Column::new("ID", 36),
        Column::new("Name", 20).flexible(),
        Column::new("Remote", 30),
        Column::new("Project", 12),
    ];
    let rows: Vec<Vec<Option<String>>> = items
        .iter()
        .map(|r| {
            vec![
                Some(ctx.color.paint(&r.id, colors::MUTED)),
                Some(r.name.clone()),
                r.canonical_remote.clone(),
                r.project_id.clone(),
            ]
        })
        .collect();
    print!("{}", render_table(&columns, &rows, &ctx.color));
}

fn render_scope_plan_human(plan: &ScopePlan, color: &ColorConfig) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}\n",
        color.bold("Scope preview — no changes made")
    ));
    out.push_str(&format!("  Repository:  {}\n", plan.response.repository_id));
    out.push_str(&format!("  Scope:       {}\n", plan.request.memory_scope));
    if let Some(b) = &plan.request.branch {
        out.push_str(&format!("  Branch:      {b}\n"));
    }
    if let Some(e) = &plan.request.experiment {
        out.push_str(&format!("  Experiment:  {e}\n"));
    }
    out.push_str(&format!("  Count:       {}\n", plan.response.count));
    out.push_str(&format!(
        "  Fingerprint: {}\n",
        plan.response.preview_fingerprint
    ));
    out.push('\n');

    let columns = [
        Column::new("Memory ID", 20),
        Column::new("Type", 10),
        Column::new("Source", 20).flexible(),
        Column::new("Before / After", 24),
    ];
    let rows: Vec<Vec<Option<String>>> = plan
        .response
        .items
        .iter()
        .map(|item| {
            vec![
                Some(color.paint(&item.memory_id, colors::MUTED)),
                Some(item.kind.clone()),
                item.original_working_directory.clone(),
                Some(format!(
                    "{} → {}",
                    scope_summary(&item.before),
                    scope_summary(&item.after)
                )),
            ]
        })
        .collect();
    out.push_str(&render_table(&columns, &rows, color));
    out.push('\n');
    out.push_str(&color.paint(
        "No changes were made. Save this with --format json > plan.json, then run `tokanban repo scope apply --plan plan.json` to apply exactly this reviewed selection.",
        colors::MUTED,
    ));
    out.push('\n');
    out
}

fn scope_summary(fields: &MemoryScopeFields) -> String {
    match fields.memory_scope.as_str() {
        "branch" => format!("branch:{}", fields.branch.as_deref().unwrap_or(EM_DASH)),
        "experiment" => format!(
            "experiment:{}",
            fields.experiment.as_deref().unwrap_or(EM_DASH)
        ),
        other => other.to_string(),
    }
}

fn render_scope_operation_human(
    verb: &str,
    resp: &ScopeOperationResponse,
    color: &ColorConfig,
) -> String {
    let check = color.paint("✓", colors::SUCCESS);
    let mut out = format!("{check} {verb} scope operation {}\n", resp.id);
    out.push_str(&format!("  Repository:  {}\n", resp.repository_id));
    out.push_str(&format!("  Kind:        {}\n", resp.kind));
    if let Some(restores) = &resp.restores_operation_id {
        out.push_str(&format!("  Restores:    {restores}\n"));
    }
    if let Some(count) = resp.count {
        out.push_str(&format!("  Memories:    {count}\n"));
    }
    out.push_str(&format!(
        "  Replayed:    {}\n",
        if resp.replayed {
            "yes (idempotent retry)"
        } else {
            "no"
        }
    ));
    out.push_str(&format!("  Created:     {}\n", resp.created_at));
    out
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_remote_strips_http_credentials_query_and_fragment() {
        assert_eq!(
            normalize_remote("https://user:secret@github.com/acme/widgets.git?token=abc#frag"),
            "https://github.com/acme/widgets"
        );
    }

    #[test]
    fn normalize_remote_strips_scp_like_ssh_user() {
        assert_eq!(
            normalize_remote("git@github.com:acme/widgets.git"),
            "ssh://github.com/acme/widgets"
        );
    }

    #[test]
    fn normalize_remote_strips_explicit_ssh_scheme_user() {
        assert_eq!(
            normalize_remote("ssh://deploy@git.example.com:2222/acme/widgets.git"),
            "ssh://git.example.com:2222/acme/widgets"
        );
    }

    #[test]
    fn normalize_remote_leaves_local_paths_untouched() {
        assert_eq!(
            normalize_remote("/local/path/to/repo.git"),
            "/local/path/to/repo.git"
        );
    }

    #[test]
    fn normalize_checkout_kind_accepts_known_values_only() {
        assert_eq!(normalize_checkout_kind("Worktree").unwrap(), "worktree");
        assert_eq!(normalize_checkout_kind("MAIN").unwrap(), "main");
        assert!(normalize_checkout_kind("bogus").is_err());
    }

    #[test]
    fn normalize_memory_scope_defaults_to_repository_and_rejects_unknown() {
        assert_eq!(normalize_memory_scope(None).unwrap(), "repository");
        assert_eq!(normalize_memory_scope(Some("")).unwrap(), "repository");
        assert_eq!(normalize_memory_scope(Some("Branch")).unwrap(), "branch");
        assert_eq!(normalize_memory_scope(Some("workdir")).unwrap(), "workdir");
        assert_eq!(
            normalize_memory_scope(Some("experiment")).unwrap(),
            "experiment"
        );
        assert!(normalize_memory_scope(Some("bogus")).is_err());
    }

    #[test]
    fn validate_memory_ids_enforces_one_to_fifty_and_uniqueness() {
        assert!(validate_memory_ids(&[]).is_err());
        assert!(validate_memory_ids(&["".to_string()]).is_err());
        let dup = vec!["a".to_string(), "a".to_string()];
        assert!(validate_memory_ids(&dup).is_err());
        let too_many: Vec<String> = (0..51).map(|i| i.to_string()).collect();
        assert!(validate_memory_ids(&too_many).is_err());
        let ok: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        assert!(validate_memory_ids(&ok).is_ok());
        assert!(validate_memory_ids(&["one".to_string()]).is_ok());
    }

    #[test]
    fn validate_fingerprint_requires_64_lowercase_hex_chars() {
        let good = "a".repeat(64);
        assert!(validate_fingerprint(&good).is_ok());
        assert!(validate_fingerprint("not-hex").is_err());
        assert!(validate_fingerprint(&"A".repeat(64)).is_err());
        assert!(validate_fingerprint(&"a".repeat(63)).is_err());
    }

    #[test]
    fn read_scope_plan_rejects_oversized_files_without_echoing_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("plan.json");
        let huge = "x".repeat((MAX_PLAN_FILE_BYTES + 1) as usize);
        std::fs::write(&path, format!("{{\"padding\":\"{huge}\"}}")).unwrap();

        let err = read_scope_plan(&path).unwrap_err();
        let message = err.to_string();
        assert!(!message.contains(&huge));
        assert!(message.contains("too large"));
    }

    #[test]
    fn read_scope_plan_rejects_malformed_json_without_echoing_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("plan.json");
        std::fs::write(&path, "tk_super_secret_token{{{not json").unwrap();

        let err = read_scope_plan(&path).unwrap_err();
        let message = err.to_string();
        assert!(!message.contains("tk_super_secret_token"));
        assert!(message.contains("not a valid scope preview plan"));
    }
}
