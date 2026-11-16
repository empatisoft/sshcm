# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
- CI on macOS and Linux: `cargo fmt --check`, `cargo clippy`, `cargo test`.
