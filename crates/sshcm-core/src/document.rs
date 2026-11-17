//! A config file as an editable document.
//!
//! [`Document`] wraps the parsed lines and offers the three edits sshcm
//! performs: add a block, update a block in place, remove a block. Lines
//! outside the touched block are never rewritten.

use std::fmt;
use std::path::PathBuf;

use crate::model::{Connection, Proxy, hop_host, replace_hop_host};
use crate::parser::{Line, parse_lines, quote_if_needed, render};

const DESC_PREFIX: &str = "sshcm:desc";
const TAGS_PREFIX: &str = "sshcm:tags";
const DEFAULT_INDENT: &str = "    ";

/// Whether a block starts with `Host` or `Match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Host,
    Match,
}

/// The lines that make up one block: `start..end`, with the `Host`/`Match`
/// line at `head`. Comments directly above the head line belong to the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub kind: BlockKind,
    pub start: usize,
    pub head: usize,
    pub end: usize,
}

/// One `Host` block as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Everything on the `Host` line.
    pub patterns: Vec<String>,
    /// True for wildcard or negated patterns (`*`, `*.example.com`, `!x`).
    pub is_pattern: bool,
    /// The included file this block lives in; `None` for the main config.
    pub source: Option<PathBuf>,
    /// The block's settings. `conn.alias` is the first name on the line.
    pub conn: Connection,
    /// The block's text as it is in the file.
    pub text: String,
}

impl Entry {
    pub fn alias(&self) -> &str {
        &self.conn.alias
    }

    /// Wildcard patterns cannot be edited or removed.
    pub fn read_only(&self) -> bool {
        self.is_pattern
    }
}

/// Why an edit could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    NotFound(String),
    ReadOnly(String),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::NotFound(a) => write!(f, "No connection named {a}."),
            DocError::ReadOnly(a) => write!(f, "{a} is read-only: patterns cannot be changed."),
        }
    }
}

impl std::error::Error for DocError {}

/// A parsed config file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    lines: Vec<Line>,
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

impl Document {
    pub fn parse(input: &str) -> Document {
        Document {
            lines: parse_lines(input),
        }
    }

    /// The file content. For an unmodified document this equals the input.
    pub fn render(&self) -> String {
        render(&self.lines)
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The line ending the file uses.
    fn eol(&self) -> &'static str {
        match self.lines.iter().find(|l| !l.eol().is_empty()) {
            Some(l) if l.eol() == "\r\n" => "\r\n",
            _ => "\n",
        }
    }

    /// All `Host` and `Match` blocks, in file order.
    pub fn spans(&self) -> Vec<Span> {
        let heads: Vec<(usize, BlockKind)> = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let d = l.as_directive()?;
                if d.is("Host") {
                    Some((i, BlockKind::Host))
                } else if d.is("Match") {
                    Some((i, BlockKind::Match))
                } else {
                    None
                }
            })
            .collect();

        // A block starts at the first of the comment lines directly above
        // its head line.
        let starts: Vec<usize> = heads
            .iter()
            .enumerate()
            .map(|(n, &(head, _))| {
                let floor = if n == 0 { 0 } else { heads[n - 1].0 + 1 };
                let mut start = head;
                while start > floor && self.lines[start - 1].is_comment() {
                    start -= 1;
                }
                start
            })
            .collect();

        heads
            .iter()
            .enumerate()
            .map(|(n, &(head, kind))| {
                let limit = starts.get(n + 1).copied().unwrap_or(self.lines.len());
                // A block ends after its last setting, plus any comments
                // attached to it without a blank line in between.
                let last = (head..limit)
                    .rev()
                    .find(|&i| self.lines[i].as_directive().is_some())
                    .unwrap_or(head);
                let mut end = last + 1;
                while end < limit && !self.lines[end].is_blank() {
                    end += 1;
                }
                Span {
                    kind,
                    start: starts[n],
                    head,
                    end,
                }
            })
            .collect()
    }

    fn patterns(&self, span: &Span) -> Vec<String> {
        self.lines[span.head]
            .as_directive()
            .map(|d| d.args())
            .unwrap_or_default()
    }

    fn text_of(&self, span: &Span) -> String {
        render(&self.lines[span.start..span.end])
    }

    /// All `Host` blocks of this file. `Match` blocks are not listed.
    pub fn entries(&self) -> Vec<Entry> {
        self.spans()
            .iter()
            .filter(|s| s.kind == BlockKind::Host)
            .filter_map(|span| {
                let patterns = self.patterns(span);
                let first = patterns.first()?.clone();
                Some(Entry {
                    is_pattern: patterns.iter().any(|p| is_pattern(p)),
                    patterns,
                    source: None,
                    conn: read_connection(&self.lines[span.start..span.end], first),
                    text: self.text_of(span),
                })
            })
            .collect()
    }

    /// Arguments of every `Include` line.
    pub fn includes(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(Line::as_directive)
            .filter(|d| d.is("Include"))
            .flat_map(|d| d.args())
            .collect()
    }

    /// The editable block named `alias`.
    fn editable_span(&self, alias: &str) -> Result<Span, DocError> {
        let mut read_only = false;
        for span in self.spans() {
            if span.kind != BlockKind::Host {
                continue;
            }
            let patterns = self.patterns(&span);
            if !patterns.iter().any(|p| p == alias) {
                continue;
            }
            if patterns.iter().any(|p| is_pattern(p)) {
                read_only = true;
                continue;
            }
            return Ok(span);
        }
        if read_only {
            Err(DocError::ReadOnly(alias.to_string()))
        } else {
            Err(DocError::NotFound(alias.to_string()))
        }
    }

    /// Text of the editable block named `alias`.
    pub fn block_text(&self, alias: &str) -> Option<String> {
        self.editable_span(alias).ok().map(|s| self.text_of(&s))
    }

    /// Adds a new block after the last block that is not a pattern, so a
    /// trailing `Host *` keeps applying after it. ssh uses the first value it
    /// finds for each setting.
    pub fn add(&mut self, conn: &Connection) {
        let eol = self.eol();
        let block = render_block(conn, DEFAULT_INDENT, eol);
        let spans = self.spans();
        let last_plain = spans.iter().rfind(|s| {
            s.kind == BlockKind::Host && !self.patterns(s).iter().any(|p| is_pattern(p))
        });
        let at = match (last_plain, spans.first()) {
            (Some(span), _) => span.end,
            (None, Some(first)) => first.start,
            (None, None) => self.lines.len(),
        };

        let mut new = Vec::new();
        if at > 0 {
            self.lines[at - 1].ensure_eol(eol);
            if !self.lines[at - 1].is_blank() {
                new.push(Line::blank(eol));
            }
        }
        new.extend(block);
        if at < self.lines.len() && !self.lines[at].is_blank() {
            new.push(Line::blank(eol));
        }
        self.lines.splice(at..at, new);
    }

    /// Updates the block named `alias` to match `new`. Only settings whose
    /// value changed are touched: a changed line is rewritten in place, a
    /// dropped setting loses its line, a new setting is appended to the block
    /// using the block's own indentation.
    ///
    /// When the alias changes, `ProxyJump` values that refer to the old alias
    /// are updated too.
    pub fn update(&mut self, alias: &str, new: &Connection) -> Result<(), DocError> {
        let span = self.editable_span(alias)?;
        let eol = self.eol();
        let mut block: Vec<Line> = self.lines[span.start..span.end].to_vec();
        let old = read_connection(&block, alias.to_string());

        if old.hostname != new.hostname {
            set_setting(&mut block, "HostName", new.hostname.as_deref(), eol);
        }
        if old.user != new.user {
            set_setting(&mut block, "User", new.user.as_deref(), eol);
        }
        if old.explicit_port() != new.explicit_port() {
            let port = new.explicit_port().map(|p| p.to_string());
            set_setting(&mut block, "Port", port.as_deref(), eol);
        }
        if old.identity_file != new.identity_file {
            let file = new.identity_file.as_deref().map(quote_if_needed);
            set_setting(&mut block, "IdentityFile", file.as_deref(), eol);
        }
        if old.proxy != new.proxy {
            // ProxyJump and ProxyCommand never live in the same block.
            let directive = new.proxy.directive();
            for key in ["ProxyJump", "ProxyCommand"] {
                if directive.as_ref().is_none_or(|(k, _)| *k != key) {
                    set_setting(&mut block, key, None, eol);
                }
            }
            if let Some((key, value)) = directive {
                set_setting(&mut block, key, Some(&value), eol);
            }
        }
        if old.description != new.description {
            set_meta(&mut block, DESC_PREFIX, new.description.as_deref(), eol);
        }
        if old.tags != new.tags {
            let tags = (!new.tags.is_empty()).then(|| new.tags.join(", "));
            set_meta(&mut block, TAGS_PREFIX, tags.as_deref(), eol);
        }
        if new.alias != alias {
            let head = head_index(&block);
            let patterns: Vec<String> = block[head]
                .as_directive()
                .map(|d| d.args())
                .unwrap_or_default()
                .into_iter()
                .map(|p| if p == alias { new.alias.clone() } else { p })
                .collect();
            block[head].set_value(&patterns.join(" "));
        }

        self.lines.splice(span.start..span.end, block);
        if new.alias != alias {
            self.rename_jump_host(alias, &new.alias);
        }
        Ok(())
    }

    /// Removes the block named `alias` together with its comments, without
    /// leaving a double blank line behind.
    pub fn remove(&mut self, alias: &str) -> Result<(), DocError> {
        let span = self.editable_span(alias)?;
        self.lines.drain(span.start..span.end);
        let at = span.start;
        if at < self.lines.len() {
            let blank_before = at == 0 || self.lines[at - 1].is_blank();
            if blank_before && self.lines[at].is_blank() {
                self.lines.remove(at);
            }
        } else if at > 0 && self.lines[at - 1].is_blank() {
            self.lines.remove(at - 1);
        }
        Ok(())
    }

    /// Points every `ProxyJump` that goes through `old` at `new`. Returns the
    /// names of the blocks that changed.
    pub fn rename_jump_host(&mut self, old: &str, new: &str) -> Vec<String> {
        let mut changed = Vec::new();
        for span in self.spans() {
            for i in span.head + 1..span.end {
                let Some(d) = self.lines[i].as_directive() else {
                    continue;
                };
                if !d.is("ProxyJump") {
                    continue;
                }
                let hops: Vec<String> = d
                    .value
                    .split(',')
                    .map(|hop| {
                        let hop = hop.trim();
                        if hop_host(hop) == old {
                            replace_hop_host(hop, new)
                        } else {
                            hop.to_string()
                        }
                    })
                    .collect();
                let value = hops.join(",");
                if value != d.value {
                    self.lines[i].set_value(&value);
                    if let Some(name) = self.patterns(&span).first() {
                        changed.push(name.clone());
                    }
                }
            }
        }
        changed
    }
}

/// Names of the connections that use `alias` as a jump host.
pub fn dependents(entries: &[Entry], alias: &str) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.alias() != alias && e.conn.proxy.jump_hosts().contains(&alias))
        .map(|e| e.alias().to_string())
        .collect()
}

/// True for `Host` patterns that are not plain names.
pub fn is_pattern(name: &str) -> bool {
    name.contains(['*', '?']) || name.starts_with('!')
}

/// The lines of a brand-new block. Settings equal to their default are left
/// out, which keeps blocks short and lets `Host *` defaults apply.
pub fn render_block(conn: &Connection, indent: &str, eol: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(desc) = &conn.description {
        lines.push(Line::comment(&format!("# {DESC_PREFIX} {desc}"), eol));
    }
    if !conn.tags.is_empty() {
        let tags = conn.tags.join(", ");
        lines.push(Line::comment(&format!("# {TAGS_PREFIX} {tags}"), eol));
    }
    lines.push(Line::directive("", "Host", &conn.alias, eol));
    let mut push = |key: &str, value: &str| lines.push(Line::directive(indent, key, value, eol));
    if let Some(hostname) = &conn.hostname {
        push("HostName", hostname);
    }
    if let Some(user) = &conn.user {
        push("User", user);
    }
    if let Some(port) = conn.explicit_port() {
        push("Port", &port.to_string());
    }
    if let Some(file) = &conn.identity_file {
        push("IdentityFile", &quote_if_needed(file));
    }
    if let Some((key, value)) = conn.proxy.directive() {
        push(key, &value);
    }
    // Settings sshcm does not manage; present when a block is copied.
    for (key, value) in &conn.other {
        push(key, value);
    }
    lines
}

/// The text of a brand-new block, for previews.
pub fn render_block_text(conn: &Connection) -> String {
    render(&render_block(conn, DEFAULT_INDENT, "\n"))
}

/// Index of the `Host` line inside a block's lines.
fn head_index(block: &[Line]) -> usize {
    block
        .iter()
        .position(|l| l.as_directive().is_some())
        .unwrap_or(0)
}

/// Reads the settings of one block.
fn read_connection(block: &[Line], alias: String) -> Connection {
    let head = head_index(block);
    let mut conn = Connection {
        alias,
        ..Connection::default()
    };
    let mut proxy_seen = false;

    for line in &block[..head] {
        if let Some(desc) = meta_value(line, DESC_PREFIX) {
            conn.description = Some(desc).filter(|d| !d.is_empty());
        } else if let Some(tags) = meta_value(line, TAGS_PREFIX) {
            conn.tags = tags
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
        }
    }

    for d in block[head + 1..].iter().filter_map(Line::as_directive) {
        let value = d.unquoted();
        if d.is("HostName") && conn.hostname.is_none() {
            conn.hostname = Some(value);
        } else if d.is("User") && conn.user.is_none() {
            conn.user = Some(value);
        } else if d.is("Port") && conn.port.is_none() && value.parse::<u16>().is_ok_and(|p| p > 0) {
            conn.port = value.parse().ok();
        } else if d.is("IdentityFile") && conn.identity_file.is_none() {
            conn.identity_file = Some(value);
        } else if d.is("ProxyJump") && !proxy_seen && Proxy::from_jump(&d.value) != Proxy::None {
            conn.proxy = Proxy::from_jump(&d.value);
            proxy_seen = true;
        } else if d.is("ProxyCommand")
            && !proxy_seen
            && !d.value.eq_ignore_ascii_case("none")
            && !d.value.is_empty()
        {
            conn.proxy = Proxy::from_command(&d.value);
            proxy_seen = true;
        } else {
            conn.other.push((d.key.clone(), d.value.clone()));
        }
    }
    conn
}

/// The text after `# sshcm:<prefix>` on a comment line.
fn meta_value(line: &Line, prefix: &str) -> Option<String> {
    if !line.is_comment() {
        return None;
    }
    let body = line.text().trim_start().strip_prefix('#')?.trim_start();
    let rest = body.strip_prefix(prefix)?;
    if rest.is_empty() || rest.starts_with([' ', '\t']) {
        Some(rest.trim().to_string())
    } else {
        None
    }
}

/// Sets, replaces or removes one setting inside a block's lines.
fn set_setting(block: &mut Vec<Line>, key: &str, value: Option<&str>, eol: &str) {
    let head = head_index(block);
    let is_key = |l: &Line| l.as_directive().is_some_and(|d| d.is(key));
    let existing = (head + 1..block.len()).find(|&i| is_key(&block[i]));

    match (value, existing) {
        (Some(value), Some(i)) => block[i].set_value(value),
        (Some(value), None) => {
            let settings: Vec<usize> = (head + 1..block.len())
                .filter(|&i| block[i].as_directive().is_some())
                .collect();
            let indent = settings
                .first()
                .and_then(|&i| block[i].as_directive())
                .map(|d| d.indent.clone())
                .unwrap_or_else(|| DEFAULT_INDENT.to_string());
            let at = settings.last().map_or(head + 1, |&i| i + 1);
            block[at - 1].ensure_eol(eol);
            block.insert(at, Line::directive(&indent, key, value, eol));
        }
        (None, _) => {
            // ssh reads every IdentityFile line of a block, and sshcm shows
            // and manages only the first. Removing "the" identity file must
            // not take the lines the user never saw with it.
            let only_first = key.eq_ignore_ascii_case("IdentityFile");
            let mut i = head + 1;
            while i < block.len() {
                if is_key(&block[i]) {
                    block.remove(i);
                    if only_first {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
}

/// Sets, replaces or removes a `# sshcm:` comment above the `Host` line.
/// The description always sits above the tags.
fn set_meta(block: &mut Vec<Line>, prefix: &str, value: Option<&str>, eol: &str) {
    let head = head_index(block);
    let existing = (0..head).find(|&i| meta_value(&block[i], prefix).is_some());
    let indent = block[head]
        .as_directive()
        .map(|d| d.indent.clone())
        .unwrap_or_default();

    match (value, existing) {
        (Some(value), Some(i)) => {
            block[i] = Line::comment(&format!("{indent}# {prefix} {value}"), block[i].eol());
        }
        (Some(value), None) => {
            let at = if prefix == DESC_PREFIX {
                (0..head)
                    .find(|&i| meta_value(&block[i], TAGS_PREFIX).is_some())
                    .unwrap_or(head)
            } else {
                head
            };
            block.insert(
                at,
                Line::comment(&format!("{indent}# {prefix} {value}"), eol),
            );
        }
        (None, Some(i)) => {
            block.remove(i);
        }
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Global defaults
ServerAliveInterval 30

# Jump host for the production network
Host bastion
    HostName bastion.example.com
    User ops
    IdentityFile ~/.ssh/id_ed25519

Host web-prod
\tHostName 10.0.2.10
\tUser deploy
\tProxyJump bastion
\tServerAliveInterval 60

Host staging
    HostName 203.0.113.24
    User ubuntu
    IdentityFile ~/.ssh/id_ed25519

Match host *.internal
    ForwardAgent yes

Host *
    AddKeysToAgent yes
";

    fn conn(alias: &str, hostname: &str) -> Connection {
        Connection {
            alias: alias.to_string(),
            hostname: Some(hostname.to_string()),
            ..Connection::default()
        }
    }

    fn entry(doc: &Document, alias: &str) -> Entry {
        doc.entries()
            .into_iter()
            .find(|e| e.alias() == alias)
            .unwrap_or_else(|| panic!("no entry {alias}"))
    }

    /// Asserts that `new` equals `old` with `removed` replaced by `added`.
    fn assert_only_change(old: &str, new: &str, removed: &str, added: &str) {
        assert!(old.contains(removed), "old text lacks:\n{removed}");
        assert_eq!(new, old.replacen(removed, added, 1));
    }

    #[test]
    fn unmodified_document_renders_identically() {
        assert_eq!(Document::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn lists_host_blocks_but_not_match_blocks() {
        let doc = Document::parse(SAMPLE);
        let names: Vec<String> = doc
            .entries()
            .iter()
            .map(|e| e.alias().to_string())
            .collect();
        assert_eq!(names, ["bastion", "web-prod", "staging", "*"]);
        assert!(entry(&doc, "*").read_only());
        assert!(!entry(&doc, "bastion").read_only());
    }

    #[test]
    fn reads_settings_and_keeps_unknown_ones() {
        let doc = Document::parse(SAMPLE);
        let web = entry(&doc, "web-prod").conn;
        assert_eq!(web.hostname.as_deref(), Some("10.0.2.10"));
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, None);
        assert_eq!(web.proxy, Proxy::Jump(vec!["bastion".to_string()]));
        assert_eq!(
            web.other,
            [("ServerAliveInterval".to_string(), "60".to_string())]
        );
    }

    #[test]
    fn comments_directly_above_host_belong_to_the_block() {
        let doc = Document::parse(SAMPLE);
        assert!(
            entry(&doc, "bastion")
                .text
                .starts_with("# Jump host for the production network\nHost bastion\n")
        );
        // The global comment is separated by settings and a blank line.
        assert!(!entry(&doc, "bastion").text.contains("Global defaults"));
    }

    #[test]
    fn keywords_are_case_insensitive_and_equals_form_is_read() {
        let doc = Document::parse("host a\n  hostname=h.example\n  PORT = 2200\n  user \"bob\"\n");
        let a = entry(&doc, "a").conn;
        assert_eq!(a.hostname.as_deref(), Some("h.example"));
        assert_eq!(a.port, Some(2200));
        assert_eq!(a.user.as_deref(), Some("bob"));
    }

    #[test]
    fn multiple_names_on_one_line_are_one_entry() {
        let doc = Document::parse("Host a b\n    HostName h\n");
        let entries = doc.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].patterns, ["a", "b"]);
        assert_eq!(entries[0].alias(), "a");
        assert!(!entries[0].read_only());
    }

    #[test]
    fn patterns_are_read_only() {
        let doc = Document::parse("Host *.example.com\n  User a\nHost !bad good\n  User b\n");
        assert!(doc.entries().iter().all(Entry::read_only));
        let mut doc = doc;
        assert_eq!(
            doc.remove("good"),
            Err(DocError::ReadOnly("good".to_string()))
        );
        assert_eq!(
            doc.remove("missing"),
            Err(DocError::NotFound("missing".to_string()))
        );
    }

    #[test]
    fn add_goes_after_last_plain_block_and_before_trailing_patterns() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = conn("ihu", "app.example.edu");
        c.user = Some("deploy".to_string());
        c.port = Some(22);
        c.identity_file = Some("~/.ssh/id_ed25519".to_string());
        c.proxy = Proxy::Jump(vec!["bastion".to_string()]);
        doc.add(&c);

        let anchor = "    IdentityFile ~/.ssh/id_ed25519\n\nMatch host";
        let block = "    IdentityFile ~/.ssh/id_ed25519\n\n\
Host ihu\n    HostName app.example.edu\n    User deploy\n    IdentityFile ~/.ssh/id_ed25519\n    ProxyJump bastion\n\n\
Match host";
        assert_only_change(SAMPLE, &doc.render(), anchor, block);
    }

    #[test]
    fn add_to_empty_document() {
        let mut doc = Document::parse("");
        doc.add(&conn("a", "h"));
        assert_eq!(doc.render(), "Host a\n    HostName h\n");
    }

    #[test]
    fn add_before_patterns_when_there_is_no_plain_block() {
        let mut doc = Document::parse("Compression yes\n# everything\nHost *\n    User me\n");
        doc.add(&conn("a", "h"));
        assert_eq!(
            doc.render(),
            "Compression yes\n\nHost a\n    HostName h\n\n# everything\nHost *\n    User me\n"
        );
    }

    #[test]
    fn add_after_a_last_line_without_newline() {
        let mut doc = Document::parse("Host a\n    HostName h");
        doc.add(&conn("b", "h2"));
        assert_eq!(
            doc.render(),
            "Host a\n    HostName h\n\nHost b\n    HostName h2\n"
        );
    }

    #[test]
    fn add_uses_the_files_line_ending() {
        let mut doc = Document::parse("Host a\r\n    HostName h\r\n");
        doc.add(&conn("b", "h2"));
        assert_eq!(
            doc.render(),
            "Host a\r\n    HostName h\r\n\r\nHost b\r\n    HostName h2\r\n"
        );
    }

    #[test]
    fn add_writes_other_settings_of_a_copied_connection() {
        let source = Document::parse(
            "Host a\n    HostName h\n    IdentityFile ~/.ssh/one\n    IdentityFile ~/.ssh/two\n    ServerAliveInterval 60\n",
        );
        let mut copy = entry(&source, "a").conn;
        copy.alias = "b".to_string();
        let mut doc = Document::parse("");
        doc.add(&copy);
        assert_eq!(
            doc.render(),
            "Host b\n    HostName h\n    IdentityFile ~/.ssh/one\n    IdentityFile ~/.ssh/two\n    ServerAliveInterval 60\n"
        );
        assert_eq!(entry(&doc, "b").conn, copy);
    }

    #[test]
    fn add_writes_description_and_tags_as_comments() {
        let mut doc = Document::parse("");
        let mut c = conn("web-prod", "10.0.2.10");
        c.description = Some("Üretim web sunucusu".to_string());
        c.tags = vec!["prod".to_string(), "web".to_string()];
        doc.add(&c);
        assert_eq!(
            doc.render(),
            "# sshcm:desc Üretim web sunucusu\n# sshcm:tags prod, web\nHost web-prod\n    HostName 10.0.2.10\n"
        );
        let read = entry(&doc, "web-prod").conn;
        assert_eq!(read.description.as_deref(), Some("Üretim web sunucusu"));
        assert_eq!(read.tags, ["prod", "web"]);
    }

    #[test]
    fn update_changes_only_the_target_line() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "staging").conn;
        c.user = Some("deploy".to_string());
        doc.update("staging", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "    HostName 203.0.113.24\n    User ubuntu\n",
            "    HostName 203.0.113.24\n    User deploy\n",
        );
    }

    #[test]
    fn update_appends_new_settings_at_block_end() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "staging").conn;
        c.user = Some("deploy".to_string());
        c.port = Some(2222);
        c.proxy = Proxy::Socks5("10.0.0.1:1080".to_string());
        doc.update("staging", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "    User ubuntu\n    IdentityFile ~/.ssh/id_ed25519\n\nMatch",
            "    User deploy\n    IdentityFile ~/.ssh/id_ed25519\n    Port 2222\n    ProxyCommand nc -X 5 -x 10.0.0.1:1080 %h %p\n\nMatch",
        );
    }

    #[test]
    fn update_uses_the_blocks_own_indentation() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "web-prod").conn;
        c.port = Some(2200);
        doc.update("web-prod", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "\tServerAliveInterval 60\n",
            "\tServerAliveInterval 60\n\tPort 2200\n",
        );
    }

    #[test]
    fn update_removes_dropped_settings() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "bastion").conn;
        c.user = None;
        c.identity_file = None;
        doc.update("bastion", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "    HostName bastion.example.com\n    User ops\n    IdentityFile ~/.ssh/id_ed25519\n",
            "    HostName bastion.example.com\n",
        );
    }

    #[test]
    fn removing_the_identity_file_keeps_additional_identity_lines() {
        let input = "Host a\n    HostName h\n    IdentityFile ~/.ssh/first\n    IdentityFile ~/.ssh/second\n    User me\n";
        let mut doc = Document::parse(input);
        let mut c = entry(&doc, "a").conn;
        assert_eq!(c.identity_file.as_deref(), Some("~/.ssh/first"));
        c.identity_file = None;
        doc.update("a", &c).unwrap();
        assert_eq!(
            doc.render(),
            "Host a\n    HostName h\n    IdentityFile ~/.ssh/second\n    User me\n"
        );
        // The remaining line is now the one sshcm shows.
        assert_eq!(
            entry(&doc, "a").conn.identity_file.as_deref(),
            Some("~/.ssh/second")
        );
    }

    #[test]
    fn update_switches_proxy_kind_without_keeping_both() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "web-prod").conn;
        c.proxy = Proxy::Command("ssh -W %h:%p gate".to_string());
        doc.update("web-prod", &c).unwrap();
        let text = entry(&doc, "web-prod").text;
        assert!(!text.contains("ProxyJump"));
        assert!(text.ends_with("\tServerAliveInterval 60\n\tProxyCommand ssh -W %h:%p gate\n"));

        let mut c = entry(&doc, "web-prod").conn;
        c.proxy = Proxy::None;
        doc.update("web-prod", &c).unwrap();
        let text = entry(&doc, "web-prod").text;
        assert!(!text.contains("Proxy"));
    }

    #[test]
    fn update_with_no_changes_is_byte_identical() {
        let mut doc = Document::parse(SAMPLE);
        for alias in ["bastion", "web-prod", "staging"] {
            let c = entry(&doc, alias).conn;
            doc.update(alias, &c).unwrap();
        }
        assert_eq!(doc.render(), SAMPLE);
    }

    #[test]
    fn explicit_default_port_is_not_touched_unless_changed() {
        let input = "Host a\n    HostName h\n    Port 22\n";
        let mut doc = Document::parse(input);
        let mut c = entry(&doc, "a").conn;
        c.user = Some("me".to_string());
        doc.update("a", &c).unwrap();
        assert_eq!(
            doc.render(),
            "Host a\n    HostName h\n    Port 22\n    User me\n"
        );
    }

    #[test]
    fn update_keeps_keyword_spelling_separator_and_trailing_comment() {
        let mut doc = Document::parse("Host a\n  hostname = old.example # primary\n");
        let mut c = entry(&doc, "a").conn;
        c.hostname = Some("new.example".to_string());
        doc.update("a", &c).unwrap();
        assert_eq!(doc.render(), "Host a\n  hostname = new.example # primary\n");
    }

    #[test]
    fn rename_updates_host_line_and_dependents() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "bastion").conn;
        c.alias = "gate".to_string();
        doc.update("bastion", &c).unwrap();
        let expected = SAMPLE
            .replace("Host bastion\n", "Host gate\n")
            .replace("ProxyJump bastion\n", "ProxyJump gate\n");
        assert_eq!(doc.render(), expected);
    }

    #[test]
    fn rename_keeps_user_and_port_of_a_jump_hop() {
        let mut doc = Document::parse(
            "Host gate\n    HostName g\nHost a\n    HostName h\n    ProxyJump first, ops@gate:2222\n",
        );
        assert_eq!(doc.rename_jump_host("gate", "bastion"), ["a"]);
        assert!(doc.render().contains("ProxyJump first,ops@bastion:2222\n"));
    }

    #[test]
    fn update_description_and_tags() {
        let mut doc = Document::parse(SAMPLE);
        let mut c = entry(&doc, "bastion").conn;
        c.tags = vec!["prod".to_string()];
        doc.update("bastion", &c).unwrap();
        let mut c = entry(&doc, "bastion").conn;
        c.description = Some("Gateway".to_string());
        doc.update("bastion", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "# Jump host for the production network\nHost bastion\n",
            "# Jump host for the production network\n# sshcm:desc Gateway\n# sshcm:tags prod\nHost bastion\n",
        );

        let mut c = entry(&doc, "bastion").conn;
        c.description = None;
        c.tags = vec!["prod".to_string(), "jump".to_string()];
        doc.update("bastion", &c).unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "# Jump host for the production network\nHost bastion\n",
            "# Jump host for the production network\n# sshcm:tags prod, jump\nHost bastion\n",
        );
    }

    #[test]
    fn remove_takes_the_block_and_its_comments_only() {
        let mut doc = Document::parse(SAMPLE);
        doc.remove("bastion").unwrap();
        assert_only_change(
            SAMPLE,
            &doc.render(),
            "# Jump host for the production network\nHost bastion\n    HostName bastion.example.com\n    User ops\n    IdentityFile ~/.ssh/id_ed25519\n\n",
            "",
        );
        assert!(!doc.render().contains("\n\n\n"));
    }

    #[test]
    fn remove_last_block_leaves_no_trailing_blank_line() {
        let mut doc = Document::parse("Host a\n    HostName h\n\nHost b\n    HostName h2\n");
        doc.remove("b").unwrap();
        assert_eq!(doc.render(), "Host a\n    HostName h\n");
        doc.remove("a").unwrap();
        assert_eq!(doc.render(), "");
    }

    #[test]
    fn remove_first_block_leaves_no_leading_blank_line() {
        let mut doc = Document::parse("Host a\n    HostName h\n\nHost b\n    HostName h2\n");
        doc.remove("a").unwrap();
        assert_eq!(doc.render(), "Host b\n    HostName h2\n");
    }

    #[test]
    fn remove_keeps_free_standing_comments() {
        let input = "Host a\n    HostName h\n\n# Section: databases\n\nHost b\n    HostName h2\n";
        let mut doc = Document::parse(input);
        doc.remove("a").unwrap();
        assert_eq!(
            doc.render(),
            "# Section: databases\n\nHost b\n    HostName h2\n"
        );
    }

    #[test]
    fn dependents_lists_connections_jumping_through_an_alias() {
        let doc = Document::parse(SAMPLE);
        assert_eq!(dependents(&doc.entries(), "bastion"), ["web-prod"]);
        assert!(dependents(&doc.entries(), "staging").is_empty());
    }

    #[test]
    fn existing_custom_proxy_command_is_kept_as_is() {
        let input = "Host a\n    HostName h\n    ProxyCommand  corkscrew proxy 8080 %h %p\n";
        let mut doc = Document::parse(input);
        let mut c = entry(&doc, "a").conn;
        assert_eq!(
            c.proxy,
            Proxy::Command("corkscrew proxy 8080 %h %p".to_string())
        );
        c.user = Some("me".to_string());
        doc.update("a", &c).unwrap();
        assert!(doc.render().starts_with(input));
    }

    #[test]
    fn includes_are_collected() {
        let doc = Document::parse("Include config.d/* ~/.ssh/extra\nHost a\n    Include inner\n");
        assert_eq!(doc.includes(), ["config.d/*", "~/.ssh/extra", "inner"]);
    }
}
