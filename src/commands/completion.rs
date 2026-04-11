use crate::error::{CliError, Result};

/// Generate a shell completion script for the given shell.
///
/// Usage: `tokanban completion bash|zsh|fish`
///
/// Per spec §7.1:
/// ```bash
/// tokanban completion bash > ~/.bash_completion.d/tokanban
/// tokanban completion zsh  > ~/.zsh/completions/_tokanban
/// tokanban completion fish > ~/.config/fish/completions/tokanban.fish
/// ```
pub fn handle(shell: &str) -> Result<()> {
    generate_to(shell, &mut std::io::stdout())
}

/// Generate a completion script to an arbitrary writer (testable variant).
pub fn generate_to(shell: &str, out: &mut impl std::io::Write) -> Result<()> {
    use clap::CommandFactory;
    use clap_complete::{generate, shells};

    let mut cmd = crate::cli::Cli::command();
    let bin_name = "tokanban";

    match shell.to_ascii_lowercase().as_str() {
        "bash" => generate(shells::Bash, &mut cmd, bin_name, out),
        "zsh" => generate(shells::Zsh, &mut cmd, bin_name, out),
        "fish" => generate(shells::Fish, &mut cmd, bin_name, out),
        other => {
            return Err(CliError::Config(format!(
                "Unknown shell '{other}'. Supported: bash, zsh, fish"
            )));
        }
    }

    Ok(())
}
