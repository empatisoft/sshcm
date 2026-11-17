//! End-to-end tests of the subcommands with `--yes`, each in its own
//! temporary `HOME`. Interactive prompts are not tested.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

struct Sandbox {
    dir: TempDir,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir(dir.path().join(".ssh")).expect("create .ssh");
        fs::write(dir.path().join(".ssh/id_ed25519"), "").expect("key");
        fs::write(dir.path().join(".ssh/id_ed25519.pub"), "").expect("public key");
        Sandbox { dir }
    }

    fn home(&self) -> &Path {
        self.dir.path()
    }

    fn config_path(&self) -> PathBuf {
        self.home().join(".ssh/config")
    }

    fn config(&self) -> String {
        fs::read_to_string(self.config_path()).expect("config file")
    }

    fn write_config(&self, content: &str) {
        fs::write(self.config_path(), content).expect("write config");
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sshcm"));
        cmd.env("HOME", self.home())
            .env("NO_COLOR", "1")
            .env_remove("SSHCM_LANG")
            .env_remove("XDG_CONFIG_HOME")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Run {
        finish(self.command().args(args).output().expect("run sshcm"))
    }

    /// Adds the three connections most tests start from.
    fn seed(&self) {
        for args in [
            &[
                "add",
                "bastion",
                "--hostname",
                "bastion.example.com",
                "--user",
                "ops",
            ][..],
            &[
                "add",
                "web",
                "--hostname",
                "10.0.2.10",
                "--jump",
                "bastion",
                "--tags",
                "prod,web",
            ],
            &[
                "add",
                "staging",
                "--hostname",
                "203.0.113.24",
                "--user",
                "ubuntu",
            ],
        ] {
            let run = self.run(&[args, &["--yes"]].concat());
            assert_eq!(run.code, 0, "{}", run.stderr);
        }
    }
}

fn finish(output: Output) -> Run {
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

const SEEDED: &str = "\
Host bastion
    HostName bastion.example.com
    User ops

# sshcm:tags prod, web
Host web
    HostName 10.0.2.10
    ProxyJump bastion

Host staging
    HostName 203.0.113.24
    User ubuntu
";

#[test]
fn add_creates_the_config_and_reports_what_it_did() {
    let sb = Sandbox::new();
    let run = sb.run(&[
        "add",
        "ihu",
        "--hostname",
        "app.example.edu",
        "--user",
        "deploy",
        "--port",
        "22",
        "--identity-file",
        "~/.ssh/id_ed25519",
        "--desc",
        "Application server",
        "--yes",
    ]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        sb.config(),
        "# sshcm:desc Application server\nHost ihu\n    HostName app.example.edu\n    User deploy\n    IdentityFile ~/.ssh/id_ed25519\n"
    );
    assert!(run.stdout.contains("Will be added to ~/.ssh/config"));
    assert!(run.stdout.contains("  + Host ihu"));
    assert!(
        run.stdout
            .contains("Port 22 is the default, so it is not written.")
    );
    assert!(run.stdout.contains("Saved ihu"));
    let mode = fs::metadata(sb.config_path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn add_edit_remove_round_trip() {
    let sb = Sandbox::new();
    sb.seed();
    assert_eq!(sb.config(), SEEDED);

    let run = sb.run(&[
        "edit",
        "staging",
        "--user",
        "deploy",
        "--port",
        "2222",
        "--socks5",
        "10.0.0.1:1080",
        "--yes",
    ]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("  -     User ubuntu"));
    assert!(run.stdout.contains("  +     User deploy"));
    assert!(run.stdout.contains("backup: ~/.ssh/sshcm-backups/config-"));
    assert_eq!(
        sb.config(),
        SEEDED.replace(
            "    User ubuntu\n",
            "    User deploy\n    Port 2222\n    ProxyCommand nc -X 5 -x 10.0.0.1:1080 %h %p\n"
        )
    );

    // An empty value clears a setting; --no-proxy drops the proxy line.
    let run = sb.run(&["edit", "staging", "--port", "", "--no-proxy", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(sb.config(), SEEDED.replace("User ubuntu", "User deploy"));

    let run = sb.run(&["rm", "staging", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("  - Host staging"));
    assert!(run.stdout.contains("Removed staging"));
    assert_eq!(
        sb.config(),
        SEEDED.replace(
            "\nHost staging\n    HostName 203.0.113.24\n    User ubuntu\n",
            ""
        )
    );
}

#[test]
fn edit_without_changes_writes_nothing() {
    let sb = Sandbox::new();
    sb.seed();
    let backups = || {
        fs::read_dir(sb.home().join(".ssh/sshcm-backups"))
            .unwrap()
            .count()
    };
    let before = backups();
    let run = sb.run(&["edit", "bastion", "--user", "ops", "--yes"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("No changes."));
    assert_eq!(backups(), before);
    assert_eq!(sb.config(), SEEDED);
}

#[test]
fn rename_updates_connections_that_jump_through_it() {
    let sb = Sandbox::new();
    sb.seed();
    let run = sb.run(&["edit", "bastion", "--rename", "gate", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("ProxyJump is also updated in: web"));
    assert_eq!(
        sb.config(),
        SEEDED
            .replace("Host bastion\n", "Host gate\n")
            .replace("ProxyJump bastion\n", "ProxyJump gate\n")
    );
}

#[test]
fn rm_warns_about_dependent_connections() {
    let sb = Sandbox::new();
    sb.seed();
    let run = sb.run(&["rm", "bastion", "--dry-run"]);
    assert_eq!(run.code, 0);
    assert!(
        run.stdout
            .contains("1 connection uses bastion as its jump host: web")
    );
    assert!(
        run.stdout
            .contains("They will not connect until you change their proxy.")
    );
    assert_eq!(sb.config(), SEEDED, "dry run must not write");
}

#[test]
fn dry_run_never_writes() {
    let sb = Sandbox::new();
    let run = sb.run(&[
        "add",
        "a",
        "--hostname",
        "a.example.com",
        "--dry-run",
        "--yes",
    ]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("  + Host a"));
    assert!(run.stdout.contains("Dry run: nothing was written."));
    assert!(!sb.config_path().exists());

    sb.seed();
    let run = sb.run(&["edit", "web", "--user", "root", "--dry-run"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("  +     User root"));
    assert_eq!(sb.config(), SEEDED);
}

#[test]
fn invalid_input_exits_with_1_and_leaves_the_file_alone() {
    let sb = Sandbox::new();
    sb.seed();
    let cases: [(&[&str], &str); 9] = [
        (
            &["add", "web", "--hostname", "x.example.com"],
            "already exists",
        ),
        (
            &["add", "bad alias", "--hostname", "x.example.com"],
            "cannot contain spaces",
        ),
        (
            &["add", "a*", "--hostname", "x.example.com"],
            "cannot contain '*'",
        ),
        (&["add", "new"], "HostName is required"),
        (
            &["add", "new", "--hostname", "x", "--port", "70000"],
            "between 1 and 65535",
        ),
        (
            &[
                "add",
                "new",
                "--hostname",
                "x",
                "--identity-file",
                "~/.ssh/missing",
            ],
            "Identity file not found",
        ),
        (
            &["edit", "bastion", "--jump", "web"],
            "already jumps through",
        ),
        (
            &["edit", "web", "--jump", "web"],
            "cannot be its own jump host",
        ),
        (&["rm", "nope"], "No connection named nope"),
    ];
    for (args, message) in cases {
        let run = sb.run(&[args, &["--yes"]].concat());
        assert_eq!(run.code, 1, "{args:?}: {}", run.stdout);
        assert!(run.stderr.contains(message), "{args:?}: {}", run.stderr);
        assert!(run.stderr.starts_with("error: "), "{}", run.stderr);
        assert_eq!(sb.config(), SEEDED, "{args:?}");
    }
}

#[test]
fn included_files_are_edited_in_place_and_patterns_are_read_only() {
    let sb = Sandbox::new();
    fs::create_dir(sb.home().join(".ssh/config.d")).unwrap();
    let work = sb.home().join(".ssh/config.d/work");
    fs::write(&work, "Host legacy-vpn\n    HostName vpn.example.com\n").unwrap();
    let config =
        "Include config.d/*\n\nHost a\n    HostName a.example.com\n\nHost *\n    User me\n";
    sb.write_config(config);

    let run = sb.run(&["list"]);
    assert_eq!(run.code, 0);
    assert_eq!(
        run.stdout,
        "ALIAS       HOSTNAME         USER  PORT  PROXY  FILE\n\
         a           a.example.com          22\n\
         legacy-vpn  vpn.example.com        22           ~/.ssh/config.d/work\n\
         \n\
         Read-only\n\
         *  pattern\n"
    );
    assert!(
        sb.run(&["show", "legacy-vpn"])
            .stdout
            .contains("Included from  ~/.ssh/config.d/work\n")
    );

    // Patterns cannot be changed; names from included files are taken.
    let run = sb.run(&["rm", "*", "--yes"]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("read-only"), "{}", run.stderr);
    assert!(
        sb.run(&["add", "legacy-vpn", "--hostname", "x", "--yes"])
            .stderr
            .contains("already exists")
    );

    // Editing changes the included file and leaves the main config alone.
    let run = sb.run(&["edit", "legacy-vpn", "--user", "deploy", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("  +     User deploy"));
    assert!(
        run.stdout
            .contains("backup: ~/.ssh/sshcm-backups/config.d__work-"),
        "{}",
        run.stdout
    );
    assert_eq!(
        fs::read_to_string(&work).unwrap(),
        "Host legacy-vpn\n    HostName vpn.example.com\n    User deploy\n"
    );
    assert_eq!(sb.config(), config);

    // A new connection lands in the main config, before the trailing `Host *`.
    let run = sb.run(&["add", "b", "--hostname", "b.example.com", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let with_b = "Include config.d/*\n\nHost a\n    HostName a.example.com\n\nHost b\n    HostName b.example.com\n\nHost *\n    User me\n";
    assert_eq!(sb.config(), with_b);

    // Removing says which file the block comes from.
    let run = sb.run(&["rm", "legacy-vpn", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stdout
            .contains("Will be removed from ~/.ssh/config.d/work")
    );
    assert_eq!(fs::read_to_string(&work).unwrap(), "");
    assert_eq!(sb.config(), with_b);
}

#[test]
fn hand_written_blocks_can_be_edited() {
    let sb = Sandbox::new();
    // No HostName (ssh uses the alias), and values outside sshcm's own rules.
    let config = "Host github.com\n    User git\n\nHost odd=name\n    HostName host/with/slash\n    User -weird\n";
    sb.write_config(config);

    let run = sb.run(&["edit", "github.com", "--port", "443", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let run = sb.run(&["edit", "odd=name", "--port", "2222", "--yes"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        sb.config(),
        "Host github.com\n    User git\n    Port 443\n\nHost odd=name\n    HostName host/with/slash\n    User -weird\n    Port 2222\n"
    );

    // Changing such a value is checked; a new connection needs a HostName.
    let run = sb.run(&["edit", "odd=name", "--user=-other", "--yes"]);
    assert_eq!(run.code, 1);
    assert!(
        run.stderr.contains("User cannot contain spaces"),
        "{}",
        run.stderr
    );
    let run = sb.run(&["add", "new", "--yes"]);
    assert!(run.stderr.contains("HostName is required"));
}

#[test]
fn missing_nc_is_reported_for_socks_and_http_proxies() {
    let sb = Sandbox::new();
    let empty = sb.home().join("empty-path");
    fs::create_dir(&empty).unwrap();
    let args = [
        "add",
        "a",
        "--hostname",
        "a.example.com",
        "--socks5",
        "10.0.0.1:1080",
        "--yes",
    ];
    let run = finish(
        sb.command()
            .env("PATH", &empty)
            .args(args)
            .output()
            .unwrap(),
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stdout.contains("The nc command was not found."),
        "{}",
        run.stdout
    );
    // Without ssh on the PATH the config check is skipped, and said so.
    assert!(
        run.stdout.contains("the config was not checked"),
        "{}",
        run.stdout
    );
    assert!(
        sb.config()
            .contains("ProxyCommand nc -X 5 -x 10.0.0.1:1080 %h %p")
    );
}

#[test]
fn list_and_show() {
    let sb = Sandbox::new();
    sb.seed();
    let run = sb.run(&["list"]);
    assert_eq!(
        run.stdout,
        "ALIAS    HOSTNAME             USER    PORT  PROXY        TAGS\n\
         bastion  bastion.example.com  ops     22\n\
         web      10.0.2.10                    22    via bastion  prod, web\n\
         staging  203.0.113.24         ubuntu  22\n"
    );

    let run = sb.run(&["list", "--tag", "prod"]);
    assert!(run.stdout.contains("web ") && !run.stdout.contains("staging"));
    let run = sb.run(&["list", "--tag", "nope"]);
    assert_eq!(run.stdout, "No connections with the tag nope.\n");

    let run = sb.run(&["show", "web"]);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("HostName       10.0.2.10\n"));
    assert!(run.stdout.contains("Port           22 (default)\n"));
    assert!(run.stdout.contains("Proxy          Jump host → bastion\n"));
    assert!(run.stdout.contains("Tags           prod, web\n"));
    assert!(
        run.stdout
            .contains("Config block\n  # sshcm:tags prod, web\n  Host web\n")
    );

    assert_eq!(sb.run(&["show", "nope"]).code, 1);
}

#[test]
fn config_flag_selects_another_file() {
    let sb = Sandbox::new();
    let other = sb.home().join("other_config");
    let other_str = other.to_str().unwrap();
    let run = sb.run(&[
        "--config",
        other_str,
        "add",
        "a",
        "--hostname",
        "a.example.com",
        "--yes",
    ]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        fs::read_to_string(&other).unwrap(),
        "Host a\n    HostName a.example.com\n"
    );
    assert!(!sb.config_path().exists());
    // The flag also works after the subcommand.
    let run = sb.run(&["list", "--config", other_str]);
    assert!(run.stdout.contains("a.example.com"));
}

#[test]
fn prompts_without_a_terminal_fail_cleanly() {
    let sb = Sandbox::new();
    sb.seed();
    for args in [
        &["add"][..],
        &["edit", "web"],
        &["rm", "web"],
        &["show"],
        &[],
    ] {
        let run = sb.run(args);
        assert_eq!(run.code, 1, "{args:?}");
        assert!(
            run.stderr.contains("interactive terminal"),
            "{args:?}: {}",
            run.stderr
        );
    }
    assert_eq!(sb.config(), SEEDED);
}

#[test]
fn settings_and_language() {
    let sb = Sandbox::new();
    assert_eq!(sb.run(&["config", "get", "language"]).stdout, "en\n");
    assert_eq!(sb.run(&["config", "get", "backup_keep"]).stdout, "10\n");
    assert_eq!(sb.run(&["config", "set", "backup_keep", "0"]).code, 1);
    assert_eq!(sb.run(&["config", "set", "language", "de"]).code, 1);
    assert_eq!(sb.run(&["config", "get", "nope"]).code, 1);

    assert_eq!(
        sb.run(&["list"]).stdout,
        "There are no connections yet. Add one with: sshcm add\n"
    );
    assert_eq!(sb.run(&["config", "set", "language", "tr"]).code, 0);
    assert_eq!(
        fs::read_to_string(sb.home().join(".config/sshcm/config.toml")).unwrap(),
        "language = \"tr\"\nbackup_keep = 10\n"
    );
    assert_eq!(
        sb.run(&["list"]).stdout,
        "Henüz bağlantı yok. Eklemek için: sshcm add\n"
    );

    // Order: --lang, then SSHCM_LANG, then the settings file.
    let env_en = finish(
        sb.command()
            .env("SSHCM_LANG", "en")
            .arg("list")
            .output()
            .unwrap(),
    );
    assert!(env_en.stdout.starts_with("There are no connections"));
    let flag_tr = finish(
        sb.command()
            .env("SSHCM_LANG", "en")
            .args(["list", "--lang", "tr"])
            .output()
            .unwrap(),
    );
    assert!(flag_tr.stdout.starts_with("Henüz bağlantı yok"));

    // Errors and help follow the language; command names do not.
    let run = sb.run(&["rm", "yok", "--yes"]);
    assert_eq!(run.stderr, "hata: yok adında bir bağlantı yok.\n");
    assert!(sb.run(&["--help"]).stdout.contains("Yeni bağlantı oluştur"));
    assert!(
        sb.run(&["--help", "--lang", "en"])
            .stdout
            .contains("Create a new connection")
    );
}

#[test]
fn backup_keep_limits_the_number_of_backups() {
    let sb = Sandbox::new();
    assert_eq!(sb.run(&["config", "set", "backup_keep", "2"]).code, 0);
    for i in 0..5 {
        let alias = format!("h{i}");
        let run = sb.run(&["add", &alias, "--hostname", "h.example.com", "--yes"]);
        assert_eq!(run.code, 0, "{}", run.stderr);
    }
    let count = fs::read_dir(sb.home().join(".ssh/sshcm-backups"))
        .unwrap()
        .count();
    assert_eq!(count, 2);
}

/// `connect` replaces the process with ssh: arguments are passed through as
/// a list and the exit code is ssh's.
#[test]
fn connect_hands_over_to_ssh() {
    let sb = Sandbox::new();
    sb.write_config(SEEDED);
    let bin = sb.home().join("bin");
    fs::create_dir(&bin).unwrap();
    let fake_ssh = bin.join("ssh");
    fs::write(
        &fake_ssh,
        "#!/bin/sh\nfor arg in \"$@\"; do echo \"arg=$arg\"; done\nexit 7\n",
    )
    .unwrap();
    fs::set_permissions(&fake_ssh, fs::Permissions::from_mode(0o755)).unwrap();

    let run = finish(
        sb.command()
            .env("PATH", &bin)
            .args([
                "connect",
                "web",
                "--",
                "-L",
                "8080:localhost:80",
                "echo it's; here",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(run.code, 7, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "arg=web\narg=-L\narg=8080:localhost:80\narg=echo it's; here\n"
    );

    // With --config, ssh is told to read the same file.
    let config = sb.config_path();
    let run = finish(
        sb.command()
            .env("PATH", &bin)
            .args(["--config", config.to_str().unwrap(), "connect", "web"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        run.stdout,
        format!("arg=-F\narg={}\narg=web\n", config.display())
    );

    let run = finish(
        sb.command()
            .env("PATH", &bin)
            .args(["connect", "nope"])
            .output()
            .unwrap(),
    );
    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("No connection named nope"));
}

#[test]
fn non_utf8_config_is_refused() {
    let sb = Sandbox::new();
    fs::write(sb.config_path(), [0xff, 0xfe, b'\n']).unwrap();
    let run = sb.run(&["list"]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("not valid UTF-8"));
}
