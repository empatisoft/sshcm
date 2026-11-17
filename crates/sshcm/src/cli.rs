//! Command line definition. Help texts are attached at runtime so they can
//! follow the interface language; names and flags never change.

use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, Args, Command, CommandFactory, Parser, Subcommand};
use sshcm_core::model::{collapse_tilde, parse_port, parse_tags};
use sshcm_core::{Connection, Proxy};

use crate::error::Result;
use crate::i18n::tr;

#[derive(Debug, Parser)]
#[command(
    name = "sshcm",
    version,
    disable_help_subcommand = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
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
        #[arg(long, value_name = "GROUP")]
        group: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Groups,
    Show {
        alias: Option<String>,
        #[arg(long)]
        json: bool,
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
    Copy {
        alias: Option<String>,
        new_alias: Option<String>,
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
    Backups,
    Restore {
        backup: Option<String>,
    },
    Completions {
        shell: clap_complete::Shell,
    },
    /// Prints the man page; used when packaging.
    #[command(hide = true)]
    Man,
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

    #[arg(long, value_name = "GROUP")]
    pub group: Option<String>,
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
            || self.group.is_some()
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
        if let Some(v) = &self.group {
            conn.group = non_empty(v);
        }
        Ok(())
    }
}

/// The clap command with help texts in the current language.
pub fn command() -> Command {
    // clap's own help and version flags are replaced so that their
    // descriptions follow the interface language too.
    let help = Arg::new("help")
        .short('h')
        .long("help")
        .global(true)
        .action(ArgAction::Help)
        .help(tr("Print help", "Yardımı göster"));
    let version = Arg::new("version")
        .short('V')
        .long("version")
        .action(ArgAction::Version)
        .help(tr("Print version", "Sürümü göster"));

    let command = Cli::command()
        .arg(help)
        .arg(version)
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
        .mut_arg("lang", |a| a.help(tr("Interface language", "Arayüz dili")))
        .mut_subcommand("list", |c| {
            c.about(tr("Show all connections", "Tüm bağlantıları göster"))
                .mut_arg("tag", |a| {
                    a.help(tr(
                        "Only show connections with this tag",
                        "Yalnızca bu etiketi taşıyan bağlantıları göster",
                    ))
                })
                .mut_arg("group", |a| {
                    a.help(tr(
                        "Only show connections of this group",
                        "Yalnızca bu gruptaki bağlantıları göster",
                    ))
                })
                .mut_arg("json", json_help)
        })
        .mut_subcommand("groups", |c| {
            c.about(tr(
                "List the groups and how many connections each has",
                "Grupları ve her birindeki bağlantı sayısını listele",
            ))
        })
        .mut_subcommand("show", |c| {
            c.about(tr(
                "Show every setting of one connection",
                "Tek bir bağlantının tüm ayarlarını göster",
            ))
            .mut_arg("alias", alias_help)
            .mut_arg("json", json_help)
        })
        .mut_subcommand("add", |c| {
            fields_help(c.about(tr("Create a new connection", "Yeni bağlantı oluştur"))).mut_arg(
                "alias",
                |a| a.help(tr("Name of the new connection", "Yeni bağlantının adı")),
            )
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
        .mut_subcommand("copy", |c| {
            c.about(tr(
                "Create a connection from an existing one",
                "Mevcut bir bağlantıdan yeni bağlantı oluştur",
            ))
            .mut_arg("alias", |a| {
                a.help(tr(
                    "Connection to copy; picked from a list when left out",
                    "Kopyalanacak bağlantı; verilmezse listeden seçilir",
                ))
            })
            .mut_arg("new_alias", |a| {
                a.help(tr("Alias of the copy", "Kopyanın alias'ı"))
            })
        })
        .mut_subcommand("rm", |c| {
            c.about(tr("Delete a connection", "Bir bağlantıyı sil"))
                .mut_arg("alias", alias_help)
        })
        .mut_subcommand("connect", |c| {
            c.about(tr("Open a connection with ssh", "ssh ile bağlantı aç"))
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
                c.about(tr(
                    "Print the value of a setting",
                    "Bir ayarın değerini yazdır",
                ))
                .mut_arg("key", key_help)
            })
            .mut_subcommand("set", |c| {
                c.about(tr("Change a setting", "Bir ayarı değiştir"))
                    .mut_arg("key", key_help)
                    .mut_arg("value", |a| a.help(tr("New value", "Yeni değer")))
            })
        })
        .mut_subcommand("backups", |c| {
            c.about(tr(
                "List the backups of the config",
                "Config dosyasının yedeklerini listele",
            ))
        })
        .mut_subcommand("restore", |c| {
            c.about(tr(
                "Put a backup back in place of the config",
                "Bir yedeği config dosyasının yerine geri yükle",
            ))
            .mut_arg("backup", |a| {
                a.help(tr(
                    "Backup file name or path; picked from a list when left out",
                    "Yedek dosyasının adı veya yolu; verilmezse listeden seçilir",
                ))
            })
        })
        .mut_subcommand("completions", |c| {
            c.about(tr(
                "Print a completion script for your shell",
                "Kabuğunuz için tamamlama betiğini yazdır",
            ))
            .mut_arg("shell", |a| a.help(tr("Shell", "Kabuk")))
        });
    localize_headings(command)
}

/// Sets the headings of every help page ("Usage", "Commands", "Arguments",
/// "Options") in the interface language.
fn localize_headings(command: Command) -> Command {
    let usage = tr("Usage:", "Kullanım:");
    let template = format!(
        "{{before-help}}{{about-with-newline}}\n{usage} {{usage}}\n\n{{all-args}}{{after-help}}"
    );
    let arguments = tr("Arguments", "Argümanlar");
    let options = tr("Options", "Seçenekler");
    let commands = tr("Commands", "Komutlar");

    let subcommands: Vec<String> = command
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    let names: Vec<(String, bool)> = command
        .get_arguments()
        .map(|a| (a.get_id().to_string(), a.is_positional()))
        .collect();

    let mut command = command
        .help_template(template)
        .subcommand_help_heading(commands);
    for (name, positional) in names {
        let heading = if positional { arguments } else { options };
        command = command.mut_arg(name, |a| a.help_heading(heading));
    }
    for name in subcommands {
        command = command.mut_subcommand(name, localize_headings);
    }
    command
}

fn json_help(arg: clap::Arg) -> clap::Arg {
    arg.help(tr(
        "Print JSON instead of text, for scripts",
        "Betikler için metin yerine JSON yazdır",
    ))
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
    .mut_arg("group", |a| {
        a.help(tr(
            "Group (workspace) the connection belongs to",
            "Bağlantının ait olduğu grup (çalışma alanı)",
        ))
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
