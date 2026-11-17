//! `sshcm list`: every connection as a table, read-only ones at the end.

use sshcm_core::Entry;
use sshcm_core::model::collapse_tilde;

use super::{Ctx, proxy_short};
use crate::error::Result;
use crate::i18n::{tr, trf};
use crate::style::{self, pad};

pub fn run(ctx: &Ctx, tag: Option<&str>) -> Result<()> {
    let store = ctx.load()?;
    let entries: Vec<Entry> = store
        .entries()
        .into_iter()
        .filter(|e| tag.is_none_or(|t| e.conn.tags.iter().any(|x| x == t)))
        .collect();
    let (read_only, editable): (Vec<&Entry>, Vec<&Entry>) =
        entries.iter().partition(|e| e.read_only());

    if entries.is_empty() {
        match tag {
            Some(tag) => println!(
                "{}",
                trf(
                    "No connections with the tag {}.",
                    "{} etiketini taşıyan bağlantı yok.",
                    &[tag]
                )
            ),
            None => println!(
                "{}",
                tr(
                    "There are no connections yet. Add one with: sshcm add",
                    "Henüz bağlantı yok. Eklemek için: sshcm add"
                )
            ),
        }
        return Ok(());
    }

    if !editable.is_empty() {
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
        for (i, line) in table_lines(&rows).iter().enumerate() {
            if i == 0 {
                println!("{}", style::dim(line));
            } else {
                println!("{line}");
            }
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
