# Contributing

Thanks for your interest in sshcm.

## Development

You need Rust 1.100 or newer and an OpenSSH client (`ssh`) on your `PATH`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same three commands on macOS and Linux for every change.

## Layout

- `crates/sshcm-core` holds all logic: the lossless parser, block editing,
  validation, and safe file writing. It is the only code that reads or writes
  the config file.
- `crates/sshcm` is the command line: argument parsing, prompts, and output.
  Keep it thin. Interactive prompts are not tested, so logic belongs in the
  core where it is.

## Rules that matter

- sshcm must never change a byte of the config file outside the block it is
  editing. Every parser change needs a round-trip test: read a fixture, write
  it back, compare bytes.
- Tests must never touch the real `~/.ssh`. Use a temporary directory and pass
  it as `HOME` or `--config`.
- sshcm does not read keys, passwords or passphrases, and does not open
  network connections.
