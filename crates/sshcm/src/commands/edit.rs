//! `sshcm edit`: the same questions as `add`, starting from the current
//! values, then a diff of the block instead of the whole block.

use sshcm_core::diff_lines;
use sshcm_core::model::validate;

use super::{
    Ctx, dry_run_note, nothing_changed, offer_connect, pick, print_changes, report_saved,
    warn_if_nc_missing,
};
use crate::cli::Fields;
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::prompt::{self, form};
use crate::style;

pub fn run(
    ctx: &Ctx,
    alias: Option<String>,
    rename: Option<String>,
    fields: &Fields,
) -> Result<()> {
    let mut store = ctx.load()?;
    let entry = pick(
        &store,
        alias,
        true,
        tr(
            "Which connection do you want to edit?",
            "Hangi bağlantıyı düzenlemek istiyorsunuz?",
        ),
    )?;
    let original = entry.alias().to_string();

    let mut preset = entry.conn.clone();
    fields.apply(&mut preset, &ctx.home)?;
    // Values given as flags are the whole edit; otherwise ask every question.
    let direct = ctx.yes || rename.is_some() || fields.any();
    if let Some(new_alias) = rename {
        preset.alias = new_alias.trim().to_string();
    }
    let conn = if direct {
        preset
    } else {
        form::fill(&store, preset, Some(&original), &form::Skip::default())?
    };

    // An unchanged key path is not re-checked: the block may point at a key
    // that only exists on another machine.
    let key_changed = conn.identity_file != entry.conn.identity_file;
    validate(
        &conn,
        Some(&original),
        &store.known(Some(&original)),
        &ctx.home,
        key_changed,
    )?;

    // Apply the edit to a copy first: it gives the preview, and the real
    // store stays untouched until the user confirms.
    let mut edited = store.clone();
    edited.update(&original, &conn)?;
    if !edited.is_modified() {
        println!("{}", tr("No changes.", "Değişiklik yok."));
        return Ok(());
    }

    if !direct {
        // Separate the preview from the questions above it.
        println!();
    }
    println!(
        "{}",
        style::dim(&trf(
            "Changes to {} · unchanged lines are shown dimmed",
            "{} üzerindeki değişiklikler · değişmeyen satırlar soluk gösterilir",
            &[&original]
        ))
    );
    let new_text = edited.block_text(&conn.alias).unwrap_or_default();
    print_changes(&diff_lines(&entry.text, &new_text));
    if conn.alias != original {
        let dependents = store.dependents(&original);
        if !dependents.is_empty() {
            println!(
                "{}",
                style::dim(&trf(
                    "ProxyJump is also updated in: {}",
                    "ProxyJump şu kayıtlarda da güncellenir: {}",
                    &[&dependents.join(", ")]
                ))
            );
        }
    }
    warn_if_nc_missing(&conn);

    if ctx.dry_run {
        dry_run_note();
        return Ok(());
    }
    println!();
    if !ctx.yes
        && !prompt::confirm(
            tr("Apply these changes?", "Bu değişiklikler uygulansın mı?"),
            true,
        )?
    {
        nothing_changed();
        return Ok(());
    }

    store = edited;
    let report = store.save(&ctx.save_options(Some(&conn.alias)))?;
    report_saved(
        &trf("Saved {}", "{} kaydedildi", &[&conn.alias]),
        &store,
        &report,
        Some(&conn.alias),
    );
    offer_connect(ctx, &conn.alias)
}
