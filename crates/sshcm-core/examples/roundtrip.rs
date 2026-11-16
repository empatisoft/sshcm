//! Checks that a config file survives a read/write round trip unchanged.
//! Read-only: the file is compared in memory and never written.
//!
//! ```sh
//! cargo run -p sshcm-core --example roundtrip -- ~/.ssh/config
//! ```

use std::process::ExitCode;

use sshcm_core::Document;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: roundtrip <config file>");
        return ExitCode::from(2);
    };
    let input = match std::fs::read_to_string(&path) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::from(2);
        }
    };
    let doc = Document::parse(&input);
    let entries = doc.entries();
    let editable = entries.iter().filter(|e| !e.read_only()).count();

    // Saving every entry without changes must not alter a byte either.
    let mut untouched = doc.clone();
    for entry in entries.iter().filter(|e| !e.read_only()) {
        if untouched.update(entry.alias(), &entry.conn).is_err() {
            eprintln!("{path}: could not re-save {}", entry.alias());
            return ExitCode::FAILURE;
        }
    }

    if doc.render() == input && untouched.render() == input {
        println!(
            "{path}: identical after round trip ({} lines, {} blocks, {editable} editable)",
            doc.lines().len(),
            entries.len()
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("{path}: round trip changed the file");
        ExitCode::FAILURE
    }
}
