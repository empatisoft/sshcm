//! One module per subcommand, plus the pieces they share.

pub mod add;
pub mod backups;
pub mod config;
pub mod connect;
pub mod copy;
pub mod edit;
pub mod list;
pub mod rm;
pub mod shell;
pub mod show;

use std::path::PathBuf;

use sshcm_core::model::collapse_tilde;
use sshcm_core::{
    Change, Connection, DocError, Entry, Proxy, SaveOptions, SaveReport, Store, Verification,
};

use crate::error::{AppError, Result};
use crate::i18n::{tr, trf};
use crate::settings::Settings;
use crate::{exec, prompt, style};

/// What every command needs: where things are and the global flags.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub home: PathBuf,
    /// `--config`; `None` means `~/.ssh/config`.
    pub config: Option<PathBuf>,
    pub dry_run: bool,
    pub yes: bool,
    pub settings: Settings,
}

impl Ctx {
    pub fn load(&self) -> Result<Store> {
        let path = self
            .config
            .clone()
            .unwrap_or_else(|| Store::default_path(&self.home));
        Ok(Store::load(path, &self.home)?)
    }

    pub fn save_options(&self, verify_alias: Option<&str>) -> SaveOptions {
        SaveOptions {
            backup_keep: self.settings.backup_keep,
            verify_alias: verify_alias.map(str::to_string),
        }
    }
}

/// Short proxy label for lists: `via bastion`, `socks5`, `http`, `command`.
pub fn proxy_short(proxy: &Proxy) -> String {
    match proxy {
        Proxy::None => String::new(),
        Proxy::Jump(hops) => format!("via {}", hops.join(",")),
        Proxy::Socks5(_) => "socks5".to_string(),
        Proxy::Http(_) => "http".to_string(),
        Proxy::Command(_) => "command".to_string(),
    }
}

/// The file a block lives in, with the home directory shortened to `~`.
pub fn location(store: &Store, entry: &Entry) -> String {
    match &entry.source {
        Some(path) => collapse_tilde(&path.to_string_lossy(), store.home()),
        None => store.display_path(),
    }
}

/// Finds the connection to work on: by name when given, otherwise from a
/// list. Patterns are never offered.
pub fn pick(store: &Store, alias: Option<String>, editable: bool, message: &str) -> Result<Entry> {
    if let Some(alias) = alias {
        let entry = store
            .find(&alias)
            .ok_or_else(|| DocError::NotFound(alias.clone()))?;
        if (editable && entry.read_only()) || entry.is_pattern {
            return Err(DocError::ReadOnly(alias).into());
        }
        return Ok(entry);
    }

    let entries: Vec<Entry> = store
        .entries()
        .into_iter()
        .filter(|e| !e.is_pattern && !(editable && e.read_only()))
        .collect();
    if entries.is_empty() {
        return Err(AppError::msg(tr(
            "There are no connections yet. Add one with: sshcm add",
            "Henüz bağlantı yok. Eklemek için: sshcm add",
        )));
    }
    prompt::select_entry(message, entries)
}

/// Prints block lines with a colored `+`, `-` or blank marker.
pub fn print_changes(lines: &[(Change, String)]) {
    for (change, text) in lines {
        let line = match change {
            Change::Added => style::green(&format!("+ {text}")),
            Change::Removed => style::red(&format!("- {text}")),
            Change::Same => style::dim(&format!("  {text}")),
        };
        println!("  {line}");
    }
}

pub fn dry_run_note() {
    println!();
    println!(
        "{}",
        style::dim(tr(
            "Dry run: nothing was written.",
            "Deneme çalıştırması: hiçbir şey yazılmadı."
        ))
    );
}

pub fn nothing_changed() {
    println!(
        "{}",
        tr("Nothing was changed.", "Hiçbir şey değiştirilmedi.")
    );
}

/// The lines after a successful write: what was saved, where the backup is,
/// and whether ssh accepted the file.
pub fn report_saved(
    headline: &str,
    store: &Store,
    report: &SaveReport,
    verify_alias: Option<&str>,
) {
    let mut line = format!("{} {headline}", style::green("✓"));
    if !report.backups.is_empty() {
        let shown: Vec<String> = report
            .backups
            .iter()
            .map(|path| collapse_tilde(&path.to_string_lossy(), store.home()))
            .collect();
        line.push_str("  ");
        line.push_str(&style::dim(&trf(
            "backup: {}",
            "yedek: {}",
            &[&shown.join(", ")],
        )));
    }
    println!("{line}");
    match report.verification {
        Verification::Passed => {
            let command = format!("ssh -G {}", verify_alias.unwrap_or("…"));
            println!(
                "{} {}  {}",
                style::green("✓"),
                tr("Config check passed", "Config denetimi geçti"),
                style::dim(if verify_alias.is_some() {
                    &command
                } else {
                    "ssh -G"
                })
            );
        }
        Verification::Skipped => warn(tr(
            "The ssh command was not found, so the config was not checked.",
            "ssh komutu bulunamadığı için config dosyası denetlenmedi.",
        )),
    }
}

pub fn warn(message: &str) {
    println!("{} {message}", style::yellow(&style::bold("!")));
}

/// SOCKS5 and HTTP proxies rely on the OpenBSD `nc`; say so when it is
/// missing.
pub fn warn_if_nc_missing(conn: &Connection) {
    if let Some(message) = nc_warning(conn) {
        warn(&message);
    }
}

/// The warning to show for a connection whose proxy needs `nc`, when the
/// `nc` on this machine is missing or cannot do it.
pub fn nc_warning(conn: &Connection) -> Option<String> {
    if !conn.proxy.needs_nc() {
        return None;
    }
    match exec::nc_support() {
        exec::Nc::Usable => None,
        exec::Nc::Missing => Some(
            tr(
                "The nc command was not found. SOCKS5 and HTTP proxies need the OpenBSD netcat (package netcat-openbsd on Debian and Ubuntu).",
                "nc komutu bulunamadı. SOCKS5 ve HTTP proxy için OpenBSD netcat gerekir (Debian ve Ubuntu'da netcat-openbsd paketi).",
            )
            .to_string(),
        ),
        exec::Nc::NoProxyOption => Some(
            tr(
                "The nc on this machine has no -X option, so this proxy will not work here. Install the OpenBSD netcat (netcat-openbsd on Debian and Ubuntu, openbsd-netcat on Arch, netcat on Fedora).",
                "Bu makinedeki nc komutunda -X seçeneği yok, bu proxy burada çalışmaz. OpenBSD netcat kurun (Debian ve Ubuntu'da netcat-openbsd, Arch'ta openbsd-netcat, Fedora'da netcat).",
            )
            .to_string(),
        ),
    }
}

/// One connection as JSON. Keys follow the ssh_config keywords.
pub fn entry_json(store: &Store, entry: &Entry) -> serde_json::Value {
    let conn = &entry.conn;
    let (jump, command) = match &conn.proxy {
        Proxy::None => (None, None),
        Proxy::Jump(hops) => (Some(hops.clone()), None),
        other => (None, other.directive().map(|(_, value)| value)),
    };
    let other: Vec<serde_json::Value> = conn
        .other
        .iter()
        .map(|(key, value)| serde_json::json!({ "key": key, "value": value }))
        .collect();
    serde_json::json!({
        "alias": conn.alias,
        "aliases": entry.patterns,
        "hostname": conn.hostname,
        "user": conn.user,
        "port": conn.effective_port(),
        "identity_file": conn.identity_file,
        "proxy_jump": jump,
        "proxy_command": command,
        "description": conn.description,
        "tags": conn.tags,
        "other": other,
        "file": location_path(store, entry),
        "read_only": entry.read_only(),
    })
}

/// The file a block lives in, as a full path.
fn location_path(store: &Store, entry: &Entry) -> String {
    entry
        .source
        .as_deref()
        .unwrap_or(store.path())
        .to_string_lossy()
        .into_owned()
}

/// Asks whether to connect right away, and hands over to ssh on yes.
pub fn offer_connect(ctx: &Ctx, alias: &str) -> Result<()> {
    if ctx.yes {
        return Ok(());
    }
    println!();
    let question = trf(
        "Connect to {} now?",
        "{} bağlantısı şimdi açılsın mı?",
        &[alias],
    );
    if prompt::confirm(&question, false)? {
        return Err(exec::ssh(ctx.config.as_deref(), alias, &[]));
    }
    Ok(())
}
