# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.0] - 2026-11-17

### Added

- `sshcm backups` lists the backups and `sshcm restore [backup]` puts one
  back after showing the difference. A restore is an ordinary write: the
  content it replaces is backed up and the result is checked with ssh.
- `sshcm copy [alias] [new alias]`, and `c` in the full-screen interface,
  create a connection from an existing one, including the settings sshcm has
  no field for.
- `--json` for `list` and `show`.
- `sshcm completions <shell>` prints a completion script.
- sshcm now checks which `nc` is installed: a netcat without `-X` (GNU
  netcat, ncat, BusyBox) cannot run the SOCKS5/HTTP proxy command, and saving
  such a proxy warns about it.
- The details panel and the form preview scroll with PgUp and PgDn.
- The interface shows the backup path after saving and removing.
- Help headings ("Usage", "Options", ...) follow the interface language.

## [0.3.1] - 2026-11-17

### Fixed

- `--dry-run` was ignored by the full-screen interface: saving and removing
  wrote to the config. The interface now shows a "dry run" tag and writes
  nothing.
- A connection without a `HostName` line (ssh then uses the alias, as in
  `Host github.com`) could not be edited. Only new connections need one.
- When editing, values that were already in the file are no longer checked
  against sshcm's own rules, so a hand-written value cannot stop you from
  changing another setting. A value you change is still checked.
- Setting the identity file to "none" removed every `IdentityFile` line of
  the block. It now removes the one sshcm shows; the next one takes its
  place.
- Esc on a form with changes asks before discarding them. "Keep editing" is
  the default.
- The interface now warns, like the subcommands, when a SOCKS5 or HTTP proxy
  is saved and `nc` is not installed.

### Changed

- `sshcm-core`: `model::validate` takes the connection as it was before the
  edit instead of the old alias and a flag.

## [0.3.0] - 2026-11-17

### Added

- Full-screen interface when `sshcm` runs without a subcommand: a searchable
  list with the details and config block of the selected connection, a form
  with validation as you type and a live preview, a proxy picker with
  ordered jump hosts, a remove dialog that defaults to Cancel, and a help
  screen.
- Fuzzy search over alias, address, user and tags.

### Changed

- The interface replaces the question-based main menu. The subcommands and
  their prompts are unchanged.

## [0.2.0] - 2026-11-17

### Added

- Connections in `Include`d files can be edited and removed. The change is
  written to the file the connection lives in, with its own backup.
- Renaming a connection updates `ProxyJump` references in included files too.
- `sshcm list` shows a FILE column for connections from included files.

### Changed

- Only wildcard patterns are read-only now.
- `sshcm-core`: `SaveReport::backup` is now `backups`, one per rewritten
  file; `Entry::read_only` no longer covers included files. Use
  `Store::add`, `Store::update` and `Store::remove` to edit across files.

### Fixed

- Backups of a file whose name starts with another file's name (for example
  `config-extra` next to `config`) are no longer counted, and pruned, as
  backups of the shorter name.

## [0.1.0] - 2026-11-17

### Added

- Lossless, line-based `ssh_config` parser: an unmodified file is written
  back byte for byte.
- `add`, `edit`, `rm`, `list`, `show` and `connect` subcommands, and a main
  menu when `sshcm` runs without arguments.
- Proxy support: jump hosts (`ProxyJump`, also chained), SOCKS5 and HTTP
  proxies through `nc`, and custom `ProxyCommand`. Loops are refused and
  removing a jump host that others use asks again.
- Description and tags stored as `# sshcm:` comments; `list --tag`.
- Safe writes: atomic replace, timestamped backups in
  `~/.ssh/sshcm-backups`, `600`/`700` permissions, symlink-aware, cancelled
  when the file changed on disk, verified with `ssh -G` and restored on
  failure.
- `Include`d files and wildcard patterns are listed read-only.
- `--config`, `--dry-run`, `--yes` and `--lang` flags; settings can be passed
  as flags for scripts.
- `sshcm config` with the `language` (English, Turkish) and `backup_keep`
  settings.
- Install with Homebrew (`brew install empatisoft/tap/sshcm`), the install
  script, prebuilt binaries for macOS and Linux (static musl builds), or
  `cargo install sshcm`.
- CI on macOS and Linux: `cargo fmt --check`, `cargo clippy`, `cargo test`.
