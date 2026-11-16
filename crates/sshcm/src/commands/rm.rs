//! `sshcm rm`: show the block, warn about dependents, confirm, remove.

use sshcm_core::Change;

use super::{Ctx, dry_run_note, nothing_changed, pick, print_changes, report_saved, warn};
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::{prompt, style};

pub fn run(ctx: &Ctx, alias: Option<String>) -> Result<()> {
    let mut store = ctx.load()?;
    let entry = pick(
        &store,
        alias,
        true,
        tr(
            "Which connection do you want to remove?",
            "Hangi bağlantıyı silmek istiyorsunuz?",
        ),
    )?;
    let alias = entry.alias().to_string();

    println!(
        "{}",
        style::dim(&trf(
            "Will be removed from {}",
            "{} dosyasından silinecek",
            &[&store.display_path()]
        ))
    );
    let block: Vec<(Change, String)> = entry
        .text
        .lines()
        .map(|line| (Change::Removed, line.to_string()))
        .collect();
    print_changes(&block);

    // Connections that jump through this one stop working once it is gone.
    let dependents = store.dependents(&alias);
    if !dependents.is_empty() {
        println!();
        let count = dependents.len().to_string();
        let names = style::bold(&dependents.join(", "));
        let headline = if dependents.len() == 1 {
            trf(
                "1 connection uses {} as its jump host: {}",
                "1 bağlantı {} kaydını atlama sunucusu olarak kullanıyor: {}",
                &[&alias, &names],
            )
        } else {
            trf(
                "{} connections use {} as their jump host: {}",
                "{} bağlantı {} kaydını atlama sunucusu olarak kullanıyor: {}",
                &[&count, &alias, &names],
            )
        };
        warn(&headline);
        println!(
            "  {}",
            style::dim(tr(
                "They will not connect until you change their proxy.",
                "Proxy ayarlarını değiştirene kadar bağlanamazlar."
            ))
        );
    }

    if ctx.dry_run {
        dry_run_note();
        return Ok(());
    }
    println!();
    // The default answer is No.
    let question = if dependents.is_empty() {
        trf("Remove {}?", "{} silinsin mi?", &[&alias])
    } else {
        trf("Remove {} anyway?", "{} yine de silinsin mi?", &[&alias])
    };
    if !ctx.yes && !prompt::confirm(&question, false)? {
        nothing_changed();
        return Ok(());
    }

    store.doc.remove(&alias)?;
    let report = store.save(&ctx.save_options(None))?;
    report_saved(
        &trf("Removed {}", "{} silindi", &[&alias]),
        &store,
        &report,
        None,
    );
    Ok(())
}
