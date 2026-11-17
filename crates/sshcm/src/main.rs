//! sshcm: SSH Connection Manager.
//!
//! This binary only collects input and shows results. Reading and writing
//! the config file happens in `sshcm-core`.

mod cli;
mod commands;
mod error;
mod exec;
mod i18n;
mod prompt;
mod settings;
mod style;
mod tui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::FromArgMatches;

use crate::cli::{Cli, Cmd};
use crate::commands::Ctx;
use crate::error::{AppError, Result};
use crate::i18n::{Lang, tr};
use crate::settings::Settings;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if let AppError::Message(message) = &err {
                eprintln!("{} {message}", style::red(tr("error:", "hata:")));
            }
            ExitCode::from(err.exit_code())
        }
    }
}

fn run() -> Result<()> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::msg("HOME is not set."))?;

    // A broken settings file must not lock the user out of
    // `sshcm config set`, so fall back to the defaults and say so.
    let (settings, settings_error) = match Settings::load(&home) {
        Ok(settings) => (settings, None),
        Err(err) => (Settings::default(), Some(err)),
    };

    // The language has to be known before the arguments are parsed, because
    // the help texts depend on it.
    i18n::set(language(&settings));
    if let Some(AppError::Message(message)) = settings_error {
        eprintln!("{} {message}", style::yellow(tr("warning:", "uyarı:")));
    }

    let cli = Cli::from_arg_matches(&cli::command().get_matches()).unwrap_or_else(|err| err.exit());
    let ctx = Ctx {
        home,
        config: cli.config,
        dry_run: cli.dry_run,
        yes: cli.yes,
        settings,
    };
    prompt::init();

    match cli.command {
        None => tui::run(&ctx),
        Some(Cmd::List { tag }) => commands::list::run(&ctx, tag.as_deref()),
        Some(Cmd::Show { alias }) => commands::show::run(&ctx, alias),
        Some(Cmd::Add { alias, fields }) => commands::add::run(&ctx, alias, &fields),
        Some(Cmd::Edit {
            alias,
            rename,
            fields,
        }) => commands::edit::run(&ctx, alias, rename, &fields),
        Some(Cmd::Rm { alias }) => commands::rm::run(&ctx, alias),
        Some(Cmd::Connect { alias, ssh_args }) => commands::connect::run(&ctx, alias, &ssh_args),
        Some(Cmd::Config { action }) => commands::config::run(&ctx, action),
    }
}

/// Language order: `--lang`, `SSHCM_LANG`, the settings file, English.
fn language(settings: &Settings) -> Lang {
    let args: Vec<String> = std::env::args().collect();
    let before_dashes = args.iter().take_while(|arg| *arg != "--");
    let from_flag = before_dashes.enumerate().find_map(|(i, arg)| {
        if let Some(value) = arg.strip_prefix("--lang=") {
            Lang::parse(value)
        } else if arg == "--lang" {
            args.get(i + 1).and_then(|v| Lang::parse(v))
        } else {
            None
        }
    });
    from_flag
        .or_else(|| {
            std::env::var("SSHCM_LANG")
                .ok()
                .and_then(|v| Lang::parse(&v))
        })
        .unwrap_or_else(|| settings.lang())
}
