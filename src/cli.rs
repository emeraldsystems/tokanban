/// Top-level CLI struct and command enum
///
/// Moved to lib.rs so it's accessible from both main.rs and commands::completion

use clap::{Parser, Subcommand};

use crate::commands;
use crate::config::AppConfig;

#[derive(Debug, Parser)]
#[command(
    name = "tokanban",
    about = "CLI for the Tokanban task management platform",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// Override default workspace
    #[arg(long, global = true)]
    pub workspace: Option<String>,

    /// Override default project
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Output format: json, table, card
    #[arg(long, global = true)]
    pub format: Option<String>,

    /// Suppress non-error output
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Include detailed logging and response bodies
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Strip ANSI color codes
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Custom config file location
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    /// Override API endpoint
    #[arg(long, global = true)]
    pub api_url: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn apply_overrides(&self, config: &mut AppConfig) {
        if let Some(workspace) = &self.workspace {
            config.defaults.workspace = Some(workspace.clone());
        }
        if let Some(project) = &self.project {
            config.defaults.project = Some(project.clone());
        }
        if let Some(url) = &self.api_url {
            config.api.url = url.clone();
        }
        if self.no_color {
            config.ui.no_color = true;
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Authentication commands
    #[command(subcommand)]
    Auth(commands::auth::AuthCommand),

    /// Workspace management
    #[command(subcommand)]
    Workspace(commands::workspace::WorkspaceCommand),

    /// Project management
    #[command(subcommand)]
    Project(commands::project::ProjectCommand),

    /// Task management
    #[command(subcommand)]
    Task(commands::task::TaskCommand),

    /// Sprint management
    #[command(subcommand)]
    Sprint(commands::sprint::SprintCommand),

    /// Comment management
    #[command(subcommand)]
    Comment(commands::comment::CommentCommand),

    /// Member management
    #[command(subcommand)]
    Member(commands::member::MemberCommand),

    /// Agent token management
    #[command(subcommand)]
    Agent(commands::agent::AgentCommand),

    /// Workflow configuration
    #[command(subcommand)]
    Workflow(commands::workflow::WorkflowCommand),

    /// Import data from external sources
    #[command(subcommand)]
    Import(commands::import::ImportCommand),

    /// Visualization commands
    #[command(subcommand)]
    Viz(commands::viz::VizCommand),

    /// Generate shell completion scripts
    Completion {
        /// Shell type: bash, zsh, or fish
        shell: String,
    },
}
