//! Colors of the full-screen interface.
//!
//! Text has to stay readable on dark and on light terminals, and on themes
//! that remap the 16 ANSI colors. "Bright black", the usual choice for
//! secondary text, is close to invisible on many dark themes, so it is not
//! used. Instead the background of the terminal is asked for once at start:
//!
//! - known dark or light background: fixed grays and accents from the
//!   256-color palette, picked for contrast against that background;
//! - unknown: no fixed colors for secondary text at all, only the terminal's
//!   own foreground, dimmed.
//!
//! `NO_COLOR` turns every color off; bold, dim and reverse remain.

use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};

/// What is known about the terminal's background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Dark,
    Light,
    /// Could not be determined: rely on the terminal's own colors.
    #[default]
    Unknown,
}

impl Mode {
    /// Reads the `theme` setting: `dark`, `light`, anything else is `auto`.
    pub fn parse(value: &str) -> Option<Mode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Mode::Dark),
            "light" => Some(Mode::Light),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    color: bool,
    mode: Mode,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::new(Mode::Unknown)
    }
}

impl Theme {
    pub fn new(mode: Mode) -> Theme {
        Theme {
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
            mode,
        }
    }

    /// The theme for this terminal. `setting` is the `theme` setting (or
    /// `SSHCM_THEME`): `dark` and `light` are taken as given, `auto` asks
    /// the terminal. Must run before the screen is taken over.
    pub fn detect(setting: &str) -> Theme {
        let mode = Mode::parse(setting).unwrap_or_else(detect_mode);
        Theme::new(mode)
    }

    fn pick(self, dark: Color, light: Color, unknown: Option<Color>) -> Style {
        if !self.color {
            return Style::new();
        }
        match (self.mode, unknown) {
            (Mode::Dark, _) => Style::new().fg(dark),
            (Mode::Light, _) => Style::new().fg(light),
            (Mode::Unknown, Some(color)) => Style::new().fg(color),
            (Mode::Unknown, None) => Style::new(),
        }
    }

    /// Secondary text: labels, hints, placeholders.
    pub fn dim(self) -> Style {
        match (self.color, self.mode) {
            // #9e9e9e on dark and #626262 on light: both clear of 4.5:1.
            (true, Mode::Dark) => Style::new().fg(Color::Indexed(247)),
            (true, Mode::Light) => Style::new().fg(Color::Indexed(241)),
            // The terminal's own text color, dimmed: readable wherever the
            // normal text is.
            _ => Style::new().add_modifier(Modifier::DIM),
        }
    }

    /// Borders of panels that do not have the focus.
    pub fn border(self) -> Style {
        match (self.color, self.mode) {
            (true, Mode::Dark) => Style::new().fg(Color::Indexed(243)),
            (true, Mode::Light) => Style::new().fg(Color::Indexed(246)),
            _ => Style::new().add_modifier(Modifier::DIM),
        }
    }

    /// The highlight color: selection, focus, key names.
    pub fn accent(self) -> Style {
        // Cyan is unreadable on white, so light terminals get a dark blue.
        self.pick(Color::Indexed(80), Color::Indexed(25), Some(Color::Cyan))
    }

    pub fn ok(self) -> Style {
        self.pick(Color::Indexed(114), Color::Indexed(28), Some(Color::Green))
    }

    pub fn warn(self) -> Style {
        // Yellow on white is the classic unreadable pair.
        self.pick(
            Color::Indexed(179),
            Color::Indexed(130),
            Some(Color::Yellow),
        )
    }

    pub fn error(self) -> Style {
        self.pick(Color::Indexed(210), Color::Indexed(160), Some(Color::Red))
    }

    pub fn strong(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    /// Panel titles: the normal text color in bold, whatever the border is.
    pub fn title(self) -> Style {
        Style::new()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM)
    }

    /// A selected button or tab.
    pub fn chosen(self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }

    /// Background of the selected row in a list.
    pub fn selected_row(self) -> Style {
        match (self.color, self.mode) {
            (true, Mode::Dark) => Style::new().bg(Color::Indexed(237)),
            (true, Mode::Light) => Style::new().bg(Color::Indexed(254)),
            _ => Style::new(),
        }
    }
}

/// Asks the terminal for its background color; falls back to `COLORFGBG`.
fn detect_mode() -> Mode {
    // The fixed colors come from the 256-color palette. A terminal that
    // does not announce it gets the relative styles instead.
    let term = std::env::var("TERM").unwrap_or_default();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    if !term.contains("256color") && colorterm.is_empty() {
        return Mode::Unknown;
    }

    let mut options = terminal_colorsaurus::QueryOptions::default();
    // Terminals that answer do so within a few milliseconds; the others
    // must not hold up the start.
    options.timeout = Duration::from_millis(200);
    match terminal_colorsaurus::theme_mode(options) {
        Ok(terminal_colorsaurus::ThemeMode::Dark) => Mode::Dark,
        Ok(terminal_colorsaurus::ThemeMode::Light) => Mode::Light,
        Err(_) => from_colorfgbg(&std::env::var("COLORFGBG").unwrap_or_default()),
    }
}

/// `COLORFGBG` is `fg;bg` (sometimes `fg;x;bg`) in ANSI color numbers.
fn from_colorfgbg(value: &str) -> Mode {
    match value
        .rsplit(';')
        .next()
        .and_then(|bg| bg.parse::<u8>().ok())
    {
        Some(0..=6 | 8) => Mode::Dark,
        Some(7 | 9..=15) => Mode::Light,
        _ => Mode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorfgbg_is_read_from_its_last_field() {
        assert_eq!(from_colorfgbg("15;0"), Mode::Dark);
        assert_eq!(from_colorfgbg("0;15"), Mode::Light);
        assert_eq!(from_colorfgbg("0;default;15"), Mode::Light);
        assert_eq!(from_colorfgbg("7;8"), Mode::Dark);
        assert_eq!(from_colorfgbg(""), Mode::Unknown);
        assert_eq!(from_colorfgbg("0;default"), Mode::Unknown);
    }

    #[test]
    fn setting_overrides_detection() {
        assert_eq!(Mode::parse("dark"), Some(Mode::Dark));
        assert_eq!(Mode::parse(" Light "), Some(Mode::Light));
        assert_eq!(Mode::parse("auto"), None);
    }

    /// Bright black (ANSI 8) is what made secondary text vanish on dark
    /// themes; no style may fall back to it.
    #[test]
    fn no_style_uses_bright_black() {
        for mode in [Mode::Dark, Mode::Light, Mode::Unknown] {
            let theme = Theme { color: true, mode };
            for style in [
                theme.dim(),
                theme.border(),
                theme.accent(),
                theme.ok(),
                theme.warn(),
                theme.error(),
                theme.title(),
            ] {
                assert_ne!(style.fg, Some(Color::DarkGray), "{mode:?}");
                assert_ne!(style.fg, Some(Color::Indexed(8)), "{mode:?}");
                assert_ne!(style.fg, Some(Color::Black), "{mode:?}");
            }
        }
    }

    #[test]
    fn secondary_text_has_enough_contrast_on_known_backgrounds() {
        // Gray level of xterm palette entries 232..=255.
        let gray = |index: u8| 8.0 + 10.0 * f64::from(index - 232);
        let luminance = |level: f64| {
            let c = level / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let contrast = |a: f64, b: f64| {
            let (hi, lo) = if a > b { (a, b) } else { (b, a) };
            (luminance(hi) + 0.05) / (luminance(lo) + 0.05)
        };
        let level = |style: Style| match style.fg {
            Some(Color::Indexed(index)) => gray(index),
            other => panic!("expected a palette gray, got {other:?}"),
        };

        let dark = Theme {
            color: true,
            mode: Mode::Dark,
        };
        let light = Theme {
            color: true,
            mode: Mode::Light,
        };
        // Against a typical dark (#1e2124) and a white background.
        assert!(contrast(level(dark.dim()), 33.0) >= 4.5);
        assert!(contrast(level(light.dim()), 255.0) >= 4.5);
        assert!(contrast(level(dark.border()), 33.0) >= 3.0);
        assert!(contrast(level(light.border()), 255.0) >= 3.0);
    }

    #[test]
    fn unknown_background_uses_only_relative_styles_for_secondary_text() {
        let theme = Theme {
            color: true,
            mode: Mode::Unknown,
        };
        assert_eq!(theme.dim().fg, None);
        assert_eq!(theme.border().fg, None);
        assert_eq!(theme.selected_row().bg, None);
        let plain = Theme {
            color: false,
            mode: Mode::Dark,
        };
        for style in [
            plain.dim(),
            plain.accent(),
            plain.error(),
            plain.selected_row(),
        ] {
            assert_eq!((style.fg, style.bg), (None, None));
        }
    }
}
