//! Command line definition. Help texts are attached at runtime so they can
//! follow the interface language; names and flags never change.

use std::path::{Path, PathBuf};

use clap::{Args, Command, CommandFactory, Parser, Subcommand};
use sshcm_core::model::{collapse_tilde, parse_port, parse_tags};
use sshcm_core::{Connection, Proxy};

use crate::error::Result;
use crate::i18n::tr;

#[derive(Debug, Parser)]
#[command(name = "sshcm", version, disable_help_subcommand = true)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub dry_run: bool,

    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    #[arg(long, global = true, value_name = "LANG", value_parser = ["en", "tr"])]
    pub lang: Option<String>,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    List {
        #[arg(long, value_name = "TAG")]
        tag: Option<String>,
    },
    Show {
        alias: Option<String>,
    },
    Add {
        alias: Option<String>,
        #[command(flatten)]
        fields: Fields,
    },
    Edit {
        alias: Option<String>,
        #[arg(long, value_name = "ALIAS")]
        rename: Option<String>,
        #[command(flatten)]
        fields: Fields,
    },
    #[command(visible_alias = "remove")]
    Rm {
        alias: Option<String>,
    },
    Connect {
        alias: Option<String>,
        #[arg(last = true, value_name = "SSH_ARGS")]
        ssh_args: Vec<String>,
    },
    Config {
        #[command(subcommand)]
        action: Option<ConfigCmd>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    Get { key: String },
    Set { key: String, value: String },
}

/// Connection settings given as flags, for scripts. An empty value clears
/// the setting when editing.
#[derive(Debug, Default, Args)]
pub struct Fields {
    #[arg(long, value_name = "HOST")]
    pub hostname: Option<String>,

    #[arg(long, value_name = "USER")]
    pub user: Option<String>,

    #[arg(long, value_name = "PORT")]
    pub port: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub identity_file: Option<String>,

    #[arg(long, value_name = "HOST", value_delimiter = ',', group = "proxy")]
    pub jump: Vec<String>,

    #[arg(long, value_name = "HOST:PORT", group = "proxy")]
    pub socks5: Option<String>,

    #[arg(long, value_name = "HOST:PORT", group = "proxy")]
    pub http_proxy: Option<String>,

    #[arg(long, value_name = "COMMAND", group = "proxy")]
    pub proxy_command: Option<String>,

    #[arg(long, group = "proxy")]
    pub no_proxy: bool,

    #[arg(long, value_name = "TEXT")]
    pub desc: Option<String>,

    #[arg(long, value_name = "TAGS")]
    pub tags: Option<String>,
}

impl Fields {
    pub fn has_proxy(&self) -> bool {
        !self.jump.is_empty()
            || self.socks5.is_some()
            || self.http_proxy.is_some()
            || self.proxy_command.is_some()
            || self.no_proxy
    }

    /// True when at least one setting was given as a flag.
    pub fn any(&self) -> bool {
        self.hostname.is_some()
            || self.user.is_some()
            || self.port.is_some()
            || self.identity_file.is_some()
            || self.has_proxy()
            || self.desc.is_some()
            || self.tags.is_some()
    }

    /// Copies the given flags onto `conn`.
    pub fn apply(&self, conn: &mut Connection, home: &Path) -> Result<()> {
        let non_empty = |v: &String| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(v) = &self.hostname {
            conn.hostname = non_empty(v);
        }
        if let Some(v) = &self.user {
            conn.user = non_empty(v);
        }
        if let Some(v) = &self.port {
            conn.port = match non_empty(v) {
                Some(port) => Some(parse_port(&port)?),
                None => None,
            };
        }
        if let Some(v) = &self.identity_file {
            conn.identity_file = non_empty(v).map(|p| collapse_tilde(&p, home));
        }
        if !self.jump.is_empty() {
            conn.proxy = Proxy::Jump(self.jump.iter().map(|h| h.trim().to_string()).collect());
        } else if let Some(addr) = &self.socks5 {
            conn.proxy = Proxy::Socks5(addr.trim().to_string());
        } else if let Some(addr) = &self.http_proxy {
            conn.proxy = Proxy::Http(addr.trim().to_string());
        } else if let Some(cmd) = &self.proxy_command {
            conn.proxy = Proxy::from_command(cmd.trim());
        } else if self.no_proxy {
            conn.proxy = Proxy::None;
        }
        if let Some(v) = &self.desc {
            conn.description = non_empty(v);
        }
        if let Some(v) = &self.tags {
            conn.tags = parse_tags(v)?;
        }
        Ok(())
    }
}

/// The clap command with help texts in the current language.
pub fn command() -> Command {
    Cli::command()
        .about(tr(
            "SSH Connection Manager: add, edit, remove and list the connections in ~/.ssh/config",
            "SSH Connection Manager: ~/.ssh/config içindeki bağlantıları ekler, düzenler, siler ve listeler",
        ))
        .after_help(tr(
            "Run sshcm without a command to open the full-screen interface.",
            "Tam ekran arayüzü açmak için sshcm komutunu argümansız çalıştırın.",
        ))
        .mut_arg("config", |a| {
            a.help(tr(
                "Use this config file instead of ~/.ssh/config",
                "~/.ssh/config yerine bu config dosyasını kullan",
            ))
        })
        .mut_arg("dry_run", |a| {
            a.help(tr(
                "Show what would change without writing",
                "Yazmadan, nelerin değişeceğini göster",
            ))
        })
        .mut_arg("yes", |a| {
            a.help(tr(
                "Do not ask for confirmation (for scripts)",
                "Onay sorma (betikler için)",
            ))
        })
        .mut_arg("lang", |a| {
            a.help(tr("Interface language", "Arayüz dili"))
        })
        .mut_subcommand("list", |c| {
            c.about(tr("Show all connections", "Tüm bağlantıları göster"))
                .mut_arg("tag", |a| {
                    a.help(tr(
                        "Only show connections with this tag",
                        "Yalnızca bu etiketi taşıyan bağlantıları göster",
                    ))
                })
        })
        .mut_subcommand("show", |c| {
            c.about(tr(
                "Show every setting of one connection",
                "Tek bir bağlantının tüm ayarlarını göster",
            ))
            .mut_arg("alias", alias_help)
        })
        .mut_subcommand("add", |c| {
            fields_help(c.about(tr("Create a new connection", "Yeni bağlantı oluştur")))
                .mut_arg("alias", |a| {
                    a.help(tr(
                        "Name of the new connection",
                        "Yeni bağlantının adı",
                    ))
                })
        })
        .mut_subcommand("edit", |c| {
            fields_help(c.about(tr(
                "Change an existing connection",
                "Mevcut bir bağlantıyı değiştir",
            )))
            .mut_arg("alias", alias_help)
            .mut_arg("rename", |a| {
                a.help(tr(
                    "Give the connection a new alias",
                    "Bağlantıya yeni bir alias ver",
                ))
            })
        })
        .mut_subcommand("rm", |c| {
            c.about(tr("Delete a connection", "Bir bağlantıyı sil"))
                .mut_arg("alias", alias_help)
        })
        .mut_subcommand("connect", |c| {
            c.about(tr(
                "Open a connection with ssh",
                "ssh ile bağlantı aç",
            ))
            .mut_arg("alias", alias_help)
            .mut_arg("ssh_args", |a| {
                a.help(tr(
                    "Extra arguments passed to ssh",
                    "ssh komutuna iletilecek ek argümanlar",
                ))
            })
        })
        .mut_subcommand("config", |c| {
            c.about(tr(
                "Show or change sshcm's own settings",
                "sshcm'in kendi ayarlarını göster veya değiştir",
            ))
            .mut_subcommand("get", |c| {
                c.about(tr("Print the value of a setting", "Bir ayarın değerini yazdır"))
                    .mut_arg("key", key_help)
            })
            .mut_subcommand("set", |c| {
                c.about(tr("Change a setting", "Bir ayarı değiştir"))
                    .mut_arg("key", key_help)
                    .mut_arg("value", |a| a.help(tr("New value", "Yeni değer")))
            })
        })
}

fn alias_help(arg: clap::Arg) -> clap::Arg {
    arg.help(tr(
        "Connection alias; picked from a list when left out",
        "Bağlantı alias'ı; verilmezse listeden seçilir",
    ))
}

fn key_help(arg: clap::Arg) -> clap::Arg {
    arg.help(tr(
        "Setting name: language or backup_keep",
        "Ayar adı: language veya backup_keep",
    ))
}

fn fields_help(cmd: Command) -> Command {
    cmd.mut_arg("hostname", |a| {
        a.help(tr("Server address (HostName)", "Sunucu adresi (HostName)"))
    })
    .mut_arg("user", |a| a.help(tr("User name", "Kullanıcı adı")))
    .mut_arg("port", |a| a.help(tr("Port, 1-65535", "Port, 1-65535")))
    .mut_arg("identity_file", |a| {
        a.help(tr("Path of the private key", "Özel anahtarın yolu"))
    })
    .mut_arg("jump", |a| {
        a.help(tr(
            "Jump host: an alias or user@host:port; separate several with commas",
            "Atlama sunucusu: alias veya kullanıcı@adres:port; birden fazlası virgülle ayrılır",
        ))
    })
    .mut_arg("socks5", |a| {
        a.help(tr(
            "Go through a SOCKS5 proxy",
            "SOCKS5 proxy üzerinden git",
        ))
    })
    .mut_arg("http_proxy", |a| {
        a.help(tr("Go through an HTTP proxy", "HTTP proxy üzerinden git"))
    })
    .mut_arg("proxy_command", |a| {
        a.help(tr("Custom ProxyCommand", "Özel ProxyCommand"))
    })
    .mut_arg("no_proxy", |a| {
        a.help(tr("Connect directly", "Doğrudan bağlan"))
    })
    .mut_arg("desc", |a| {
        a.help(tr("One-line description", "Tek satırlık açıklama"))
    })
    .mut_arg("tags", |a| {
        a.help(tr("Comma-separated tags", "Virgülle ayrılmış etiketler"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_definition_is_consistent() {
        command().debug_assert();
    }
}
