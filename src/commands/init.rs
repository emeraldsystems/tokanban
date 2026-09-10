//! `tokanban init` — idempotent bootstrap for Claude Code, Codex, and Cursor:
//! registers the Tokanban MCP server, installs the memory behavioral block
//! (`CLAUDE.md` / `AGENTS.md` / `.cursorrules`), and (Claude Code only)
//! registers `SessionStart` plus `Stop` / `SessionEnd` usage-reporting hooks when the
//! marketplace plugin is not already enabled.
//!
//! Every step is skip-if-present and never overwrites unrelated content. By
//! default (no `--yes`) the command only previews planned changes; nothing is
//! written to disk unless `--yes` is passed and `--dry-run` is not.
//!
//! MCP transport formats differ per harness and are **not** interchangeable:
//! - Claude Code: `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`),
//!   `"type": "http"`, header `Authorization: Bearer ${TOKANBAN_API_KEY}`.
//! - Codex: `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`),
//!   `[mcp_servers.tokanban]` with `bearer_token_env_var = "TOKANBAN_API_KEY"`
//!   (Codex reads the token from that named env var itself, not header
//!   interpolation). Existing files are validated with `toml` and the new
//!   table is appended as text — never round-tripped/reserialized — so
//!   unrelated comments and formatting are preserved byte-for-byte.
//! - Cursor: `~/.cursor/mcp.json` (or project-local), header
//!   `Authorization: Bearer ${env:TOKANBAN_API_KEY}` (Cursor's own env
//!   interpolation syntax, distinct from Claude's `${VAR}`), no `"type"`
//!   field.
//!
//! `CLAUDE_CONFIG_DIR`, when set, identifies the *active* Claude Code
//! account: only that account's `.claude.json` gates the AlreadyPresent /
//! write decision. A tokanban entry found in a different account's file is
//! reported as a diagnostic note only — it must never suppress installing
//! into the active account (that would be a false "success").
//!
//! All filesystem locations are resolved into an [`InitPaths`] value up
//! front so tests can construct one directly against a tempdir instead of
//! touching the real HOME / CLAUDE_CONFIG_DIR / CODEX_HOME (mirrors
//! `commands::doctor`).

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::{json, Value};
use toml::Value as TomlValue;
use url::{Host, Url};

use crate::config::AppConfig;
use crate::error::Result;
use crate::format::{colors, ColorConfig, OutputFormat};

/// The exact command registered for the `Stop` / `SessionEnd` hooks. Must
/// match the command in `cli/plugins/tokanban/hooks/hooks.json` so plugin-
/// managed and `init`-managed installs behave identically.
const USAGE_HOOK_COMMAND: &str = "tmp=$(mktemp \"${TMPDIR:-/tmp}/tokanban-usage.XXXXXX\") && cat > \"$tmp\" && (tokanban session report-usage < \"$tmp\" >/dev/null 2>&1; rm -f \"$tmp\") >/dev/null 2>&1 &";
const START_HOOK_COMMAND: &str = "tokanban session start-hook 2>/dev/null || true";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Harness {
    #[value(name = "claude-code")]
    ClaudeCode,
    Codex,
    Cursor,
}

impl Harness {
    pub fn label(self) -> &'static str {
        match self {
            Harness::ClaudeCode => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Cursor => "Cursor",
        }
    }
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Harness to configure (auto-detected from local config dirs if omitted)
    #[arg(long, value_enum)]
    pub harness: Option<Harness>,

    /// Print planned changes without writing anything
    #[arg(long)]
    pub dry_run: bool,

    /// Apply the planned changes (without this, `init` only previews them)
    #[arg(long)]
    pub yes: bool,

    /// Directory to bootstrap for the behavioral block and project-scoped MCP config
    /// (defaults to the current directory)
    #[arg(long, value_name = "DIR")]
    pub target_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Resolved input paths (kept separate from the step logic so tests can inject
// tempdir paths instead of mutating the real HOME / CLAUDE_CONFIG_DIR / CODEX_HOME).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InitPaths {
    /// Project directory the behavioral block (and project-scoped MCP config
    /// duplicate checks) is written into / read from.
    pub target_dir: PathBuf,
    /// Resolved Claude Code config dir (`$CLAUDE_CONFIG_DIR` or `~/.claude`).
    pub claude_dir: Option<PathBuf>,
    /// Whether `claude_dir` came from the `CLAUDE_CONFIG_DIR` env var.
    pub claude_dir_from_env: bool,
    /// The **active account's** `.claude.json`: `$CLAUDE_CONFIG_DIR/.claude.json`
    /// when the env var is set, else `~/.claude.json`. This is the only file
    /// that gates AlreadyPresent / write decisions for Claude Code.
    pub claude_json_primary: Option<PathBuf>,
    /// The *other* `.claude.json` location (the one not selected as primary).
    /// Scanned only to warn about a possible different-account entry; a match
    /// here must never suppress installing into `claude_json_primary`.
    pub claude_json_diagnostic_candidates: Vec<PathBuf>,
    /// Files scanned (read-only), global-first then project-local, to
    /// determine whether the tokanban plugin is enabled. A later (more
    /// specific) explicit `enabledPlugins` entry overrides an earlier one,
    /// matching Claude Code's own project-over-global precedence.
    pub claude_plugin_candidates: Vec<PathBuf>,
    /// Codex `config.toml` candidates: `[0]` is `$CODEX_HOME/config.toml`
    /// (or `~/.codex/config.toml` if unset) — the write target. Remaining
    /// entries (the project-local `.codex/config.toml`) are checked only to
    /// avoid registering a duplicate. Empty if no config dir could be
    /// determined at all.
    pub codex_config_candidates: Vec<PathBuf>,
    /// Cursor MCP config candidates (global then project-local), in priority
    /// order; `[0]` is the write target, the rest are duplicate-checks only.
    pub cursor_mcp_candidates: Vec<PathBuf>,
}

/// Resolve every path `init` needs from the real environment (HOME,
/// `CLAUDE_CONFIG_DIR`, `CODEX_HOME`). Tests should construct [`InitPaths`]
/// directly with fixture paths instead of calling this.
pub fn resolve_init_paths(target_dir: PathBuf) -> InitPaths {
    let home_dir = dirs::home_dir();

    let claude_dir_env = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let claude_dir_from_env = claude_dir_env.is_some();
    let claude_dir = claude_dir_env.or_else(|| home_dir.as_ref().map(|h| h.join(".claude")));

    let (claude_json_primary, claude_json_diagnostic_candidates) = if claude_dir_from_env {
        let primary = claude_dir.as_ref().map(|d| d.join(".claude.json"));
        let other: Vec<PathBuf> = home_dir
            .as_ref()
            .map(|h| h.join(".claude.json"))
            .into_iter()
            .collect();
        (primary, other)
    } else {
        let primary = home_dir.as_ref().map(|h| h.join(".claude.json"));
        let other: Vec<PathBuf> = claude_dir
            .as_ref()
            .map(|d| d.join(".claude.json"))
            .into_iter()
            .collect();
        (primary, other)
    };

    let mut claude_plugin_candidates = Vec::new();
    if let Some(dir) = &claude_dir {
        claude_plugin_candidates.push(dir.join("settings.json"));
        claude_plugin_candidates.push(dir.join("settings.local.json"));
        claude_plugin_candidates.push(dir.join("plugins").join("config.json"));
        claude_plugin_candidates.push(dir.join("plugins").join("installed_plugins.json"));
    }
    // Project-local settings are scanned last so an explicit value there
    // overrides a global one.
    claude_plugin_candidates.push(target_dir.join(".claude").join("settings.json"));
    claude_plugin_candidates.push(target_dir.join(".claude").join("settings.local.json"));

    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| home_dir.as_ref().map(|h| h.join(".codex")));
    let codex_config_candidates = match &codex_dir {
        Some(dir) => vec![
            dir.join("config.toml"),
            target_dir.join(".codex").join("config.toml"),
        ],
        // No home dir and no CODEX_HOME override: refuse rather than
        // silently writing into the project-local file as if it were the
        // account-wide config.
        None => Vec::new(),
    };

    // Global config is the write target; the project-local file is only
    // checked so an existing project-scoped entry isn't duplicated globally.
    let mut cursor_mcp_candidates = Vec::new();
    if let Some(dir) = &home_dir {
        cursor_mcp_candidates.push(dir.join(".cursor").join("mcp.json"));
    }
    cursor_mcp_candidates.push(target_dir.join(".cursor").join("mcp.json"));

    InitPaths {
        target_dir,
        claude_dir,
        claude_dir_from_env,
        claude_json_primary,
        claude_json_diagnostic_candidates,
        claude_plugin_candidates,
        codex_config_candidates,
        cursor_mcp_candidates,
    }
}

/// Detect the harness to configure when `--harness` was not passed: Claude
/// Code if its config dir exists, else Codex, else Cursor, else Claude Code
/// (the documented default for a from-scratch install).
pub fn detect_harness(paths: &InitPaths) -> Harness {
    if paths.claude_dir.as_ref().is_some_and(|d| d.exists()) {
        Harness::ClaudeCode
    } else if paths
        .codex_config_candidates
        .first()
        .and_then(|p| p.parent())
        .is_some_and(|d| d.exists())
    {
        Harness::Codex
    } else if paths
        .cursor_mcp_candidates
        .iter()
        .any(|p| p.parent().is_some_and(|d| d.exists()))
    {
        Harness::Cursor
    } else {
        Harness::ClaudeCode
    }
}

// ---------------------------------------------------------------------------
// Report shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Created,
    Updated,
    AlreadyPresent,
    WouldCreate,
    WouldUpdate,
    Skipped,
    Refused,
}

impl StepStatus {
    fn for_write(apply: bool, will_create: bool) -> StepStatus {
        match (apply, will_create) {
            (true, true) => StepStatus::Created,
            (true, false) => StepStatus::Updated,
            (false, true) => StepStatus::WouldCreate,
            (false, false) => StepStatus::WouldUpdate,
        }
    }

    fn is_pending_or_applied_write(self) -> bool {
        matches!(
            self,
            StepStatus::Created
                | StepStatus::Updated
                | StepStatus::WouldCreate
                | StepStatus::WouldUpdate
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InitStep {
    pub name: String,
    pub status: StepStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitReport {
    pub harness: &'static str,
    pub applied: bool,
    pub steps: Vec<InitStep>,
}

impl InitReport {
    pub fn any_pending_or_applied_write(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.status.is_pending_or_applied_write())
    }

    pub fn any_refused(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.status == StepStatus::Refused)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn handle(
    args: &InitArgs,
    cli_config: &AppConfig,
    format: OutputFormat,
    no_color: bool,
) -> Result<()> {
    let target_dir = match &args.target_dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir()?,
    };
    let paths = resolve_init_paths(target_dir);
    let harness = args.harness.unwrap_or_else(|| detect_harness(&paths));
    let apply = args.yes && !args.dry_run;

    let report = run_init(&paths, harness, apply, cli_config)?;

    match format.resolve() {
        OutputFormat::Json => crate::format::print_json(&report),
        _ => print!(
            "{}",
            render_human(&report, apply, args.yes, &ColorConfig::new(no_color))
        ),
    }

    if report.any_refused() {
        return Err(crate::error::CliError::Config(
            "One or more setup steps could not be completed. Review the reported steps and retry after fixing the configuration.".to_string(),
        ));
    }
    Ok(())
}

/// Run every bootstrap step against pre-resolved paths. Pure aside from the
/// filesystem reads/writes each step performs; `apply = false` guarantees no
/// writes occur (used for both `--dry-run` and the default preview mode).
pub fn run_init(
    paths: &InitPaths,
    harness: Harness,
    apply: bool,
    cli_config: &AppConfig,
) -> Result<InitReport> {
    let mcp_step = match harness {
        Harness::ClaudeCode => step_mcp_config_claude(paths, cli_config, apply)?,
        Harness::Codex => step_mcp_config_codex(paths, cli_config, apply)?,
        Harness::Cursor => step_mcp_config_cursor(paths, cli_config, apply)?,
    };
    let steps = vec![
        mcp_step,
        step_behavior_block(paths, harness, apply)?,
        step_usage_hook(paths, harness, apply)?,
    ];
    Ok(InitReport {
        harness: harness.label(),
        applied: apply,
        steps,
    })
}

// ---------------------------------------------------------------------------
// Shared: API URL validation (never leak credentials/query/fragment)
// ---------------------------------------------------------------------------

/// Validates the configured API base URL before it is ever embedded in a
/// generated config file or echoed in a diagnostic. Rejects embedded
/// credentials, query strings, and fragments (all of which could carry
/// secrets); requires `https://` except for loopback addresses (allowed so
/// tests/fixtures can point at `http://127.0.0.1:<port>`).
///
/// On failure, returns a generic, harness-agnostic reason string that never
/// contains any part of the original URL.
fn validate_api_url(api_url: &str) -> std::result::Result<Url, String> {
    let parsed = Url::parse(api_url)
        .map_err(|_| "the configured API URL could not be parsed".to_string())?;

    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback_url(&parsed) => {}
        "http" => {
            return Err(
                "the configured API URL uses http:// but is not a loopback address; https:// is required"
                    .to_string(),
            )
        }
        other => {
            return Err(format!(
                "the configured API URL uses an unsupported scheme '{other}'"
            ))
        }
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("the configured API URL must not contain embedded credentials".to_string());
    }
    if parsed.query().is_some() {
        return Err("the configured API URL must not contain a query string".to_string());
    }
    if parsed.fragment().is_some() {
        return Err("the configured API URL must not contain a fragment".to_string());
    }

    Ok(parsed)
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    }
}

/// `url` has already been validated to carry no credentials/query/fragment,
/// so it is safe to embed directly in a generated config file.
fn mcp_url_from_validated(url: &Url) -> String {
    let trimmed = url.as_str().trim_end_matches('/');
    if trimmed.ends_with("/mcp") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/mcp")
    }
}

/// Never resolves or prints the actual credential value. Only the *source*
/// of available auth is reported, to give honest guidance when no key can be
/// found yet — and, notably, to be explicit that `tokanban auth login` alone
/// does not populate the `TOKANBAN_API_KEY` env var these MCP entries read.
fn auth_guidance(cli_config: &AppConfig) -> &'static str {
    let env_key_set = std::env::var("TOKANBAN_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let cli_authenticated = [&cli_config.auth.access_token, &cli_config.auth.token]
        .iter()
        .any(|token| token.as_deref().is_some_and(|v| !v.trim().is_empty()));

    if env_key_set {
        "TOKANBAN_API_KEY is set in this shell, so the entry will authenticate automatically."
    } else if cli_authenticated {
        "TOKANBAN_API_KEY is not set. Note: `tokanban auth login` only stores an OAuth session in this CLI's own config — it does not set TOKANBAN_API_KEY, which is what the MCP entry reads. Create a long-lived key with `tokanban agent create`, then `export TOKANBAN_API_KEY=<key>` and restart the harness."
    } else {
        "No Tokanban credentials were found. Run `tokanban auth login` and create an agent key, or `export TOKANBAN_API_KEY=<key>` directly, then restart the harness so the MCP entry can authenticate."
    }
}

fn has_tokanban_mcp_entry(value: &Value) -> bool {
    value
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(|servers| servers.contains_key("tokanban"))
}

fn read_json_lenient(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn write_json_file(path: &Path, value: &Value) -> Result<()> {
    let contents = serde_json::to_string_pretty(value)?;
    atomic_write(path, format!("{contents}\n").as_bytes())?;
    Ok(())
}

/// Keep existing configuration intact if writing is interrupted. Resolve an
/// existing symlink so dotfile-managed configuration keeps its original link.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    let destination = if path.is_symlink() {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".tokanban-init-{:016x}.tmp", rand::random::<u64>()));
    // Only clean up a temporary file this invocation successfully created.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> std::io::Result<()> {
        if let Ok(metadata) = fs::metadata(&destination) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Step 1a: MCP server configuration — Claude Code
// ---------------------------------------------------------------------------

fn insert_claude_mcp_entry(root: &mut Value, mcp_url: &str) {
    let obj = root.as_object_mut().expect("validated object");
    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    let servers_obj = servers.as_object_mut().expect("validated object");
    servers_obj.insert(
        "tokanban".to_string(),
        json!({
            "type": "http",
            "url": mcp_url,
            "headers": {
                "Authorization": "Bearer ${TOKANBAN_API_KEY}",
                "X-Tokanban-Tool-Scope": "core,memory"
            }
        }),
    );
}

fn step_mcp_config_claude(
    paths: &InitPaths,
    cli_config: &AppConfig,
    apply: bool,
) -> Result<InitStep> {
    let name = "MCP server configuration".to_string();

    let Some(primary) = paths.claude_json_primary.clone() else {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: "Could not determine the active Claude Code account's config location (no home directory or CLAUDE_CONFIG_DIR detected).".to_string(),
        });
    };

    let existing_contents = if primary.exists() {
        Some(fs::read_to_string(&primary)?)
    } else {
        None
    };

    let mut root: Value = match &existing_contents {
        None => json!({}),
        Some(contents) => match serde_json::from_str(contents) {
            Ok(value) => value,
            Err(err) => {
                return Ok(InitStep {
                    name,
                    status: StepStatus::Refused,
                    detail: format!(
                        "{} exists but is not valid JSON ({err}); leaving it untouched. Fix or remove the file, then re-run `tokanban init`.",
                        primary.display()
                    ),
                });
            }
        },
    };

    if !root.is_object() {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: format!(
                "{} does not contain a JSON object at the top level; refusing to modify.",
                primary.display()
            ),
        });
    }
    if let Some(mcp_servers) = root.get("mcpServers") {
        if !mcp_servers.is_object() {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!(
                    "`mcpServers` in {} is not a JSON object; refusing to modify.",
                    primary.display()
                ),
            });
        }
    }

    // Diagnostic-only: a tokanban entry in a *different* account/location
    // never gates the decision for the active account below.
    let cross_account_note = paths
        .claude_json_diagnostic_candidates
        .iter()
        .find(|path| read_json_lenient(path).is_some_and(|v| has_tokanban_mcp_entry(&v)));

    if has_tokanban_mcp_entry(&root) {
        let mut detail = format!(
            "tokanban MCP server already configured in {}.",
            primary.display()
        );
        if let Some(other) = cross_account_note {
            detail.push_str(&format!(
                " Note: {} also has a tokanban entry from a different account/location; this does not affect the active account.",
                other.display()
            ));
        }
        return Ok(InitStep {
            name,
            status: StepStatus::AlreadyPresent,
            detail,
        });
    }

    let parsed_url = match validate_api_url(&cli_config.api.url) {
        Ok(url) => url,
        Err(reason) => {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!("Cannot configure the MCP entry: {reason}."),
            })
        }
    };

    let will_create = existing_contents.is_none();

    if apply {
        insert_claude_mcp_entry(&mut root, &mcp_url_from_validated(&parsed_url));
        write_json_file(&primary, &root)?;
    }

    let mut detail = format!(
        "{} {} with a tokanban MCP entry (\"type\": \"http\"). {}",
        if will_create { "Create" } else { "Update" },
        primary.display(),
        auth_guidance(cli_config)
    );
    if let Some(other) = cross_account_note {
        detail.push_str(&format!(
            " Note: {} also has a tokanban entry from a different account/location; only {} was changed.",
            other.display(),
            primary.display()
        ));
    }

    Ok(InitStep {
        name,
        status: StepStatus::for_write(apply, will_create),
        detail,
    })
}

// ---------------------------------------------------------------------------
// Step 1b: MCP server configuration — Cursor
// ---------------------------------------------------------------------------

fn insert_cursor_mcp_entry(root: &mut Value, mcp_url: &str) {
    let obj = root.as_object_mut().expect("validated object");
    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    let servers_obj = servers.as_object_mut().expect("validated object");
    servers_obj.insert(
        "tokanban".to_string(),
        json!({
            "url": mcp_url,
            "headers": {
                "Authorization": "Bearer ${env:TOKANBAN_API_KEY}",
                "X-Tokanban-Tool-Scope": "core,memory"
            }
        }),
    );
}

fn step_mcp_config_cursor(
    paths: &InitPaths,
    cli_config: &AppConfig,
    apply: bool,
) -> Result<InitStep> {
    let name = "MCP server configuration".to_string();
    let candidates = &paths.cursor_mcp_candidates;

    let Some(primary) = candidates.first().cloned() else {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail:
                "Could not determine a config location for Cursor (no home directory detected)."
                    .to_string(),
        });
    };

    for path in candidates {
        if let Some(value) = read_json_lenient(path) {
            if has_tokanban_mcp_entry(&value) {
                return Ok(InitStep {
                    name,
                    status: StepStatus::AlreadyPresent,
                    detail: format!(
                        "tokanban MCP server already configured in {}.",
                        path.display()
                    ),
                });
            }
        }
    }

    let existing_contents = if primary.exists() {
        Some(fs::read_to_string(&primary)?)
    } else {
        None
    };

    let mut root: Value = match &existing_contents {
        None => json!({}),
        Some(contents) => match serde_json::from_str(contents) {
            Ok(value) => value,
            Err(err) => {
                return Ok(InitStep {
                    name,
                    status: StepStatus::Refused,
                    detail: format!(
                        "{} exists but is not valid JSON ({err}); leaving it untouched. Fix or remove the file, then re-run `tokanban init`.",
                        primary.display()
                    ),
                });
            }
        },
    };

    if !root.is_object() {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: format!(
                "{} does not contain a JSON object at the top level; refusing to modify.",
                primary.display()
            ),
        });
    }
    if let Some(mcp_servers) = root.get("mcpServers") {
        if !mcp_servers.is_object() {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!(
                    "`mcpServers` in {} is not a JSON object; refusing to modify.",
                    primary.display()
                ),
            });
        }
    }

    let parsed_url = match validate_api_url(&cli_config.api.url) {
        Ok(url) => url,
        Err(reason) => {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!("Cannot configure the MCP entry: {reason}."),
            })
        }
    };

    let will_create = existing_contents.is_none();

    if apply {
        insert_cursor_mcp_entry(&mut root, &mcp_url_from_validated(&parsed_url));
        write_json_file(&primary, &root)?;
    }

    Ok(InitStep {
        name,
        status: StepStatus::for_write(apply, will_create),
        detail: format!(
            "{} {} with a tokanban MCP entry (url + headers, Cursor's ${{env:VAR}} substitution, no \"type\" field). {}",
            if will_create { "Create" } else { "Update" },
            primary.display(),
            auth_guidance(cli_config)
        ),
    })
}

// ---------------------------------------------------------------------------
// Step 1c: MCP server configuration — Codex (TOML, text-append, never
// round-tripped/reserialized so unrelated comments/formatting survive)
// ---------------------------------------------------------------------------

fn toml_has_tokanban_mcp_entry(value: &TomlValue) -> bool {
    value
        .as_table()
        .and_then(|t| t.get("mcp_servers"))
        .and_then(TomlValue::as_table)
        .is_some_and(|t| t.contains_key("tokanban"))
}

fn toml_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn codex_mcp_toml_snippet(mcp_url: &str) -> String {
    format!(
        "[mcp_servers.tokanban]\nurl = {}\nbearer_token_env_var = \"TOKANBAN_API_KEY\"\nhttp_headers = {{ \"X-Tokanban-Tool-Scope\" = \"core,memory\" }}\n",
        toml_quote(mcp_url)
    )
}

fn step_mcp_config_codex(
    paths: &InitPaths,
    cli_config: &AppConfig,
    apply: bool,
) -> Result<InitStep> {
    let name = "MCP server configuration".to_string();
    let candidates = &paths.codex_config_candidates;

    let Some(primary) = candidates.first().cloned() else {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: "Could not determine a config location for Codex (no home directory or CODEX_HOME detected).".to_string(),
        });
    };

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        if let Ok(value) = toml::from_str::<TomlValue>(&contents) {
            if toml_has_tokanban_mcp_entry(&value) {
                return Ok(InitStep {
                    name,
                    status: StepStatus::AlreadyPresent,
                    detail: format!(
                        "tokanban MCP server already configured in {}.",
                        path.display()
                    ),
                });
            }
        }
    }

    let existing_contents = if primary.exists() {
        Some(fs::read_to_string(&primary)?)
    } else {
        None
    };

    if let Some(contents) = &existing_contents {
        let value = match toml::from_str::<TomlValue>(contents) {
            Ok(value) => value,
            // `toml::de::Error`'s Display quotes the offending source line,
            // which can echo secrets — never include it in the message.
            Err(_) => {
                return Ok(InitStep {
                    name,
                    status: StepStatus::Refused,
                    detail: format!(
                        "{} exists but is not valid TOML; leaving it untouched. Fix or remove the file, then re-run `tokanban init` (contents are not shown here to avoid leaking secrets).",
                        primary.display()
                    ),
                });
            }
        };
        if let Some(mcp_servers) = value.as_table().and_then(|t| t.get("mcp_servers")) {
            if mcp_servers.as_table().is_none() {
                return Ok(InitStep {
                    name,
                    status: StepStatus::Refused,
                    detail: format!(
                        "`mcp_servers` in {} is not a TOML table; refusing to modify.",
                        primary.display()
                    ),
                });
            }
        }
    }

    let parsed_url = match validate_api_url(&cli_config.api.url) {
        Ok(url) => url,
        Err(reason) => {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!("Cannot configure the MCP entry: {reason}."),
            })
        }
    };

    let snippet = codex_mcp_toml_snippet(&mcp_url_from_validated(&parsed_url));
    let new_contents = match &existing_contents {
        None => snippet.clone(),
        Some(contents) => {
            let mut updated = contents.clone();
            if !updated.is_empty() {
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push('\n');
            }
            updated.push_str(&snippet);
            updated
        }
    };

    // Validate the fully-appended document is still well-formed (guards
    // against e.g. `mcp_servers` having been defined as an inline table,
    // which cannot be legally reopened with a `[mcp_servers.tokanban]`
    // header) before ever touching disk.
    if toml::from_str::<TomlValue>(&new_contents).is_err() {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: format!(
                "Appending the tokanban entry to {} would produce invalid TOML (likely a conflicting `mcp_servers` definition); leaving the file untouched.",
                primary.display()
            ),
        });
    }

    let will_create = existing_contents.is_none();

    if apply {
        if let Some(parent) = primary.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&primary, new_contents.as_bytes())?;
    }

    Ok(InitStep {
        name,
        status: StepStatus::for_write(apply, will_create),
        detail: format!(
            "{} {} with a `[mcp_servers.tokanban]` entry (bearer_token_env_var = \"TOKANBAN_API_KEY\"), preserving existing content byte-for-byte. {}",
            if will_create { "Create" } else { "Append to" },
            primary.display(),
            auth_guidance(cli_config)
        ),
    })
}

// ---------------------------------------------------------------------------
// Step 2: behavioral block (CLAUDE.md / AGENTS.md / .cursorrules)
// ---------------------------------------------------------------------------

fn behavior_block_filename(harness: Harness) -> &'static str {
    match harness {
        Harness::ClaudeCode => "CLAUDE.md",
        Harness::Codex => "AGENTS.md",
        Harness::Cursor => ".cursorrules",
    }
}

fn behavior_block_marker(harness: Harness) -> &'static str {
    match harness {
        Harness::ClaudeCode => "## Tokanban Memory",
        Harness::Codex => "## Agent Memory (Tokanban)",
        Harness::Cursor => "## Tokanban Memory",
    }
}

fn behavior_block_template(harness: Harness) -> &'static str {
    match harness {
        Harness::ClaudeCode => {
            include_str!("../../plugins/tokanban/templates/CLAUDE.md.memory-block.md")
        }
        Harness::Codex => {
            include_str!("../../plugins/tokanban/templates/AGENTS.md.memory-block.md")
        }
        Harness::Cursor => {
            include_str!("../../plugins/tokanban/templates/cursorrules.memory-block.md")
        }
    }
}

fn step_behavior_block(paths: &InitPaths, harness: Harness, apply: bool) -> Result<InitStep> {
    let filename = behavior_block_filename(harness);
    let path = paths.target_dir.join(filename);
    let marker = behavior_block_marker(harness);
    let name = format!("Behavior block ({filename})");

    let existing = if path.exists() {
        Some(fs::read_to_string(&path)?)
    } else {
        None
    };

    if let Some(contents) = &existing {
        if contents.contains(marker) {
            return Ok(InitStep {
                name,
                status: StepStatus::AlreadyPresent,
                detail: format!("{filename} already contains the Tokanban memory block."),
            });
        }
    }

    let will_create = existing.is_none();

    if apply {
        let template = behavior_block_template(harness).trim_end();
        let new_contents = match &existing {
            None => format!("{template}\n"),
            Some(contents) => {
                let mut updated = contents.clone();
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push('\n');
                updated.push_str(template);
                updated.push('\n');
                updated
            }
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&path, new_contents.as_bytes())?;
    }

    Ok(InitStep {
        name,
        status: StepStatus::for_write(apply, will_create),
        detail: if will_create {
            format!("Create {filename} with the Tokanban memory block.")
        } else {
            format!("Append the Tokanban memory block to existing {filename}.")
        },
    })
}

// ---------------------------------------------------------------------------
// Step 3: usage-reporting hook (Claude Code only)
// ---------------------------------------------------------------------------

fn is_tokanban_plugin_key(key: &str) -> bool {
    key.split('@')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("tokanban"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginState {
    Enabled,
    Disabled,
    Unknown,
}

/// Scans the same kind of candidate files `doctor` looks at (best-effort,
/// read-only), global-first then project-local, and returns the *most
/// specific* explicit `enabledPlugins` value found. A later (more specific)
/// file overwrites an earlier one, so an explicit per-project `false`
/// overrides a global `true` — mere presence in a plugin registry is not
/// "enabled". Returns `Unknown` when no explicit setting was found anywhere,
/// which is treated as "not managed" (safer to register the hook — reporting
/// is monotonic/idempotent — than to silently skip it on uncertain grounds).
fn plugin_enabled_state(paths: &InitPaths) -> PluginState {
    let mut state = PluginState::Unknown;
    for path in &paths.claude_plugin_candidates {
        let Some(value) = read_json_lenient(path) else {
            continue;
        };
        let Some(entries) = value.get("enabledPlugins").and_then(Value::as_object) else {
            continue;
        };
        for (key, enabled) in entries {
            if is_tokanban_plugin_key(key) {
                if let Some(b) = enabled.as_bool() {
                    state = if b {
                        PluginState::Enabled
                    } else {
                        PluginState::Disabled
                    };
                }
            }
        }
    }
    state
}

fn command_mentions_tokanban_hook(entry: &Value, event: &str) -> bool {
    let Some(hook_list) = entry.get("hooks").and_then(Value::as_array) else {
        return false;
    };
    hook_list.iter().any(|hook| {
        hook.get("type").and_then(Value::as_str) == Some("command")
            && hook
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|cmd| {
                    let normalized = cmd
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_ascii_lowercase();
                    normalized.contains("tokanban")
                        && normalized.contains(if event == "SessionStart" {
                            "session start-hook"
                        } else {
                            "session report-usage"
                        })
                })
    })
}

fn event_hook_registered(root: &Value, event: &str) -> bool {
    root.get("hooks")
        .and_then(Value::as_object)
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| command_mentions_tokanban_hook(entry, event))
        })
}

fn merge_missing_usage_hooks(root: &mut Value, missing_events: &[&str]) {
    let obj = root.as_object_mut().expect("validated object");
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = hooks.as_object_mut().expect("validated object");
    for event in missing_events {
        let arr = hooks_obj.entry(*event).or_insert_with(|| json!([]));
        let arr = arr.as_array_mut().expect("validated array");
        arr.push(json!({
            "matcher": "*",
            "hooks": [{"type": "command", "command": if *event == "SessionStart" { START_HOOK_COMMAND } else { USAGE_HOOK_COMMAND }}]
        }));
    }
}

fn step_usage_hook(paths: &InitPaths, harness: Harness, apply: bool) -> Result<InitStep> {
    let name = "Usage reporting hook".to_string();

    if harness != Harness::ClaudeCode {
        return Ok(InitStep {
            name,
            status: StepStatus::Skipped,
            detail: format!(
                "Live usage reporting currently requires Claude Code; tokanban has no hook mechanism for {} yet.",
                harness.label()
            ),
        });
    }

    if plugin_enabled_state(paths) == PluginState::Enabled {
        return Ok(InitStep {
            name,
            status: StepStatus::Skipped,
            detail: "The tokanban Claude Code plugin appears enabled and should already manage the SessionStart/Stop/SessionEnd hooks (based on local settings only — this cannot confirm the plugin is actually loaded at runtime). Run `tokanban doctor` if usage reporting isn't working.".to_string(),
        });
    }

    let Some(claude_dir) = &paths.claude_dir else {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: "Could not determine the Claude Code config directory (no home directory or CLAUDE_CONFIG_DIR detected).".to_string(),
        });
    };
    let settings_path = claude_dir.join("settings.json");

    let existing = if settings_path.exists() {
        Some(fs::read_to_string(&settings_path)?)
    } else {
        None
    };

    let mut root: Value = match &existing {
        None => json!({}),
        Some(contents) => match serde_json::from_str(contents) {
            Ok(value) => value,
            Err(err) => {
                return Ok(InitStep {
                    name,
                    status: StepStatus::Refused,
                    detail: format!(
                        "{} exists but is not valid JSON ({err}); leaving it untouched. Fix or remove the file, then re-run `tokanban init`.",
                        settings_path.display()
                    ),
                });
            }
        },
    };

    if !root.is_object() {
        return Ok(InitStep {
            name,
            status: StepStatus::Refused,
            detail: format!(
                "{} does not contain a JSON object at the top level; refusing to modify.",
                settings_path.display()
            ),
        });
    }

    let events = ["SessionStart", "Stop", "SessionEnd"];
    let missing: Vec<&str> = events
        .into_iter()
        .filter(|event| !event_hook_registered(&root, event))
        .collect();

    if missing.is_empty() {
        return Ok(InitStep {
            name,
            status: StepStatus::AlreadyPresent,
            detail: format!(
                "SessionStart, Stop and SessionEnd hooks already registered in {}.",
                settings_path.display()
            ),
        });
    }

    if let Some(hooks) = root.get("hooks") {
        if !hooks.is_object() {
            return Ok(InitStep {
                name,
                status: StepStatus::Refused,
                detail: format!(
                    "`hooks` in {} is not a JSON object; refusing to modify.",
                    settings_path.display()
                ),
            });
        }
        for event in ["SessionStart", "Stop", "SessionEnd"] {
            if let Some(entries) = hooks.get(event) {
                if !entries.is_array() {
                    return Ok(InitStep {
                        name,
                        status: StepStatus::Refused,
                        detail: format!(
                            "`hooks.{event}` in {} is not a JSON array; refusing to modify.",
                            settings_path.display()
                        ),
                    });
                }
            }
        }
    }

    let will_create = existing.is_none();

    if apply {
        merge_missing_usage_hooks(&mut root, &missing);
        write_json_file(&settings_path, &root)?;
    }

    Ok(InitStep {
        name,
        status: StepStatus::for_write(apply, will_create),
        detail: format!(
            "Register missing usage hook(s) ({}) in {} calling the Tokanban session startup or usage helper, preserving any existing hooks (CLAUDE_CONFIG_DIR override: {}).",
            missing.join(" and "),
            settings_path.display(),
            if paths.claude_dir_from_env { "yes" } else { "no (default ~/.claude)" }
        ),
    })
}

// ---------------------------------------------------------------------------
// Human rendering
// ---------------------------------------------------------------------------

fn render_human(
    report: &InitReport,
    apply: bool,
    requested_yes: bool,
    color: &ColorConfig,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("tokanban init — {}\n\n", report.harness));

    for step in &report.steps {
        let (label, code) = status_label(step.status);
        out.push_str(&format!("  [{}] {}\n", color.paint(label, code), step.name));
        out.push_str(&format!("      {}\n", step.detail));
    }

    out.push('\n');
    let any_changed = report.any_pending_or_applied_write();
    if apply {
        if any_changed {
            out.push_str("Bootstrap complete.\n");
        } else {
            out.push_str("Already configured; nothing to do.\n");
        }
    } else if any_changed {
        out.push_str("Dry run: no changes were written.\n");
        if !requested_yes {
            out.push_str("Re-run with --yes to apply these changes.\n");
        }
    } else {
        out.push_str("Nothing to do; already configured.\n");
    }

    if report.any_refused() {
        out.push_str("Some steps were refused; see details above before re-running.\n");
    }

    out
}

fn status_label(status: StepStatus) -> (&'static str, u8) {
    match status {
        StepStatus::Created => ("created", colors::SUCCESS),
        StepStatus::Updated => ("updated", colors::SUCCESS),
        StepStatus::AlreadyPresent => ("ok", colors::MUTED),
        StepStatus::WouldCreate => ("would create", colors::HIGH),
        StepStatus::WouldUpdate => ("would update", colors::HIGH),
        StepStatus::Skipped => ("skipped", colors::MUTED),
        StepStatus::Refused => ("refused", colors::ERROR),
    }
}
