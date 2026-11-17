//! `sshcm show`: every setting of one connection and its config block.

use sshcm_core::Proxy;
use sshcm_core::model::{DEFAULT_PORT, collapse_tilde};

use super::{Ctx, pick};
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::style::{self, pad};

pub fn run(ctx: &Ctx, alias: Option<String>) -> Result<()> {
    let store = ctx.load()?;
    let entry = pick(
        &store,
        alias,
        false,
        tr(
            "Which connection do you want to see?",
            "Hangi bağlantıyı görmek istiyorsunuz?",
        ),
    )?;
    let conn = &entry.conn;
    let none = || style::dim(tr("none", "yok"));

    let port = match conn.port {
        Some(port) if port != DEFAULT_PORT => port.to_string(),
        _ => format!(
            "{DEFAULT_PORT} {}",
            style::dim(tr("(default)", "(varsayılan)"))
        ),
    };
    let proxy = match &conn.proxy {
        Proxy::None => none(),
        Proxy::Jump(hops) => trf(
            "Jump host → {}",
            "Atlama sunucusu → {}",
            &[&hops.join(" → ")],
        ),
        Proxy::Socks5(addr) => format!("SOCKS5 {addr}"),
        Proxy::Http(addr) => format!("HTTP {addr}"),
        Proxy::Command(cmd) => trf("Custom command: {}", "Özel komut: {}", &[cmd]),
    };

    let mut rows: Vec<(&str, String)> = vec![
        (tr("Alias", "Alias"), entry.patterns.join(" ")),
        ("HostName", conn.hostname.clone().unwrap_or_else(none)),
        ("User", conn.user.clone().unwrap_or_else(none)),
        ("Port", port),
        (
            tr("Identity file", "Anahtar dosyası"),
            conn.identity_file.clone().unwrap_or_else(none),
        ),
        ("Proxy", proxy),
    ];
    if let Some(description) = &conn.description {
        rows.push((tr("Description", "Açıklama"), description.clone()));
    }
    if !conn.tags.is_empty() {
        rows.push((tr("Tags", "Etiketler"), conn.tags.join(", ")));
    }
    for (i, (key, value)) in conn.other.iter().enumerate() {
        let label = if i == 0 {
            tr("Other settings", "Diğer ayarlar")
        } else {
            ""
        };
        rows.push((label, format!("{key} {value}")));
    }
    if let Some(source) = &entry.source {
        let path = collapse_tilde(&source.to_string_lossy(), &ctx.home);
        rows.push((tr("Included from", "Dahil edildiği dosya"), path));
    }

    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    for (label, value) in &rows {
        println!("{}{value}", style::dim(&pad(label, width)));
    }

    println!();
    println!("{}", style::dim(tr("Config block", "Config bloğu")));
    for line in entry.text.lines() {
        println!("  {line}");
    }
    Ok(())
}
