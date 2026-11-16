//! Line-based, lossless parser for `ssh_config` files.
//!
//! The file is turned into an ordered list of [`Line`]s, not a semantic tree.
//! Every line keeps its original text, so writing an unmodified document back
//! reproduces the input byte for byte.

/// What a line is, as far as sshcm cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    /// Empty or whitespace only.
    Blank,
    /// First non-blank character is `#`.
    Comment,
    /// `Keyword value` or `Keyword=value`.
    Directive(Directive),
}

/// The pieces of a directive line. Joined back together they give the line's
/// text without its line ending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// Leading whitespace.
    pub indent: String,
    /// Keyword as written in the file.
    pub key: String,
    /// Text between keyword and value: whitespace, optionally with one `=`.
    pub sep: String,
    /// The value, without surrounding whitespace or a trailing comment.
    pub value: String,
    /// Whatever follows the value: trailing whitespace and/or a `# comment`.
    pub tail: String,
}

impl Directive {
    /// True when this directive's keyword is `name`, ignoring case.
    pub fn is(&self, name: &str) -> bool {
        self.key.eq_ignore_ascii_case(name)
    }

    /// The value split into arguments, with double quotes removed.
    pub fn args(&self) -> Vec<String> {
        split_args(&self.value)
    }

    /// The value as a single string with surrounding quotes removed.
    pub fn unquoted(&self) -> String {
        unquote(&self.value)
    }

    fn text(&self) -> String {
        format!(
            "{}{}{}{}{}",
            self.indent, self.key, self.sep, self.value, self.tail
        )
    }
}

/// One line of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Line text without the line ending.
    text: String,
    /// `"\n"`, `"\r\n"`, or `""` for a last line without a newline.
    eol: String,
    kind: LineKind,
}

impl Line {
    /// Parses one line. `raw` may end with `\n` or `\r\n`.
    pub fn parse(raw: &str) -> Line {
        let (text, eol) = if let Some(t) = raw.strip_suffix("\r\n") {
            (t, "\r\n")
        } else if let Some(t) = raw.strip_suffix('\n') {
            (t, "\n")
        } else {
            (raw, "")
        };
        Line {
            text: text.to_string(),
            eol: eol.to_string(),
            kind: classify(text),
        }
    }

    /// Builds a new directive line.
    pub fn directive(indent: &str, key: &str, value: &str, eol: &str) -> Line {
        let d = Directive {
            indent: indent.to_string(),
            key: key.to_string(),
            sep: " ".to_string(),
            value: value.to_string(),
            tail: String::new(),
        };
        Line {
            text: d.text(),
            eol: eol.to_string(),
            kind: LineKind::Directive(d),
        }
    }

    /// Builds a new comment line. `text` must start with `#`.
    pub fn comment(text: &str, eol: &str) -> Line {
        Line {
            text: text.to_string(),
            eol: eol.to_string(),
            kind: LineKind::Comment,
        }
    }

    /// Builds an empty line.
    pub fn blank(eol: &str) -> Line {
        Line {
            text: String::new(),
            eol: eol.to_string(),
            kind: LineKind::Blank,
        }
    }

    pub fn kind(&self) -> &LineKind {
        &self.kind
    }

    /// Line text without the line ending.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn eol(&self) -> &str {
        &self.eol
    }

    pub fn is_blank(&self) -> bool {
        self.kind == LineKind::Blank
    }

    pub fn is_comment(&self) -> bool {
        self.kind == LineKind::Comment
    }

    pub fn as_directive(&self) -> Option<&Directive> {
        match &self.kind {
            LineKind::Directive(d) => Some(d),
            _ => None,
        }
    }

    /// Replaces the value of a directive line. Indentation, keyword spelling,
    /// separator and any trailing comment are kept. No-op on other lines.
    pub fn set_value(&mut self, value: &str) {
        if let LineKind::Directive(d) = &mut self.kind {
            d.value = value.to_string();
            self.text = d.text();
        }
    }

    /// Gives a last line without a newline the given line ending.
    pub fn ensure_eol(&mut self, eol: &str) {
        if self.eol.is_empty() {
            self.eol = eol.to_string();
        }
    }

    pub(crate) fn write_to(&self, out: &mut String) {
        out.push_str(&self.text);
        out.push_str(&self.eol);
    }
}

/// Joins lines back into file content.
pub fn render(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        line.write_to(&mut out);
    }
    out
}

/// Splits file content into lines, keeping every byte.
pub fn parse_lines(input: &str) -> Vec<Line> {
    input.split_inclusive('\n').map(Line::parse).collect()
}

fn classify(text: &str) -> LineKind {
    let body = text.trim_start_matches([' ', '\t']);
    let indent = &text[..text.len() - body.len()];
    if body.trim().is_empty() {
        return LineKind::Blank;
    }
    if body.starts_with('#') {
        return LineKind::Comment;
    }

    // Keyword: up to the first whitespace or `=`.
    let key_end = body.find([' ', '\t', '=']).unwrap_or(body.len());
    let key = &body[..key_end];
    let rest = &body[key_end..];

    // Separator: whitespace, at most one `=`, whitespace.
    let after_ws = rest.trim_start_matches([' ', '\t']);
    let after_eq = after_ws.strip_prefix('=').unwrap_or(after_ws);
    let value_and_tail = after_eq.trim_start_matches([' ', '\t']);
    let sep = &rest[..rest.len() - value_and_tail.len()];

    let value_len = value_len(value_and_tail);
    let value = value_and_tail[..value_len].trim_end_matches([' ', '\t', '\r']);
    let tail = &value_and_tail[value.len()..];

    LineKind::Directive(Directive {
        indent: indent.to_string(),
        key: key.to_string(),
        sep: sep.to_string(),
        value: value.to_string(),
        tail: tail.to_string(),
    })
}

/// Length of the value part: everything up to an unquoted `#` that starts a
/// word (OpenSSH treats that as a trailing comment), or the whole string.
fn value_len(s: &str) -> usize {
    let mut in_quotes = false;
    let mut prev_ws = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes && prev_ws => return i,
            _ => {}
        }
        prev_ws = c == ' ' || c == '\t';
    }
    s.len()
}

/// Splits a value into whitespace-separated arguments. Double quotes group
/// words and are removed.
pub fn split_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut started = false;
    for c in value.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            ' ' | '\t' if !in_quotes => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            _ => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(cur);
    }
    args
}

/// Removes one pair of surrounding double quotes, if present.
pub fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// Wraps a value in double quotes when it contains whitespace.
pub fn quote_if_needed(value: &str) -> String {
    if value.contains([' ', '\t']) && !value.starts_with('"') {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directive(raw: &str) -> Directive {
        Line::parse(raw).as_directive().cloned().expect("directive")
    }

    #[test]
    fn classifies_blank_and_comment_lines() {
        assert!(Line::parse("\n").is_blank());
        assert!(Line::parse("   \t\n").is_blank());
        assert!(Line::parse("# hello\n").is_comment());
        assert!(Line::parse("    # indented\n").is_comment());
    }

    #[test]
    fn parses_space_separated_directive() {
        let d = directive("    HostName example.com\n");
        assert_eq!(d.indent, "    ");
        assert_eq!(d.key, "HostName");
        assert_eq!(d.sep, " ");
        assert_eq!(d.value, "example.com");
        assert_eq!(d.tail, "");
    }

    #[test]
    fn parses_equals_separated_directive() {
        assert_eq!(directive("Port=2222").value, "2222");
        let d = directive("\tPort = 2222  ");
        assert_eq!(d.sep, " = ");
        assert_eq!(d.value, "2222");
        assert_eq!(d.tail, "  ");
    }

    #[test]
    fn keyword_match_ignores_case() {
        assert!(directive("hostname x").is("HostName"));
        assert!(directive("HOSTNAME x").is("hostname"));
    }

    #[test]
    fn splits_trailing_comment_from_value() {
        let d = directive("Port 2222 # custom port");
        assert_eq!(d.value, "2222");
        assert_eq!(d.tail, " # custom port");
        // A `#` inside a word or inside quotes is part of the value.
        assert_eq!(directive("User a#b").value, "a#b");
        assert_eq!(directive("ProxyCommand \"x # y\" z").value, "\"x # y\" z");
    }

    #[test]
    fn splits_and_unquotes_arguments() {
        assert_eq!(directive("Host a b  c").args(), ["a", "b", "c"]);
        assert_eq!(
            directive("IdentityFile \"~/my keys/id\"").unquoted(),
            "~/my keys/id"
        );
        assert_eq!(split_args("\"a b\" c"), ["a b", "c"]);
    }

    #[test]
    fn set_value_keeps_everything_else() {
        let mut l = Line::parse("\tuser = ubuntu # who\r\n");
        l.set_value("deploy");
        let mut out = String::new();
        l.write_to(&mut out);
        assert_eq!(out, "\tuser = deploy # who\r\n");
    }

    #[test]
    fn lines_round_trip_including_missing_final_newline() {
        for input in ["", "\n", "Host a\n  User b", "a\r\nb\r\n\r\n", "  \n#x\n\n"] {
            let mut out = String::new();
            for l in parse_lines(input) {
                l.write_to(&mut out);
            }
            assert_eq!(out, input);
        }
    }
}
