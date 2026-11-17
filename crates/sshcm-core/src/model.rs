//! The connection model and its validation rules.

use std::fmt;
use std::path::Path;

/// Port ssh uses when a block has no `Port` line.
pub const DEFAULT_PORT: u16 = 22;

/// How a connection reaches its host.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Proxy {
    /// Connect directly; no line is written.
    #[default]
    None,
    /// `ProxyJump a,b`: go through one or more hosts, in order.
    Jump(Vec<String>),
    /// `ProxyCommand nc -X 5 -x <addr> %h %p`
    Socks5(String),
    /// `ProxyCommand nc -X connect -x <addr> %h %p`
    Http(String),
    /// Any other `ProxyCommand`, kept exactly as written.
    Command(String),
}

impl Proxy {
    /// Reads a `ProxyJump` value.
    pub fn from_jump(value: &str) -> Proxy {
        let hops: Vec<String> = value
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        if hops.is_empty() || (hops.len() == 1 && hops[0].eq_ignore_ascii_case("none")) {
            Proxy::None
        } else {
            Proxy::Jump(hops)
        }
    }

    /// Reads a `ProxyCommand` value. Commands in the exact shape sshcm writes
    /// for SOCKS5 and HTTP proxies are recognised; anything else is custom.
    pub fn from_command(value: &str) -> Proxy {
        let parts: Vec<&str> = value.split_whitespace().collect();
        match parts.as_slice() {
            ["nc", "-X", "5", "-x", addr, "%h", "%p"] => Proxy::Socks5(addr.to_string()),
            ["nc", "-X", "connect", "-x", addr, "%h", "%p"] => Proxy::Http(addr.to_string()),
            _ => Proxy::Command(value.to_string()),
        }
    }

    /// The config line for this proxy as `(keyword, value)`.
    pub fn directive(&self) -> Option<(&'static str, String)> {
        match self {
            Proxy::None => None,
            Proxy::Jump(hops) => Some(("ProxyJump", hops.join(","))),
            Proxy::Socks5(addr) => Some(("ProxyCommand", format!("nc -X 5 -x {addr} %h %p"))),
            Proxy::Http(addr) => Some(("ProxyCommand", format!("nc -X connect -x {addr} %h %p"))),
            Proxy::Command(cmd) => Some(("ProxyCommand", cmd.clone())),
        }
    }

    /// Host names of the jump hosts, without `user@` and `:port`.
    pub fn jump_hosts(&self) -> Vec<&str> {
        match self {
            Proxy::Jump(hops) => hops.iter().map(|h| hop_host(h)).collect(),
            _ => Vec::new(),
        }
    }

    /// True when this proxy relies on the `nc` command.
    pub fn needs_nc(&self) -> bool {
        matches!(self, Proxy::Socks5(_) | Proxy::Http(_))
    }
}

/// The host part of a jump hop written as `[user@]host[:port]`.
pub fn hop_host(hop: &str) -> &str {
    let (start, end) = hop_host_range(hop);
    &hop[start..end]
}

/// Replaces the host part of a jump hop, keeping `user@` and `:port`.
pub fn replace_hop_host(hop: &str, new_host: &str) -> String {
    let (start, end) = hop_host_range(hop);
    format!("{}{}{}", &hop[..start], new_host, &hop[end..])
}

fn hop_host_range(hop: &str) -> (usize, usize) {
    let start = hop.rfind('@').map_or(0, |i| i + 1);
    let rest = &hop[start..];
    let len = if rest.starts_with('[') {
        // Bracketed IPv6 address, optionally followed by `:port`.
        rest.find(']').map_or(rest.len(), |i| i + 1)
    } else if rest.matches(':').count() == 1 {
        rest.find(':').unwrap_or(rest.len())
    } else {
        rest.len()
    };
    (start, start + len)
}

/// One connection: a single `Host` block of the config file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Connection {
    /// `Host`
    pub alias: String,
    /// `HostName`
    pub hostname: Option<String>,
    /// `User`
    pub user: Option<String>,
    /// `Port`. `None` and `Some(22)` both mean the default.
    pub port: Option<u16>,
    /// `IdentityFile`, stored with `~` where possible.
    pub identity_file: Option<String>,
    /// `ProxyJump` or `ProxyCommand`
    pub proxy: Proxy,
    /// `# sshcm:group` comment above the block: the workspace this
    /// connection belongs to. A connection has at most one group.
    pub group: Option<String>,
    /// `# sshcm:desc` comment above the block.
    pub description: Option<String>,
    /// `# sshcm:tags` comment above the block.
    pub tags: Vec<String>,
    /// Every other setting in the block, as `(keyword, value)`. Shown but
    /// never edited.
    pub other: Vec<(String, String)>,
}

impl Connection {
    /// The port to write to the file: `None` when it is the default.
    pub fn explicit_port(&self) -> Option<u16> {
        self.port.filter(|p| *p != DEFAULT_PORT)
    }

    /// The port ssh will use.
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    /// `user@host:port` for lists; user and a default port are left out.
    pub fn target(&self) -> String {
        let mut s = String::new();
        if let Some(user) = &self.user {
            s.push_str(user);
            s.push('@');
        }
        s.push_str(self.hostname.as_deref().unwrap_or(&self.alias));
        if let Some(port) = self.explicit_port() {
            s.push_str(&format!(":{port}"));
        }
        s
    }
}

/// Why a value was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    AliasEmpty,
    AliasInvalidChar(char),
    AliasLeadingDash,
    AliasTaken(String),
    HostNameEmpty,
    HostNameInvalid,
    UserInvalid,
    PortInvalid,
    IdentityFileMissing(String),
    JumpEmpty,
    JumpInvalid(String),
    JumpSelf(String),
    JumpCycle(String),
    ProxyAddressInvalid,
    ProxyCommandEmpty,
    TagInvalid(String),
    GroupInvalid(String),
    DescriptionMultiline,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ValidationError::*;
        match self {
            AliasEmpty => write!(f, "Alias is required."),
            AliasInvalidChar(c) if c.is_whitespace() => {
                write!(f, "Alias cannot contain spaces.")
            }
            AliasInvalidChar(c) => write!(f, "Alias cannot contain '{c}'."),
            AliasLeadingDash => write!(f, "Alias cannot start with '-'."),
            AliasTaken(a) => write!(f, "A connection named {a} already exists."),
            HostNameEmpty => write!(f, "HostName is required."),
            HostNameInvalid => write!(f, "HostName must be a domain name or an IP address."),
            UserInvalid => write!(f, "User cannot contain spaces."),
            PortInvalid => write!(f, "Port must be between 1 and 65535."),
            IdentityFileMissing(p) => write!(f, "Identity file not found: {p}"),
            JumpEmpty => write!(f, "Pick at least one jump host."),
            JumpInvalid(h) => write!(f, "Not a valid jump host: {h}"),
            JumpSelf(a) => write!(f, "{a} cannot be its own jump host."),
            JumpCycle(h) => write!(f, "{h} already jumps through this connection."),
            ProxyAddressInvalid => write!(f, "Proxy address must look like host:port."),
            ProxyCommandEmpty => write!(f, "Proxy command cannot be empty."),
            TagInvalid(t) => write!(f, "Tag '{t}' is not valid. Use letters, digits and '-'."),
            GroupInvalid(g) => write!(
                f,
                "Group '{g}' is not valid. Use letters, digits, '-', '_' and '.'."
            ),
            DescriptionMultiline => write!(f, "Description must be a single line."),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Checks the shape of an alias: not empty, no whitespace, none of `* ? ! ,`
/// and no leading `-` (so it can never be read as an ssh option).
pub fn validate_alias(alias: &str) -> Result<(), ValidationError> {
    if alias.is_empty() {
        return Err(ValidationError::AliasEmpty);
    }
    if alias.starts_with('-') {
        return Err(ValidationError::AliasLeadingDash);
    }
    if let Some(c) = alias
        .chars()
        .find(|c| c.is_whitespace() || matches!(c, '*' | '?' | '!' | ',' | '#' | '"' | '='))
    {
        return Err(ValidationError::AliasInvalidChar(c));
    }
    Ok(())
}

/// Checks a `HostName` value: a domain name or an IP address.
pub fn validate_hostname(hostname: &str) -> Result<(), ValidationError> {
    if hostname.is_empty() {
        return Err(ValidationError::HostNameEmpty);
    }
    let ok = !hostname.starts_with('-')
        && hostname
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'));
    if ok {
        Ok(())
    } else {
        Err(ValidationError::HostNameInvalid)
    }
}

/// Checks a `User` value.
pub fn validate_user(user: &str) -> Result<(), ValidationError> {
    if user.chars().any(char::is_whitespace) || user.starts_with('-') {
        Err(ValidationError::UserInvalid)
    } else {
        Ok(())
    }
}

/// Parses a port number between 1 and 65535.
pub fn parse_port(port: &str) -> Result<u16, ValidationError> {
    match port.trim().parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => Err(ValidationError::PortInvalid),
    }
}

/// Checks one jump hop: `[user@]host[:port]`.
pub fn validate_hop(hop: &str) -> Result<(), ValidationError> {
    let invalid = || ValidationError::JumpInvalid(hop.to_string());
    if hop.is_empty() || hop.starts_with('-') || hop.chars().any(|c| c.is_whitespace() || c == ',')
    {
        return Err(invalid());
    }
    let host = hop_host(hop);
    let (_, end) = hop_host_range(hop);
    let port = hop[end..].strip_prefix(':');
    let host_ok = !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']' | '%'));
    let port_ok = port.is_none_or(|p| parse_port(p).is_ok());
    if host_ok && port_ok && !hop.starts_with('@') {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// Checks a SOCKS5/HTTP proxy address: `host:port`.
pub fn validate_proxy_address(addr: &str) -> Result<(), ValidationError> {
    let Some((host, port)) = addr.rsplit_once(':') else {
        return Err(ValidationError::ProxyAddressInvalid);
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if validate_hostname(host).is_ok() && parse_port(port).is_ok() {
        Ok(())
    } else {
        Err(ValidationError::ProxyAddressInvalid)
    }
}

/// Parses a comma-separated tag list. Tags hold letters, digits and `-`.
pub fn parse_tags(input: &str) -> Result<Vec<String>, ValidationError> {
    let mut tags: Vec<String> = Vec::new();
    for tag in input.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !tag.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(ValidationError::TagInvalid(tag.to_string()));
        }
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_string());
        }
    }
    Ok(tags)
}

/// Checks a group name: one word of letters, digits, `-`, `_` and `.`.
pub fn validate_group(group: &str) -> Result<(), ValidationError> {
    let ok = !group.is_empty()
        && !group.starts_with('-')
        && group
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(ValidationError::GroupInvalid(group.to_string()))
    }
}

/// Checks a description: one line of free text.
pub fn validate_description(text: &str) -> Result<(), ValidationError> {
    if text.contains(['\n', '\r']) {
        Err(ValidationError::DescriptionMultiline)
    } else {
        Ok(())
    }
}

/// What validation needs to know about the rest of the config.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// Every name already used on a `Host` line, except the connection being
    /// edited.
    pub names: Vec<String>,
    /// For each connection, the hosts it jumps through.
    pub jumps: Vec<(String, Vec<String>)>,
}

impl Known {
    /// True when the connection `from` reaches `target` by following jump
    /// hosts. Picking `from` as a jump host of `target` would be a loop.
    pub fn jumps_through(&self, from: &str, target: &str) -> bool {
        self.reaches(from, target, &mut Vec::new())
    }

    fn reaches(&self, from: &str, target: &str, seen: &mut Vec<String>) -> bool {
        if from == target {
            return true;
        }
        if seen.iter().any(|s| s == from) {
            return false;
        }
        seen.push(from.to_string());
        self.jumps
            .iter()
            .filter(|(name, _)| name == from)
            .flat_map(|(_, hops)| hops.iter())
            .any(|hop| self.reaches(hop, target, seen))
    }
}

/// Validates a connection against the rest of the config.
///
/// `before` is the connection as it is in the file, or `None` when adding.
/// When editing, only settings whose value changed are checked: a value that
/// is already in the file is ssh's business, and must never stop the user
/// from changing something else in the block. For the same reason a block
/// without `HostName` (ssh then uses the alias) may stay that way; only a
/// new connection needs one.
pub fn validate(
    conn: &Connection,
    before: Option<&Connection>,
    known: &Known,
    home: &Path,
) -> Result<(), ValidationError> {
    let original_alias = before.map(|b| b.alias.as_str());
    let alias_changed = before.is_none_or(|b| b.alias != conn.alias);

    if alias_changed {
        validate_alias(&conn.alias)?;
    }
    if known.names.iter().any(|n| n == &conn.alias) {
        return Err(ValidationError::AliasTaken(conn.alias.clone()));
    }

    match &conn.hostname {
        Some(hostname) if before.is_none_or(|b| b.hostname.as_ref() != Some(hostname)) => {
            validate_hostname(hostname)?;
        }
        None if before.is_none() => return Err(ValidationError::HostNameEmpty),
        _ => {}
    }
    if let Some(user) = &conn.user
        && before.is_none_or(|b| b.user.as_ref() != Some(user))
    {
        validate_user(user)?;
    }
    if conn.port == Some(0) {
        return Err(ValidationError::PortInvalid);
    }
    // An unchanged key path is not checked: the block may point at a key
    // that only exists on another machine.
    if let Some(file) = &conn.identity_file
        && before.is_none_or(|b| b.identity_file.as_ref() != Some(file))
        && !file.contains('%')
        && !file.contains("${")
        && !expand_tilde(file, home).exists()
    {
        return Err(ValidationError::IdentityFileMissing(file.clone()));
    }

    // A new alias can turn an untouched proxy into a self-reference, so the
    // proxy is checked again when the alias changes.
    if alias_changed || before.is_none_or(|b| b.proxy != conn.proxy) {
        match &conn.proxy {
            Proxy::None => {}
            Proxy::Jump(hops) => {
                if hops.is_empty() {
                    return Err(ValidationError::JumpEmpty);
                }
                for hop in hops {
                    validate_hop(hop)?;
                    let host = hop_host(hop);
                    if host == conn.alias || Some(host) == original_alias {
                        return Err(ValidationError::JumpSelf(conn.alias.clone()));
                    }
                    for me in [Some(conn.alias.as_str()), original_alias]
                        .into_iter()
                        .flatten()
                    {
                        if known.reaches(host, me, &mut Vec::new()) {
                            return Err(ValidationError::JumpCycle(host.to_string()));
                        }
                    }
                }
            }
            Proxy::Socks5(addr) | Proxy::Http(addr) => validate_proxy_address(addr)?,
            Proxy::Command(cmd) => {
                if cmd.trim().is_empty() {
                    return Err(ValidationError::ProxyCommandEmpty);
                }
            }
        }
    }

    if let Some(desc) = &conn.description
        && before.is_none_or(|b| b.description.as_ref() != Some(desc))
    {
        validate_description(desc)?;
    }
    if before.is_none_or(|b| b.tags != conn.tags) {
        parse_tags(&conn.tags.join(","))?;
    }
    if let Some(group) = &conn.group
        && before.is_none_or(|b| b.group.as_ref() != Some(group))
    {
        validate_group(group)?;
    }
    Ok(())
}

/// Turns a leading `~` into the home directory.
pub fn expand_tilde(path: &str, home: &Path) -> std::path::PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        std::path::PathBuf::from(path)
    }
}

/// Shortens a path under the home directory to its `~` form.
pub fn collapse_tilde(path: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        "~".to_string()
    } else if let Some(rest) = path.strip_prefix(home)
        && rest.starts_with('/')
    {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(alias: &str) -> Connection {
        Connection {
            alias: alias.to_string(),
            hostname: Some("example.com".to_string()),
            ..Connection::default()
        }
    }

    fn known(names: &[&str], jumps: &[(&str, &[&str])]) -> Known {
        Known {
            names: names.iter().map(|s| s.to_string()).collect(),
            jumps: jumps
                .iter()
                .map(|(n, h)| (n.to_string(), h.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    /// Validates `c`; `original` is the alias of the block being edited.
    fn check(c: &Connection, original: Option<&str>, k: &Known) -> Result<(), ValidationError> {
        let before = original.map(conn);
        validate(c, before.as_ref(), k, Path::new("/nonexistent"))
    }

    #[test]
    fn alias_rules() {
        assert_eq!(validate_alias(""), Err(ValidationError::AliasEmpty));
        assert_eq!(
            validate_alias("-oProxy"),
            Err(ValidationError::AliasLeadingDash)
        );
        for (alias, c) in [
            ("a b", ' '),
            ("a*", '*'),
            ("a?", '?'),
            ("!a", '!'),
            ("a,b", ','),
        ] {
            assert_eq!(
                validate_alias(alias),
                Err(ValidationError::AliasInvalidChar(c))
            );
        }
        assert!(validate_alias("web-prod_1.example").is_ok());
    }

    #[test]
    fn duplicate_alias_is_rejected() {
        let k = known(&["staging"], &[]);
        assert_eq!(
            check(&conn("staging"), None, &k),
            Err(ValidationError::AliasTaken("staging".to_string()))
        );
        assert!(check(&conn("other"), None, &k).is_ok());
    }

    #[test]
    fn port_rules() {
        assert_eq!(parse_port("22"), Ok(22));
        assert_eq!(parse_port(" 65535 "), Ok(65535));
        for bad in ["0", "65536", "70000", "-1", "abc", ""] {
            assert_eq!(parse_port(bad), Err(ValidationError::PortInvalid));
        }
    }

    #[test]
    fn hostname_and_user_rules() {
        assert!(validate_hostname("app.example.edu").is_ok());
        assert!(validate_hostname("203.0.113.24").is_ok());
        assert!(validate_hostname("2001:db8::1").is_ok());
        assert_eq!(validate_hostname(""), Err(ValidationError::HostNameEmpty));
        assert_eq!(
            validate_hostname("a b"),
            Err(ValidationError::HostNameInvalid)
        );
        assert_eq!(
            validate_hostname("-oX"),
            Err(ValidationError::HostNameInvalid)
        );
        assert!(validate_user("deploy").is_ok());
        assert_eq!(validate_user("de ploy"), Err(ValidationError::UserInvalid));
    }

    #[test]
    fn hop_parsing() {
        assert_eq!(hop_host("bastion"), "bastion");
        assert_eq!(hop_host("ops@bastion:2222"), "bastion");
        assert_eq!(hop_host("ops@[2001:db8::1]:22"), "[2001:db8::1]");
        assert_eq!(
            replace_hop_host("ops@bastion:2222", "gate"),
            "ops@gate:2222"
        );
        assert!(validate_hop("ops@bastion.example.com:2222").is_ok());
        assert!(validate_hop("bastion").is_ok());
        for bad in ["", "a b", "a,b", "-J", "host:0", "host:99999", "@host"] {
            assert!(validate_hop(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn proxy_cannot_be_itself() {
        let mut c = conn("web");
        c.proxy = Proxy::Jump(vec!["web".to_string()]);
        assert_eq!(
            check(&c, None, &Known::default()),
            Err(ValidationError::JumpSelf("web".to_string()))
        );
    }

    #[test]
    fn proxy_cycle_is_rejected() {
        // db jumps through web; web must not jump through db.
        let k = known(&["db", "gate"], &[("db", &["mid"]), ("mid", &["web"])]);
        let mut c = conn("web");
        c.proxy = Proxy::Jump(vec!["gate".to_string(), "db".to_string()]);
        assert_eq!(
            check(&c, Some("web"), &k),
            Err(ValidationError::JumpCycle("db".to_string()))
        );
        c.proxy = Proxy::Jump(vec!["gate".to_string()]);
        assert!(check(&c, Some("web"), &k).is_ok());
    }

    #[test]
    fn cycle_check_uses_the_alias_before_renaming() {
        let k = known(&["db"], &[("db", &["web"])]);
        let mut c = conn("web2");
        c.proxy = Proxy::Jump(vec!["db".to_string()]);
        assert_eq!(
            check(&c, Some("web"), &k),
            Err(ValidationError::JumpCycle("db".to_string()))
        );
    }

    #[test]
    fn proxy_values_round_trip() {
        let socks = Proxy::Socks5("10.0.0.1:1080".to_string());
        let (key, value) = socks.directive().unwrap();
        assert_eq!(
            (key, value.as_str()),
            ("ProxyCommand", "nc -X 5 -x 10.0.0.1:1080 %h %p")
        );
        assert_eq!(Proxy::from_command(&value), socks);

        let http = Proxy::Http("proxy:3128".to_string());
        let (_, value) = http.directive().unwrap();
        assert_eq!(value, "nc -X connect -x proxy:3128 %h %p");
        assert_eq!(Proxy::from_command(&value), http);

        let custom = "ssh -W %h:%p gate";
        assert_eq!(
            Proxy::from_command(custom),
            Proxy::Command(custom.to_string())
        );
        assert_eq!(
            Proxy::from_jump("a, b").directive(),
            Some(("ProxyJump", "a,b".to_string()))
        );
        assert_eq!(Proxy::from_jump("none"), Proxy::None);
    }

    #[test]
    fn proxy_address_and_command_rules() {
        assert!(validate_proxy_address("10.0.0.1:1080").is_ok());
        assert!(validate_proxy_address("10.0.0.1").is_err());
        assert!(validate_proxy_address("host:0").is_err());
        let mut c = conn("a");
        c.proxy = Proxy::Command("  ".to_string());
        assert_eq!(
            check(&c, None, &Known::default()),
            Err(ValidationError::ProxyCommandEmpty)
        );
    }

    #[test]
    fn missing_identity_file_is_rejected_only_when_it_changed() {
        let mut c = conn("a");
        c.identity_file = Some("~/.ssh/nope".to_string());
        assert!(matches!(
            check(&c, None, &Known::default()),
            Err(ValidationError::IdentityFileMissing(_))
        ));
        // The block already pointed at that file: not sshcm's call.
        let home = Path::new("/nonexistent");
        assert!(validate(&c, Some(&c), &Known::default(), home).is_ok());
        let mut edited = c.clone();
        edited.identity_file = Some("~/.ssh/other".to_string());
        assert!(matches!(
            validate(&edited, Some(&c), &Known::default(), home),
            Err(ValidationError::IdentityFileMissing(_))
        ));
    }

    #[test]
    fn a_block_without_hostname_can_be_edited_but_not_created() {
        let home = Path::new("/nonexistent");
        let before = Connection {
            alias: "github.com".to_string(),
            user: Some("git".to_string()),
            ..Connection::default()
        };
        let mut edited = before.clone();
        edited.port = Some(443);
        assert!(validate(&edited, Some(&before), &Known::default(), home).is_ok());
        assert_eq!(
            validate(&before, None, &Known::default(), home),
            Err(ValidationError::HostNameEmpty)
        );
        // A HostName that is being set is still checked.
        edited.hostname = Some("not a host".to_string());
        assert_eq!(
            validate(&edited, Some(&before), &Known::default(), home),
            Err(ValidationError::HostNameInvalid)
        );
    }

    #[test]
    fn values_already_in_the_file_do_not_block_other_edits() {
        let home = Path::new("/nonexistent");
        // Written by hand: fine for ssh, outside sshcm's own rules.
        let before = Connection {
            alias: "odd=name".to_string(),
            hostname: Some("host/with/slash".to_string()),
            user: Some("-weird".to_string()),
            description: None,
            tags: vec!["has space".to_string()],
            ..Connection::default()
        };
        let mut edited = before.clone();
        edited.port = Some(2222);
        assert!(validate(&edited, Some(&before), &Known::default(), home).is_ok());

        // Touching one of those values brings its rule back.
        let mut renamed = before.clone();
        renamed.alias = "still=odd".to_string();
        assert_eq!(
            validate(&renamed, Some(&before), &Known::default(), home),
            Err(ValidationError::AliasInvalidChar('='))
        );
        let mut retagged = before.clone();
        retagged.tags.push("ok".to_string());
        assert!(validate(&retagged, Some(&before), &Known::default(), home).is_err());
    }

    #[test]
    fn renaming_rechecks_an_untouched_proxy() {
        let home = Path::new("/nonexistent");
        let mut before = conn("web");
        before.proxy = Proxy::Jump(vec!["gate".to_string()]);
        let mut renamed = before.clone();
        renamed.alias = "gate".to_string();
        assert_eq!(
            validate(&renamed, Some(&before), &Known::default(), home),
            Err(ValidationError::JumpSelf("gate".to_string()))
        );
    }

    #[test]
    fn tags_and_description() {
        assert_eq!(parse_tags("prod, web,prod").unwrap(), ["prod", "web"]);
        assert_eq!(parse_tags("").unwrap(), Vec::<String>::new());
        assert!(parse_tags("a b").is_err());
        assert!(validate_description("one line").is_ok());
        assert!(validate_description("two\nlines").is_err());
    }

    #[test]
    fn group_names() {
        for good in [
            "personal",
            "a-company",
            "b_company",
            "client.eu",
            "şirket",
            "2026",
        ] {
            assert!(validate_group(good).is_ok(), "{good}");
        }
        for bad in ["", "two words", "a,b", "-dash", "a/b", "a#b"] {
            assert_eq!(
                validate_group(bad),
                Err(ValidationError::GroupInvalid(bad.to_string())),
                "{bad}"
            );
        }
        let mut c = conn("a");
        c.group = Some("not valid".to_string());
        assert!(check(&c, None, &Known::default()).is_err());
        // A group that is already in the file is left alone.
        let home = Path::new("/nonexistent");
        assert!(validate(&c, Some(&c), &Known::default(), home).is_ok());
    }

    #[test]
    fn tilde_helpers() {
        let home = Path::new("/home/me");
        assert_eq!(collapse_tilde("/home/me/.ssh/id", home), "~/.ssh/id");
        assert_eq!(collapse_tilde("/home/meow/x", home), "/home/meow/x");
        assert_eq!(
            expand_tilde("~/.ssh/id", home),
            Path::new("/home/me/.ssh/id")
        );
        assert_eq!(expand_tilde("/abs", home), Path::new("/abs"));
    }

    #[test]
    fn target_omits_defaults() {
        let mut c = conn("a");
        assert_eq!(c.target(), "example.com");
        c.user = Some("deploy".to_string());
        c.port = Some(22);
        assert_eq!(c.target(), "deploy@example.com");
        c.port = Some(2222);
        assert_eq!(c.target(), "deploy@example.com:2222");
    }
}
