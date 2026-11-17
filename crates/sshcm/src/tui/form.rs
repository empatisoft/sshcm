//! The add/edit form: field values, live validation and the preview.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sshcm_core::model::{
    collapse_tilde, expand_tilde, parse_port, parse_tags, validate, validate_alias,
    validate_hostname, validate_user,
};
use sshcm_core::{
    Change, Connection, Entry, Proxy, SaveReport, Store, ValidationError, diff_lines,
    render_block_text,
};

use super::input::TextInput;
use super::picker::{KeyPicker, Picked, ProxyPicker};
use crate::commands::{Ctx, nc_warning};
use crate::i18n::{self, tr};
use crate::prompt::keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Alias,
    HostName,
    User,
    Port,
    IdentityFile,
    Proxy,
    Description,
    Tags,
}

impl Field {
    pub const ALL: [Field; 8] = [
        Field::Alias,
        Field::HostName,
        Field::User,
        Field::Port,
        Field::IdentityFile,
        Field::Proxy,
        Field::Description,
        Field::Tags,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Alias => "Alias",
            Field::HostName => "HostName",
            Field::User => "User",
            Field::Port => "Port",
            Field::IdentityFile => tr("Identity file", "Anahtar dosyası"),
            Field::Proxy => "Proxy",
            Field::Description => tr("Description", "Açıklama"),
            Field::Tags => tr("Tags", "Etiketler"),
        }
    }

    fn step(self, forward: bool) -> Field {
        let i = Field::ALL.iter().position(|f| *f == self).unwrap_or(0);
        let n = Field::ALL.len();
        Field::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Keys(KeyPicker),
    Proxy(Box<ProxyPicker>),
}

/// What a key press in the form led to.
#[derive(Debug)]
pub enum Outcome {
    Stay,
    Close,
    /// Nothing differed from the file, so nothing was written.
    Unchanged,
    Saved {
        alias: String,
        report: SaveReport,
        /// Something the user should know about the saved connection.
        warning: Option<String>,
    },
    /// `--dry-run`: the change was valid, and nothing was written.
    DryRun,
}

#[derive(Debug, Clone)]
pub struct Form {
    /// The entry being edited; `None` when adding.
    pub original: Option<Entry>,
    pub alias: TextInput,
    pub hostname: TextInput,
    pub user: TextInput,
    pub port: TextInput,
    pub identity_file: TextInput,
    pub proxy: Proxy,
    pub description: TextInput,
    pub tags: TextInput,
    pub focus: Field,
    /// Set by the first save attempt; from then on empty required fields are
    /// reported too.
    pub attempted: bool,
    /// Error from the last failed save.
    pub message: Option<String>,
    pub overlay: Option<Overlay>,
    /// Shown after Esc on a changed form; true while "Discard" is selected.
    pub discard: Option<bool>,
    keys: Vec<String>,
    /// Field texts and proxy as the form opened.
    initial: (Vec<String>, Proxy),
}

impl Form {
    pub fn add(ctx: &Ctx) -> Form {
        Form::new(ctx, None)
    }

    pub fn edit(ctx: &Ctx, entry: Entry) -> Form {
        Form::new(ctx, Some(entry))
    }

    fn new(ctx: &Ctx, original: Option<Entry>) -> Form {
        let conn = original
            .as_ref()
            .map(|e| e.conn.clone())
            .unwrap_or_default();
        let mut form = Form {
            alias: TextInput::new(&conn.alias),
            hostname: TextInput::new(conn.hostname.as_deref().unwrap_or("")),
            user: TextInput::new(conn.user.as_deref().unwrap_or("")),
            port: TextInput::new(&conn.port.map(|p| p.to_string()).unwrap_or_default()),
            identity_file: TextInput::new(conn.identity_file.as_deref().unwrap_or("")),
            proxy: conn.proxy.clone(),
            description: TextInput::new(conn.description.as_deref().unwrap_or("")),
            tags: TextInput::new(&conn.tags.join(", ")),
            focus: Field::Alias,
            attempted: false,
            message: None,
            overlay: None,
            discard: None,
            keys: keys::discover(&ctx.home),
            initial: (Vec::new(), Proxy::None),
            original,
        };
        form.initial = form.snapshot();
        form
    }

    fn original_alias(&self) -> Option<&str> {
        self.original.as_ref().map(|e| e.alias())
    }

    pub fn input(&self, field: Field) -> Option<&TextInput> {
        match field {
            Field::Alias => Some(&self.alias),
            Field::HostName => Some(&self.hostname),
            Field::User => Some(&self.user),
            Field::Port => Some(&self.port),
            Field::IdentityFile => Some(&self.identity_file),
            Field::Proxy => None,
            Field::Description => Some(&self.description),
            Field::Tags => Some(&self.tags),
        }
    }

    fn input_mut(&mut self, field: Field) -> Option<&mut TextInput> {
        match field {
            Field::Alias => Some(&mut self.alias),
            Field::HostName => Some(&mut self.hostname),
            Field::User => Some(&mut self.user),
            Field::Port => Some(&mut self.port),
            Field::IdentityFile => Some(&mut self.identity_file),
            Field::Proxy => None,
            Field::Description => Some(&mut self.description),
            Field::Tags => Some(&mut self.tags),
        }
    }

    /// The connection the form currently describes. Values that cannot be
    /// used yet (a port that is not a number, tags with bad characters) fall
    /// back to what the entry had, so the preview stays meaningful.
    pub fn connection(&self) -> Connection {
        let mut conn = self
            .original
            .as_ref()
            .map(|e| e.conn.clone())
            .unwrap_or_default();
        let optional = |input: &TextInput| Some(input.text().to_string()).filter(|v| !v.is_empty());
        conn.alias = self.alias.text().to_string();
        conn.hostname = optional(&self.hostname);
        conn.user = optional(&self.user);
        if self.port.is_empty() {
            conn.port = None;
        } else if let Ok(port) = parse_port(self.port.text()) {
            conn.port = Some(port);
        }
        conn.identity_file = optional(&self.identity_file);
        conn.proxy = self.proxy.clone();
        conn.description = optional(&self.description);
        if let Ok(tags) = parse_tags(self.tags.text()) {
            conn.tags = tags;
        }
        conn
    }

    /// Problems with the current values, at most one per field, in field
    /// order. Empty required fields only count after a save attempt.
    pub fn errors(&self, store: &Store) -> Vec<(Field, String)> {
        let mut errors: Vec<(Field, ValidationError)> = Vec::new();
        let known = store.known(self.original_alias());
        let editing = self.original.is_some();
        // When editing, a value that is already in the file is not judged:
        // it must never stop the user from changing another field.
        let check = |field: Field| !editing || self.changed(field);

        let alias = self.alias.text();
        if alias.is_empty() {
            if self.attempted {
                errors.push((Field::Alias, ValidationError::AliasEmpty));
            }
        } else if let Some(err) = validate_alias(alias).err().filter(|_| check(Field::Alias)) {
            errors.push((Field::Alias, err));
        } else if known.names.iter().any(|n| n == alias) {
            errors.push((Field::Alias, ValidationError::AliasTaken(alias.to_string())));
        }

        // Only a new connection needs a HostName; without one ssh uses the
        // alias.
        if self.hostname.is_empty() {
            if self.attempted && !editing {
                errors.push((Field::HostName, ValidationError::HostNameEmpty));
            }
        } else if check(Field::HostName)
            && let Err(err) = validate_hostname(self.hostname.text())
        {
            errors.push((Field::HostName, err));
        }

        if check(Field::User)
            && let Err(err) = validate_user(self.user.text())
        {
            errors.push((Field::User, err));
        }
        if !self.port.is_empty()
            && let Err(err) = parse_port(self.port.text())
        {
            errors.push((Field::Port, err));
        }

        // A key path is only checked when it was changed: an existing block
        // may point at a key that lives on another machine.
        let key = self.identity_file.text();
        if !key.is_empty()
            && self.changed(Field::IdentityFile)
            && !key.contains('%')
            && !expand_tilde(key, store.home()).is_file()
        {
            errors.push((
                Field::IdentityFile,
                ValidationError::IdentityFileMissing(key.to_string()),
            ));
        }

        if check(Field::Tags)
            && let Err(err) = parse_tags(self.tags.text())
        {
            errors.push((Field::Tags, err));
        }

        // With the single fields in order, let the core check the whole
        // connection; what is left are proxy problems such as loops.
        if errors.is_empty()
            && !alias.is_empty()
            && (editing || !self.hostname.is_empty())
            && let Err(err) = validate(
                &self.connection(),
                self.original.as_ref().map(|e| &e.conn),
                &known,
                store.home(),
            )
        {
            errors.push((Field::Proxy, err));
        }

        errors.sort_by_key(|(field, _)| Field::ALL.iter().position(|f| f == field));
        errors
            .into_iter()
            .map(|(field, err)| (field, i18n::validation(&err)))
            .collect()
    }

    /// The text of every field plus the proxy: what "unchanged" is measured
    /// against.
    fn snapshot(&self) -> (Vec<String>, Proxy) {
        let texts = Field::ALL
            .iter()
            .map(|field| {
                self.input(*field)
                    .map(|input| input.text().to_string())
                    .unwrap_or_default()
            })
            .collect();
        (texts, self.proxy.clone())
    }

    /// True when the field's text differs from what the form opened with.
    fn changed(&self, field: Field) -> bool {
        let index = Field::ALL.iter().position(|f| *f == field).unwrap_or(0);
        self.input(field)
            .is_some_and(|input| input.text() != self.initial.0[index])
    }

    /// True when anything was typed or picked since the form opened.
    pub fn is_dirty(&self) -> bool {
        self.snapshot() != self.initial
    }

    /// The block as it would be written: every line added for a new
    /// connection, a diff against the current block when editing.
    pub fn preview(&self, store: &Store) -> Vec<(Change, String)> {
        let mut conn = self.connection();
        match &self.original {
            None => {
                if conn.alias.is_empty() {
                    conn.alias = "…".to_string();
                }
                render_block_text(&conn)
                    .lines()
                    .map(|line| (Change::Added, line.to_string()))
                    .collect()
            }
            Some(entry) => {
                if validate_alias(&conn.alias).is_err() {
                    conn.alias = entry.alias().to_string();
                }
                let mut edited = store.clone();
                let new_text = edited
                    .update(entry.alias(), &conn)
                    .ok()
                    .and_then(|()| edited.block_text(&conn.alias))
                    .unwrap_or_else(|| entry.text.clone());
                diff_lines(&entry.text, &new_text)
            }
        }
    }

    fn open_overlay(&mut self, store: &Store) {
        match self.focus {
            Field::IdentityFile if !self.keys.is_empty() => {
                self.overlay = Some(Overlay::Keys(KeyPicker::new(
                    &self.keys,
                    self.identity_file.text(),
                )));
            }
            Field::Proxy => {
                let mut me: Vec<String> = Vec::new();
                if !self.alias.is_empty() {
                    me.push(self.alias.text().to_string());
                }
                // The alias before editing goes last: validation looks the
                // edited block up by it.
                if let Some(original) = self.original_alias()
                    && !me.iter().any(|m| m == original)
                {
                    me.push(original.to_string());
                }
                let name = if self.alias.is_empty() {
                    tr("new connection", "yeni bağlantı")
                } else {
                    self.alias.text()
                };
                self.overlay = Some(Overlay::Proxy(Box::new(ProxyPicker::new(
                    store,
                    &self.proxy,
                    &me,
                    name,
                ))));
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, store: &mut Store, ctx: &Ctx) -> Outcome {
        match &mut self.overlay {
            Some(Overlay::Keys(picker)) => {
                match picker.on_key(key) {
                    Some(Ok(choice)) => {
                        self.identity_file.set(choice.as_deref().unwrap_or(""));
                        self.overlay = None;
                    }
                    Some(Err(())) => self.overlay = None,
                    None => {}
                }
                return Outcome::Stay;
            }
            Some(Overlay::Proxy(picker)) => {
                match picker.on_key(key) {
                    Picked::Done(proxy) => {
                        self.proxy = proxy;
                        self.overlay = None;
                    }
                    Picked::Cancel => self.overlay = None,
                    Picked::Stay => {}
                }
                return Outcome::Stay;
            }
            None => {}
        }

        // The "discard changes?" question takes every key while it is open.
        if let Some(selected) = self.discard {
            match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                    self.discard = Some(!selected);
                }
                KeyCode::Char('y') => return Outcome::Close,
                KeyCode::Enter if selected => return Outcome::Close,
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n') => self.discard = None,
                _ => {}
            }
            return Outcome::Stay;
        }

        self.message = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Esc on a changed form asks first; "Keep editing" is the default.
            KeyCode::Esc if self.is_dirty() => self.discard = Some(false),
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Char('s') if ctrl => return self.save(store, ctx),
            KeyCode::Tab | KeyCode::Down => self.focus = self.focus.step(true),
            KeyCode::BackTab | KeyCode::Up => self.focus = self.focus.step(false),
            KeyCode::Enter if matches!(self.focus, Field::Proxy | Field::IdentityFile) => {
                self.open_overlay(store);
            }
            KeyCode::Char(' ') if self.focus == Field::Proxy => self.open_overlay(store),
            // Enter moves on, like Tab, so the form can be filled top to bottom.
            KeyCode::Enter => self.focus = self.focus.step(true),
            _ => {
                let focus = self.focus;
                if let Some(input) = self.input_mut(focus) {
                    input.handle(key);
                }
            }
        }
        Outcome::Stay
    }

    fn save(&mut self, store: &mut Store, ctx: &Ctx) -> Outcome {
        self.attempted = true;
        if let Some((field, _)) = self.errors(store).first() {
            self.focus = *field;
            return Outcome::Stay;
        }

        let mut conn = self.connection();
        conn.identity_file = conn
            .identity_file
            .map(|path| collapse_tilde(&path, store.home()));
        let applied = match self.original_alias() {
            Some(original) => store.update(original, &conn),
            None => {
                store.add(&conn);
                Ok(())
            }
        };
        if let Err(err) = applied {
            self.message = Some(i18n::doc_error(&err));
            return Outcome::Stay;
        }
        if !store.is_modified() {
            return Outcome::Unchanged;
        }
        if ctx.dry_run {
            // Valid, and that is all: put the store back as it is on disk.
            if let Ok(fresh) = ctx.load() {
                *store = fresh;
            }
            return Outcome::DryRun;
        }
        match store.save(&ctx.save_options(Some(&conn.alias))) {
            Ok(report) => Outcome::Saved {
                warning: nc_warning(&conn).map(str::to_string),
                alias: conn.alias,
                report,
            },
            Err(err) => {
                self.message = Some(i18n::store_error(&err));
                // The file on disk is as it was; drop the edit from memory too.
                if let Ok(fresh) = ctx.load() {
                    *store = fresh;
                }
                Outcome::Stay
            }
        }
    }
}
