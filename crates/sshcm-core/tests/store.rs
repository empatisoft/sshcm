//! File safety: atomic writes, backups, permissions, symlinks, concurrent
//! changes and the ssh check. Everything runs in a temporary directory.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use sshcm_core::{Connection, Error, Proxy, SaveOptions, Store, Verification};
use tempfile::TempDir;

const BASIC: &str = include_str!("fixtures/basic.config");

struct Home {
    dir: TempDir,
}

impl Home {
    fn new() -> Home {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir(dir.path().join(".ssh")).expect("create .ssh");
        Home { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn config(&self) -> PathBuf {
        self.path().join(".ssh/config")
    }

    fn write_config(&self, content: &str) {
        fs::write(self.config(), content).expect("write config");
    }

    /// A store whose ssh check always passes, to keep tests independent of
    /// the ssh installed on the machine.
    fn store(&self) -> Store {
        Store::load(self.config(), self.path())
            .expect("load")
            .with_ssh_program("true")
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn conn(alias: &str, hostname: &str) -> Connection {
    Connection {
        alias: alias.to_string(),
        hostname: Some(hostname.to_string()),
        ..Connection::default()
    }
}

fn opts() -> SaveOptions {
    SaveOptions::default()
}

#[test]
fn first_save_creates_the_file_with_mode_600() {
    let dir = TempDir::new().unwrap();
    let home = dir.path();
    let config = home.join(".ssh/config");
    let mut store = Store::load(&config, home).unwrap().with_ssh_program("true");
    assert!(store.is_new());
    store.doc.add(&conn("a", "a.example.com"));
    let report = store.save(&opts()).unwrap();

    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "Host a\n    HostName a.example.com\n"
    );
    assert_eq!(mode(&config), 0o600);
    assert_eq!(mode(&home.join(".ssh")), 0o700);
    assert!(
        report.backups.is_empty(),
        "nothing to back up on first write"
    );
    assert!(!store.is_new());
}

#[test]
fn save_fixes_loose_permissions() {
    let home = Home::new();
    home.write_config(BASIC);
    fs::set_permissions(home.config(), fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(home.path().join(".ssh"), fs::Permissions::from_mode(0o755)).unwrap();

    let mut store = home.store();
    store.doc.add(&conn("a", "a.example.com"));
    store.save(&opts()).unwrap();

    assert_eq!(mode(&home.config()), 0o600);
    assert_eq!(mode(&home.path().join(".ssh")), 0o700);
}

#[test]
fn save_does_not_chmod_directories_other_than_dot_ssh() {
    let dir = TempDir::new().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o755)).unwrap();
    let config = elsewhere.join("my_config");
    fs::write(&config, BASIC).unwrap();

    let mut store = Store::load(&config, dir.path())
        .unwrap()
        .with_ssh_program("true");
    store.doc.add(&conn("a", "a.example.com"));
    store.save(&opts()).unwrap();
    assert_eq!(mode(&elsewhere), 0o755);
    assert!(elsewhere.join("sshcm-backups").is_dir());
}

#[test]
fn every_write_leaves_a_backup_of_the_previous_content() {
    let home = Home::new();
    home.write_config(BASIC);
    let mut store = home.store();
    store.doc.remove("dev").unwrap();
    let report = store.save(&opts()).unwrap();

    let backup = report.backups.first().cloned().expect("backup path");
    assert_eq!(fs::read_to_string(&backup).unwrap(), BASIC);
    assert_eq!(mode(&backup), 0o600);
    assert_eq!(
        backup.parent().unwrap(),
        home.path().join(".ssh/sshcm-backups")
    );
    assert_eq!(mode(backup.parent().unwrap()), 0o700);

    // config-YYYYMMDD-HHMMSS
    let name = backup.file_name().unwrap().to_string_lossy().into_owned();
    let stamp = name.strip_prefix("config-").expect("prefix");
    assert_eq!(stamp.len(), 15, "{name}");
    assert!(
        stamp
            .chars()
            .enumerate()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() }),
        "{name}"
    );
    assert_ne!(fs::read_to_string(home.config()).unwrap(), BASIC);
}

#[test]
fn only_the_newest_backups_are_kept() {
    let home = Home::new();
    home.write_config("Host seed\n    HostName seed.example.com\n");
    let mut store = home.store();
    let keep = SaveOptions {
        backup_keep: 3,
        verify_alias: None,
    };
    for i in 0..6 {
        store.doc.add(&conn(&format!("h{i}"), "h.example.com"));
        store.save(&keep).unwrap();
    }
    let backups = store.backups().unwrap();
    assert_eq!(backups.len(), 3);
    // The newest backup holds the state before the last write: h0..h4.
    let newest = fs::read_to_string(backups.last().unwrap()).unwrap();
    assert!(newest.contains("Host h4\n") && !newest.contains("Host h5\n"));
    let oldest = fs::read_to_string(&backups[0]).unwrap();
    assert!(oldest.contains("Host h2\n") && !oldest.contains("Host h3\n"));
}

#[test]
fn symlinked_config_is_updated_through_the_link() {
    let home = Home::new();
    let dotfiles = home.path().join("dotfiles");
    fs::create_dir(&dotfiles).unwrap();
    let real = dotfiles.join("ssh_config");
    fs::write(&real, BASIC).unwrap();
    symlink(&real, home.config()).unwrap();

    let mut store = home.store();
    store.doc.add(&conn("a", "a.example.com"));
    store.save(&opts()).unwrap();

    let link = fs::symlink_metadata(home.config()).unwrap();
    assert!(link.file_type().is_symlink(), "the link must survive");
    assert!(fs::read_to_string(&real).unwrap().contains("Host a\n"));
    assert!(home.path().join(".ssh/sshcm-backups").is_dir());
}

#[test]
fn a_change_by_another_program_cancels_the_write() {
    let home = Home::new();
    home.write_config(BASIC);
    let mut store = home.store();
    store.doc.add(&conn("a", "a.example.com"));

    let theirs = format!("{BASIC}\nHost theirs\n    HostName t.example.com\n");
    home.write_config(&theirs);

    assert!(matches!(store.save(&opts()), Err(Error::ChangedOnDisk(_))));
    assert_eq!(fs::read_to_string(home.config()).unwrap(), theirs);
    assert!(store.backups().unwrap().is_empty());
}

#[test]
fn a_file_created_by_another_program_cancels_the_first_write() {
    let home = Home::new();
    let mut store = home.store();
    store.doc.add(&conn("a", "a.example.com"));
    home.write_config("Host theirs\n");
    assert!(matches!(store.save(&opts()), Err(Error::ChangedOnDisk(_))));
    assert_eq!(fs::read_to_string(home.config()).unwrap(), "Host theirs\n");
}

#[test]
fn failed_ssh_check_restores_the_previous_content() {
    let home = Home::new();
    home.write_config(BASIC);
    let mut store = Store::load(home.config(), home.path())
        .unwrap()
        .with_ssh_program("false");
    store.doc.add(&conn("a", "a.example.com"));

    assert!(matches!(
        store.save(&opts()),
        Err(Error::VerifyFailed { .. })
    ));
    assert_eq!(fs::read_to_string(home.config()).unwrap(), BASIC);
    assert_eq!(mode(&home.config()), 0o600);
}

#[test]
fn failed_ssh_check_removes_a_file_that_did_not_exist_before() {
    let home = Home::new();
    let mut store = Store::load(home.config(), home.path())
        .unwrap()
        .with_ssh_program("false");
    store.doc.add(&conn("a", "a.example.com"));
    assert!(matches!(
        store.save(&opts()),
        Err(Error::VerifyFailed { .. })
    ));
    assert!(!home.config().exists());
}

#[test]
fn missing_ssh_command_skips_the_check() {
    let home = Home::new();
    let mut store = Store::load(home.config(), home.path())
        .unwrap()
        .with_ssh_program("/nonexistent/sshcm-test-ssh");
    store.doc.add(&conn("a", "a.example.com"));
    let report = store.save(&opts()).unwrap();
    assert_eq!(report.verification, Verification::Skipped);
    assert!(home.config().exists());
}

#[test]
fn non_utf8_config_is_refused() {
    let home = Home::new();
    fs::write(home.config(), [b'H', b'o', 0xff, 0xfe, b'\n']).unwrap();
    assert!(matches!(
        Store::load(home.config(), home.path()),
        Err(Error::NotUtf8(_))
    ));
}

/// A home whose config includes `config.d/*` and `config-extra`.
fn home_with_includes() -> Home {
    let home = Home::new();
    home.write_config(
        "Include config.d/* config-extra\n\nHost bastion\n    HostName bastion.example.com\n\nHost *\n    User me\n",
    );
    fs::create_dir(home.path().join(".ssh/config.d")).unwrap();
    fs::write(home.path().join(".ssh/config.d/work"), WORK).unwrap();
    fs::write(home.path().join(".ssh/config-extra"), EXTRA).unwrap();
    home
}

const WORK: &str = "\
# Work machines
Host legacy-vpn
\tHostName vpn.example.com
\tUser onur

Host db
\tHostName db.internal
\tProxyJump bastion
";

const EXTRA: &str = "Host extra\n    HostName extra.example.com\n";

fn backup_names(home: &Home) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(home.path().join(".ssh/sshcm-backups"))
        .map(|dir| {
            dir.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn included_files_are_listed_with_their_source() {
    let home = Home::new();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::copy(fixtures.join("include-match.config"), home.config()).unwrap();
    fs::create_dir(home.path().join(".ssh/config.d")).unwrap();
    fs::copy(
        fixtures.join("config.d/work.config"),
        home.path().join(".ssh/config.d/work.config"),
    )
    .unwrap();

    let store = home.store();
    let entries = store.entries();
    let names: Vec<&str> = entries.iter().map(|e| e.alias()).collect();
    assert_eq!(
        names,
        [
            "*.corp.example.com",
            "bastion",
            "build?",
            "app",
            "*",
            "legacy-vpn"
        ]
    );

    let vpn = store.find("legacy-vpn").unwrap();
    assert!(!vpn.read_only());
    assert_eq!(
        vpn.source.as_deref(),
        Some(home.path().join(".ssh/config.d/work.config").as_path())
    );

    // Patterns stay read-only; blocks of included files are editable.
    let editable: Vec<&str> = entries
        .iter()
        .filter(|e| !e.read_only())
        .map(|e| e.alias())
        .collect();
    assert_eq!(editable, ["bastion", "app", "legacy-vpn"]);

    // The second name on a shared Host line also finds the entry.
    assert_eq!(
        store.find("bastion.corp.example.com").unwrap().alias(),
        "bastion"
    );
    // Names in included files count as taken.
    assert!(store.known(None).names.iter().any(|n| n == "legacy-vpn"));
}

#[test]
fn editing_an_included_entry_rewrites_only_its_own_file() {
    let home = home_with_includes();
    let main_before = fs::read_to_string(home.config()).unwrap();
    let work = home.path().join(".ssh/config.d/work");

    let mut store = home.store();
    let mut vpn = store.find("legacy-vpn").unwrap().conn;
    vpn.user = Some("deploy".to_string());
    vpn.port = Some(2222);
    store.update("legacy-vpn", &vpn).unwrap();
    assert!(store.is_modified());
    let report = store.save(&opts()).unwrap();
    assert!(!store.is_modified());

    // The block keeps its tab indentation; the rest of the file is intact.
    assert_eq!(
        fs::read_to_string(&work).unwrap(),
        WORK.replace("\tUser onur\n", "\tUser deploy\n\tPort 2222\n")
    );
    assert_eq!(mode(&work), 0o600);
    assert_eq!(fs::read_to_string(home.config()).unwrap(), main_before);
    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config-extra")).unwrap(),
        EXTRA
    );

    // One backup, of the included file, kept next to the main config.
    assert_eq!(report.backups.len(), 1);
    let backup = &report.backups[0];
    assert_eq!(fs::read_to_string(backup).unwrap(), WORK);
    assert_eq!(mode(backup), 0o600);
    let names = backup_names(&home);
    assert_eq!(names.len(), 1);
    assert!(names[0].starts_with("config.d__work-"), "{names:?}");
    // No backup directory appears inside the included directory, where the
    // Include pattern would pick it up.
    assert!(!home.path().join(".ssh/config.d/sshcm-backups").exists());
}

#[test]
fn removing_an_included_entry_rewrites_only_its_own_file() {
    let home = home_with_includes();
    let main_before = fs::read_to_string(home.config()).unwrap();
    let mut store = home.store();
    store.remove("legacy-vpn").unwrap();
    store.save(&opts()).unwrap();

    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config.d/work")).unwrap(),
        "Host db\n\tHostName db.internal\n\tProxyJump bastion\n"
    );
    assert_eq!(fs::read_to_string(home.config()).unwrap(), main_before);
    assert!(home.store().find("legacy-vpn").is_none());
}

#[test]
fn new_connections_always_go_to_the_main_config() {
    let home = home_with_includes();
    let mut store = home.store();
    store.add(&conn("new", "new.example.com"));
    store.save(&opts()).unwrap();

    assert!(fs::read_to_string(home.config()).unwrap().contains(
        "Host bastion\n    HostName bastion.example.com\n\nHost new\n    HostName new.example.com\n\nHost *\n"
    ));
    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config.d/work")).unwrap(),
        WORK
    );
    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config-extra")).unwrap(),
        EXTRA
    );
}

#[test]
fn renaming_updates_jump_references_across_files() {
    let home = home_with_includes();
    let mut store = home.store();
    assert_eq!(store.dependents("bastion"), ["db"]);
    let mut bastion = store.find("bastion").unwrap().conn;
    bastion.alias = "gate".to_string();
    store.update("bastion", &bastion).unwrap();
    let report = store.save(&opts()).unwrap();

    assert!(
        fs::read_to_string(home.config())
            .unwrap()
            .contains("Host gate\n")
    );
    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config.d/work")).unwrap(),
        WORK.replace("ProxyJump bastion", "ProxyJump gate")
    );
    // Both files were rewritten, so both were backed up.
    assert_eq!(report.backups.len(), 2);
    let names = backup_names(&home);
    assert!(names.iter().any(|n| n.starts_with("config-2")), "{names:?}");
    assert!(
        names.iter().any(|n| n.starts_with("config.d__work-")),
        "{names:?}"
    );
}

#[test]
fn a_changed_included_file_cancels_the_whole_write() {
    let home = home_with_includes();
    let main_before = fs::read_to_string(home.config()).unwrap();
    let work = home.path().join(".ssh/config.d/work");

    // The rename touches both files; only the included one changes on disk.
    let mut store = home.store();
    let mut bastion = store.find("bastion").unwrap().conn;
    bastion.alias = "gate".to_string();
    store.update("bastion", &bastion).unwrap();
    let theirs = format!("{WORK}\nHost theirs\n\tHostName t.example.com\n");
    fs::write(&work, &theirs).unwrap();

    match store.save(&opts()) {
        Err(Error::ChangedOnDisk(path)) => assert_eq!(path, work),
        other => panic!("expected ChangedOnDisk, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(home.config()).unwrap(), main_before);
    assert_eq!(fs::read_to_string(&work).unwrap(), theirs);
    assert!(backup_names(&home).is_empty());
}

#[test]
fn failed_ssh_check_restores_every_written_file() {
    let home = home_with_includes();
    let main_before = fs::read_to_string(home.config()).unwrap();
    let mut store = Store::load(home.config(), home.path())
        .unwrap()
        .with_ssh_program("false");
    let mut bastion = store.find("bastion").unwrap().conn;
    bastion.alias = "gate".to_string();
    store.update("bastion", &bastion).unwrap();

    assert!(matches!(
        store.save(&opts()),
        Err(Error::VerifyFailed { .. })
    ));
    assert_eq!(fs::read_to_string(home.config()).unwrap(), main_before);
    assert_eq!(
        fs::read_to_string(home.path().join(".ssh/config.d/work")).unwrap(),
        WORK
    );
}

#[test]
fn backups_of_files_with_similar_names_are_pruned_separately() {
    // `config-extra-<time>` must not be counted as a backup of `config`.
    let home = home_with_includes();
    let keep = SaveOptions {
        backup_keep: 2,
        verify_alias: None,
    };
    let mut store = home.store();
    for i in 0..4 {
        let mut extra = store.find("extra").unwrap().conn;
        extra.user = Some(format!("u{i}"));
        store.update("extra", &extra).unwrap();
        store.save(&keep).unwrap();
        store.add(&conn(&format!("h{i}"), "h.example.com"));
        store.save(&keep).unwrap();
    }
    let names = backup_names(&home);
    let of = |prefix: &str| {
        names
            .iter()
            .filter(|n| {
                n.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with("20"))
            })
            .count()
    };
    assert_eq!(of("config-"), 2, "{names:?}");
    assert_eq!(of("config-extra-"), 2, "{names:?}");
    assert_eq!(store.backups().unwrap().len(), 2);
}

#[test]
fn patterns_in_included_files_stay_read_only() {
    let home = Home::new();
    home.write_config("Include extra\n");
    fs::write(home.path().join(".ssh/extra"), "Host *.corp\n    User me\n").unwrap();
    let mut store = home.store();
    assert!(store.find("*.corp").unwrap().read_only());
    assert!(matches!(
        store.remove("*.corp"),
        Err(sshcm_core::DocError::ReadOnly(_))
    ));
    assert!(matches!(
        store.remove("missing"),
        Err(sshcm_core::DocError::NotFound(_))
    ));
}

#[test]
fn include_loops_do_not_hang() {
    let home = Home::new();
    home.write_config("Include config\nInclude loop.config\nHost a\n    HostName a\n");
    fs::write(
        home.path().join(".ssh/loop.config"),
        "Include loop.config\nHost b\n",
    )
    .unwrap();
    let names: Vec<String> = home
        .store()
        .entries()
        .iter()
        .map(|e| e.alias().to_string())
        .collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn crud_on_a_real_file_changes_only_the_target_block() {
    let home = Home::new();
    home.write_config(BASIC);

    // Add
    let mut store = home.store();
    let mut ihu = conn("ihu", "app.example.edu");
    ihu.user = Some("deploy".to_string());
    ihu.proxy = Proxy::Jump(vec!["bastion".to_string()]);
    store.doc.add(&ihu);
    store.save(&opts()).unwrap();
    let added =
        "Host ihu\n    HostName app.example.edu\n    User deploy\n    ProxyJump bastion\n\n";
    let after_add = BASIC.replace("Host *\n", &format!("{added}Host *\n"));
    assert_eq!(fs::read_to_string(home.config()).unwrap(), after_add);

    // Edit
    let mut store = home.store();
    let mut dev = store.find("dev").unwrap().conn;
    dev.port = Some(22);
    dev.user = Some("root".to_string());
    store.doc.update("dev", &dev).unwrap();
    store.save(&opts()).unwrap();
    let after_edit = after_add.replace(
        "    User deploy\n    Port 2222\n\n# sshcm:desc",
        "    User root\n\n# sshcm:desc",
    );
    assert_ne!(after_edit, after_add);
    assert_eq!(fs::read_to_string(home.config()).unwrap(), after_edit);

    // Remove
    let mut store = home.store();
    assert_eq!(store.dependents("bastion"), ["db-prod", "web-prod", "ihu"]);
    store.doc.remove("ihu").unwrap();
    store.save(&opts()).unwrap();
    assert_eq!(
        fs::read_to_string(home.config()).unwrap(),
        after_edit.replace(added, "")
    );
}

/// The file sshcm writes must be accepted by the real ssh. Skipped only
/// where no ssh client is installed.
#[test]
fn written_config_is_accepted_by_ssh() {
    if Command::new("ssh").arg("-V").output().is_err() {
        eprintln!("ssh not found; skipping");
        return;
    }
    let home = Home::new();
    home.write_config(BASIC);
    let mut store = Store::load(home.config(), home.path()).unwrap();

    let mut c = conn("ihu", "app.example.edu");
    c.user = Some("deploy".to_string());
    c.port = Some(2200);
    c.proxy = Proxy::Http("proxy.example.com:3128".to_string());
    c.description = Some("Application server".to_string());
    c.tags = vec!["prod".to_string()];
    store.doc.add(&c);
    let report = store
        .save(&SaveOptions {
            backup_keep: 10,
            verify_alias: Some("ihu".to_string()),
        })
        .unwrap();
    assert_eq!(report.verification, Verification::Passed);

    let out = Command::new("ssh")
        .arg("-F")
        .arg(home.config())
        .args(["-G", "ihu"])
        .output()
        .unwrap();
    let settings = String::from_utf8_lossy(&out.stdout).to_lowercase();
    assert!(settings.contains("hostname app.example.edu"));
    assert!(settings.contains("user deploy"));
    assert!(settings.contains("port 2200"));
    assert!(settings.contains("proxycommand nc -x connect -x proxy.example.com:3128 %h %p"));
}

/// A config ssh cannot parse is restored after the attempt to write it.
#[test]
fn ssh_rejecting_the_file_restores_it() {
    if Command::new("ssh").arg("-V").output().is_err() {
        eprintln!("ssh not found; skipping");
        return;
    }
    let home = Home::new();
    let broken = "Host a\n    HostName a.example.com\n    NotARealSshOption yes\n";
    home.write_config(broken);
    let mut store = Store::load(home.config(), home.path()).unwrap();
    store.doc.add(&conn("b", "b.example.com"));
    match store.save(&opts()) {
        Err(Error::VerifyFailed { stderr }) => assert!(!stderr.is_empty()),
        other => panic!("expected the ssh check to fail, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(home.config()).unwrap(), broken);
}
