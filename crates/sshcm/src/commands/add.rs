//! `sshcm add`: ask for the settings, preview the block, save.

use sshcm_core::model::{DEFAULT_PORT, validate};
use sshcm_core::{Change, Connection, render_block_text};

use super::{
    Ctx, dry_run_note, nothing_changed, offer_connect, print_changes, report_saved,
    warn_if_nc_missing,
};
use crate::cli::Fields;
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::prompt::{self, form};
use crate::style;

pub fn run(ctx: &Ctx, alias: Option<String>, fields: &Fields) -> Result<()> {
    let mut store = ctx.load()?;

    let mut preset = Connection::default();
    fields.apply(&mut preset, &ctx.home)?;
    let skip = if ctx.yes {
        form::Skip::all()
    } else {
        form::Skip {
            alias: alias.is_some(),
            hostname: fields.hostname.is_some(),
            user: fields.user.is_some(),
            port: fields.port.is_some(),
            identity_file: fields.identity_file.is_some(),
            proxy: fields.has_proxy(),
            description: fields.desc.is_some(),
            tags: fields.tags.is_some(),
        }
    };
    preset.alias = alias.unwrap_or_default();

    let conn = form::fill(&store, preset, None, &skip)?;
    validate(&conn, None, &store.known(None), &ctx.home, true)?;

    if !ctx.yes {
        // Separate the preview from the questions above it.
        println!();
    }
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
    if conn.port == Some(DEFAULT_PORT) {
        println!(
            "{}",
            style::dim(tr(
                "Port 22 is the default, so it is not written.",
                "Port 22 varsayılan olduğu için yazılmaz."
            ))
        );
    }
    warn_if_nc_missing(&conn);

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

    store.doc.add(&conn);
    let report = store.save(&ctx.save_options(Some(&conn.alias)))?;
    report_saved(
        &trf("Saved {}", "{} kaydedildi", &[&conn.alias]),
        &store,
        &report,
        Some(&conn.alias),
    );
    offer_connect(ctx, &conn.alias)
}
