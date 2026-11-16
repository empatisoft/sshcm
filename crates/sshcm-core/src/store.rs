//! Reading and safely writing the config file.
//!
//! Every write follows the same steps: check that the file did not change
//! since it was read, copy it to a timestamped backup, write the new content
//! to a temporary file in the same directory, `fsync`, and `rename` it over
//! the original. Afterwards `ssh -G` must accept the file, or the previous
//! content is put back.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::document::{Document, Entry, dependents};
use crate::model::{Known, collapse_tilde, expand_tilde};

const BACKUP_DIR: &str = "sshcm-backups";
const MAX_INCLUDE_DEPTH: usize = 16;

/// Things that can go wrong while reading or writing the config.
#[derive(Debug)]
pub enum Error {
    /// A file system operation failed.
    Io { path: PathBuf, source: io::Error },
    /// The config file is not valid UTF-8; sshcm will not touch it.
    NotUtf8(PathBuf),
    /// The file changed on disk after sshcm read it.
    ChangedOnDisk(PathBuf),
    /// `ssh -G` rejected the written file. The previous content is back.
    VerifyFailed { stderr: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::NotUtf8(path) => write!(f, "{} is not valid UTF-8.", path.display()),
            Error::ChangedOnDisk(path) => write!(
                f,
                "{} was changed by another program. Nothing was written; try again.",
                path.display()
            ),
            Error::VerifyFailed { stderr } => write!(
                f,
                "ssh rejected the new config, so the previous file was restored.\n{stderr}"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Options for [`Store::save`].
#[derive(Debug, Clone)]
pub struct SaveOptions {
    /// How many backups to keep. Older ones are deleted after a successful
    /// write.
    pub backup_keep: usize,
    /// Host name passed to `ssh -G` to check the written file. `None` checks
    /// the file with a placeholder name.
    pub verify_alias: Option<String>,
}

impl Default for SaveOptions {
    fn default() -> Self {
        SaveOptions {
            backup_keep: 10,
            verify_alias: None,
        }
    }
}

/// Result of the `ssh -G` check after a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// ssh accepted the file.
    Passed,
    /// The `ssh` command was not found, so the check did not run.
    Skipped,
}

/// What a successful save did.
#[derive(Debug, Clone)]
pub struct SaveReport {
    /// Backup of the previous content; `None` when the file was just created.
    pub backup: Option<PathBuf>,
    pub verification: Verification,
}

/// The config file, the files it includes, and the way to write it back.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    home: PathBuf,
    /// Content at read time; `None` when the file did not exist.
    original: Option<String>,
    /// The main config. Edit this, then call [`Store::save`].
    pub doc: Document,
    included: Vec<(PathBuf, Document)>,
    ssh: OsString,
}

impl Store {
    /// `~/.ssh/config`
    pub fn default_path(home: &Path) -> PathBuf {
        home.join(".ssh").join("config")
    }

    /// Reads the config at `path` and every file it includes. A missing file
    /// is an empty config; it is created on the first save.
    pub fn load(path: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Result<Store, Error> {
        let path = path.into();
        let home = home.into();
        let original = read_optional(&path)?;
        let doc = Document::parse(original.as_deref().unwrap_or(""));

        let mut included = Vec::new();
        let mut seen = HashSet::new();
        if let Ok(canonical) = fs::canonicalize(&path) {
            seen.insert(canonical);
        }
        load_includes(&doc, &home, 1, &mut seen, &mut included);

        Ok(Store {
            path,
            home,
            original,
            doc,
            included,
            ssh: OsString::from("ssh"),
        })
    }

    /// Uses another program instead of `ssh` for the check after writing.
    pub fn with_ssh_program(mut self, program: impl Into<OsString>) -> Store {
        self.ssh = program.into();
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The config path with the home directory shortened to `~`.
    pub fn display_path(&self) -> String {
        collapse_tilde(&self.path.to_string_lossy(), &self.home)
    }

    /// True when the config file does not exist yet.
    pub fn is_new(&self) -> bool {
        self.original.is_none()
    }

    /// Blocks of the main config followed by blocks of included files.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = self.doc.entries();
        for (path, doc) in &self.included {
            entries.extend(doc.entries().into_iter().map(|mut e| {
                e.source = Some(path.clone());
                e
            }));
        }
        entries
    }

    /// The first block named `alias`, wherever it lives.
    pub fn find(&self, alias: &str) -> Option<Entry> {
        let entries = self.entries();
        entries
            .iter()
            .find(|e| e.alias() == alias)
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.patterns.iter().any(|p| p == alias))
            })
            .cloned()
    }

    /// Names of the connections that use `alias` as a jump host.
    pub fn dependents(&self, alias: &str) -> Vec<String> {
        dependents(&self.entries(), alias)
    }

    /// What validation needs to know about the other connections.
    /// `editing` is the alias being edited, whose names stay available to it.
    pub fn known(&self, editing: Option<&str>) -> Known {
        let entries = self.entries();
        let mut known = Known::default();
        for entry in &entries {
            let own = editing.is_some_and(|a| entry.alias() == a && entry.source.is_none());
            if own {
                // The other names on a shared `Host a b` line stay taken.
                known.names.extend(
                    entry
                        .patterns
                        .iter()
                        .filter(|p| Some(p.as_str()) != editing)
                        .cloned(),
                );
                continue;
            }
            known.names.extend(entry.patterns.iter().cloned());
            let hops: Vec<String> = entry
                .conn
                .proxy
                .jump_hosts()
                .into_iter()
                .map(str::to_string)
                .collect();
            if !hops.is_empty() {
                for name in &entry.patterns {
                    known.jumps.push((name.clone(), hops.clone()));
                }
            }
        }
        known
    }

    /// Where backups of this config are kept.
    pub fn backup_dir(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(BACKUP_DIR)
    }

    /// Writes `self.doc` to disk. See the module docs for the steps.
    pub fn save(&mut self, opts: &SaveOptions) -> Result<SaveReport, Error> {
        let content = self.doc.render();
        // A symlinked config (dotfiles) is updated through the link.
        let target = resolve_symlink(&self.path);

        if read_optional(&target)? != self.original {
            return Err(Error::ChangedOnDisk(self.path.clone()));
        }

        self.prepare_directory(&target)?;
        let backup = match &self.original {
            Some(original) => Some(self.write_backup(original)?),
            None => None,
        };

        atomic_write(&target, content.as_bytes())?;

        let verification = match self.verify(opts.verify_alias.as_deref()) {
            Ok(v) => v,
            Err(err) => {
                // Put the previous state back. The backup stays on disk.
                match &self.original {
                    Some(original) => atomic_write(&target, original.as_bytes())?,
                    None => fs::remove_file(&target).map_err(io_err(&target))?,
                }
                return Err(err);
            }
        };

        self.prune_backups(opts.backup_keep.max(1))?;
        self.original = Some(content);
        Ok(SaveReport {
            backup,
            verification,
        })
    }

    /// Creates the config's directory if needed and keeps `~/.ssh` at 700.
    fn prepare_directory(&self, target: &Path) -> Result<(), Error> {
        for dir in [self.path.parent(), target.parent()].into_iter().flatten() {
            if !dir.as_os_str().is_empty() && !dir.exists() {
                DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(dir)
                    .map_err(io_err(dir))?;
            }
        }
        let ssh_dir = self.home.join(".ssh");
        if self.path.parent() == Some(ssh_dir.as_path()) {
            let mode = fs::metadata(&ssh_dir)
                .map_err(io_err(&ssh_dir))?
                .permissions()
                .mode();
            if mode & 0o777 != 0o700 {
                fs::set_permissions(&ssh_dir, Permissions::from_mode(0o700))
                    .map_err(io_err(&ssh_dir))?;
            }
        }
        Ok(())
    }

    fn backup_prefix(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config".to_string());
        format!("{name}-")
    }

    /// Copies the previous content to `sshcm-backups/config-YYYYMMDD-HHMMSS`.
    fn write_backup(&self, original: &str) -> Result<PathBuf, Error> {
        let dir = self.backup_dir();
        if !dir.exists() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
                .map_err(io_err(&dir))?;
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let base = format!("{}{stamp}", self.backup_prefix());
        // Two writes in the same second get a numeric suffix. The suffix
        // continues after the highest one in use, so that a name freed by
        // pruning is never reused for a newer backup.
        let first = fs::read_dir(&dir)
            .map_err(io_err(&dir))?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                match name.strip_prefix(&base)? {
                    "" => Some(0),
                    rest => rest.strip_prefix('-')?.parse::<u32>().ok(),
                }
            })
            .max()
            .map_or(0, |n| n + 1);
        for n in first.. {
            let name = if n == 0 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            let path = dir.join(name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(original.as_bytes()).map_err(io_err(&path))?;
                    file.sync_all().map_err(io_err(&path))?;
                    return Ok(path);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err(&path)(e)),
            }
        }
        unreachable!("the loop above only ends by returning")
    }

    /// Backups of this config, oldest first.
    pub fn backups(&self) -> Result<Vec<PathBuf>, Error> {
        let dir = self.backup_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let prefix = self.backup_prefix();
        let mut found: Vec<(String, PathBuf)> = fs::read_dir(&dir)
            .map_err(io_err(&dir))?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let stamp = name.strip_prefix(&prefix)?;
                // Sort by timestamp, then by the same-second counter.
                let (time, counter) = match stamp.match_indices('-').nth(1) {
                    Some((i, _)) => (&stamp[..i], stamp[i + 1..].parse::<u32>().ok()?),
                    None => (stamp, 0),
                };
                Some((format!("{time}-{counter:010}"), e.path()))
            })
            .collect();
        found.sort();
        Ok(found.into_iter().map(|(_, path)| path).collect())
    }

    fn prune_backups(&self, keep: usize) -> Result<(), Error> {
        let backups = self.backups()?;
        let excess = backups.len().saturating_sub(keep);
        for path in &backups[..excess] {
            fs::remove_file(path).map_err(io_err(path))?;
        }
        Ok(())
    }

    /// Runs `ssh -F <config> -G <alias>`: ssh parses the whole file and
    /// prints the resulting settings without connecting.
    fn verify(&self, alias: Option<&str>) -> Result<Verification, Error> {
        let output = Command::new(&self.ssh)
            .arg("-F")
            .arg(&self.path)
            .arg("-G")
            .arg("--")
            .arg(alias.unwrap_or("sshcm-config-check"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        match output {
            Ok(out) if out.status.success() => Ok(Verification::Passed),
            Ok(out) => Err(Error::VerifyFailed {
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Verification::Skipped),
            Err(e) => Err(Error::Io {
                path: PathBuf::from(&self.ssh),
                source: e,
            }),
        }
    }
}

/// Reads a file as UTF-8 text; `None` when it does not exist.
fn read_optional(path: &Path) -> Result<Option<String>, Error> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| Error::NotUtf8(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(path)(e)),
    }
}

/// The file a symlink points to, or the path itself.
fn resolve_symlink(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if !is_link {
        return path.to_path_buf();
    }
    fs::canonicalize(path).unwrap_or_else(|_| {
        // Dangling link: write where it points.
        match fs::read_link(path) {
            Ok(dest) if dest.is_absolute() => dest,
            Ok(dest) => path.parent().unwrap_or_else(|| Path::new(".")).join(dest),
            Err(_) => path.to_path_buf(),
        }
    })
}

/// Replaces `target` with `content` in one step, with mode 600.
fn atomic_write(target: &Path, content: &[u8]) -> Result<(), Error> {
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_err(dir))?;
    tmp.write_all(content).map_err(io_err(target))?;
    tmp.as_file()
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(io_err(target))?;
    tmp.as_file().sync_all().map_err(io_err(target))?;
    tmp.persist(target).map_err(|e| io_err(target)(e.error))?;
    // Make the rename itself durable.
    if let Ok(dir) = File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Reads the files named by `Include` lines, depth first. Files that cannot
/// be read are skipped, as ssh does for patterns that match nothing.
fn load_includes(
    doc: &Document,
    home: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, Document)>,
) {
    if depth > MAX_INCLUDE_DEPTH {
        return;
    }
    for pattern in doc.includes() {
        // Relative paths are looked up in ~/.ssh, as ssh does for user configs.
        let mut full = expand_tilde(&pattern, home);
        if full.is_relative() {
            full = home.join(".ssh").join(full);
        }
        let Ok(paths) = glob::glob(&full.to_string_lossy()) else {
            continue;
        };
        for path in paths.filter_map(Result::ok) {
            let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !path.is_file() || !seen.insert(key) {
                continue;
            }
            let Ok(Some(text)) = read_optional(&path) else {
                continue;
            };
            let included = Document::parse(&text);
            out.push((path, included.clone()));
            load_includes(&included, home, depth + 1, seen, out);
        }
    }
}
