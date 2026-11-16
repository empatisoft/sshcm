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
        let mut rows: Vec<[String; 6]> =
            vec![["ALIAS", "HOSTNAME", "USER", "PORT", "PROXY", "TAGS"].map(String::from)];
        rows.extend(editable.iter().map(|e| {
            [
                e.patterns.join(" "),
                e.conn.hostname.clone().unwrap_or_default(),
                e.conn.user.clone().unwrap_or_default(),
                e.conn.effective_port().to_string(),
                proxy_short(&e.conn.proxy),
                e.conn.tags.join(", "),
            ]
        }));
        print_table(&rows);
    }

    if !read_only.is_empty() {
        if !editable.is_empty() {
            println!();
        }
        println!("{}", style::dim(tr("Read-only", "Salt okunur")));
        let rows: Vec<[String; 2]> = read_only
            .iter()
            .map(|e| {
                let note = match &e.source {
                    Some(path) => trf(
                        "included from {}",
                        "{} dosyasından dahil edildi",
                        &[&collapse_tilde(&path.to_string_lossy(), &ctx.home)],
                    ),
                    None => tr("pattern", "desen").to_string(),
                };
                [e.patterns.join(" "), note]
            })
            .collect();
        for line in table_lines(&rows) {
            println!("{}", style::dim(&line));
        }
    }
    Ok(())
}

/// Lays out rows in columns, two spaces apart, without trailing spaces.
fn table_lines<const N: usize>(rows: &[[String; N]]) -> Vec<String> {
    let widths: Vec<usize> = (0..N)
        .map(|col| {
            rows.iter()
                .map(|row| row[col].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = (0..N).map(|col| pad(&row[col], widths[col])).collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect()
}

/// Prints the table with a dimmed header row.
fn print_table(rows: &[[String; 6]]) {
    // Hide the TAGS column when no connection has tags.
    let has_tags = rows.iter().skip(1).any(|row| !row[5].is_empty());
    let lines = if has_tags {
        table_lines(rows)
    } else {
        let narrow: Vec<[String; 5]> = rows
            .iter()
            .map(|r| {
                [
                    r[0].clone(),
                    r[1].clone(),
                    r[2].clone(),
                    r[3].clone(),
                    r[4].clone(),
                ]
            })
            .collect();
        table_lines(&narrow)
    };
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            println!("{}", style::dim(line));
        } else {
            println!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_aligns_columns_without_trailing_spaces() {
        let rows = [
            ["ALIAS".to_string(), "HOST".to_string(), "X".to_string()],
            [
                "a".to_string(),
                "long.example.com".to_string(),
                String::new(),
            ],
            ["üretim".to_string(), "h".to_string(), "y".to_string()],
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
}
