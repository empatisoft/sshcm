//! The pop-ups of the form: the proxy picker and the key file list.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use sshcm_core::model::{hop_host, validate_hop, validate_proxy_address};
use sshcm_core::{Proxy, Store, ValidationError};

use super::input::TextInput;
use crate::i18n::{self, tr};

/// The four proxy options, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    None,
    Jump,
    Server,
    Custom,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::None, Kind::Jump, Kind::Server, Kind::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Kind::None => tr("None", "Yok"),
            Kind::Jump => tr("Jump host", "Atlama sunucusu"),
            Kind::Server => "SOCKS5 / HTTP",
            Kind::Custom => tr("Custom command", "Özel komut"),
        }
    }

    fn step(self, forward: bool) -> Kind {
        let i = Kind::ALL.iter().position(|k| *k == self).unwrap_or(0);
        let n = Kind::ALL.len();
        Kind::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

/// A connection offered as a jump host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub alias: String,
    pub target: String,
    /// Why this connection cannot be picked, if it cannot.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    Stay,
    Cancel,
    Done(Proxy),
}

#[derive(Debug, Clone)]
pub struct ProxyPicker {
    pub kind: Kind,
    /// The connection being edited, for the title and the route line.
    pub name: String,
    pub hosts: Vec<Host>,
    /// Row under the cursor; `hosts.len()` is the "type one" field.
    pub cursor: usize,
    /// Picked aliases, in jump order.
    pub chain: Vec<String>,
    /// Hops typed by hand, comma separated; they follow the picked ones.
    pub manual: TextInput,
    pub http: bool,
    pub address: TextInput,
    pub command: TextInput,
    pub error: Option<String>,
    me: Vec<String>,
}

impl ProxyPicker {
    /// `me` holds the names of the connection being edited (its current and
    /// its new alias), which cannot be their own jump host.
    pub fn new(store: &Store, current: &Proxy, me: &[String], name: &str) -> ProxyPicker {
        let known = store.known(me.last().map(String::as_str));
        let hosts: Vec<Host> = store
            .entries()
            .iter()
            .filter(|e| !e.is_pattern)
            .map(|e| {
                let alias = e.alias().to_string();
                let blocked = if me.contains(&alias) {
                    Some(
                        tr(
                            "this connection, cannot jump through itself",
                            "bu bağlantı, kendi üzerinden atlayamaz",
                        )
                        .to_string(),
                    )
                } else if me.iter().any(|m| known.jumps_through(&alias, m)) {
                    Some(
                        tr(
                            "already jumps through this connection",
                            "zaten bu bağlantı üzerinden atlıyor",
                        )
                        .to_string(),
                    )
                } else {
                    None
                };
                Host {
                    alias,
                    target: e.conn.target(),
                    blocked,
                }
            })
            .collect();

        let mut picker = ProxyPicker {
            kind: Kind::None,
            name: name.to_string(),
            hosts,
            cursor: 0,
            chain: Vec::new(),
            manual: TextInput::default(),
            http: false,
            address: TextInput::default(),
            command: TextInput::default(),
            error: None,
            me: me.to_vec(),
        };
        match current {
            Proxy::None => {}
            Proxy::Jump(hops) => {
                picker.kind = Kind::Jump;
                let (picked, typed): (Vec<String>, Vec<String>) = hops
                    .iter()
                    .cloned()
                    .partition(|hop| picker.hosts.iter().any(|h| &h.alias == hop));
                picker.chain = picked;
                picker.manual.set(&typed.join(","));
            }
            Proxy::Socks5(addr) => {
                picker.kind = Kind::Server;
                picker.address.set(addr);
            }
            Proxy::Http(addr) => {
                picker.kind = Kind::Server;
                picker.http = true;
                picker.address.set(addr);
            }
            Proxy::Command(cmd) => {
                picker.kind = Kind::Custom;
                picker.command.set(cmd);
            }
        }
        if let Some(first) = picker.chain.first() {
            picker.cursor = picker
                .hosts
                .iter()
                .position(|h| &h.alias == first)
                .unwrap_or(0);
        } else {
            picker.cursor = picker
                .hosts
                .iter()
                .position(|h| h.blocked.is_none())
                .unwrap_or(picker.hosts.len());
        }
        picker
    }

    /// True when the cursor is in the "type one" field.
    pub fn on_manual(&self) -> bool {
        self.kind == Kind::Jump && self.cursor >= self.hosts.len()
    }

    fn manual_hops(&self) -> Vec<String> {
        self.manual
            .text()
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect()
    }

    /// The proxy the current selection stands for, without validating it.
    pub fn proxy(&self) -> Proxy {
        match self.kind {
            Kind::None => Proxy::None,
            Kind::Jump => {
                let mut hops = self.chain.clone();
                hops.extend(self.manual_hops());
                if hops.is_empty() {
                    Proxy::None
                } else {
                    Proxy::Jump(hops)
                }
            }
            Kind::Server if self.address.is_empty() => Proxy::None,
            Kind::Server if self.http => Proxy::Http(self.address.text().to_string()),
            Kind::Server => Proxy::Socks5(self.address.text().to_string()),
            Kind::Custom if self.command.is_empty() => Proxy::None,
            Kind::Custom => Proxy::from_command(self.command.text()),
        }
    }

    fn check(&self) -> Result<Proxy, ValidationError> {
        match self.kind {
            Kind::None => {}
            Kind::Jump => {
                for hop in self.manual_hops() {
                    validate_hop(&hop)?;
                    if self.me.iter().any(|m| m == hop_host(&hop)) {
                        return Err(ValidationError::JumpSelf(hop));
                    }
                }
                if self.chain.is_empty() && self.manual_hops().is_empty() {
                    return Err(ValidationError::JumpEmpty);
                }
            }
            Kind::Server => validate_proxy_address(self.address.text())?,
            Kind::Custom => {
                if self.command.is_empty() {
                    return Err(ValidationError::ProxyCommandEmpty);
                }
            }
        }
        Ok(self.proxy())
    }

    fn toggle(&mut self) {
        let Some(host) = self.hosts.get(self.cursor) else {
            return;
        };
        if host.blocked.is_some() {
            return;
        }
        match self.chain.iter().position(|a| a == &host.alias) {
            Some(i) => {
                self.chain.remove(i);
            }
            None => self.chain.push(host.alias.clone()),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Picked {
        self.error = None;
        match key.code {
            KeyCode::Esc => return Picked::Cancel,
            KeyCode::Tab => self.kind = self.kind.step(true),
            KeyCode::BackTab => self.kind = self.kind.step(false),
            KeyCode::Enter => {
                // Enter on a host with nothing picked yet picks that host.
                if self.kind == Kind::Jump && self.chain.is_empty() && self.manual.is_empty() {
                    self.toggle();
                }
                match self.check() {
                    Ok(proxy) => return Picked::Done(proxy),
                    Err(err) => self.error = Some(i18n::validation(&err)),
                }
            }
            _ => match self.kind {
                Kind::None => {}
                Kind::Jump => match key.code {
                    KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                    KeyCode::Down => self.cursor = (self.cursor + 1).min(self.hosts.len()),
                    KeyCode::Char(' ') if !self.on_manual() => self.toggle(),
                    _ if self.on_manual() => {
                        self.manual.handle(key);
                    }
                    _ => {}
                },
                Kind::Server => match key.code {
                    KeyCode::Up | KeyCode::Down => self.http = !self.http,
                    _ => {
                        self.address.handle(key);
                    }
                },
                Kind::Custom => {
                    self.command.handle(key);
                }
            },
        }
        Picked::Stay
    }
}

/// A short list to pick one value from: the key files found in `~/.ssh`, or
/// the groups in use. The first row is "None".
#[derive(Debug, Clone)]
pub struct KeyPicker {
    pub title: String,
    pub options: Vec<Option<String>>,
    pub cursor: usize,
}

impl KeyPicker {
    pub fn new(title: &str, keys: &[String], current: &str) -> KeyPicker {
        let mut options: Vec<Option<String>> = vec![None];
        options.extend(keys.iter().cloned().map(Some));
        let cursor = options
            .iter()
            .position(|o| o.as_deref() == Some(current))
            .unwrap_or(0);
        KeyPicker {
            title: title.to_string(),
            options,
            cursor,
        }
    }

    /// `Some(choice)` once the user picked a row; the inner `None` is the
    /// "None" row. `Err(())` means cancelled.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Result<Option<String>, ()>> {
        match key.code {
            KeyCode::Esc => Some(Err(())),
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(self.options.len() - 1);
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => Some(Ok(self.options[self.cursor].clone())),
            _ => None,
        }
    }
}
