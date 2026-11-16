//! Hands the process over to the system `ssh`. sshcm does not stay in
//! between: after `exec` the process *is* ssh, and its exit code is ssh's.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use crate::error::AppError;
use crate::i18n::{tr, trf};

/// Replaces this process with `ssh [-F config] alias [args...]`.
///
/// Arguments are passed as a list, never through a shell. `alias` must not
/// start with `-`; callers check that. Only returns when ssh could not be
/// started.
pub fn ssh(config: Option<&Path>, alias: &str, args: &[String]) -> AppError {
    let mut cmd = Command::new("ssh");
    if let Some(config) = config {
        cmd.arg("-F").arg(config);
    }
    // Extra arguments follow the host: ssh reads options there too, and
    // anything else becomes the remote command.
    cmd.arg(alias).args(args);
    let err = cmd.exec();
    if err.kind() == std::io::ErrorKind::NotFound {
        AppError::msg(tr(
            "The ssh command was not found. Install an OpenSSH client first.",
            "ssh komutu bulunamadı. Önce bir OpenSSH istemcisi kurun.",
        ))
    } else {
        AppError::Message(trf(
            "Could not start ssh: {}",
            "ssh başlatılamadı: {}",
            &[&err.to_string()],
        ))
    }
}

/// True when `program` is found in one of the `PATH` directories.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
