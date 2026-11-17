//! Tests of the interface state and of what gets drawn. The terminal is
//! ratatui's in-memory test backend; the config lives in a temporary HOME.

use std::fs;
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use super::app::{App, Effect, Screen, Tone};
use super::form::Field;
use super::ui;
use crate::commands::Ctx;
use crate::settings::Settings;

const CONFIG: &str = "\
# Jump host for the production network
Host bastion
    HostName bastion.example.com
    User ops
    IdentityFile ~/.ssh/id_ed25519

Host db-prod
    HostName 10.0.2.15
    User postgres
    ProxyJump bastion

# sshcm:tags prod, web
Host web-prod
    HostName 10.0.2.10
    User deploy
    ProxyJump bastion
    ServerAliveInterval 60

Host staging
    HostName 203.0.113.24
    User ubuntu
    Port 2222

Host *
    AddKeysToAgent yes
";

struct Fixture {
    dir: TempDir,
    app: App,
}

impl Fixture {
    fn new(config: &str) -> Fixture {
        Fixture::with(config, false)
    }

    fn dry_run(config: &str) -> Fixture {
        Fixture::with(config, true)
    }

    fn with(config: &str, dry_run: bool) -> Fixture {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir(dir.path().join(".ssh")).expect("create .ssh");
        fs::write(dir.path().join(".ssh/id_ed25519"), "").expect("key");
        fs::write(dir.path().join(".ssh/id_ed25519.pub"), "").expect("public key");
        if !config.is_empty() {
            fs::write(dir.path().join(".ssh/config"), config).expect("config");
        }
        let ctx = Ctx {
            home: dir.path().to_path_buf(),
            config: None,
            dry_run,
            yes: false,
            settings: Settings::default(),
        };
        let app = App::new(ctx).expect("app");
        Fixture { dir, app }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.path().join(".ssh/config")
    }

    fn config(&self) -> String {
        fs::read_to_string(self.config_path()).expect("config")
    }

    fn press(&mut self, code: KeyCode) -> Effect {
        self.app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(&mut self, c: char) -> Effect {
        self.app
            .on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn chars(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    /// Clears the focused text field.
    fn clear(&mut self) {
        self.ctrl('e');
        self.ctrl('u');
    }

    fn selected(&self) -> String {
        self.app
            .current()
            .map(|e| e.alias().to_string())
            .unwrap_or_default()
    }

    /// Draws the interface and returns the screen as text, one line per row.
    fn screen(&self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| ui::draw(frame, &self.app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

#[test]
#[ignore = "prints the screens for a visual check: cargo test -p sshcm dump -- --ignored --nocapture"]
fn dump_screens() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Down);
    f.press(KeyCode::Down);
    println!("{}", f.screen(110, 24));
    println!("{}", f.screen(70, 14));
    f.press(KeyCode::Char('?'));
    println!("{}", f.screen(110, 30));
    f.press(KeyCode::Esc);
    f.press(KeyCode::Char('e'));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.chars("70000");
    println!("{}", f.screen(110, 24));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    println!("{}", f.screen(110, 24));
    f.press(KeyCode::Esc);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    println!("{}", f.screen(110, 24));
    f.press(KeyCode::Tab);
    println!("{}", f.screen(110, 24));
    f.press(KeyCode::Esc);
    f.press(KeyCode::Esc);
    f.press(KeyCode::Home);
    f.press(KeyCode::Char('d'));
    println!("{}", f.screen(110, 24));
}

// ------------------------------------------------------------ main screen --

#[test]
fn list_shows_connections_first_and_patterns_last() {
    let f = Fixture::new(CONFIG);
    let names: Vec<&str> = f
        .app
        .visible
        .iter()
        .map(|&i| f.app.entries[i].alias())
        .collect();
    assert_eq!(names, ["bastion", "db-prod", "web-prod", "staging", "*"]);
    assert_eq!(f.app.connection_count(), 4);
    assert_eq!(f.selected(), "bastion");
}

#[test]
fn selection_moves_and_stops_at_the_ends() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Up);
    assert_eq!(f.selected(), "bastion");
    f.press(KeyCode::Down);
    f.press(KeyCode::Char('j'));
    assert_eq!(f.selected(), "web-prod");
    f.press(KeyCode::Char('k'));
    assert_eq!(f.selected(), "db-prod");
    f.press(KeyCode::End);
    f.press(KeyCode::Down);
    assert_eq!(f.selected(), "*");
    f.press(KeyCode::Home);
    assert_eq!(f.selected(), "bastion");
}

#[test]
fn enter_connects_and_quit_keys_leave() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Down);
    assert_eq!(
        f.press(KeyCode::Enter),
        Effect::Connect("db-prod".to_string())
    );
    assert_eq!(f.press(KeyCode::Char('q')), Effect::Quit);
    assert_eq!(f.ctrl('c'), Effect::Interrupt);
}

#[test]
fn patterns_are_read_only() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    for key in [KeyCode::Enter, KeyCode::Char('e'), KeyCode::Char('d')] {
        assert_eq!(f.press(key), Effect::None);
        assert!(matches!(f.app.screen, Screen::Main));
        let status = f.app.status.clone().expect("status");
        assert_eq!(status.tone, Tone::Warn);
        assert_eq!(status.text, "* is a pattern and is read-only.");
    }
    assert_eq!(f.config(), CONFIG);
}

#[test]
fn search_filters_fuzzily_and_keeps_typed_letters_out_of_shortcuts() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('/'));
    assert!(f.app.searching);
    // `q`, `a`, `d` and `e` are text while searching, not commands.
    f.chars("prod");
    let names: Vec<&str> = f
        .app
        .visible
        .iter()
        .map(|&i| f.app.entries[i].alias())
        .collect();
    // web-prod also carries the tag "prod", so it ranks first.
    assert_eq!(names, ["web-prod", "db-prod"]);
    assert!(matches!(f.app.screen, Screen::Main));

    // Enter keeps the filter and gives the keys back.
    f.press(KeyCode::Enter);
    assert!(!f.app.searching);
    f.press(KeyCode::Down);
    assert_eq!(f.selected(), "db-prod");
    // Esc clears the filter.
    f.press(KeyCode::Esc);
    assert_eq!(f.app.visible.len(), 5);

    // Letters do not have to be adjacent; address and tags are searched too.
    f.press(KeyCode::Char('/'));
    f.chars("wbp");
    assert_eq!(f.selected(), "web-prod");
    f.press(KeyCode::Esc);
    f.press(KeyCode::Char('/'));
    f.chars("203.0");
    assert_eq!(f.selected(), "staging");
    f.press(KeyCode::Esc);
    assert!(!f.app.searching);
    f.press(KeyCode::Char('/'));
    f.chars("zzzz");
    assert!(f.app.visible.is_empty());
    assert_eq!(f.press(KeyCode::Enter), Effect::None);
    assert!(f.screen(100, 20).contains("No matches."));
}

#[test]
fn help_opens_and_any_key_closes_it() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('?'));
    assert!(matches!(f.app.screen, Screen::Help));
    assert!(f.screen(110, 30).contains("Keyboard shortcuts"));
    f.press(KeyCode::Char('x'));
    assert!(matches!(f.app.screen, Screen::Main));
}

// ------------------------------------------------------------------ form --

#[test]
fn adding_a_connection_writes_the_block_and_selects_it() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('a'));
    f.chars("ihu");
    f.press(KeyCode::Tab);
    f.chars("app.example.edu");
    f.press(KeyCode::Tab);
    f.chars("deploy");
    f.press(KeyCode::Tab);
    f.chars("22");
    // Identity file: open the list and take the key found in ~/.ssh.
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    f.press(KeyCode::Down);
    f.press(KeyCode::Enter);
    // Proxy: open the picker, switch to "Jump host", take the first host.
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    f.press(KeyCode::Tab);
    f.chars("App server");
    f.press(KeyCode::Tab);
    f.chars("prod, edu");

    let screen = f.screen(110, 24);
    assert!(screen.contains("Add connection"), "{screen}");
    assert!(screen.contains("+ Host ihu"), "{screen}");
    assert!(screen.contains("+     ProxyJump bastion"), "{screen}");

    f.ctrl('s');
    assert!(matches!(f.app.screen, Screen::Main));
    let added = "# sshcm:desc App server\n# sshcm:tags prod, edu\nHost ihu\n    HostName app.example.edu\n    User deploy\n    IdentityFile ~/.ssh/id_ed25519\n    ProxyJump bastion\n\n";
    assert_eq!(
        f.config(),
        CONFIG.replace("Host *\n", &format!("{added}Host *\n"))
    );
    assert_eq!(f.selected(), "ihu");
    let status = f.app.status.clone().expect("status");
    assert_eq!((status.tone, status.text.as_str()), (Tone::Ok, "Saved ihu"));
    assert!(f.screen(110, 24).contains("✓ Saved ihu"));
}

#[test]
fn saving_an_incomplete_form_reports_the_fields_and_writes_nothing() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('a'));
    // Nothing is reported before the first save attempt.
    assert!(f.screen(110, 24).contains("ctrl+s save"));
    assert!(!f.screen(110, 24).contains("needs attention"));

    f.ctrl('s');
    let Screen::Form(form) = &f.app.screen else {
        panic!("the form must stay open");
    };
    assert_eq!(form.focus, Field::Alias);
    let screen = f.screen(110, 24);
    assert!(screen.contains("Alias is required."), "{screen}");
    assert!(screen.contains("HostName is required."), "{screen}");
    assert!(screen.contains("! 2 fields need attention"), "{screen}");
    assert_eq!(f.config(), CONFIG);
}

#[test]
fn field_errors_show_while_typing() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('a'));
    f.chars("staging");
    // Long messages wrap under the field instead of being cut.
    let screen = f.screen(110, 24);
    assert!(
        screen.contains("A connection named staging already"),
        "{screen}"
    );
    assert!(screen.contains("exists."), "{screen}");
    assert!(
        f.screen(140, 24)
            .contains("A connection named staging already exists.")
    );
    f.clear();
    f.chars("a b");
    assert!(f.screen(110, 24).contains("Alias cannot contain spaces."));
    f.clear();
    f.chars("ok");
    f.press(KeyCode::Tab);
    f.chars("h.example.com");
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.chars("70000");
    let screen = f.screen(110, 24);
    assert!(
        screen.contains("Port must be between 1 and 65535."),
        "{screen}"
    );
    assert!(screen.contains("! 1 field needs attention"), "{screen}");
    assert!(
        screen.contains("Port is added to the preview once its value is"),
        "{screen}"
    );
    assert!(!screen.contains("Port 70000"), "{screen}");

    // Saving jumps to the field that needs fixing.
    f.press(KeyCode::Tab);
    f.ctrl('s');
    let Screen::Form(form) = &f.app.screen else {
        panic!("the form must stay open");
    };
    assert_eq!(form.focus, Field::Port);
    f.clear();
    f.chars("2200");
    f.ctrl('s');
    assert!(matches!(f.app.screen, Screen::Main));
    assert!(
        f.config()
            .contains("Host ok\n    HostName h.example.com\n    Port 2200\n")
    );
}

#[test]
fn editing_changes_only_the_edited_line() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    f.press(KeyCode::Up);
    assert_eq!(f.selected(), "staging");
    f.press(KeyCode::Char('e'));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.clear();
    f.chars("deploy");

    let screen = f.screen(110, 24);
    assert!(screen.contains("Edit connection  staging"), "{screen}");
    assert!(screen.contains("-     User ubuntu"), "{screen}");
    assert!(screen.contains("+     User deploy"), "{screen}");

    f.ctrl('s');
    assert_eq!(f.config(), CONFIG.replace("User ubuntu", "User deploy"));
    assert_eq!(f.selected(), "staging");
}

#[test]
fn renaming_in_the_form_updates_jump_references() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('e'));
    f.clear();
    f.chars("gate");
    f.ctrl('s');
    assert_eq!(
        f.config(),
        CONFIG
            .replace("Host bastion\n", "Host gate\n")
            .replace("ProxyJump bastion", "ProxyJump gate")
    );
    assert_eq!(f.selected(), "gate");
}

#[test]
fn saving_without_changes_writes_nothing() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('e'));
    f.ctrl('s');
    assert!(matches!(f.app.screen, Screen::Main));
    assert_eq!(f.app.status.clone().expect("status").text, "No changes.");
    assert_eq!(f.config(), CONFIG);
    assert!(!f.dir.path().join(".ssh/sshcm-backups").exists());
}

#[test]
fn escape_closes_an_untouched_form_at_once() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('e'));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Esc);
    assert!(matches!(f.app.screen, Screen::Main));
    // Typing something and deleting it again is not a change either.
    f.press(KeyCode::Char('a'));
    f.chars("x");
    f.press(KeyCode::Backspace);
    f.press(KeyCode::Esc);
    assert!(matches!(f.app.screen, Screen::Main));
}

#[test]
fn escape_on_a_changed_form_asks_before_discarding() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('e'));
    f.chars("-changed");
    f.press(KeyCode::Esc);
    let screen = f.screen(110, 24);
    assert!(screen.contains("Discard changes?"), "{screen}");
    assert!(
        screen.contains("Keep editing is selected by default."),
        "{screen}"
    );

    // Enter takes the default: back to the form, text intact.
    f.press(KeyCode::Enter);
    let Screen::Form(form) = &f.app.screen else {
        panic!("the form must stay open");
    };
    assert_eq!(form.alias.value(), "bastion-changed");
    assert!(form.discard.is_none());

    // Esc inside the question also keeps editing.
    f.press(KeyCode::Esc);
    f.press(KeyCode::Esc);
    assert!(matches!(f.app.screen, Screen::Form(_)));

    // Choosing "Discard" closes the form without writing.
    f.press(KeyCode::Esc);
    f.press(KeyCode::Right);
    f.press(KeyCode::Enter);
    assert!(matches!(f.app.screen, Screen::Main));
    assert_eq!(f.config(), CONFIG);

    // A changed proxy counts as a change, and `y` is a shortcut.
    open_proxy_picker(&mut f);
    f.press(KeyCode::Tab);
    f.chars("10.0.0.1:1080");
    f.press(KeyCode::Enter);
    f.press(KeyCode::Esc);
    assert!(f.screen(110, 24).contains("Discard changes?"));
    f.press(KeyCode::Char('y'));
    assert!(matches!(f.app.screen, Screen::Main));
    assert_eq!(f.config(), CONFIG);
}

#[test]
fn letters_typed_into_the_form_are_not_list_shortcuts() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('a'));
    f.chars("qdae?/");
    assert!(matches!(f.app.screen, Screen::Form(_)));
}

#[test]
fn dry_run_writes_nothing_from_the_interface() {
    let mut f = Fixture::dry_run(CONFIG);
    assert!(f.screen(110, 24).contains("dry run"));

    // Edit and save.
    f.press(KeyCode::Char('e'));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.clear();
    f.chars("root");
    assert!(f.screen(110, 24).contains("+     User root"));
    f.ctrl('s');
    assert!(matches!(f.app.screen, Screen::Main));
    let status = f.app.status.clone().expect("status");
    assert_eq!(
        (status.tone, status.text.as_str()),
        (Tone::Warn, "Dry run: nothing was written.")
    );
    assert_eq!(f.config(), CONFIG);
    // The list still shows what is on disk.
    assert!(f.screen(110, 24).contains("ops@bastion.example.com"));

    // Add.
    f.press(KeyCode::Char('a'));
    f.chars("new");
    f.press(KeyCode::Tab);
    f.chars("new.example.com");
    f.ctrl('s');
    assert_eq!(f.config(), CONFIG);
    assert_eq!(f.app.connection_count(), 4);

    // Remove.
    f.press(KeyCode::Char('d'));
    f.press(KeyCode::Right);
    f.press(KeyCode::Enter);
    assert_eq!(f.config(), CONFIG);
    assert_eq!(f.app.connection_count(), 4);
    assert_eq!(
        f.app.status.clone().expect("status").text,
        "Dry run: nothing was written."
    );
    assert!(!f.dir.path().join(".ssh/sshcm-backups").exists());
}

#[test]
fn a_block_without_hostname_can_be_edited() {
    let config = "Host github.com\n    User git\n";
    let mut f = Fixture::new(config);
    f.press(KeyCode::Char('e'));
    for _ in 0..3 {
        f.press(KeyCode::Tab);
    }
    f.chars("443");
    f.ctrl('s');
    assert!(
        matches!(f.app.screen, Screen::Main),
        "{}",
        f.screen(110, 24)
    );
    assert_eq!(f.config(), "Host github.com\n    User git\n    Port 443\n");

    // A new connection still needs one.
    f.press(KeyCode::Char('a'));
    f.chars("new");
    f.ctrl('s');
    assert!(f.screen(110, 24).contains("HostName is required."));
}

#[test]
fn values_already_in_the_file_do_not_block_other_edits() {
    // Written by hand: fine for ssh, outside sshcm's own rules.
    let config =
        "# sshcm:tags has space\nHost odd=name\n    HostName host/with/slash\n    User -weird\n";
    let mut f = Fixture::new(config);
    f.press(KeyCode::Char('e'));
    assert!(!f.screen(110, 24).contains("needs attention"));
    for _ in 0..3 {
        f.press(KeyCode::Tab);
    }
    f.chars("2222");
    f.ctrl('s');
    assert!(
        matches!(f.app.screen, Screen::Main),
        "{}",
        f.screen(110, 24)
    );
    assert_eq!(f.config(), format!("{config}    Port 2222\n"));

    // Touching such a value brings its rule back.
    f.press(KeyCode::Char('e'));
    f.chars("x");
    assert!(f.screen(110, 24).contains("Alias cannot contain '='."));
}

#[test]
fn first_connection_creates_the_config_file() {
    let mut f = Fixture::new("");
    assert!(
        f.screen(100, 20)
            .contains("No connections yet. Press a to add one.")
    );
    assert!(f.screen(100, 20).contains("0 connections"));
    f.press(KeyCode::Char('a'));
    f.chars("first");
    f.press(KeyCode::Enter);
    f.chars("first.example.com");
    f.ctrl('s');
    assert_eq!(f.config(), "Host first\n    HostName first.example.com\n");
    assert!(f.screen(100, 20).contains("1 connection "));
}

// --------------------------------------------------------------- pickers --

/// Opens the form for the selected connection with the proxy picker shown.
fn open_proxy_picker(f: &mut Fixture) {
    f.press(KeyCode::Char('e'));
    for _ in 0..5 {
        f.press(KeyCode::Tab);
    }
    f.press(KeyCode::Enter);
}

#[test]
fn jump_hosts_are_chained_in_the_order_they_are_picked() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    f.press(KeyCode::Up);
    open_proxy_picker(&mut f);
    // staging has no proxy yet: move to "Jump host".
    f.press(KeyCode::Tab);
    // Rows: bastion, db-prod, web-prod, staging (itself). Pick web-prod, then bastion.
    f.press(KeyCode::Down);
    f.press(KeyCode::Down);
    f.press(KeyCode::Char(' '));
    f.press(KeyCode::Up);
    f.press(KeyCode::Up);
    f.press(KeyCode::Char(' '));
    let screen = f.screen(110, 26);
    assert!(screen.contains("[2] bastion"), "{screen}");
    assert!(screen.contains("[1] web-prod"), "{screen}");
    assert!(
        screen.contains("this computer → web-prod → bastion → staging"),
        "{screen}"
    );
    assert!(screen.contains("ProxyJump web-prod,bastion"), "{screen}");
    assert!(
        screen.contains("this connection, cannot jump through itself"),
        "{screen}"
    );

    f.press(KeyCode::Enter);
    f.ctrl('s');
    assert!(
        f.config()
            .contains("    Port 2222\n    ProxyJump web-prod,bastion\n")
    );
}

#[test]
fn hosts_that_would_form_a_loop_cannot_be_picked() {
    let mut f = Fixture::new(CONFIG);
    // bastion: db-prod and web-prod already jump through it.
    open_proxy_picker(&mut f);
    f.press(KeyCode::Tab);
    let screen = f.screen(110, 26);
    assert_eq!(
        screen
            .matches("already jumps through this connection")
            .count(),
        2,
        "{screen}"
    );
    // The cursor starts on the first host that can be picked.
    f.press(KeyCode::Char(' '));
    assert!(f.screen(110, 26).contains("[1] staging"));
    // Space on a blocked row does nothing.
    f.press(KeyCode::Up);
    f.press(KeyCode::Char(' '));
    assert!(!f.screen(110, 26).contains("[2]"));
}

#[test]
fn a_typed_jump_host_is_validated() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    f.press(KeyCode::Up);
    open_proxy_picker(&mut f);
    f.press(KeyCode::Tab);
    for _ in 0..4 {
        f.press(KeyCode::Down);
    }
    f.chars("ops@gate:99999");
    f.press(KeyCode::Enter);
    let screen = f.screen(110, 26);
    assert!(
        screen.contains("Not a valid jump host: ops@gate:99999"),
        "{screen}"
    );
    for _ in 0..3 {
        f.press(KeyCode::Backspace);
    }
    f.press(KeyCode::Enter);
    f.ctrl('s');
    assert!(f.config().contains("    ProxyJump ops@gate:99\n"));
}

#[test]
fn socks_http_and_custom_proxies_are_written_as_proxy_command() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    f.press(KeyCode::Up);
    open_proxy_picker(&mut f);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    // An empty address is refused.
    f.press(KeyCode::Enter);
    assert!(
        f.screen(110, 26)
            .contains("Proxy address must look like host:port.")
    );
    f.chars("10.0.0.1:1080");
    assert!(
        f.screen(110, 26)
            .contains("ProxyCommand nc -X 5 -x 10.0.0.1:1080 %h %p")
    );
    f.press(KeyCode::Down);
    assert!(
        f.screen(110, 26)
            .contains("ProxyCommand nc -X connect -x 10.0.0.1:1080 %h %p")
    );
    f.press(KeyCode::Enter);
    f.ctrl('s');
    assert!(
        f.config()
            .contains("    ProxyCommand nc -X connect -x 10.0.0.1:1080 %h %p\n")
    );

    // Reopen: the picker starts on the saved choice. Switch to a custom
    // command; the old line must go.
    open_proxy_picker(&mut f);
    assert!(f.screen(110, 26).contains("10.0.0.1:1080"));
    f.press(KeyCode::Tab);
    f.chars("ssh -W %h:%p gate");
    f.press(KeyCode::Enter);
    f.ctrl('s');
    assert!(f.config().contains("    ProxyCommand ssh -W %h:%p gate\n"));
    assert!(!f.config().contains("nc -X"));

    // And back to no proxy at all.
    open_proxy_picker(&mut f);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    f.ctrl('s');
    assert_eq!(f.config(), CONFIG);
}

#[test]
fn escape_in_a_picker_keeps_the_previous_value() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Down);
    open_proxy_picker(&mut f);
    f.press(KeyCode::BackTab);
    f.press(KeyCode::Esc);
    assert!(matches!(f.app.screen, Screen::Form(_)));
    f.ctrl('s');
    assert_eq!(f.config(), CONFIG);
}

// ---------------------------------------------------------------- delete --

#[test]
fn delete_defaults_to_cancel() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Char('d'));
    let screen = f.screen(110, 24);
    assert!(screen.contains("Remove bastion?"), "{screen}");
    assert!(screen.contains("- Host bastion"), "{screen}");
    assert!(
        screen.contains("! 2 connections use bastion as their jump host"),
        "{screen}"
    );
    assert!(
        screen.contains("Cancel is selected by default."),
        "{screen}"
    );
    f.press(KeyCode::Enter);
    assert!(matches!(f.app.screen, Screen::Main));
    assert_eq!(f.config(), CONFIG);

    f.press(KeyCode::Char('d'));
    f.press(KeyCode::Right);
    f.press(KeyCode::Esc);
    assert_eq!(f.config(), CONFIG);
}

#[test]
fn delete_removes_the_block_after_choosing_remove() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::End);
    f.press(KeyCode::Up);
    f.press(KeyCode::Char('d'));
    assert!(!f.screen(110, 24).contains("jump host"));
    f.press(KeyCode::Right);
    f.press(KeyCode::Enter);
    assert_eq!(
        f.config(),
        CONFIG.replace(
            "Host staging\n    HostName 203.0.113.24\n    User ubuntu\n    Port 2222\n\n",
            ""
        )
    );
    let status = f.app.status.clone().expect("status");
    assert_eq!(
        (status.tone, status.text.as_str()),
        (Tone::Ok, "Removed staging")
    );
    assert_eq!(f.app.connection_count(), 3);
    assert!(f.dir.path().join(".ssh/sshcm-backups").is_dir());
}

// ---------------------------------------------------------------- layout --

#[test]
fn main_screen_layout() {
    let mut f = Fixture::new(CONFIG);
    f.press(KeyCode::Down);
    f.press(KeyCode::Down);
    let screen = f.screen(110, 24);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(lines[0].starts_with("sshcm  4 connections"), "{screen}");
    assert!(lines[0].ends_with("~/.ssh/config"), "{screen}");
    assert!(screen.contains("❯ web-prod  deploy@10.0.2.10"), "{screen}");
    assert!(screen.contains("via bastion"), "{screen}");
    assert!(screen.contains("Read-only"), "{screen}");
    assert!(
        screen.contains("Proxy             Jump host → bastion"),
        "{screen}"
    );
    assert!(
        screen.contains("Other settings    ServerAliveInterval 60"),
        "{screen}"
    );
    assert!(screen.contains("Config block"), "{screen}");
    assert!(
        lines[23].ends_with("enter connect  a add  e edit  d delete  / search  ? help  q quit"),
        "{screen}"
    );
}

#[test]
fn narrow_terminal_shows_only_the_list() {
    let f = Fixture::new(CONFIG);
    let screen = f.screen(79, 14);
    assert!(screen.contains("❯ bastion"), "{screen}");
    assert!(!screen.contains("Config block"), "{screen}");
    assert!(f.screen(80, 14).contains("Config block"));
}

#[test]
fn every_screen_survives_tiny_and_huge_terminals() {
    let sizes = [(1, 1), (3, 2), (10, 3), (20, 5), (40, 8), (300, 80)];
    let mut f = Fixture::new(CONFIG);
    let draw_all = |f: &Fixture| {
        for (w, h) in sizes {
            f.screen(w, h);
        }
    };
    draw_all(&f);
    f.press(KeyCode::Char('?'));
    draw_all(&f);
    f.press(KeyCode::Esc);
    f.press(KeyCode::Char('d'));
    draw_all(&f);
    f.press(KeyCode::Esc);
    f.press(KeyCode::Char('/'));
    f.chars("a very long search text that is wider than a small terminal");
    draw_all(&f);
    f.press(KeyCode::Esc);
    f.press(KeyCode::Char('e'));
    f.chars(&"x".repeat(200));
    draw_all(&f);
    for _ in 0..4 {
        f.press(KeyCode::Tab);
    }
    f.press(KeyCode::Enter);
    draw_all(&f);
    f.press(KeyCode::Esc);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    for _ in 0..4 {
        draw_all(&f);
        f.chars(&"y".repeat(120));
        f.press(KeyCode::Tab);
    }
}

#[test]
fn long_lists_scroll_with_the_selection() {
    let mut config = String::new();
    for i in 0..40 {
        config.push_str(&format!(
            "Host h{i:02}\n    HostName h{i:02}.example.com\n\n"
        ));
    }
    let mut f = Fixture::new(&config);
    assert!(f.screen(100, 12).contains("❯ h00"));
    for _ in 0..25 {
        f.press(KeyCode::Down);
    }
    let screen = f.screen(100, 12);
    assert!(screen.contains("❯ h25"), "{screen}");
    assert!(!screen.contains("h00 "), "{screen}");
    f.press(KeyCode::End);
    assert!(f.screen(100, 12).contains("❯ h39"));
}
