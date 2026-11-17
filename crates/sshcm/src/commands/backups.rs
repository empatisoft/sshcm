//! `sshcm backups` and `sshcm restore`.
//!
//! Restoring is an ordinary write: the content being replaced is backed up
//! first and the result is checked with ssh, so a restore can be undone by
//! another restore.

use sshcm_core::model::collapse_tilde;
use sshcm_core::{Backup, Change, Store, diff_lines};

use super::{Ctx, dry_run_note, nothing_changed, print_changes, report_saved};
use crate::error::{AppError, Result};
use crate::i18n::{tr, trf};
use crate::prompt::{self, Choice};
use crate::style::{self, pad};

/// `20261117-084622` as `2026-11-17 08:46:22`.
fn readable(time: &str) -> String {
    match time.as_bytes() {
        [y @ .., b'-', _, _, _, _, _, _] if y.len() == 8 && time.is_ascii() => format!(
            "{}-{}-{} {}:{}:{}",
            &time[0..4],
            &time[4..6],
            &time[6..8],
            &time[9..11],
            &time[11..13],
            &time[13..15]
        ),
        _ => time.to_string(),
    }
}

fn tilde(store: &Store, path: &std::path::Path) -> String {
    collapse_tilde(&path.to_string_lossy(), store.home())
}

pub fn list(ctx: &Ctx) -> Result<()> {
    let store = ctx.load()?;
    let backups = store.all_backups()?;
    if backups.is_empty() {
        println!("{}", tr("There are no backups yet.", "Henüz yedek yok."));
        return Ok(());
    }
    let rows: Vec<[String; 3]> = backups
        .iter()
        .map(|b| {
            [
                readable(&b.time),
                tilde(&store, &b.file),
                tilde(&store, &b.path),
            ]
        })
        .collect();
    let headers = [
        tr("TAKEN", "ALINDI"),
        tr("FILE", "DOSYA"),
        tr("BACKUP", "YEDEK"),
    ];
    let widths: Vec<usize> = (0..3)
        .map(|col| {
            rows.iter()
                .map(|row| row[col].chars().count())
                .chain([headers[col].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: [&str; 3]| {
        format!(
            "{}  {}  {}",
            pad(cells[0], widths[0]),
            pad(cells[1], widths[1]),
            cells[2]
        )
    };
    println!("{}", style::dim(&line(headers)));
    for row in &rows {
        println!("{}", line([&row[0], &row[1], &row[2]]));
    }
    Ok(())
}

/// Finds the backup named on the command line: by file name or by path.
fn find(store: &Store, backups: &[Backup], name: &str) -> Option<Backup> {
    backups
        .iter()
        .find(|b| {
            b.path
                .file_name()
                .is_some_and(|f| f.to_string_lossy() == name)
                || b.path.to_string_lossy() == name
                || tilde(store, &b.path) == name
        })
        .cloned()
}

pub fn restore(ctx: &Ctx, backup: Option<String>) -> Result<()> {
    let mut store = ctx.load()?;
    let backups = store.all_backups()?;
    if backups.is_empty() {
        return Err(AppError::msg(tr(
            "There are no backups yet.",
            "Henüz yedek yok.",
        )));
    }

    let backup = match backup {
        Some(name) => find(&store, &backups, &name).ok_or_else(|| {
            AppError::Message(trf(
                "No backup named {}. See: sshcm backups",
                "{} adında bir yedek yok. Bakınız: sshcm backups",
                &[&name],
            ))
        })?,
        None => {
            let choices = backups
                .iter()
                .map(|b| {
                    let label = format!("{}  {}", readable(&b.time), tilde(&store, &b.file));
                    Choice::plain(&label, b.clone())
                })
                .collect();
            prompt::select(
                tr(
                    "Which backup do you want to restore?",
                    "Hangi yedeği geri yüklemek istiyorsunuz?",
                ),
                choices,
                0,
            )?
        }
    };

    let file = tilde(&store, &backup.file);
    let current = store.content_of(&backup.file).unwrap_or_default();
    store.restore(&backup)?;
    let restored = store.content_of(&backup.file).unwrap_or_default();
    if !store.is_modified() {
        println!(
            "{}",
            trf(
                "{} already has the content of this backup.",
                "{} zaten bu yedeğin içeriğine sahip.",
                &[&file]
            )
        );
        return Ok(());
    }

    println!(
        "{}",
        style::dim(&trf(
            "Changes to {} · backup taken {}",
            "{} üzerindeki değişiklikler · yedek tarihi {}",
            &[&file, &readable(&backup.time)]
        ))
    );
    // Only the lines that differ; a config can be long.
    let diff = diff_lines(&current, &restored);
    let unchanged = diff.iter().filter(|(c, _)| *c == Change::Same).count();
    let changed: Vec<(Change, String)> = diff
        .into_iter()
        .filter(|(change, _)| *change != Change::Same)
        .collect();
    print_changes(&changed);
    println!(
        "{}",
        style::dim(&trf(
            "{} other lines stay as they are.",
            "Diğer {} satır olduğu gibi kalır.",
            &[&unchanged.to_string()]
        ))
    );

    if ctx.dry_run {
        dry_run_note();
        return Ok(());
    }
    println!();
    // Restoring replaces the file, so the default answer is No.
    let question = trf("Restore {}?", "{} geri yüklensin mi?", &[&file]);
    if !ctx.yes && !prompt::confirm(&question, false)? {
        nothing_changed();
        return Ok(());
    }

    let report = store.save(&ctx.save_options(None))?;
    report_saved(
        &trf("Restored {}", "{} geri yüklendi", &[&file]),
        &store,
        &report,
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::readable;

    #[test]
    fn backup_times_are_shown_readably() {
        assert_eq!(readable("20261117-084622"), "2026-11-17 08:46:22");
        assert_eq!(readable("odd"), "odd");
        assert_eq!(readable("2026111-0846220"), "2026111-0846220");
    }
}
