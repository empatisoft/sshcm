//! The question-by-question form used by `add` and `edit`.
//!
//! When editing, every question starts with the current value, so Enter
//! keeps it.

use std::path::{Path, PathBuf};

use inquire::Text;
use inquire::validator::Validation;
use sshcm_core::model::{
    self, collapse_tilde, expand_tilde, parse_port, parse_tags, validate_alias, validate_group,
    validate_hop, validate_hostname, validate_proxy_address, validate_user,
};
use sshcm_core::{Connection, DEFAULT_PORT, Known, Proxy, Store, ValidationError};

use super::{Choice, confirm, keys, select};
use crate::error::Result;
use crate::i18n::{self, tr, trf};

/// Which questions to leave out because the value came from a flag.
#[derive(Debug, Clone, Copy, Default)]
pub struct Skip {
    pub alias: bool,
    pub hostname: bool,
    pub user: bool,
    pub port: bool,
    pub identity_file: bool,
    pub proxy: bool,
    pub description: bool,
    pub tags: bool,
    pub group: bool,
}

impl Skip {
    pub fn all() -> Skip {
        Skip {
            alias: true,
            hostname: true,
            user: true,
            port: true,
            identity_file: true,
            proxy: true,
            description: true,
            tags: true,
            group: true,
        }
    }
}

type CheckResult = std::result::Result<(), ValidationError>;

/// Asks one line of text. `check` runs on every Enter and its message is
/// shown above the prompt until the value is accepted.
fn ask(
    message: &str,
    initial: Option<&str>,
    default: Option<&str>,
    help: Option<&str>,
    check: impl Fn(&str) -> CheckResult + Clone + 'static,
) -> Result<Option<String>> {
    let has_default = default.is_some();
    let mut prompt = Text::new(message).with_validator(move |input: &str| {
        let input = input.trim();
        // An empty answer means "the default" when there is one.
        if input.is_empty() && has_default {
            return Ok(Validation::Valid);
        }
        Ok(match check(input) {
            Ok(()) => Validation::Valid,
            Err(err) => Validation::Invalid(i18n::validation(&err).into()),
        })
    });
    if let Some(initial) = initial {
        prompt = prompt.with_initial_value(initial);
    }
    if let Some(default) = default {
        prompt = prompt.with_default(default);
    }
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }
    let answer = prompt.prompt()?;
    Ok(Some(answer.trim().to_string()).filter(|a| !a.is_empty()))
}

/// Runs the form. `preset` holds the starting values; `original` is the
/// alias being edited, or `None` when adding.
pub fn fill(
    store: &Store,
    preset: Connection,
    original: Option<&str>,
    skip: &Skip,
) -> Result<Connection> {
    let mut conn = preset;
    let editing = original.is_some();
    let known = store.known(original);
    let home = store.home().to_path_buf();

    if !skip.alias {
        let names = known.names.clone();
        let unchanged = conn.alias.clone();
        conn.alias = ask(
            tr("Alias:", "Alias:"),
            Some(conn.alias.as_str()).filter(|a| !a.is_empty()),
            None,
            None,
            move |input| {
                // A name that is already in the file is not re-judged.
                if !editing || input != unchanged {
                    validate_alias(input)?;
                }
                if names.iter().any(|n| n == input) {
                    return Err(ValidationError::AliasTaken(input.to_string()));
                }
                Ok(())
            },
        )?
        .unwrap_or_default();
    }

    if !skip.hostname {
        let unchanged = conn.hostname.clone().unwrap_or_default();
        conn.hostname = ask(
            tr("HostName:", "HostName:"),
            conn.hostname.as_deref(),
            None,
            // A block without HostName is valid: ssh then uses the alias.
            editing.then(|| {
                tr(
                    "Leave empty to connect to the alias itself",
                    "Alias'ın kendisine bağlanmak için boş bırakın",
                )
            }),
            move |input| {
                if editing && (input.is_empty() || input == unchanged) {
                    Ok(())
                } else {
                    validate_hostname(input)
                }
            },
        )?;
    }

    if !skip.user {
        let unchanged = conn.user.clone().unwrap_or_default();
        conn.user = ask(
            tr("User:", "User:"),
            conn.user.as_deref(),
            None,
            Some(tr(
                "Leave empty to use your current user name",
                "Mevcut kullanıcı adınızı kullanmak için boş bırakın",
            )),
            move |input| {
                if input.is_empty() || (editing && input == unchanged) {
                    Ok(())
                } else {
                    validate_user(input)
                }
            },
        )?;
    }

    if !skip.port {
        let default = DEFAULT_PORT.to_string();
        let current = conn.port.map(|p| p.to_string());
        // Editing a block that has a Port line starts from that value;
        // otherwise 22 is offered and Enter accepts it.
        let (initial, default) = match &current {
            Some(port) if editing => (Some(port.as_str()), None),
            Some(port) => (None, Some(port.as_str())),
            None => (None, Some(default.as_str())),
        };
        let answer = ask(tr("Port:", "Port:"), initial, default, None, |input| {
            if input.is_empty() {
                Ok(())
            } else {
                parse_port(input).map(|_| ())
            }
        })?;
        conn.port = match answer {
            Some(port) => Some(parse_port(&port)?),
            None => None,
        };
    }

    if !skip.identity_file {
        conn.identity_file = ask_identity_file(&home, conn.identity_file.as_deref(), editing)?;
    }

    if !skip.proxy {
        let me: Vec<String> = [Some(conn.alias.as_str()), original]
            .into_iter()
            .flatten()
            .map(str::to_string)
            .collect();
        conn.proxy = ask_proxy(store, &known, &me, &conn.proxy)?;
    }

    if !skip.description {
        conn.description = ask(
            tr("Description:", "Açıklama:"),
            conn.description.as_deref(),
            None,
            Some(tr("Optional, one line", "İsteğe bağlı, tek satır")),
            |_| Ok(()),
        )?;
    }

    if !skip.tags {
        let current = conn.tags.join(", ");
        let answer = ask(
            tr("Tags:", "Etiketler:"),
            Some(current.as_str()).filter(|c| !c.is_empty()),
            None,
            Some(tr(
                "Optional, separated by commas",
                "İsteğe bağlı, virgülle ayrılır",
            )),
            |input| parse_tags(input).map(|_| ()),
        )?;
        conn.tags = parse_tags(&answer.unwrap_or_default())?;
    }

    if !skip.group {
        let existing: Vec<String> = store.groups().into_iter().map(|(name, _)| name).collect();
        let help = if existing.is_empty() {
            tr(
                "Optional: a workspace such as personal or a-company",
                "İsteğe bağlı: personal veya a-company gibi bir çalışma alanı",
            )
            .to_string()
        } else {
            trf(
                "Optional. Groups in use: {}",
                "İsteğe bağlı. Kullanılan gruplar: {}",
                &[&existing.join(", ")],
            )
        };
        let unchanged = conn.group.clone().unwrap_or_default();
        conn.group = ask(
            tr("Group:", "Grup:"),
            conn.group.as_deref(),
            None,
            Some(&help),
            move |input| {
                if input.is_empty() || (editing && input == unchanged) {
                    Ok(())
                } else {
                    validate_group(input)
                }
            },
        )?;
    }

    Ok(conn)
}

#[derive(Debug, Clone)]
enum KeyChoice {
    Path(String),
    Other,
    None,
}

fn ask_identity_file(home: &Path, current: Option<&str>, editing: bool) -> Result<Option<String>> {
    let mut paths = keys::discover(home);
    if let Some(current) = current
        && !paths.iter().any(|p| p == current)
    {
        paths.insert(0, current.to_string());
    }

    let mut choices: Vec<Choice<KeyChoice>> = paths
        .iter()
        .map(|p| Choice::plain(p, KeyChoice::Path(p.clone())))
        .collect();
    choices.push(Choice::plain(
        tr("Enter another path", "Başka bir yol gir"),
        KeyChoice::Other,
    ));
    choices.push(Choice::plain(tr("None", "Yok"), KeyChoice::None));

    let none_index = choices.len() - 1;
    let cursor = match current {
        Some(current) => paths.iter().position(|p| p == current).unwrap_or(0),
        // When editing a connection without a key, keep "None" under the cursor.
        None if editing => none_index,
        None => 0,
    };

    match select(tr("Identity file:", "Anahtar dosyası:"), choices, cursor)? {
        KeyChoice::Path(path) => Ok(Some(path)),
        KeyChoice::None => Ok(None),
        KeyChoice::Other => {
            let home_for_check: PathBuf = home.to_path_buf();
            let answer = ask(tr("Path:", "Yol:"), None, None, None, move |input| {
                if expand_tilde(input, &home_for_check).is_file() {
                    Ok(())
                } else {
                    Err(ValidationError::IdentityFileMissing(input.to_string()))
                }
            })?;
            Ok(answer.map(|p| collapse_tilde(&p, home)))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyKind {
    None,
    Jump,
    Server,
    Custom,
}

fn ask_proxy(store: &Store, known: &Known, me: &[String], current: &Proxy) -> Result<Proxy> {
    let width = 17;
    let choices = vec![
        Choice::described(
            tr("None", "Yok"),
            width,
            tr("Connect directly", "Doğrudan bağlan"),
            ProxyKind::None,
        ),
        Choice::described(
            tr("Jump host", "Atlama sunucusu"),
            width,
            tr(
                "Go through another connection (ProxyJump)",
                "Başka bir bağlantı üzerinden git (ProxyJump)",
            ),
            ProxyKind::Jump,
        ),
        Choice::described(
            "SOCKS5 / HTTP",
            width,
            tr(
                "Go through a proxy server (ProxyCommand)",
                "Bir proxy sunucusu üzerinden git (ProxyCommand)",
            ),
            ProxyKind::Server,
        ),
        Choice::described(
            tr("Custom command", "Özel komut"),
            width,
            tr(
                "Write your own ProxyCommand",
                "Kendi ProxyCommand komutunuzu yazın",
            ),
            ProxyKind::Custom,
        ),
    ];
    let cursor = match current {
        Proxy::None => 0,
        Proxy::Jump(_) => 1,
        Proxy::Socks5(_) | Proxy::Http(_) => 2,
        Proxy::Command(_) => 3,
    };

    match select(tr("Proxy:", "Proxy:"), choices, cursor)? {
        ProxyKind::None => Ok(Proxy::None),
        ProxyKind::Jump => ask_jump_hosts(store, known, me, current),
        ProxyKind::Server => {
            let (is_http, address) = match current {
                Proxy::Http(addr) => (true, Some(addr.as_str())),
                Proxy::Socks5(addr) => (false, Some(addr.as_str())),
                _ => (false, None),
            };
            let http = select(
                tr("Proxy type:", "Proxy türü:"),
                vec![Choice::plain("SOCKS5", false), Choice::plain("HTTP", true)],
                usize::from(is_http),
            )?;
            let address = ask(
                tr("Proxy address:", "Proxy adresi:"),
                address,
                None,
                Some(tr(
                    "host:port, for example 10.0.0.1:1080",
                    "adres:port, örneğin 10.0.0.1:1080",
                )),
                validate_proxy_address,
            )?
            .unwrap_or_default();
            Ok(if http {
                Proxy::Http(address)
            } else {
                Proxy::Socks5(address)
            })
        }
        ProxyKind::Custom => {
            let existing = match current {
                Proxy::None | Proxy::Jump(_) => None,
                other => other.directive().map(|(_, value)| value),
            };
            let command = ask(
                tr("Proxy command:", "Proxy komutu:"),
                existing.as_deref(),
                None,
                Some(tr(
                    "%h is the host and %p the port, for example: ssh -W %h:%p gate",
                    "%h sunucu, %p porttur; örneğin: ssh -W %h:%p gate",
                )),
                |input| {
                    if input.is_empty() {
                        Err(ValidationError::ProxyCommandEmpty)
                    } else {
                        Ok(())
                    }
                },
            )?
            .unwrap_or_default();
            Ok(Proxy::from_command(&command))
        }
    }
}

#[derive(Debug, Clone)]
enum HopChoice {
    Keep,
    Alias(String),
    Manual,
}

/// Picks one or more jump hosts, in the order ssh will go through them.
/// Connections that would create a loop are not offered.
fn ask_jump_hosts(store: &Store, known: &Known, me: &[String], current: &Proxy) -> Result<Proxy> {
    let existing: Vec<String> = match current {
        Proxy::Jump(hops) => hops.clone(),
        _ => Vec::new(),
    };
    let candidates: Vec<String> = store
        .entries()
        .iter()
        .filter(|e| !e.is_pattern)
        .map(|e| e.alias().to_string())
        .filter(|alias| !me.contains(alias) && !me.iter().any(|m| known.jumps_through(alias, m)))
        .collect();

    let mut hops: Vec<String> = Vec::new();
    loop {
        let mut choices: Vec<Choice<HopChoice>> = Vec::new();
        if hops.is_empty() && !existing.is_empty() {
            let chain = existing.join(" → ");
            choices.push(Choice::plain(
                &trf("Keep {}", "{} kalsın", &[&chain]),
                HopChoice::Keep,
            ));
        }
        choices.extend(
            candidates
                .iter()
                .filter(|c| !hops.contains(c))
                .map(|c| Choice::plain(c, HopChoice::Alias(c.clone()))),
        );
        choices.push(Choice::plain(
            tr("Type user@host:port", "kullanıcı@adres:port yaz"),
            HopChoice::Manual,
        ));

        let hop = match select(tr("Jump host:", "Atlama sunucusu:"), choices, 0)? {
            HopChoice::Keep => return Ok(Proxy::Jump(existing)),
            HopChoice::Alias(alias) => alias,
            HopChoice::Manual => {
                let me = me.to_vec();
                ask(
                    tr("Jump host address:", "Atlama sunucusu adresi:"),
                    None,
                    None,
                    Some("user@host:port"),
                    move |input| {
                        validate_hop(input)?;
                        if me.iter().any(|m| m == model::hop_host(input)) {
                            return Err(ValidationError::JumpSelf(input.to_string()));
                        }
                        Ok(())
                    },
                )?
                .unwrap_or_default()
            }
        };
        hops.push(hop);

        if hops.len() > 1 {
            println!(
                "  {}",
                crate::style::dim(&trf(
                    "Jump order: {}",
                    "Atlama sırası: {}",
                    &[&hops.join(" → ")]
                ))
            );
        }
        if !confirm(
            tr(
                "Add another jump host?",
                "Başka bir atlama sunucusu eklensin mi?",
            ),
            false,
        )? {
            return Ok(Proxy::Jump(hops));
        }
    }
}
