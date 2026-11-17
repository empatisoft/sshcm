//! Interactive prompts. This layer is deliberately thin: it asks, the core
//! validates. Prompts are not covered by automated tests.

pub mod form;
pub mod keys;

use std::fmt;

use inquire::ui::{Color, RenderConfig, Styled};
use inquire::validator::Validation;
use inquire::{Confirm, Select, Text};
use sshcm_core::{Entry, ValidationError};

use crate::error::Result;
use crate::i18n::{self, tr};
use crate::style::pad;

/// Sets the look of every prompt. inquire drops colors by itself when
/// `NO_COLOR` is set.
pub fn init() {
    let mut config = RenderConfig::default();
    let plain = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let styled = |text: &'static str, color: Color| {
        if plain {
            Styled::new(text)
        } else {
            Styled::new(text).with_fg(color)
        }
    };
    // The plain ANSI colors, not their bright variants: terminal themes
    // keep these readable on their own background, light or dark.
    config.prompt_prefix = styled("?", Color::DarkCyan);
    config.answered_prompt_prefix = styled(">", Color::DarkGreen);
    config.highlighted_option_prefix = styled("❯", Color::DarkCyan);
    inquire::set_global_render_config(config);
}

/// An option in a list: what is shown, what is echoed once picked, and the
/// value behind it.
#[derive(Debug, Clone)]
pub struct Choice<T> {
    pub label: String,
    pub short: String,
    pub value: T,
}

impl<T> Choice<T> {
    /// An option shown as `name  description`, echoed as `name`.
    pub fn described(name: &str, width: usize, description: &str, value: T) -> Choice<T> {
        Choice {
            label: format!("{}{description}", pad(name, width))
                .trim_end()
                .to_string(),
            short: name.to_string(),
            value,
        }
    }

    pub fn plain(name: &str, value: T) -> Choice<T> {
        Choice {
            label: name.to_string(),
            short: name.to_string(),
            value,
        }
    }
}

impl<T> fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn select_help() -> &'static str {
    tr(
        "↑↓ move · enter select · type to filter",
        "↑↓ gezin · enter seç · süzmek için yazın",
    )
}

/// Shows a list and returns the value of the picked option.
pub fn select<T>(message: &str, choices: Vec<Choice<T>>, cursor: usize) -> Result<T> {
    let cursor = cursor.min(choices.len().saturating_sub(1));
    let picked = Select::new(message, choices)
        .with_starting_cursor(cursor)
        .with_help_message(select_help())
        .with_page_size(12)
        .with_formatter(&|option| option.value.short.clone())
        .prompt()?;
    Ok(picked.value)
}

pub fn confirm(message: &str, default: bool) -> Result<bool> {
    Ok(Confirm::new(message).with_default(default).prompt()?)
}

/// Asks for one line of text that must pass `check`.
pub fn ask_text(
    message: &str,
    check: impl Fn(&str) -> std::result::Result<(), ValidationError> + Clone + 'static,
) -> Result<String> {
    let answer = Text::new(message)
        .with_validator(move |input: &str| {
            Ok(match check(input.trim()) {
                Ok(()) => Validation::Valid,
                Err(err) => Validation::Invalid(i18n::validation(&err).into()),
            })
        })
        .prompt()?;
    Ok(answer.trim().to_string())
}

/// Lets the user pick one connection from a searchable list.
pub fn select_entry(message: &str, entries: Vec<Entry>) -> Result<Entry> {
    let width = entries
        .iter()
        .map(|e| e.alias().chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let choices = entries
        .into_iter()
        .map(|entry| {
            let alias = entry.alias().to_string();
            let mut detail = entry.conn.target();
            let via = crate::commands::proxy_short(&entry.conn.proxy);
            if !via.is_empty() {
                detail.push_str(&format!("  {via}"));
            }
            Choice::described(&alias, width, &detail, entry)
        })
        .collect();
    select(message, choices, 0)
}
