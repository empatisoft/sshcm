//! Drawing. Every function here turns state into widgets and changes
//! nothing. Colors come from the terminal's own palette, and none are used
//! when `NO_COLOR` is set.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use sshcm_core::model::{DEFAULT_PORT, collapse_tilde, parse_port};
use sshcm_core::{Change, Entry, Proxy};

use super::app::{App, DeleteDialog, Screen, Tone};
use super::form::{Field, Form, Overlay};
use super::input::TextInput;
use super::picker::{KeyPicker, Kind, ProxyPicker};
use crate::commands::proxy_short;
use crate::i18n::{tr, trf};

/// Below this width the detail panel is hidden and only the list is shown.
const NARROW: u16 = 80;
const LABEL_WIDTH: usize = 18;

#[derive(Clone, Copy)]
struct Theme {
    color: bool,
}

impl Theme {
    fn current() -> Theme {
        Theme {
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        }
    }

    fn fg(self, color: Color) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            Style::new()
        }
    }

    fn accent(self) -> Style {
        self.fg(Color::Cyan)
    }

    fn dim(self) -> Style {
        if self.color {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    }

    fn ok(self) -> Style {
        self.fg(Color::Green)
    }

    fn warn(self) -> Style {
        self.fg(Color::Yellow)
    }

    fn error(self) -> Style {
        self.fg(Color::Red)
    }

    fn strong(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    /// A selected button or tab.
    fn chosen(self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn pad(text: &str, to: usize) -> String {
    format!("{text}{}", " ".repeat(to.saturating_sub(width(text))))
}

/// Cuts `text` to `max` characters, ending in `…` when something was cut.
fn fit(text: &str, max: usize) -> String {
    if width(text) <= max {
        text.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut cut: String = text.chars().take(max - 1).collect();
        cut.push('…');
        cut
    }
}

/// Breaks `text` into lines of at most `max` characters at spaces. A word
/// longer than a line is cut.
fn wrap(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word = fit(word, max);
        if !current.is_empty() && width(&current) + 1 + width(&word) > max {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// A rectangle of at most the given size in the middle of `area`.
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = Theme::current();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    match &app.screen {
        Screen::Form(form) => {
            draw_form_header(frame, header, app, form, theme);
            draw_form(frame, body, app, form, theme);
            draw_form_footer(frame, footer, app, form, theme);
            match &form.overlay {
                Some(Overlay::Proxy(picker)) => draw_proxy_picker(frame, body, picker, theme),
                Some(Overlay::Keys(picker)) => draw_key_picker(frame, body, picker, theme),
                None => {}
            }
            if let Some(discard_selected) = form.discard {
                draw_discard(frame, body, discard_selected, theme);
            }
        }
        screen => {
            draw_main_header(frame, header, app, theme);
            draw_main(frame, body, app, theme);
            match screen {
                Screen::Delete(dialog) => {
                    draw_delete(frame, body, app, dialog, theme);
                    draw_footer(
                        frame,
                        footer,
                        Line::styled(
                            tr(
                                "Cancel is selected by default.",
                                "Varsayılan seçim Vazgeç'tir.",
                            ),
                            theme.dim(),
                        ),
                        &[
                            ("←→", tr("choose", "seç")),
                            ("enter", tr("confirm", "onayla")),
                            ("esc", tr("cancel", "vazgeç")),
                        ],
                        theme,
                    );
                }
                Screen::Help => {
                    draw_help(frame, body, theme);
                    draw_footer(
                        frame,
                        footer,
                        Line::default(),
                        &[("esc", tr("close", "kapat"))],
                        theme,
                    );
                }
                _ => draw_main_footer(frame, footer, app, theme),
            }
        }
    }
}

// ---------------------------------------------------------------- header --

fn draw_header(
    frame: &mut Frame,
    area: Rect,
    mut left: Vec<Span<'static>>,
    app: &App,
    theme: Theme,
) {
    left.insert(
        0,
        Span::styled("sshcm", theme.accent().patch(theme.strong())),
    );
    if app.ctx.dry_run {
        // Always visible, so nobody takes a dry run for the real thing.
        left.push(Span::raw("  "));
        left.push(Span::styled(
            format!(" {} ", tr("dry run", "deneme")),
            theme.warn().patch(theme.chosen()),
        ));
    }
    let path = app.store.display_path();
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    frame.render_widget(
        Paragraph::new(Line::styled(path, theme.dim())).alignment(Alignment::Right),
        area,
    );
}

fn draw_main_header(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let count = app.connection_count();
    let text = if count == 1 {
        tr("1 connection", "1 bağlantı").to_string()
    } else {
        trf("{} connections", "{} bağlantı", &[&count.to_string()])
    };
    draw_header(
        frame,
        area,
        vec![Span::raw("  "), Span::styled(text, theme.dim())],
        app,
        theme,
    );
}

fn draw_form_header(frame: &mut Frame, area: Rect, app: &App, form: &Form, theme: Theme) {
    let mut spans = vec![Span::raw("  ")];
    match &form.original {
        Some(entry) => {
            spans.push(Span::raw(tr("Edit connection", "Bağlantıyı düzenle")));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(entry.alias().to_string(), theme.dim()));
        }
        None => spans.push(Span::raw(tr("Add connection", "Bağlantı ekle"))),
    }
    draw_header(frame, area, spans, app, theme);
}

// ---------------------------------------------------------------- footer --

/// Left text plus right-aligned key hints. Hints that do not fit are dropped
/// from the end, so the most important ones should come first.
fn draw_footer(
    frame: &mut Frame,
    area: Rect,
    left: Line<'static>,
    hints: &[(&str, &str)],
    theme: Theme,
) {
    let available = (area.width as usize).saturating_sub(left.width() + 2);
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for (key, label) in hints {
        let needed = width(key) + 1 + width(label) + if spans.is_empty() { 0 } else { 2 };
        if used + needed > available {
            break;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key.to_string(), theme.accent()));
        spans.push(Span::styled(format!(" {label}"), theme.dim()));
        used += needed;
    }
    frame.render_widget(Paragraph::new(left), area);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
        area,
    );
}

fn draw_main_footer(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let left = match &app.status {
        Some(status) => {
            let (mark, style) = match status.tone {
                Tone::Ok => ("✓", theme.ok()),
                Tone::Warn => ("!", theme.warn()),
                Tone::Error => ("!", theme.error()),
            };
            let room = (area.width as usize).saturating_sub(2);
            Line::from(vec![
                Span::styled(mark, style.patch(theme.strong())),
                Span::raw(" "),
                Span::raw(fit(&status.text, room)),
            ])
        }
        None => Line::default(),
    };
    if app.searching {
        draw_footer(
            frame,
            area,
            left,
            &[
                ("enter", tr("keep filter", "süzgeci tut")),
                ("esc", tr("clear", "temizle")),
                ("↑↓", tr("move", "gezin")),
            ],
            theme,
        );
    } else {
        draw_footer(
            frame,
            area,
            left,
            &[
                ("enter", tr("connect", "bağlan")),
                ("a", tr("add", "ekle")),
                ("e", tr("edit", "düzenle")),
                ("d", tr("delete", "sil")),
                ("/", tr("search", "ara")),
                ("?", tr("help", "yardım")),
                ("q", tr("quit", "çık")),
            ],
            theme,
        );
    }
}

fn draw_form_footer(frame: &mut Frame, area: Rect, app: &App, form: &Form, theme: Theme) {
    if form.discard.is_some() {
        draw_footer(
            frame,
            area,
            Line::styled(
                tr(
                    "Keep editing is selected by default.",
                    "Varsayılan seçim düzenlemeye devam etmektir.",
                ),
                theme.dim(),
            ),
            &[
                ("←→", tr("choose", "seç")),
                ("enter", tr("confirm", "onayla")),
                ("esc", tr("keep editing", "düzenlemeye dön")),
            ],
            theme,
        );
        return;
    }
    match &form.overlay {
        Some(Overlay::Proxy(picker)) => {
            let hints: Vec<(&str, &str)> = match picker.kind {
                Kind::Jump => vec![
                    ("↑↓", tr("move", "gezin")),
                    ("space", tr("select", "seç")),
                    ("tab", tr("proxy type", "proxy türü")),
                    ("enter", tr("confirm", "onayla")),
                    ("esc", tr("cancel", "vazgeç")),
                ],
                Kind::Server => vec![
                    ("↑↓", "SOCKS5 / HTTP"),
                    ("tab", tr("proxy type", "proxy türü")),
                    ("enter", tr("confirm", "onayla")),
                    ("esc", tr("cancel", "vazgeç")),
                ],
                _ => vec![
                    ("tab", tr("proxy type", "proxy türü")),
                    ("enter", tr("confirm", "onayla")),
                    ("esc", tr("cancel", "vazgeç")),
                ],
            };
            let left = if picker.kind == Kind::Jump {
                tr(
                    "Select more than one to chain them in order.",
                    "Sırayla zincirlemek için birden fazla seçin.",
                )
            } else {
                ""
            };
            draw_footer(frame, area, Line::styled(left, theme.dim()), &hints, theme);
        }
        Some(Overlay::Keys(_)) => draw_footer(
            frame,
            area,
            Line::default(),
            &[
                ("↑↓", tr("move", "gezin")),
                ("enter", tr("select", "seç")),
                ("esc", tr("cancel", "vazgeç")),
            ],
            theme,
        ),
        None => {
            let errors = form.errors(&app.store);
            let room = (area.width as usize).saturating_sub(2);
            let left = if let Some(message) = &form.message {
                Line::from(vec![
                    Span::styled("!", theme.error().patch(theme.strong())),
                    Span::raw(" "),
                    Span::styled(
                        fit(message.lines().next().unwrap_or(""), room),
                        theme.error(),
                    ),
                ])
            } else if errors.is_empty() {
                Line::default()
            } else {
                let text = if errors.len() == 1 {
                    tr("1 field needs attention", "1 alan düzeltilmeli").to_string()
                } else {
                    trf(
                        "{} fields need attention",
                        "{} alan düzeltilmeli",
                        &[&errors.len().to_string()],
                    )
                };
                Line::from(vec![
                    Span::styled("!", theme.error().patch(theme.strong())),
                    Span::raw(" "),
                    Span::styled(text, theme.error()),
                ])
            };
            let pick = matches!(form.focus, Field::Proxy | Field::IdentityFile);
            let mut hints: Vec<(&str, &str)> = Vec::new();
            hints.push(("ctrl+s", tr("save", "kaydet")));
            hints.push(("esc", tr("cancel", "vazgeç")));
            if pick {
                hints.push(("enter", tr("choose", "seç")));
            }
            hints.push(("tab", tr("next field", "sonraki alan")));
            hints.push(("shift+tab", tr("previous", "önceki")));
            draw_footer(frame, area, left, &hints, theme);
        }
    }
}

// ------------------------------------------------------------ main screen --

fn draw_main(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    if area.width < NARROW {
        draw_list(frame, area, app, theme);
        return;
    }
    let list_width = (area.width * 45 / 100).clamp(36, 56);
    let [list, detail] =
        Layout::horizontal([Constraint::Length(list_width), Constraint::Min(0)]).areas(area);
    draw_list(frame, list, app, theme);
    draw_detail(frame, detail, app, theme);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let block = Block::bordered()
        .border_style(theme.accent())
        .title(Span::styled(
            format!(" {} ", tr("Connections", "Bağlantılar")),
            theme.strong(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width < 4 {
        return;
    }
    let [search, rows] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    // Search line.
    let mut spans = vec![Span::styled("/ ", theme.accent())];
    if app.search.value().is_empty() && !app.searching {
        spans.push(Span::styled(
            tr(
                "Search by alias, host or user",
                "Alias, adres veya kullanıcıya göre ara",
            ),
            theme.dim(),
        ));
    } else {
        spans.push(Span::raw(app.search.value().to_string()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), search);
    if app.searching && matches!(app.screen, Screen::Main) {
        let x = search.x + 2 + app.search.cursor() as u16;
        frame.set_cursor_position(Position::new(
            x.min(search.right().saturating_sub(1)),
            search.y,
        ));
    }

    // Rows: connections, then a heading and the read-only patterns.
    let inner_width = rows.width as usize;
    let alias_width = app
        .visible
        .iter()
        .map(|&i| width(&app.entries[i].patterns.join(" ")))
        .max()
        .unwrap_or(0)
        .clamp(4, 18);
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0;
    let mut heading_done = false;
    for (position, &index) in app.visible.iter().enumerate() {
        let entry = &app.entries[index];
        if entry.is_pattern && !heading_done {
            heading_done = true;
            lines.push(Line::styled(
                format!("  {}", tr("Read-only", "Salt okunur")),
                theme.dim(),
            ));
        }
        let selected = position == app.selected;
        if selected {
            selected_line = lines.len();
        }
        lines.push(list_row(entry, selected, alias_width, inner_width, theme));
    }
    if app.visible.is_empty() {
        let text = if app.entries.is_empty() {
            tr(
                "No connections yet. Press a to add one.",
                "Henüz bağlantı yok. Eklemek için a tuşuna basın.",
            )
        } else {
            tr("No matches.", "Eşleşme yok.")
        };
        lines.push(Line::styled(format!("  {text}"), theme.dim()));
    }

    // Keep the selected row in view.
    let height = rows.height as usize;
    let offset = if height == 0 {
        0
    } else {
        (selected_line + 1).saturating_sub(height)
    };
    let shown: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();
    frame.render_widget(Paragraph::new(shown), rows);
}

fn list_row(
    entry: &Entry,
    selected: bool,
    alias_width: usize,
    total: usize,
    theme: Theme,
) -> Line<'static> {
    let name = fit(&entry.patterns.join(" "), alias_width);
    let detail = if entry.is_pattern {
        tr("pattern", "desen").to_string()
    } else {
        entry.conn.target()
    };
    let via = proxy_short(&entry.conn.proxy);
    // marker + name + gap, then the address, then the proxy at the right edge.
    let fixed = 2 + alias_width + 2;
    let via_room = if via.is_empty() { 0 } else { width(&via) + 2 };
    let detail_room = total.saturating_sub(fixed + via_room);
    let detail = pad(&fit(&detail, detail_room), detail_room);

    let read_only = entry.is_pattern;
    let (name_style, rest_style) = if selected {
        (
            theme.accent().patch(theme.strong()),
            if read_only { theme.dim() } else { Style::new() },
        )
    } else if read_only {
        (theme.dim(), theme.dim())
    } else {
        (Style::new(), theme.dim())
    };
    let mut spans = vec![
        Span::styled(if selected { "❯ " } else { "  " }, theme.accent()),
        Span::styled(pad(&name, alias_width + 2), name_style),
        Span::styled(detail, rest_style),
    ];
    if !via.is_empty() {
        spans.push(Span::styled(format!("  {via}"), rest_style));
    }
    Line::from(spans)
}

fn proxy_long(proxy: &Proxy) -> String {
    match proxy {
        Proxy::None => tr("None", "Yok").to_string(),
        Proxy::Jump(hops) => trf(
            "Jump host → {}",
            "Atlama sunucusu → {}",
            &[&hops.join(" → ")],
        ),
        Proxy::Socks5(addr) => format!("SOCKS5 {addr}"),
        Proxy::Http(addr) => format!("HTTP {addr}"),
        Proxy::Command(cmd) => trf("Custom command: {}", "Özel komut: {}", &[cmd]),
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App, theme: Theme) {
    let Some(entry) = app.current() else {
        frame.render_widget(Block::bordered().border_style(theme.dim()), area);
        return;
    };
    let block = Block::bordered()
        .border_style(theme.dim())
        .title(Span::styled(
            format!(" {} ", entry.patterns.join(" ")),
            theme.strong(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let room = (inner.width as usize).saturating_sub(LABEL_WIDTH);

    let conn = &entry.conn;
    let none = || tr("none", "yok").to_string();
    let mut rows: Vec<(String, String)> = vec![
        ("Alias".into(), entry.patterns.join(" ")),
        (
            "HostName".into(),
            conn.hostname.clone().unwrap_or_else(none),
        ),
        ("User".into(), conn.user.clone().unwrap_or_else(none)),
        (
            "Port".into(),
            match conn.port {
                Some(port) if port != DEFAULT_PORT => port.to_string(),
                _ => format!("{DEFAULT_PORT} {}", tr("(default)", "(varsayılan)")),
            },
        ),
        (
            tr("Identity file", "Anahtar dosyası").into(),
            conn.identity_file.clone().unwrap_or_else(none),
        ),
        ("Proxy".into(), proxy_long(&conn.proxy)),
    ];
    if let Some(description) = &conn.description {
        rows.push((tr("Description", "Açıklama").into(), description.clone()));
    }
    if !conn.tags.is_empty() {
        rows.push((tr("Tags", "Etiketler").into(), conn.tags.join(", ")));
    }
    for (i, (key, value)) in conn.other.iter().enumerate() {
        let label = if i == 0 {
            tr("Other settings", "Diğer ayarlar")
        } else {
            ""
        };
        rows.push((label.into(), format!("{key} {value}")));
    }
    if let Some(source) = &entry.source {
        rows.push((
            tr("Included from", "Dahil edildiği").into(),
            collapse_tilde(&source.to_string_lossy(), &app.ctx.home),
        ));
    }

    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(pad(&label, LABEL_WIDTH), theme.dim()),
                Span::raw(fit(&value, room)),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled(
        tr("Config block", "Config bloğu"),
        theme.dim(),
    ));
    for line in entry.text.lines() {
        let text = line.replace('\t', "    ");
        lines.push(Line::raw(format!(
            "  {}",
            fit(&text, (inner.width as usize).saturating_sub(2))
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

// ------------------------------------------------------------------ form --

/// One line with a text field: the visible part of the value, scrolled so
/// the cursor stays in view. Returns the line's spans and the cursor column.
fn input_view(input: &TextInput, room: usize) -> (String, usize) {
    let chars: Vec<char> = input.value().chars().collect();
    let room = room.max(1);
    let start = (input.cursor() + 1).saturating_sub(room);
    let shown: String = chars.iter().skip(start).take(room).collect();
    (shown, input.cursor() - start)
}

fn draw_form(frame: &mut Frame, area: Rect, app: &App, form: &Form, theme: Theme) {
    let (fields_area, preview_area) = if area.width < NARROW {
        (area, None)
    } else {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Min(0)]).areas(area);
        (left, Some(right))
    };

    let block = Block::bordered()
        .border_style(theme.accent())
        .title(Span::styled(
            format!(" {} ", tr("Connection", "Bağlantı")),
            theme.strong(),
        ));
    let inner = block.inner(fields_area);
    frame.render_widget(block, fields_area);

    let errors = form.errors(&app.store);
    let room = (inner.width as usize).saturating_sub(LABEL_WIDTH + 1);
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor: Option<Position> = None;

    for field in Field::ALL {
        let focused = form.focus == field;
        let error = errors.iter().find(|(f, _)| *f == field).map(|(_, e)| e);
        let label_style = if error.is_some() {
            theme.error().patch(theme.strong())
        } else if focused {
            theme.accent().patch(theme.strong())
        } else {
            theme.dim()
        };
        let marker = if focused { "❯ " } else { "  " };
        let mut spans = vec![
            Span::styled(marker, theme.accent()),
            Span::styled(pad(field.label(), LABEL_WIDTH - 2), label_style),
        ];
        match form.input(field) {
            Some(input) => {
                let placeholder = match field {
                    Field::Port => DEFAULT_PORT.to_string(),
                    Field::User => tr("your current user", "mevcut kullanıcınız").to_string(),
                    Field::IdentityFile => tr("none", "yok").to_string(),
                    _ => String::new(),
                };
                if input.value().is_empty() {
                    spans.push(Span::styled(fit(&placeholder, room), theme.dim()));
                } else if focused {
                    spans.push(Span::raw(input_view(input, room).0));
                } else {
                    spans.push(Span::raw(fit(input.value(), room)));
                }
                if focused && form.overlay.is_none() {
                    let column = if input.value().is_empty() {
                        0
                    } else {
                        input_view(input, room).1
                    };
                    cursor = Some(Position::new(
                        inner.x + LABEL_WIDTH as u16 + column as u16,
                        inner.y + lines.len() as u16,
                    ));
                }
            }
            None => {
                let text = format!(
                    "{} ▾",
                    fit(&proxy_long(&form.proxy), room.saturating_sub(2))
                );
                spans.push(Span::raw(text));
            }
        }
        lines.push(Line::from(spans));
        if let Some(error) = error {
            // The message is what tells the user how to fix the field, so it
            // wraps instead of being cut.
            for part in wrap(error, room) {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(LABEL_WIDTH)),
                    Span::styled(part, theme.error()),
                ]));
            }
        }
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        format!(
            "  {}",
            tr(
                "Empty fields are left out of the config file.",
                "Boş alanlar config dosyasına yazılmaz."
            )
        ),
        theme.dim(),
    ));
    frame.render_widget(Paragraph::new(lines), inner);
    if let Some(position) = cursor
        && position.y < inner.bottom()
        && position.x < inner.right()
    {
        frame.set_cursor_position(position);
    }

    if let Some(area) = preview_area {
        draw_preview(frame, area, app, form, theme);
    }
}

fn change_line(change: Change, text: &str, room: usize, theme: Theme) -> Line<'static> {
    let (sign, style) = match change {
        Change::Added => ("+ ", theme.ok()),
        Change::Removed => ("- ", theme.error()),
        Change::Same => ("  ", theme.dim()),
    };
    let text = text.replace('\t', "    ");
    Line::styled(
        format!("{sign}{}", fit(&text, room.saturating_sub(2))),
        style,
    )
}

fn draw_preview(frame: &mut Frame, area: Rect, app: &App, form: &Form, theme: Theme) {
    let block = Block::bordered()
        .border_style(theme.dim())
        .title(Span::styled(
            format!(" {} ", tr("Preview", "Önizleme")),
            theme.strong(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let room = inner.width as usize;

    let mut lines: Vec<Line> = form
        .preview(&app.store)
        .iter()
        .map(|(change, text)| change_line(*change, text, room, theme))
        .collect();
    if !form.port.is_empty() && parse_port(form.port.text()).is_err() {
        lines.push(Line::default());
        let note = tr(
            "Port is added to the preview once its value is valid.",
            "Port, değeri geçerli olduğunda önizlemeye eklenir.",
        );
        for part in wrap(note, room) {
            lines.push(Line::styled(part, theme.dim()));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------- popups --

fn popup(
    frame: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: String,
    border: Style,
    theme: Theme,
) -> Rect {
    let rect = centered(area, w, h);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(border)
        .title(Span::styled(format!(" {title} "), theme.strong()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    // One column of padding on each side.
    Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    )
}

fn draw_proxy_picker(frame: &mut Frame, area: Rect, picker: &ProxyPicker, theme: Theme) {
    let list_rows = match picker.kind {
        Kind::Jump => picker.hosts.len() + 1,
        Kind::Server => 2,
        _ => 1,
    };
    // tabs, blank, rows, blank, route, writes, error + borders
    let wanted = (list_rows + 6) as u16 + 2;
    let inner = popup(
        frame,
        area,
        74,
        wanted,
        trf("Proxy for {}", "{} için proxy", &[&picker.name]),
        theme.accent(),
        theme,
    );
    let room = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor: Option<(usize, usize)> = None;

    let mut tabs: Vec<Span> = Vec::new();
    for kind in Kind::ALL {
        let style = if kind == picker.kind {
            theme.chosen()
        } else {
            theme.dim()
        };
        tabs.push(Span::styled(format!(" {} ", kind.label()), style));
        tabs.push(Span::raw(" "));
    }
    lines.push(Line::from(tabs));
    lines.push(Line::default());

    match picker.kind {
        Kind::None => lines.push(Line::raw(tr("Connect directly.", "Doğrudan bağlan."))),
        Kind::Jump => {
            // Fit the host list into what is left of the popup.
            let fixed = 7;
            let max_rows = (inner.height as usize).saturating_sub(fixed).max(1);
            let offset = picker
                .cursor
                .min(picker.hosts.len().saturating_sub(1))
                .saturating_sub(max_rows - 1);
            let alias_width = picker
                .hosts
                .iter()
                .map(|h| width(&h.alias))
                .max()
                .unwrap_or(0)
                .clamp(4, 18);
            for (i, host) in picker.hosts.iter().enumerate().skip(offset).take(max_rows) {
                let selected = i == picker.cursor;
                let order = picker.chain.iter().position(|a| a == &host.alias);
                let marker = if selected { "❯ " } else { "  " };
                let detail_room = room.saturating_sub(2 + 4 + alias_width + 2);
                let line = if let Some(reason) = &host.blocked {
                    Line::from(vec![
                        Span::styled(marker, theme.accent()),
                        Span::styled(
                            format!(
                                " -  {}{}",
                                pad(&fit(&host.alias, alias_width), alias_width + 2),
                                fit(reason, detail_room)
                            ),
                            theme.dim(),
                        ),
                    ])
                } else {
                    let (box_text, style) = match order {
                        Some(n) => (
                            format!("[{}] ", n + 1),
                            theme.accent().patch(theme.strong()),
                        ),
                        None => ("[ ] ".to_string(), theme.dim()),
                    };
                    let name_style = if order.is_some() || selected {
                        theme.accent().patch(theme.strong())
                    } else {
                        Style::new()
                    };
                    Line::from(vec![
                        Span::styled(marker, theme.accent()),
                        Span::styled(box_text, style),
                        Span::styled(
                            pad(&fit(&host.alias, alias_width), alias_width + 2),
                            name_style,
                        ),
                        Span::styled(fit(&host.target, detail_room), theme.dim()),
                    ])
                };
                lines.push(line);
            }
            let label = tr("Or type one", "Ya da yazın");
            let on_manual = picker.on_manual();
            let mut spans = vec![
                Span::styled(if on_manual { "❯ " } else { "  " }, theme.accent()),
                Span::styled(pad(label, 14), theme.dim()),
            ];
            let field_room = room.saturating_sub(16);
            if picker.manual.value().is_empty() {
                spans.push(Span::styled("user@host:port", theme.dim()));
                if on_manual {
                    cursor = Some((lines.len(), 16));
                }
            } else {
                let (shown, column) = input_view(&picker.manual, field_room);
                spans.push(Span::raw(shown));
                if on_manual {
                    cursor = Some((lines.len(), 16 + column));
                }
            }
            lines.push(Line::from(spans));
        }
        Kind::Server => {
            let option = |label: &str, on: bool| {
                Span::styled(
                    format!(" {label} "),
                    if on { theme.chosen() } else { theme.dim() },
                )
            };
            lines.push(Line::from(vec![
                Span::styled(pad(tr("Type", "Tür"), 10), theme.dim()),
                option("SOCKS5", !picker.http),
                Span::raw(" "),
                option("HTTP", picker.http),
            ]));
            let mut spans = vec![Span::styled(pad(tr("Address", "Adres"), 10), theme.dim())];
            if picker.address.value().is_empty() {
                spans.push(Span::styled("host:port", theme.dim()));
                cursor = Some((lines.len(), 10));
            } else {
                let (shown, column) = input_view(&picker.address, room.saturating_sub(10));
                spans.push(Span::raw(shown));
                cursor = Some((lines.len(), 10 + column));
            }
            lines.push(Line::from(spans));
        }
        Kind::Custom => {
            let mut spans = vec![Span::styled(pad(tr("Command", "Komut"), 10), theme.dim())];
            if picker.command.value().is_empty() {
                spans.push(Span::styled("ssh -W %h:%p gate", theme.dim()));
                cursor = Some((lines.len(), 10));
            } else {
                let (shown, column) = input_view(&picker.command, room.saturating_sub(10));
                spans.push(Span::raw(shown));
                cursor = Some((lines.len(), 10 + column));
            }
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::default());
    let proxy = picker.proxy();
    let me = tr("this computer", "bu bilgisayar");
    let route = match &proxy {
        Proxy::Jump(hops) => format!("{me} → {} → {}", hops.join(" → "), picker.name),
        Proxy::None => format!("{me} → {}", picker.name),
        _ => format!("{me} → proxy → {}", picker.name),
    };
    let writes = match proxy.directive() {
        Some((key, value)) => format!("{key} {value}"),
        None => tr("nothing", "hiçbir şey").to_string(),
    };
    let info_room = room.saturating_sub(10);
    lines.push(Line::from(vec![
        Span::styled(pad(tr("Route", "Yol"), 10), theme.dim()),
        Span::raw(fit(&route, info_room)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(pad(tr("Writes", "Yazılan"), 10), theme.dim()),
        Span::raw(fit(&writes, info_room)),
    ]));
    if let Some(error) = &picker.error {
        lines.push(Line::styled(fit(error, room), theme.error()));
    }

    frame.render_widget(Paragraph::new(lines), inner);
    if let Some((row, column)) = cursor
        && (row as u16) < inner.height
    {
        let x = (inner.x + column as u16).min(inner.right().saturating_sub(1));
        frame.set_cursor_position(Position::new(x, inner.y + row as u16));
    }
}

fn draw_key_picker(frame: &mut Frame, area: Rect, picker: &KeyPicker, theme: Theme) {
    let inner = popup(
        frame,
        area,
        56,
        picker.options.len() as u16 + 2,
        tr("Identity file", "Anahtar dosyası").to_string(),
        theme.accent(),
        theme,
    );
    let height = inner.height as usize;
    let offset = (picker.cursor + 1).saturating_sub(height.max(1));
    let lines: Vec<Line> = picker
        .options
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, option)| {
            let selected = i == picker.cursor;
            let text = match option {
                Some(path) => path.clone(),
                None => tr("None", "Yok").to_string(),
            };
            Line::from(vec![
                Span::styled(if selected { "❯ " } else { "  " }, theme.accent()),
                Span::styled(
                    fit(&text, (inner.width as usize).saturating_sub(2)),
                    if selected {
                        theme.accent().patch(theme.strong())
                    } else {
                        Style::new()
                    },
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_delete(frame: &mut Frame, area: Rect, app: &App, dialog: &DeleteDialog, theme: Theme) {
    let alias = dialog.entry.alias();
    let block_lines: Vec<&str> = dialog.entry.text.lines().collect();
    let dependents = dialog.dependents.len();
    let extra = if dependents == 0 { 0 } else { dependents + 3 };
    let wanted = (1 + block_lines.len() + extra + 2) as u16 + 2;
    let inner = popup(
        frame,
        area,
        66,
        wanted,
        trf("Remove {}?", "{} silinsin mi?", &[alias]),
        theme.error(),
        theme,
    );
    let room = inner.width as usize;
    let file = match &dialog.entry.source {
        Some(path) => collapse_tilde(&path.to_string_lossy(), &app.ctx.home),
        None => app.store.display_path(),
    };

    let mut lines: Vec<Line> = vec![Line::styled(
        fit(
            &trf(
                "This block will be removed from {}",
                "Bu blok {} dosyasından silinecek",
                &[&file],
            ),
            room,
        ),
        theme.dim(),
    )];
    // Leave room for the dependents and the buttons on a short terminal.
    let block_room = (inner.height as usize).saturating_sub(1 + extra + 2);
    for line in block_lines.iter().take(block_room) {
        lines.push(change_line(Change::Removed, line, room, theme));
    }
    if dependents > 0 {
        let headline = if dependents == 1 {
            trf(
                "1 connection uses {} as its jump host",
                "1 bağlantı {} kaydını atlama sunucusu olarak kullanıyor",
                &[alias],
            )
        } else {
            trf(
                "{} connections use {} as their jump host",
                "{} bağlantı {} kaydını atlama sunucusu olarak kullanıyor",
                &[&dependents.to_string(), alias],
            )
        };
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("! ", theme.warn().patch(theme.strong())),
            Span::styled(fit(&headline, room.saturating_sub(2)), theme.warn()),
        ]));
        for name in &dialog.dependents {
            lines.push(Line::raw(format!(
                "  {}",
                fit(name, room.saturating_sub(2))
            )));
        }
        lines.push(Line::styled(
            format!(
                "  {}",
                fit(
                    tr(
                        "They will not connect until you change their proxy.",
                        "Proxy ayarlarını değiştirene kadar bağlanamazlar.",
                    ),
                    room.saturating_sub(2)
                )
            ),
            theme.dim(),
        ));
    }
    lines.push(Line::default());
    let button = |label: &str, on: bool, style: Style| {
        Span::styled(
            format!(" {label} "),
            if on { theme.chosen() } else { style },
        )
    };
    lines.push(
        Line::from(vec![
            button(tr("Cancel", "Vazgeç"), !dialog.remove_selected, theme.dim()),
            Span::raw("  "),
            button(tr("Remove", "Sil"), dialog.remove_selected, theme.error()),
        ])
        .right_aligned(),
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Asked after Esc on a form with changes.
fn draw_discard(frame: &mut Frame, area: Rect, discard_selected: bool, theme: Theme) {
    let inner = popup(
        frame,
        area,
        52,
        5,
        tr("Discard changes?", "Değişiklikler atılsın mı?").to_string(),
        theme.warn(),
        theme,
    );
    let button = |label: &str, on: bool, style: Style| {
        Span::styled(
            format!(" {label} "),
            if on { theme.chosen() } else { style },
        )
    };
    let lines = vec![
        Line::styled(
            fit(
                tr(
                    "What you typed in this form has not been saved.",
                    "Bu forma yazdıklarınız kaydedilmedi.",
                ),
                inner.width as usize,
            ),
            theme.dim(),
        ),
        Line::default(),
        Line::from(vec![
            button(
                tr("Keep editing", "Düzenlemeye devam"),
                !discard_selected,
                theme.dim(),
            ),
            Span::raw("  "),
            button(tr("Discard", "At"), discard_selected, theme.error()),
        ])
        .right_aligned(),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_help(frame: &mut Frame, area: Rect, theme: Theme) {
    let sections: [(&str, Vec<(&str, &str)>); 3] = [
        (
            tr("List", "Liste"),
            vec![
                (
                    "↑ ↓ / j k",
                    tr("Move through connections", "Bağlantılar arasında gezin"),
                ),
                ("/", tr("Search", "Ara")),
                ("enter", tr("Connect with ssh", "ssh ile bağlan")),
                ("a", tr("Add a connection", "Bağlantı ekle")),
                (
                    "e",
                    tr("Edit the selected connection", "Seçili bağlantıyı düzenle"),
                ),
                (
                    "d",
                    tr("Remove the selected connection", "Seçili bağlantıyı sil"),
                ),
            ],
        ),
        (
            tr("Form", "Form"),
            vec![
                ("tab", tr("Next field", "Sonraki alan")),
                ("shift+tab", tr("Previous field", "Önceki alan")),
                (
                    "enter",
                    tr("Open the list of a field", "Alanın listesini aç"),
                ),
                ("ctrl+s", tr("Save", "Kaydet")),
                (
                    "esc",
                    tr("Close the form or dialog", "Formu veya pencereyi kapat"),
                ),
            ],
        ),
        (
            tr("Anywhere", "Her yerde"),
            vec![
                ("?", tr("Show this help", "Bu yardımı göster")),
                ("q", tr("Quit", "Çık")),
            ],
        ),
    ];
    let rows: usize = sections.iter().map(|(_, keys)| keys.len() + 2).sum();
    let inner = popup(
        frame,
        area,
        62,
        rows as u16 + 2 + 1,
        tr("Keyboard shortcuts", "Klavye kısayolları").to_string(),
        theme.accent(),
        theme,
    );
    let mut lines: Vec<Line> = Vec::new();
    for (title, keys) in sections {
        lines.push(Line::styled(title, theme.dim()));
        for (key, text) in keys {
            lines.push(Line::from(vec![
                Span::styled(pad(key, 12), theme.accent()),
                Span::raw(text),
            ]));
        }
        lines.push(Line::default());
    }
    lines.push(Line::styled(
        fit(
            tr(
                "Patterns are read-only: e and d do nothing on them.",
                "Desenler salt okunurdur: e ve d onlarda çalışmaz.",
            ),
            inner.width as usize,
        ),
        theme.dim(),
    ));
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::{fit, wrap};

    #[test]
    fn fit_cuts_with_an_ellipsis() {
        assert_eq!(fit("short", 10), "short");
        assert_eq!(fit("exactly", 7), "exactly");
        assert_eq!(fit("too long", 5), "too …");
        assert_eq!(fit("üretim", 3), "ür…");
        assert_eq!(fit("x", 0), "");
    }

    #[test]
    fn wrap_breaks_at_spaces() {
        assert_eq!(
            wrap("A connection named staging already exists.", 20),
            ["A connection named", "staging already", "exists."]
        );
        assert_eq!(wrap("fits", 20), ["fits"]);
        assert_eq!(wrap("averyveryverylongword x", 8), ["averyve…", "x"]);
        assert!(wrap("", 10).is_empty());
    }
}
