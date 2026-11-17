//! `sshcm copy`: a new connection that starts as a copy of another one.

use sshcm_core::model::validate;
use sshcm_core::{Change, ValidationError, render_block_text};

use super::{Ctx, dry_run_note, nothing_changed, offer_connect, pick, print_changes, report_saved};
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::prompt;
use crate::style;

pub fn run(ctx: &Ctx, alias: Option<String>, new_alias: Option<String>) -> Result<()> {
    let mut store = ctx.load()?;
    let source = pick(
        &store,
        alias,
        false,
        tr(
            "Which connection do you want to copy?",
            "Hangi bağlantıyı kopyalamak istiyorsunuz?",
        ),
    )?;

    let known = store.known(None);
    let new_alias = match new_alias {
        Some(alias) => alias.trim().to_string(),
        None => {
            let names = known.names.clone();
            prompt::ask_text(
                tr("Alias of the copy:", "Kopyanın alias'ı:"),
                move |input| {
                    sshcm_core::model::validate_alias(input)?;
                    if names.iter().any(|n| n == input) {
                        return Err(ValidationError::AliasTaken(input.to_string()));
                    }
                    Ok(())
                },
            )?
        }
    };

    // Every setting comes from a block that is already in the file, so only
    // the new alias (and what depends on it) is checked.
    let mut conn = source.conn.clone();
    conn.alias = new_alias;
    validate(&conn, Some(&source.conn), &known, &ctx.home)?;

    println!(
        "{}",
        style::dim(&trf(
            "Will be added to {}",
            "{} dosyasına eklenecek",
            &[&store.display_path()]
        ))
    );
    let block: Vec<(Change, String)> = render_block_text(&conn)
        .lines()
        .map(|line| (Change::Added, line.to_string()))
        .collect();
    print_changes(&block);

    if ctx.dry_run {
        dry_run_note();
        return Ok(());
    }
    println!();
    if !ctx.yes
        && !prompt::confirm(
            tr("Save this connection?", "Bu bağlantı kaydedilsin mi?"),
            true,
        )?
    {
        nothing_changed();
        return Ok(());
    }

    store.add(&conn);
    let report = store.save(&ctx.save_options(Some(&conn.alias)))?;
    report_saved(
        &trf("Saved {}", "{} kaydedildi", &[&conn.alias]),
        &store,
        &report,
        Some(&conn.alias),
    );
    offer_connect(ctx, &conn.alias)
}
