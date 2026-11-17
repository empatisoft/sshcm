//! Reading and safely writing the config file.
//!
//! Every write follows the same steps: check that the file did not change
//! since it was read, copy it to a timestamped backup, write the new content
//! to a temporary file in the same directory, `fsync`, and `rename` it over
//! the original. Afterwards `ssh -G` must accept the result, or the previous
//! content is put back.
//!
//! A save can touch several files: the main config and files it includes.
//! All of them are checked before the first one is written, and all of them
//! are restored if the check with ssh fails.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::document::{DocError, Document, Entry, dependents, is_pattern};
use crate::model::{Connection, Known, collapse_tilde, expand_tilde};

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
    /// Backups of the previous content, one per file that was rewritten.
    /// Empty when the only file written was just created.
    pub backups: Vec<PathBuf>,
    pub verification: Verification,
}

/// One backup in `sshcm-backups`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// The backup file.
    pub path: PathBuf,
    /// The config file it is a copy of.
    pub file: PathBuf,
    /// When it was taken, as `YYYYMMDD-HHMMSS` in local time.
    pub time: String,
    /// Order among backups taken in the same second.
    pub counter: u32,
}

/// A file pulled in by an `Include` line.
#[derive(Debug, Clone)]
struct Included {
    path: PathBuf,
    /// Content at read time.
    original: String,
    doc: Document,
}

/// One file to write during a save.
struct Pending {
    path: PathBuf,
    /// The file behind a symlink, or `path` itself.
    target: PathBuf,
    original: Option<String>,
    content: String,
}

/// The config file, the files it includes, and the way to write them back.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    home: PathBuf,
    /// Content at read time; `None` when the file did not exist.
    original: Option<String>,
    /// The main config. New connections are always added here.
    pub doc: Document,
    included: Vec<Included>,
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

    /// True when the main config or an included file differs from what is
    /// on disk.
    pub fn is_modified(&self) -> bool {
        self.doc.render() != self.original.as_deref().unwrap_or("")
            || self.included.iter().any(|f| f.doc.render() != f.original)
    }

    /// Blocks of the main config followed by blocks of included files.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = self.doc.entries();
        for file in &self.included {
            entries.extend(file.doc.entries().into_iter().map(|mut e| {
                e.source = Some(file.path.clone());
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
        // The block an edit applies to: the first editable one with the name.
        let own = editing.and_then(|alias| {
            entries
                .iter()
                .position(|e| !e.is_pattern && e.patterns.iter().any(|p| p == alias))
        });
        let mut known = Known::default();
        for (i, entry) in entries.iter().enumerate() {
            if Some(i) == own {
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

    /// The document that holds the editable block named `alias`: the main
    /// config first, then included files in the order they were read.
    fn doc_holding(&mut self, alias: &str) -> Result<&mut Document, DocError> {
        if self.doc.block_text(alias).is_some() {
            return Ok(&mut self.doc);
        }
        let read_only = self.entries().iter().any(|e| {
            e.patterns.iter().any(|p| p == alias) && e.patterns.iter().any(|p| is_pattern(p))
        });
        match self
            .included
            .iter_mut()
            .find(|f| f.doc.block_text(alias).is_some())
        {
            Some(file) => Ok(&mut file.doc),
            None if read_only => Err(DocError::ReadOnly(alias.to_string())),
            None => Err(DocError::NotFound(alias.to_string())),
        }
    }

    /// Text of the editable block named `alias`, from whichever file holds it.
    pub fn block_text(&self, alias: &str) -> Option<String> {
        self.doc
            .block_text(alias)
            .or_else(|| self.included.iter().find_map(|f| f.doc.block_text(alias)))
    }

    /// Adds a connection. New connections always go to the main config.
    pub fn add(&mut self, conn: &Connection) {
        self.doc.add(conn);
    }

    /// Updates the block named `alias` in the file it lives in. When the
    /// alias changes, `ProxyJump` references are updated in every file.
    pub fn update(&mut self, alias: &str, conn: &Connection) -> Result<(), DocError> {
        self.doc_holding(alias)?.update(alias, conn)?;
        if conn.alias != alias {
            self.doc.rename_jump_host(alias, &conn.alias);
            for file in &mut self.included {
                file.doc.rename_jump_host(alias, &conn.alias);
            }
        }
        Ok(())
    }

    /// Removes the block named `alias` from the file it lives in.
    pub fn remove(&mut self, alias: &str) -> Result<(), DocError> {
        self.doc_holding(alias)?.remove(alias)
    }

    /// Where backups are kept: next to the main config, for every file.
    pub fn backup_dir(&self) -> PathBuf {
        self.config_dir().join(BACKUP_DIR)
    }

    fn config_dir(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// The files whose content differs from what was read.
    fn pending(&self) -> Vec<Pending> {
        let mut pending = Vec::new();
        let content = self.doc.render();
        if content != self.original.as_deref().unwrap_or("") {
            pending.push(Pending {
                path: self.path.clone(),
                // A symlinked config (dotfiles) is updated through the link.
                target: resolve_symlink(&self.path),
                original: self.original.clone(),
                content,
            });
        }
        for file in &self.included {
            let content = file.doc.render();
            if content != file.original {
                pending.push(Pending {
                    path: file.path.clone(),
                    target: resolve_symlink(&file.path),
                    original: Some(file.original.clone()),
                    content,
                });
            }
        }
        pending
    }

    /// Writes every changed file to disk. See the module docs for the steps.
    pub fn save(&mut self, opts: &SaveOptions) -> Result<SaveReport, Error> {
        let pending = self.pending();

        // Nothing is written unless every file is still as it was read.
        for file in &pending {
            if read_optional(&file.target)? != file.original {
                return Err(Error::ChangedOnDisk(file.path.clone()));
            }
        }

        let mut backups = Vec::new();
        for file in &pending {
            if file.path == self.path {
                self.prepare_directory(&file.target)?;
            }
            if let Some(original) = &file.original {
                backups.push(self.write_backup(&file.path, original)?);
            }
        }

        let mut written: Vec<&Pending> = Vec::new();
        let outcome = pending
            .iter()
            .try_for_each(|file| {
                atomic_write(&file.target, file.content.as_bytes())?;
                written.push(file);
                Ok(())
            })
            .and_then(|()| self.verify(opts.verify_alias.as_deref()));
        let verification = match outcome {
            Ok(verification) => verification,
            Err(err) => {
                // Put the previous state back. The backups stay on disk.
                for file in written {
                    match &file.original {
                        Some(original) => atomic_write(&file.target, original.as_bytes())?,
                        None => fs::remove_file(&file.target).map_err(io_err(&file.target))?,
                    }
                }
                return Err(err);
            }
        };

        for file in &pending {
            self.prune_backups(&file.path, opts.backup_keep.max(1))?;
        }
        self.original = Some(self.doc.render());
        for file in &mut self.included {
            file.original = file.doc.render();
        }
        Ok(SaveReport {
            backups,
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

    /// The start of a file's backup names. The main config gives `config-`;
    /// an included file gives its path relative to the config directory with
    /// `/` written as `__`, for example `config.d__work-`.
    fn backup_prefix(&self, path: &Path) -> String {
        let name = if path == self.path {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "config".to_string())
        } else {
            path.strip_prefix(self.config_dir())
                .unwrap_or(path)
                .to_string_lossy()
                .trim_start_matches('/')
                .replace('/', "__")
        };
        format!("{name}-")
    }

    /// Copies the previous content of `path` to
    /// `sshcm-backups/<name>-YYYYMMDD-HHMMSS`.
    fn write_backup(&self, path: &Path, original: &str) -> Result<PathBuf, Error> {
        let dir = self.backup_dir();
        if !dir.exists() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)
                .map_err(io_err(&dir))?;
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        // Two writes in the same second get a numeric suffix. The suffix
        // continues after the highest one in use, so that a name freed by
        // pruning is never reused for a newer backup.
        let first = self
            .backups_of(path)?
            .iter()
            .filter_map(|(time, counter, _)| (*time == stamp).then_some(counter + 1))
            .max()
            .unwrap_or(0);
        let base = format!("{}{stamp}", self.backup_prefix(path));
        for n in first.. {
            let name = if n == 0 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            let backup = dir.join(name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&backup)
            {
                Ok(mut file) => {
                    file.write_all(original.as_bytes())
                        .map_err(io_err(&backup))?;
                    file.sync_all().map_err(io_err(&backup))?;
                    return Ok(backup);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err(&backup)(e)),
            }
        }
        unreachable!("the loop above only ends by returning")
    }

    /// Backups of `path` as `(timestamp, same-second counter, file)`, oldest
    /// first. Only names that are exactly prefix + timestamp (+ counter)
    /// count, so `config-` never picks up backups of `config-extra`.
    fn backups_of(&self, path: &Path) -> Result<Vec<(String, u32, PathBuf)>, Error> {
        let dir = self.backup_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let prefix = self.backup_prefix(path);
        let mut found: Vec<(String, u32, PathBuf)> = fs::read_dir(&dir)
            .map_err(io_err(&dir))?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let (time, counter) = parse_stamp(name.strip_prefix(&prefix)?)?;
                Some((time, counter, e.path()))
            })
            .collect();
        found.sort();
        Ok(found)
    }

    /// Backups of the main config, oldest first.
    pub fn backups(&self) -> Result<Vec<PathBuf>, Error> {
        let found = self.backups_of(&self.path)?;
        Ok(found.into_iter().map(|(_, _, path)| path).collect())
    }

    /// Backups of the main config and of every included file, newest first.
    pub fn all_backups(&self) -> Result<Vec<Backup>, Error> {
        let files = std::iter::once(&self.path).chain(self.included.iter().map(|f| &f.path));
        let mut all = Vec::new();
        for file in files {
            for (time, counter, path) in self.backups_of(file)? {
                all.push(Backup {
                    path,
                    file: file.clone(),
                    time,
                    counter,
                });
            }
        }
        // Names only order backups of one file; across files in the same
        // second the file's modification time decides.
        let written = |b: &Backup| fs::metadata(&b.path).and_then(|m| m.modified()).ok();
        all.sort_by(|a, b| (&b.time, written(b), b.counter).cmp(&(&a.time, written(a), a.counter)));
        Ok(all)
    }

    /// Puts the content of a backup into memory in place of the file it was
    /// taken from. Nothing is written until [`Store::save`], which backs up
    /// the current content first and checks the result with ssh like any
    /// other change.
    pub fn restore(&mut self, backup: &Backup) -> Result<(), Error> {
        let text = read_optional(&backup.path)?.ok_or_else(|| Error::Io {
            path: backup.path.clone(),
            source: io::Error::from(io::ErrorKind::NotFound),
        })?;
        let doc = Document::parse(&text);
        if backup.file == self.path {
            self.doc = doc;
        } else if let Some(file) = self.included.iter_mut().find(|f| f.path == backup.file) {
            file.doc = doc;
        } else {
            return Err(Error::Io {
                path: backup.file.clone(),
                source: io::Error::from(io::ErrorKind::NotFound),
            });
        }
        Ok(())
    }

    /// Current content of the main config or of an included file, as held
    /// in memory.
    pub fn content_of(&self, file: &Path) -> Option<String> {
        if file == self.path {
            Some(self.doc.render())
        } else {
            self.included
                .iter()
                .find(|f| f.path == file)
                .map(|f| f.doc.render())
        }
    }

    fn prune_backups(&self, path: &Path, keep: usize) -> Result<(), Error> {
        let backups = self.backups_of(path)?;
        let excess = backups.len().saturating_sub(keep);
        for (_, _, backup) in &backups[..excess] {
            fs::remove_file(backup).map_err(io_err(backup))?;
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

/// Splits `YYYYMMDD-HHMMSS` or `YYYYMMDD-HHMMSS-<n>` into timestamp and
/// counter. Anything else is not a backup name.
fn parse_stamp(stamp: &str) -> Option<(String, u32)> {
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts = stamp.split('-');
    let (date, time) = (parts.next()?, parts.next()?);
    let counter = match parts.next() {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => n.parse().ok()?,
        Some(_) => return None,
        None => 0,
    };
    if parts.next().is_some() || !digits(date, 8) || !digits(time, 6) {
        return None;
    }
    Some((format!("{date}-{time}"), counter))
}

/// Reads the files named by `Include` lines, depth first. Files that cannot
/// be read are skipped, as ssh does for patterns that match nothing.
fn load_includes(
    doc: &Document,
    home: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<Included>,
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
            let Ok(Some(original)) = read_optional(&path) else {
                continue;
            };
            let included = Document::parse(&original);
            out.push(Included {
                path,
                original,
                doc: included.clone(),
            });
            load_includes(&included, home, depth + 1, seen, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_stamp;

    #[test]
    fn backup_stamps_are_parsed_strictly() {
        assert_eq!(
            parse_stamp("20261116-204900"),
            Some(("20261116-204900".to_string(), 0))
        );
        assert_eq!(
            parse_stamp("20261116-204900-12"),
            Some(("20261116-204900".to_string(), 12))
        );
        for bad in [
            "",
            "20261116",
            "extra-20261116-204900",
            "20261116-204900-",
            "20261116-204900-x",
            "20261116-204900-1-2",
            "2026111-204900",
        ] {
            assert_eq!(parse_stamp(bad), None, "{bad}");
        }
    }
}
