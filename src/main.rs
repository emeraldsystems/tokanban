use clap::Parser;
use tokanban::cli::{Cli, Command};
use tokanban::ctx::Ctx;
use tokanban::format::OutputFormat;
use tokanban::{auth, commands, config, error};

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
    // Determine output format early so local commands can use it without auth.
    let output_format = OutputFormat::detect(cli.format.as_deref(), cli.quiet);

    match &cli.command {
        // Shell completions need no config or token.
        Command::Completion { shell } => return commands::completion::handle(shell),

        // Doctor is read-only and offline by default: it works even when config is
        // missing or malformed, so it never goes through the config `?` below.
        Command::Doctor(args) => {
            return commands::doctor::handle(
                args,
                cli.config.as_ref(),
                cli.api_url.as_deref(),
                output_format,
                cli.no_color,
            )
            .await
        }

        // Auth commands get direct config access (no token required)
        Command::Auth(cmd) => {
            let mut app_config = config::load_config(cli.config.as_ref())?;
            cli.apply_overrides(&mut app_config);
            return commands::auth::handle(cmd, &mut app_config, cli.config.as_ref()).await;
        }

        // Local memory scoring does not need config or token.
        Command::Memory(cmd) => {
            return commands::memory::handle(cmd).await;
        }

        // Init bootstraps a harness (MCP config, memory block, usage hooks)
        // and gets direct config access like `auth`/`doctor`: it must work
        // without a token, and read-only steps must survive a missing or
        // malformed config file rather than erroring out via the `?` below.
        Command::Init(args) => {
            // Skill-only installation is offline and independent of account configuration.
            let mut app_config = if args.skills_only {
                config::AppConfig::default()
            } else {
                config::load_config(cli.config.as_ref()).map_err(|_| tokanban::error::CliError::Config("Tokanban config could not be read. Fix the selected config file before initializing a harness.".to_string()))?
            };
            cli.apply_overrides(&mut app_config);
            return commands::init::handle(args, &app_config, output_format, cli.no_color);
        }

        Command::Repo(commands::repo::RepoCommand::Inspect {
            path,
            binding: false,
        }) => {
            return commands::repo::handle_inspect_offline(
                path.clone(),
                output_format,
                cli.no_color,
            );
        }

        // Hidden session hook helpers are best-effort and manage auth resolution
        // internally so hook failures never interrupt the calling harness.
        Command::Session(cmd) => {
            // A malformed selected account must not silently become the
            // default account. Hooks remain best-effort without networking.
            let Ok(mut app_config) = config::load_config(cli.config.as_ref()) else {
                return Ok(());
            };
            cli.apply_overrides(&mut app_config);
            return commands::session::handle(cmd, &app_config).await;
        }

        // All other commands require authentication
        cmd => {
            let mut app_config = config::load_config(cli.config.as_ref())?;
            cli.apply_overrides(&mut app_config);

            // Build the execution context
            let mut ctx = Ctx::new(
                app_config,
                cli.config.clone(),
                cli.quiet,
                cli.verbose,
                output_format,
                cli.no_color,
            )?;
            ctx.api.set_run_attribution(
                cli.persona_key.clone(),
                cli.teammate_id.clone(),
                cli.session_id.clone(),
            );

            // Silently refresh token if needed
            auth::ensure_valid_token(&mut ctx.config, &mut ctx.api, ctx.config_path.as_ref())
                .await?;

            match cmd {
                Command::Auth(_)
                | Command::Completion { .. }
                | Command::Memory(_)
                | Command::Doctor(_) => {
                    unreachable!()
                }
                Command::Init(_) => unreachable!(),
                Command::Session(_) => unreachable!(),
                Command::Workspace(cmd) => commands::workspace::handle(cmd, &mut ctx).await,
                Command::Project(cmd) => commands::project::handle(cmd, &mut ctx).await,
                Command::Persona(cmd) => commands::persona::handle(cmd, &ctx).await,
                Command::Team(cmd) => commands::team::handle(cmd, &ctx).await,
                Command::Task(cmd) => commands::task::handle(cmd, &ctx).await,
                Command::Entity(cmd) => commands::entity::handle(cmd, &ctx).await,
                Command::Sprint(cmd) => commands::sprint::handle(cmd, &ctx).await,
                Command::Comment(cmd) => commands::comment::handle(cmd, &ctx).await,
                Command::Member(cmd) => commands::member::handle(cmd, &ctx).await,
                Command::Agent(cmd) => commands::agent::handle(cmd, &ctx).await,
                Command::Workflow(cmd) => commands::workflow::handle(cmd, &ctx).await,
                Command::Import(cmd) => commands::import::handle(cmd, &ctx).await,
                Command::Viz(cmd) => commands::viz::handle(cmd, &ctx).await,
                Command::Usage(args) => commands::usage::handle(args, &ctx).await,
                Command::Repo(cmd) => commands::repo::handle(cmd, &ctx).await,
                Command::Git(cmd) => commands::git::handle(cmd, &ctx).await,
                Command::Followup(cmd) => commands::followup::handle(cmd, &ctx).await,
            }
        }
    }
}
