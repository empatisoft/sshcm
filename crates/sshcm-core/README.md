# sshcm-core

The library behind [sshcm](https://github.com/empatisoft/sshcm), the SSH
Connection Manager.

- A line-based, lossless `ssh_config` parser: an unmodified file is written
  back byte for byte.
- Adding, updating and removing `Host` blocks without touching the rest of
  the file, including blocks that live in `Include`d files.
- Safe writes: atomic replace, timestamped backups, permission handling and a
  check with `ssh -G` that restores the previous content on failure.

The API follows the needs of the `sshcm` command line and may change between
`0.x` releases.

```rust,no_run
use sshcm_core::{Connection, SaveOptions, Store};

let home = std::path::PathBuf::from(std::env::var("HOME")?);
let mut store = Store::load(Store::default_path(&home), &home)?;

store.add(&Connection {
    alias: "web".to_string(),
    hostname: Some("10.0.2.10".to_string()),
    ..Connection::default()
});
store.save(&SaveOptions::default())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

MIT licensed.
