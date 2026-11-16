//! Finds private keys in `~/.ssh` by name only. Key files are never opened.

use std::fs;
use std::path::Path;

/// Private keys in `~/.ssh`, as `~/.ssh/<name>`: every file that has a
/// matching `<name>.pub` next to it.
pub fn discover(home: &Path) -> Vec<String> {
    let dir = home.join(".ssh");
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = read
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_key = !name.ends_with(".pub")
                && entry.path().is_file()
                && dir.join(format!("{name}.pub")).is_file();
            is_key.then(|| format!("~/.ssh/{name}"))
        })
        .collect();
    keys.sort();
    keys
}
