use clap::Parser;

/// SSH Connection Manager
#[derive(Parser)]
#[command(name = "sshcm", version, about)]
struct Cli {}

fn main() {
    Cli::parse();
}
