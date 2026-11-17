//! The state of the full-screen interface and how keys change it. Nothing
//! here touches the terminal, so all of it can be tested.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sshcm_core::{Entry, Store, Verification};

use super::form::{Form, Outcome};
use super::input::TextInput;
use crate::commands::Ctx;
use crate::error::Result;
use crate::i18n::{self, tr, trf};

/// What the event loop should do after a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    /// Ctrl+C: leave with exit code 130.
    Interrupt,
    /// Leave the interface and hand over to ssh.
    Connect(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub tone: Tone,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DeleteDialog {
    pub entry: Entry,
    pub dependents: Vec<String>,
    /// False while "Cancel" is selected, which is the default.
    pub remove_selected: bool,
}

#[derive(Debug, Clone)]
pub enum Screen {
    Main,
    Form(Box<Form>),
    Delete(Box<DeleteDialog>),
    Help,
}

pub struct App {
    pub ctx: Ctx,
    pub store: Store,
    /// Connections first, then read-only patterns.
    pub entries: Vec<Entry>,
    /// Indices into `entries` that match the search, best match first.
    pub visible: Vec<usize>,
    /// Position in `visible`.
    pub selected: usize,
    pub search: TextInput,
    pub searching: bool,
    pub status: Option<Status>,
    pub screen: Screen,
    matcher: Matcher,
}

impl App {
    pub fn new(ctx: Ctx) -> Result<App> {
        let store = ctx.load()?;
        let mut app = App {
            ctx,
            store,
            entries: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            search: TextInput::default(),
            searching: false,
            status: None,
            screen: Screen::Main,
            matcher: Matcher::new(Config::DEFAULT),
        };
        app.refresh(None);
        Ok(app)
    }

    /// Number of connections, not counting patterns.
    pub fn connection_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_pattern).count()
    }

    pub fn current(&self) -> Option<&Entry> {
        self.visible.get(self.selected).map(|&i| &self.entries[i])
    }

    /// Rebuilds the lists from the store and puts the cursor on `select`,
    /// or keeps it on the entry it was on.
    fn refresh(&mut self, select: Option<&str>) {
        let keep = select
            .map(str::to_string)
            .or_else(|| self.current().map(|e| e.alias().to_string()));
        let (patterns, connections): (Vec<Entry>, Vec<Entry>) =
            self.store.entries().into_iter().partition(|e| e.is_pattern);
        self.entries = connections;
        self.entries.extend(patterns);
        self.filter();
        if let Some(alias) = keep
            && let Some(position) = self
                .visible
                .iter()
                .position(|&i| self.entries[i].alias() == alias)
        {
            self.selected = position;
        }
    }

    /// Re-reads the config from disk.
    fn reload(&mut self, select: Option<&str>) {
        match self.ctx.load() {
            Ok(store) => self.store = store,
            Err(err) => self.set_status(Tone::Error, error_text(&err)),
        }
        self.refresh(select);
    }

    /// Applies the search text: fuzzy match on alias, address, user and tags.
    fn filter(&mut self) {
        let query = self.search.text().to_string();
        if query.is_empty() {
            self.visible = (0..self.entries.len()).collect();
        } else {
            let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
            let mut buffer = Vec::new();
            let mut scored: Vec<(u32, usize)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| {
                    let haystack = format!(
                        "{} {} {}",
                        entry.patterns.join(" "),
                        entry.conn.target(),
                        entry.conn.tags.join(" ")
                    );
                    pattern
                        .score(Utf32Str::new(&haystack, &mut buffer), &mut self.matcher)
                        .map(|score| (score, i))
                })
                .collect();
            // Best score first; file order breaks ties.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.visible = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
    }

    fn set_status(&mut self, tone: Tone, text: String) {
        self.status = Some(Status { tone, text });
    }

    fn move_selection(&mut self, down: bool) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = if down {
            (self.selected + 1).min(self.visible.len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Effect {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Effect::Interrupt;
        }
        match &mut self.screen {
            Screen::Main => self.on_main_key(key),
            Screen::Help => {
                self.screen = Screen::Main;
                Effect::None
            }
            Screen::Delete(_) => {
                self.on_delete_key(key);
                Effect::None
            }
            Screen::Form(form) => {
                let outcome = form.on_key(key, &mut self.store, &self.ctx);
                self.after_form(outcome);
                Effect::None
            }
        }
    }

    fn on_main_key(&mut self, key: KeyEvent) -> Effect {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search.set("");
                    self.filter();
                }
                KeyCode::Enter => self.searching = false,
                KeyCode::Up => self.move_selection(false),
                KeyCode::Down => self.move_selection(true),
                _ => {
                    if self.search.handle(key) {
                        self.selected = 0;
                        self.filter();
                    }
                }
            }
            return Effect::None;
        }

        self.status = None;
        match key.code {
            KeyCode::Char('q') => return Effect::Quit,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true),
            KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.visible.len().saturating_sub(1);
            }
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Esc if !self.search.is_empty() => {
                self.search.set("");
                self.filter();
            }
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Char('a') => self.screen = Screen::Form(Box::new(Form::add(&self.ctx))),
            KeyCode::Enter => {
                if let Some(entry) = self.current() {
                    if entry.is_pattern {
                        self.read_only_notice();
                    } else {
                        return Effect::Connect(entry.alias().to_string());
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Some(entry) = self.current().cloned() {
                    if entry.read_only() {
                        self.read_only_notice();
                    } else {
                        self.screen = Screen::Form(Box::new(Form::edit(&self.ctx, entry)));
                    }
                }
            }
            KeyCode::Char('d') => {
                if let Some(entry) = self.current().cloned() {
                    if entry.read_only() {
                        self.read_only_notice();
                    } else {
                        let dependents = self.store.dependents(entry.alias());
                        self.screen = Screen::Delete(Box::new(DeleteDialog {
                            entry,
                            dependents,
                            remove_selected: false,
                        }));
                    }
                }
            }
            _ => {}
        }
        Effect::None
    }

    fn read_only_notice(&mut self) {
        if let Some(alias) = self.current().map(|e| e.patterns.join(" ")) {
            self.set_status(
                Tone::Warn,
                trf(
                    "{} is a pattern and is read-only.",
                    "{} bir desendir ve salt okunurdur.",
                    &[&alias],
                ),
            );
        }
    }

    fn on_delete_key(&mut self, key: KeyEvent) {
        let Screen::Delete(dialog) = &mut self.screen else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => self.screen = Screen::Main,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                dialog.remove_selected = !dialog.remove_selected;
            }
            KeyCode::Char('h') => dialog.remove_selected = false,
            KeyCode::Char('l') => dialog.remove_selected = true,
            KeyCode::Enter if dialog.remove_selected => {
                let alias = dialog.entry.alias().to_string();
                self.screen = Screen::Main;
                self.remove(&alias);
            }
            KeyCode::Enter => self.screen = Screen::Main,
            _ => {}
        }
    }

    fn dry_run_notice(&mut self) {
        self.set_status(
            Tone::Warn,
            tr(
                "Dry run: nothing was written.",
                "Deneme çalıştırması: hiçbir şey yazılmadı.",
            )
            .to_string(),
        );
    }

    fn remove(&mut self, alias: &str) {
        if self.ctx.dry_run {
            self.dry_run_notice();
            return;
        }
        let result = self
            .store
            .remove(alias)
            .map_err(|err| i18n::doc_error(&err))
            .and_then(|()| {
                self.store
                    .save(&self.ctx.save_options(None))
                    .map_err(|err| i18n::store_error(&err))
            });
        match result {
            Ok(report) => {
                self.refresh(None);
                self.saved_status(
                    trf("Removed {}", "{} silindi", &[alias]),
                    report.verification,
                    None,
                );
            }
            Err(message) => {
                self.reload(None);
                self.set_status(Tone::Error, message);
            }
        }
    }

    fn saved_status(&mut self, text: String, verification: Verification, warning: Option<String>) {
        match verification {
            Verification::Passed => match warning {
                Some(warning) => self.set_status(Tone::Warn, format!("{text} · {warning}")),
                None => self.set_status(Tone::Ok, text),
            },
            Verification::Skipped => self.set_status(
                Tone::Warn,
                format!(
                    "{text} · {}",
                    tr(
                        "ssh was not found, config not checked",
                        "ssh bulunamadı, config denetlenmedi"
                    )
                ),
            ),
        }
    }

    fn after_form(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.screen = Screen::Main,
            Outcome::Unchanged => {
                self.screen = Screen::Main;
                self.set_status(Tone::Ok, tr("No changes.", "Değişiklik yok.").to_string());
            }
            Outcome::DryRun => {
                self.screen = Screen::Main;
                self.refresh(None);
                self.dry_run_notice();
            }
            Outcome::Saved {
                alias,
                report,
                warning,
            } => {
                self.screen = Screen::Main;
                // Show the saved connection even if a search would hide it.
                self.search.set("");
                self.searching = false;
                self.refresh(Some(&alias));
                self.saved_status(
                    trf("Saved {}", "{} kaydedildi", &[&alias]),
                    report.verification,
                    warning,
                );
            }
        }
    }
}

fn error_text(err: &crate::error::AppError) -> String {
    match err {
        crate::error::AppError::Message(message) => message.clone(),
        _ => String::new(),
    }
}
