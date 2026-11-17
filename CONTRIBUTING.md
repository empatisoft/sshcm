# Contributing

Thanks for your interest in sshcm.

## Development

You need an OpenSSH client (`ssh`) on your `PATH`. `rust-toolchain.toml` pins
the Rust version used for development and releases; rustup installs it on the
first `cargo` command. The oldest Rust that sshcm must build with is the
`rust-version` in `Cargo.toml` (1.88), which CI checks separately.

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

The full-screen interface (`crates/sshcm/src/tui`) keeps state and drawing
apart: `app`, `form` and `picker` react to keys, `ui` draws. Tests press keys
on the state and draw into an in-memory terminal. To look at every screen as
text:

```sh
cargo test -p sshcm dump -- --ignored --nocapture
```

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
3. Optional: add a repository secret `CARGO_REGISTRY_TOKEN` (a crates.io API
   token that may publish new versions). With it, the crates are published
   automatically; without it, step 4 below is done by hand.

For each release:

1. Set the version in the root `Cargo.toml`, in both places: `version` under
   `[workspace.package]` and the `sshcm-core` line under
   `[workspace.dependencies]`. Move the `Unreleased` notes in `CHANGELOG.md`
   under the new version.
2. Check what will be built: `dist plan`.
3. Tag and push: `git tag v0.5.0 && git push origin v0.5.0`. GitHub Actions
   builds the macOS and Linux binaries, creates the GitHub Release and updates
   the Homebrew formula.
4. crates.io: the "Publish crates" workflow runs after a successful release.
   Without the token, publish by hand from a clean checkout of the tag, core
   first: `cargo publish -p sshcm-core && cargo publish -p sshcm`.
