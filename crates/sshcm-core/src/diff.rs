//! A small line diff for previews.

/// How a line differs between the old and the new text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Removed,
    Added,
}

/// Line-by-line diff of two texts. Removed lines come before the added lines
/// that replace them.
pub fn diff_lines(old: &str, new: &str) -> Vec<(Change, String)> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();

    // Longest common subsequence table; blocks are a handful of lines.
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push((Change::Same, a[i].to_string()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push((Change::Removed, a[i].to_string()));
            i += 1;
        } else {
            out.push((Change::Added, b[j].to_string()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| (Change::Removed, l.to_string())));
    out.extend(b[j..].iter().map(|l| (Change::Added, l.to_string())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_changed_line_as_remove_then_add() {
        let d = diff_lines(
            "Host a\n  User x\n  Port 1\n",
            "Host a\n  User y\n  Port 1\n",
        );
        assert_eq!(
            d,
            vec![
                (Change::Same, "Host a".to_string()),
                (Change::Removed, "  User x".to_string()),
                (Change::Added, "  User y".to_string()),
                (Change::Same, "  Port 1".to_string()),
            ]
        );
    }

    #[test]
    fn handles_pure_additions_and_removals() {
        assert_eq!(
            diff_lines("", "a\n"),
            vec![(Change::Added, "a".to_string())]
        );
        assert_eq!(
            diff_lines("a\n", ""),
            vec![(Change::Removed, "a".to_string())]
        );
        assert!(
            diff_lines("a\nb\n", "a\nb\n")
                .iter()
                .all(|(c, _)| *c == Change::Same)
        );
    }
}
