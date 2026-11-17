# sshcm

SSH Connection Manager: add, edit, remove and list the connections in your
`~/.ssh/config` from the terminal.

sshcm does not connect by itself and keeps no database. The config file is the
only source of truth, so every connection you add works as-is with `ssh`,
`scp`, `rsync` and `git`.

```console
$ sshcm add
> Alias: ihu
> HostName: app.example.edu
> User: deploy
> Port: 22
> Identity file: ~/.ssh/id_ed25519
> Proxy: Jump host
> Jump host: bastion

Will be added to ~/.ssh/config
  + Host ihu
  +     HostName app.example.edu
  +     User deploy
  +     IdentityFile ~/.ssh/id_ed25519
  +     ProxyJump bastion
Port 22 is the default, so it is not written.

? Save this connection? (Y/n)
✓ Saved ihu  backup: ~/.ssh/sshcm-backups/config-20261116-204900
✓ Config check passed  ssh -G ihu
```

Runs on macOS and Linux.

## Install

Homebrew:

```sh
brew install empatisoft/tap/sshcm
```

Install script (downloads the prebuilt binary for your platform into
`~/.cargo/bin`):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/empatisoft/sshcm/releases/latest/download/sshcm-installer.sh | sh
```

Prebuilt binaries for macOS and Linux (aarch64 and x86_64; the Linux builds
are static) are on the
[releases page](https://github.com/empatisoft/sshcm/releases).

With Cargo (builds from source; needs Rust 1.88 or newer):

```sh
cargo install sshcm
```

## Usage

Run `sshcm` without arguments to open the full-screen interface, or use a
subcommand directly. When you leave out the alias of a subcommand, you pick
the connection from a searchable list.

| Command | What it does |
| --- | --- |
| `sshcm` | Full-screen interface |
| `sshcm list [--tag <tag>] [--json]` | Print the connections as a table, or as JSON |
| `sshcm show [alias] [--json]` | Show every setting of one connection |
| `sshcm add [alias]` | Create a connection |
| `sshcm edit [alias]` | Change a connection; shows a diff before saving |
| `sshcm copy [alias] [new alias]` | Create a connection from an existing one |
| `sshcm rm [alias]` | Delete a connection; the default answer is No |
| `sshcm connect [alias] [-- ssh args]` | Connect with the system `ssh` |
| `sshcm config [get\|set] ...` | sshcm's own settings |
| `sshcm backups` | List the backups of the config |
| `sshcm restore [backup]` | Put a backup back; shows the difference first |
| `sshcm completions <shell>` | Print a completion script (bash, zsh, fish, ...) |

Global flags:

- `--config <path>` uses another config file instead of `~/.ssh/config`.
- `--dry-run` shows what would change and writes nothing.
- `--yes` skips confirmations, for scripts.
- `--lang <en|tr>` sets the interface language for one run.

### Full-screen interface

```text
sshcm  4 connections                                                                             ~/.ssh/config
┌ Connections ──────────────────────────────────┐┌ web-prod ─────────────────────────────────────────────────┐
│/ Search by alias, host or user                ││Alias             web-prod                                 │
│  bastion   ops@bastion.example.com            ││HostName          10.0.2.10                                │
│  db-prod   postgres@10.0.2.15      via bastion││User              deploy                                   │
│❯ web-prod  deploy@10.0.2.10        via bastion││Port              22 (default)                             │
│  staging   ubuntu@203.0.113.24:2222           ││Identity file     none                                     │
│  Read-only                                    ││Proxy             Jump host → bastion                      │
│  *         pattern                            ││Tags              prod, web                                │
│                                               ││Other settings    ServerAliveInterval 60                   │
│                                               ││                                                           │
│                                               ││Config block                                               │
│                                               ││  # sshcm:tags prod, web                                   │
│                                               ││  Host web-prod                                            │
│                                               ││      HostName 10.0.2.10                                   │
│                                               ││      User deploy                                          │
│                                               ││      ProxyJump bastion                                    │
│                                               ││      ServerAliveInterval 60                               │
│                                               ││                                                           │
└───────────────────────────────────────────────┘└───────────────────────────────────────────────────────────┘
                                              enter connect  a add  e edit  d delete  / search  ? help  q quit
```

| Key | What it does |
| --- | --- |
| `↑` `↓` or `j` `k` | Move through the list |
| `/` | Search by alias, address, user and tags (fuzzy) |
| `Enter` | Connect to the selected connection |
| `a` | Add a connection |
| `e` | Edit the selected connection |
| `c` | Copy the selected connection into a new one |
| `d` | Remove the selected connection; "Cancel" is selected by default |
| `PgUp` / `PgDn` | Scroll the details, or the preview in the form |
| `Tab` / `Shift+Tab` | Next / previous field in the form |
| `Enter` on Proxy or Identity file | Open the picker for that field |
| `Ctrl+S` | Save the form |
| `Esc` | Close the form or dialog |
| `?` | Help |
| `q` | Quit |

The form checks every field as you type and shows the block, or the diff of
the block, that will be written. In the proxy picker, `Tab` switches between
the proxy types and `Space` selects jump hosts; selecting several chains them
in that order. Below 80 columns only the list is shown.

### Scripts

Every setting can be passed as a flag, so `add` and `edit` work without
prompts:

```sh
sshcm add web-prod --hostname 10.0.2.10 --user deploy \
  --jump bastion --tags prod,web --yes

sshcm edit web-prod --port 2222 --yes   # change one setting
sshcm edit web-prod --port "" --yes     # an empty value removes the line
sshcm edit bastion --rename gate --yes  # ProxyJump references follow
sshcm rm web-prod --yes
```

Flags for `add` and `edit`: `--hostname`, `--user`, `--port`,
`--identity-file`, `--desc`, `--tags`, and one of `--jump`, `--socks5`,
`--http-proxy`, `--proxy-command`, `--no-proxy`.

`list --json` and `show --json` print the connections for other programs.
The keys follow the ssh_config keywords:

```sh
sshcm list --json | jq -r '.[] | select(.tags | index("prod")) | .alias'
```

Exit codes: `0` success, `1` error, `130` cancelled with Esc or Ctrl+C.
`connect` exits with ssh's own exit code.

### Shell completions

```sh
sshcm completions zsh > ~/.zfunc/_sshcm        # zsh: a directory in $fpath
sshcm completions bash > ~/.local/share/bash-completion/completions/sshcm
sshcm completions fish > ~/.config/fish/completions/sshcm.fish
```

## Proxy

| Option | Written to the config |
| --- | --- |
| None | nothing |
| Jump host | `ProxyJump bastion` (several hosts: `ProxyJump a,b`) |
| SOCKS5 | `ProxyCommand nc -X 5 -x 10.0.0.1:1080 %h %p` |
| HTTP | `ProxyCommand nc -X connect -x 10.0.0.1:3128 %h %p` |
| Custom command | `ProxyCommand <your command>` |

sshcm refuses jump hosts that would form a loop, and warns before you remove a
connection that others jump through. SOCKS5 and HTTP proxies need the OpenBSD
`nc`: it ships with macOS; on Debian and Ubuntu install `netcat-openbsd`
(`openbsd-netcat` on Arch, `netcat` on Fedora). Other netcats (GNU netcat,
ncat, BusyBox) have no `-X` option; sshcm checks which one you have and warns
before saving such a proxy.

## Description and tags

A connection can carry a one-line description and tags. ssh has no setting for
these, so they are stored as comments above the `Host` line, which ssh
ignores:

```
# sshcm:desc Production web server
# sshcm:tags prod, web
Host web-prod
    HostName 10.0.2.10
```

## How sshcm treats your config

The biggest risk of a tool like this is damaging your config file. These rules
hold for every write:

- **Only the block you change is rewritten.** Everything else, including
  comments, blank lines, indentation and settings sshcm does not know, stays
  byte for byte the same.
- **Atomic writes.** New content goes to a temporary file in the same
  directory, is synced to disk and renamed over the original. A half-written
  file cannot occur.
- **Backups.** Before each write the file is copied to
  `~/.ssh/sshcm-backups/config-YYYYMMDD-HHMMSS`. The last 10 are kept; change
  that with `sshcm config set backup_keep 20`. `sshcm backups` lists them and
  `sshcm restore` puts one back after showing what would change; the content
  it replaces is backed up too, so a restore can be undone.
- **Checked by ssh.** After writing, `ssh -G <alias>` must accept the file. If
  it does not, the previous content is restored and the error is shown.
- **No surprises from other programs.** If the file changed on disk since
  sshcm read it, the write is cancelled.
- **Permissions.** The config file is kept at `600` and `~/.ssh` at `700`. A
  symlinked config (dotfiles) is updated through the link.
- **No secrets, no network.** sshcm never reads keys, passwords or
  passphrases; it only stores the path of a key file. It opens no network
  connections and has no telemetry or update check.

### Included files

Connections in files pulled in with `Include` are listed together with the
ones in your main config; `sshcm list` shows which file each comes from.
Editing or removing such a connection rewrites that file, not the main config,
with the same guarantees. Its backup goes to `~/.ssh/sshcm-backups` too, named
after the file (`config.d/work` becomes `config.d__work-YYYYMMDD-HHMMSS`). New
connections are always added to the main config, after your last regular
block, so a trailing `Host *` keeps applying to them.

Wildcard patterns such as `Host *` are listed read-only and cannot be edited
or removed. `Match` blocks are kept and not listed.

## Settings

sshcm's own settings live in `~/.config/sshcm/config.toml`.

| Setting | Default | Values |
| --- | --- | --- |
| `language` | `en` | `en`, `tr` |
| `backup_keep` | `10` | whole number, 1 or more |

```sh
sshcm config set language tr
sshcm config get backup_keep
```

Language order: `--lang`, then the `SSHCM_LANG` environment variable, then the
settings file. Command names and flags are the same in every language.
`NO_COLOR` turns colors off.

## Feedback

Found a bug, or missing something? Open an issue:
[github.com/empatisoft/sshcm/issues](https://github.com/empatisoft/sshcm/issues/new/choose).
There is a form for bug reports and one for feature requests. Please remove
real host names and addresses from anything you paste.

## Roadmap

sshcm stays at `0.x` while the command line can still change. v1.0 will fix
the command names and flags, based on the feedback collected until then.

## License

MIT. See [LICENSE](LICENSE).
