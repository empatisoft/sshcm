//! Shell completions and the man page, both generated from the command
//! definition so they cannot drift from it.

use clap_complete::Shell;

use crate::cli;
use crate::error::{AppError, Result};

/// `sshcm completions <shell>`: prints the script to standard output.
pub fn completions(shell: Shell) -> Result<()> {
    let mut command = cli::command();
    clap_complete::generate(shell, &mut command, "sshcm", &mut std::io::stdout());
    Ok(())
}

/// `sshcm man`: prints the man page in roff format.
pub fn man() -> Result<()> {
    clap_mangen::Man::new(cli::command())
        .render(&mut std::io::stdout())
        .map_err(|err| AppError::Message(err.to_string()))
}
