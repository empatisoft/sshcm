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

> **Status:** v0.1 is in development and not released yet. Runs on macOS and
> Linux.

## Install

Until the first release, build from source. You need Rust 1.100 or newer.

```sh
cargo install --git https://github.com/empatisoft/sshcm sshcm
```

Homebrew, crates.io and prebuilt binaries are planned for v0.2.

## Usage

Run `sshcm` without arguments to open the menu, or use a subcommand directly.
When you leave out the alias, you pick the connection from a searchable list.

| Command | What it does |
| --- | --- |
| `sshcm` | Menu: Connect, Add, Edit, Remove, List, Quit |
| `sshcm list [--tag <tag>]` | Print the connections as a table |
| `sshcm show [alias]` | Show every setting of one connection |
| `sshcm add [alias]` | Create a connection |
| `sshcm edit [alias]` | Change a connection; shows a diff before saving |
| `sshcm rm [alias]` | Delete a connection; the default answer is No |
| `sshcm connect [alias] [-- ssh args]` | Connect with the system `ssh` |
| `sshcm config [get\|set] ...` | sshcm's own settings |

Global flags:

- `--config <path>` uses another config file instead of `~/.ssh/config`.
- `--dry-run` shows what would change and writes nothing.
- `--yes` skips confirmations, for scripts.
- `--lang <en|tr>` sets the interface language for one run.

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

Exit codes: `0` success, `1` error, `130` cancelled with Esc or Ctrl+C.
`connect` exits with ssh's own exit code.

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
`nc`: it ships with macOS; on Debian and Ubuntu install `netcat-openbsd`.

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
  that with `sshcm config set backup_keep 20`. To restore, copy a backup over
  `~/.ssh/config`.
- **Checked by ssh.** After writing, `ssh -G <alias>` must accept the file. If
  it does not, the previous content is restored and the error is shown.
- **No surprises from other programs.** If the file changed on disk since
  sshcm read it, the write is cancelled.
- **Permissions.** The config file is kept at `600` and `~/.ssh` at `700`. A
  symlinked config (dotfiles) is updated through the link.
- **No secrets, no network.** sshcm never reads keys, passwords or
  passphrases; it only stores the path of a key file. It opens no network
  connections and has no telemetry or update check.

Read-only entries: wildcard patterns such as `Host *` and blocks that come
from `Include`d files are listed but cannot be edited or removed. `Match`
blocks are kept and not listed. New connections are added after your last
regular block, so a trailing `Host *` keeps applying to them.

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

## Roadmap

- v0.2: Homebrew tap, crates.io, prebuilt binaries
- v0.3: edit and remove connections inside `Include`d files
- v0.5: full-screen terminal interface
- v1.0: stable command line

## License

MIT. See [LICENSE](LICENSE).
