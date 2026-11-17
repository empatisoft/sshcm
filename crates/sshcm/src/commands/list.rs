//! `sshcm list`: every connection as a table, read-only ones at the end.

use sshcm_core::Entry;
use sshcm_core::model::collapse_tilde;

use super::{Ctx, by_group, entry_json, proxy_short};
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::style::{self, pad};

pub fn run(ctx: &Ctx, tag: Option<&str>, group: Option<&str>, json: bool) -> Result<()> {
    let store = ctx.load()?;
    let entries: Vec<Entry> = store
        .entries()
        .into_iter()
        .filter(|e| tag.is_none_or(|t| e.conn.tags.iter().any(|x| x == t)))
        .filter(|e| group.is_none_or(|g| e.conn.group.as_deref() == Some(g)))
        .collect();
    if json {
        let all: Vec<serde_json::Value> = entries.iter().map(|e| entry_json(&store, e)).collect();
        println!("{}", serde_json::Value::Array(all));
        return Ok(());
    }
    let (read_only, editable): (Vec<&Entry>, Vec<&Entry>) =
        entries.iter().partition(|e| e.read_only());
    // Connections of one group stay together, each group under its name.
    let editable = by_group(editable, |e| e.conn.group.as_deref());

    if entries.is_empty() {
        let message = match (tag, group) {
            (_, Some(group)) => trf(
                "No connections in the group {}. See: sshcm groups",
                "{} grubunda bağlantı yok. Bakınız: sshcm groups",
                &[group],
            ),
            (Some(tag), None) => trf(
                "No connections with the tag {}.",
                "{} etiketini taşıyan bağlantı yok.",
                &[tag],
            ),
            (None, None) => tr(
                "There are no connections yet. Add one with: sshcm add",
                "Henüz bağlantı yok. Eklemek için: sshcm add",
            )
            .to_string(),
        };
        println!("{message}");
        return Ok(());
    }

    if !editable.is_empty() {
        let grouped = group.is_none() && editable.iter().any(|e| e.conn.group.is_some());

        let mut rows: Vec<Vec<String>> = vec![
            ["ALIAS", "HOSTNAME", "USER", "PORT", "PROXY", "TAGS", "FILE"]
                .map(String::from)
                .to_vec(),
        ];
        rows.extend(editable.iter().map(|e| {
            vec![
                e.patterns.join(" "),
                e.conn.hostname.clone().unwrap_or_default(),
                e.conn.user.clone().unwrap_or_default(),
                e.conn.effective_port().to_string(),
                proxy_short(&e.conn.proxy),
                e.conn.tags.join(", "),
                // Only connections from included files name their file.
                e.source
                    .as_ref()
                    .map(|path| collapse_tilde(&path.to_string_lossy(), &ctx.home))
                    .unwrap_or_default(),
            ]
        }));
        // TAGS and FILE are only shown when some connection uses them.
        let rows = drop_empty_columns(rows, &[5, 6]);
        let lines = table_lines(&rows);
        println!("{}", style::dim(&lines[0]));
        let mut current: Option<Option<&str>> = None;
        for (entry, line) in editable.iter().zip(&lines[1..]) {
            let group_of_entry = entry.conn.group.as_deref();
            if grouped && current != Some(group_of_entry) {
                if current.is_some() {
                    println!();
                }
                println!("{}", style::bold(group_of_entry.unwrap_or(no_group())));
                current = Some(group_of_entry);
            }
            println!("{line}");
        }
    }

    if !read_only.is_empty() {
        if !editable.is_empty() {
            println!();
        }
        println!("{}", style::dim(tr("Read-only", "Salt okunur")));
        let rows: Vec<Vec<String>> = read_only
            .iter()
            .map(|e| {
                let note = match &e.source {
                    Some(path) => trf(
                        "pattern in {}",
                        "desen, {} içinde",
                        &[&collapse_tilde(&path.to_string_lossy(), &ctx.home)],
                    ),
                    None => tr("pattern", "desen").to_string(),
                };
                vec![e.patterns.join(" "), note]
            })
            .collect();
        for line in table_lines(&rows) {
            println!("{}", style::dim(&line));
        }
    }
    Ok(())
}

/// The heading for connections that belong to no group.
fn no_group() -> &'static str {
    tr("(no group)", "(grupsuz)")
}

/// `sshcm groups`: every group with its number of connections.
pub fn groups(ctx: &Ctx) -> Result<()> {
    let store = ctx.load()?;
    let groups = store.groups();
    if groups.is_empty() {
        println!(
            "{}",
            tr(
                "No groups yet. Put a connection in one with: sshcm edit <alias> --group <name>",
                "Henüz grup yok. Bir bağlantıyı gruba almak için: sshcm edit <alias> --group <ad>"
            )
        );
        return Ok(());
    }
    let ungrouped = store
        .entries()
        .iter()
        .filter(|e| !e.is_pattern && e.conn.group.is_none())
        .count();
    let mut rows: Vec<Vec<String>> = vec![vec![
        tr("GROUP", "GRUP").to_string(),
        tr("CONNECTIONS", "BAĞLANTI").to_string(),
    ]];
    rows.extend(
        groups
            .into_iter()
            .map(|(name, count)| vec![name, count.to_string()]),
    );
    if ungrouped > 0 {
        rows.push(vec![no_group().to_string(), ungrouped.to_string()]);
    }
    for (i, line) in table_lines(&rows).iter().enumerate() {
        if i == 0 {
            println!("{}", style::dim(line));
        } else {
            println!("{line}");
        }
    }
    Ok(())
}

/// Removes the given columns when no row below the header has a value there.
fn drop_empty_columns(rows: Vec<Vec<String>>, optional: &[usize]) -> Vec<Vec<String>> {
    let unused: Vec<usize> = optional
        .iter()
        .copied()
        .filter(|&col| rows.iter().skip(1).all(|row| row[col].is_empty()))
        .collect();
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .filter(|(col, _)| !unused.contains(col))
                .map(|(_, cell)| cell)
                .collect()
        })
        .collect()
}

/// Lays out rows in columns, two spaces apart, without trailing spaces.
fn table_lines(rows: &[Vec<String>]) -> Vec<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|col| {
            rows.iter()
                .map(|row| row[col].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = (0..columns)
                .map(|col| pad(&row[col], widths[col]))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn table_aligns_columns_without_trailing_spaces() {
        let rows = [
            row(&["ALIAS", "HOST", "X"]),
            row(&["a", "long.example.com", ""]),
            row(&["üretim", "h", "y"]),
        ];
        assert_eq!(
            table_lines(&rows),
            [
                "ALIAS   HOST              X",
                "a       long.example.com",
                "üretim  h                 y"
            ]
        );
    }

    #[test]
    fn unused_optional_columns_are_dropped() {
        let rows = vec![
            row(&["A", "B", "C"]),
            row(&["1", "", ""]),
            row(&["2", "", "x"]),
        ];
        assert_eq!(
            drop_empty_columns(rows, &[1, 2]),
            [row(&["A", "C"]), row(&["1", ""]), row(&["2", "x"])]
        );
    }
}
