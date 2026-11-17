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

To check that the parser leaves your own config untouched (read-only, nothing
is written):

```sh
cargo run -p sshcm-core --example roundtrip -- ~/.ssh/config
```

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

## Releasing

Releases are built by [dist](https://github.com/axodotdev/cargo-dist). The
configuration is in `dist-workspace.toml`; `.github/workflows/release.yml` is
generated from it, so change the config and run `dist generate` instead of
editing the workflow.

One-time setup:

1. Create the public repository `empatisoft/homebrew-tap`.
2. Add a repository secret `HOMEBREW_TAP_TOKEN` to `empatisoft/sshcm`: a
   token that can push to the tap repository.

For each release:

1. Set the version in the root `Cargo.toml` (and the `sshcm-core` version in
   `crates/sshcm/Cargo.toml`), and move the `Unreleased` notes in
   `CHANGELOG.md` under the new version.
2. Check what will be built: `dist plan`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`. GitHub Actions
   builds the macOS and Linux binaries, creates the GitHub Release and updates
   the Homebrew formula.
4. Publish to crates.io, core first:
   `cargo publish -p sshcm-core && cargo publish -p sshcm`.
