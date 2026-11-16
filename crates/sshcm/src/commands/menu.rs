//! The main menu shown when sshcm runs without a subcommand.

use super::{Ctx, add, connect, edit, list, rm};
use crate::cli::Fields;
use crate::error::{AppError, Result};
use crate::i18n::{tr, trf};
use crate::prompt::{self, Choice};
use crate::style;

#[derive(Debug, Clone, Copy)]
enum Action {
    Connect,
    Add,
    Edit,
    Remove,
    List,
    Quit,
}

pub fn run(ctx: &Ctx) -> Result<()> {
    loop {
        let store = ctx.load()?;
        let count = store.entries().iter().filter(|e| !e.is_pattern).count();
        let connections = if count == 1 {
            tr("1 connection", "1 bağlantı").to_string()
        } else {
            trf("{} connections", "{} bağlantı", &[&count.to_string()])
        };
        println!(
            "{}",
            style::dim(&format!(
                "sshcm {} · {connections} · {}",
                env!("CARGO_PKG_VERSION"),
                store.display_path()
            ))
        );
        println!();

        let width = 10;
        let choices = vec![
            Choice::described(
                tr("Connect", "Bağlan"),
                width,
                tr("Open a connection with ssh", "ssh ile bağlantı aç"),
                Action::Connect,
            ),
            Choice::described(
                tr("Add", "Ekle"),
                width,
                tr("Create a new connection", "Yeni bağlantı oluştur"),
                Action::Add,
            ),
            Choice::described(
                tr("Edit", "Düzenle"),
                width,
                tr(
                    "Change an existing connection",
                    "Mevcut bir bağlantıyı değiştir",
                ),
                Action::Edit,
            ),
            Choice::described(
                tr("Remove", "Sil"),
                width,
                tr("Delete a connection", "Bir bağlantıyı sil"),
                Action::Remove,
            ),
            Choice::described(
                tr("List", "Listele"),
                width,
                tr("Show all connections", "Tüm bağlantıları göster"),
                Action::List,
            ),
            Choice::described(tr("Quit", "Çık"), width, "", Action::Quit),
        ];
        let action = prompt::select(
            tr("What do you want to do?", "Ne yapmak istiyorsunuz?"),
            choices,
            0,
        )?;

        let outcome = match action {
            Action::Connect => connect::run(ctx, None, &[]),
            Action::Add => add::run(ctx, None, &Fields::default()),
            Action::Edit => edit::run(ctx, None, None, &Fields::default()),
            Action::Remove => rm::run(ctx, None),
            Action::List => list::run(ctx, None),
            Action::Quit => return Ok(()),
        };
        match outcome {
            Ok(()) => {}
            // Esc inside an action goes back to the menu.
            Err(AppError::Cancelled) => {}
            Err(AppError::Message(message)) => {
                eprintln!("{} {message}", style::red(tr("error:", "hata:")));
            }
            Err(err @ AppError::Interrupted) => return Err(err),
        }
        println!();
    }
}
