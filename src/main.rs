use clap::Parser;
use tokanban::cli::{Cli, Command};
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use tokanban::{commands, config, auth, error};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = run(cli).await;

    if let Err(e) = result {
        eprintln!("{}", e.render());
        std::process::exit(e.exit_code());
    }
}

async fn run(cli: Cli) -> error::Result<()> {
    // Load config
    let mut app_config = config::load_config(cli.config.as_ref())?;

    // Apply CLI overrides
    cli.apply_overrides(&mut app_config);

    // Determine output format
    let output_format = OutputFormat::detect(cli.format.as_deref(), cli.quiet);

    match &cli.command {
        // Shell completions need no config or token.
        Command::Completion { shell } => return commands::completion::handle(shell),

        // Auth commands get direct config access (no token required)
        Command::Auth(cmd) => {
            return commands::auth::handle(cmd, &mut app_config, cli.config.as_ref()).await;
        }

        // All other commands require authentication
        cmd => {
            // Build the execution context
            let mut ctx = Ctx::new(
                app_config,
                cli.config.clone(),
                cli.quiet,
                cli.verbose,
                output_format,
                cli.no_color,
            )?;

            // Silently refresh token if needed
            auth::ensure_valid_token(
                &mut ctx.config,
                &mut ctx.api,
                ctx.config_path.as_ref(),
            )
            .await?;

            match cmd {
                Command::Auth(_) | Command::Completion { .. } => unreachable!(),
                Command::Workspace(cmd) => commands::workspace::handle(cmd, &mut ctx).await,
                Command::Project(cmd) => commands::project::handle(cmd, &mut ctx).await,
                Command::Task(cmd) => commands::task::handle(cmd, &ctx).await,
                Command::Sprint(cmd) => commands::sprint::handle(cmd, &ctx).await,
                Command::Comment(cmd) => commands::comment::handle(cmd, &ctx).await,
                Command::Member(cmd) => commands::member::handle(cmd, &ctx).await,
                Command::Agent(cmd) => commands::agent::handle(cmd, &ctx).await,
                Command::Workflow(cmd) => commands::workflow::handle(cmd, &ctx).await,
                Command::Import(cmd) => commands::import::handle(cmd, &ctx).await,
                Command::Viz(cmd) => commands::viz::handle(cmd, &ctx).await,
            }
        }
    }
}
