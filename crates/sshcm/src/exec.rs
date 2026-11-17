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

/// What the `nc` on this machine can do for SOCKS5 and HTTP proxies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nc {
    /// No `nc` on the `PATH`.
    Missing,
    /// An `nc` without `-X`: GNU netcat, ncat, BusyBox.
    NoProxyOption,
    /// The OpenBSD netcat, which the written `ProxyCommand` needs.
    Usable,
}

/// Looks at the help text of `nc` for the `-X` option. Nothing is connected.
pub fn nc_support() -> Nc {
    if !on_path("nc") {
        return Nc::Missing;
    }
    let Ok(output) = Command::new("nc")
        .arg("-h")
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return Nc::Missing;
    };
    // OpenBSD nc prints its usage to stderr, others to stdout.
    let help = [output.stdout, output.stderr].concat();
    if String::from_utf8_lossy(&help).contains("-X") {
        Nc::Usable
    } else {
        Nc::NoProxyOption
    }
}

/// True when `program` is found in one of the `PATH` directories.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
