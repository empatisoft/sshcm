//! The full-screen interface shown when sshcm runs without a subcommand.
//!
//! `app`, `form` and `picker` hold state and react to keys; `ui` draws that
//! state. Only this module talks to the real terminal.

mod app;
mod form;
mod input;
mod picker;
mod theme;
mod ui;

#[cfg(test)]
mod tests;

use std::io::IsTerminal;

use inquire::InquireError;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use self::app::{App, Effect};
use crate::commands::Ctx;
use crate::error::{AppError, Result};
use crate::exec;

pub fn run(ctx: &Ctx) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(InquireError::NotTTY.into());
    }
    let mut app = App::new(ctx.clone())?;
    // Ask the terminal for its background while it is still in its normal
    // state: the colors are picked for a dark or a light screen.
    let setting = std::env::var("SSHCM_THEME").unwrap_or_else(|_| ctx.settings.theme.clone());
    app.theme = theme::Theme::detect(&setting);

    // `init` switches to the alternate screen and installs a panic hook that
    // restores the terminal.
    let mut terminal = ratatui::init();
    let effect = event_loop(&mut terminal, &mut app);
    ratatui::restore();

    match effect? {
        Effect::Connect(alias) => Err(exec::ssh(ctx.config.as_deref(), &alias, &[])),
        Effect::Interrupt => Err(AppError::Interrupted),
        Effect::Quit | Effect::None => Ok(()),
    }
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<Effect> {
    let io = |err: std::io::Error| AppError::Message(err.to_string());
    loop {
        terminal.draw(|frame| ui::draw(frame, app)).map_err(io)?;
        // Resize and other events only need the redraw above.
        if let Event::Key(key) = event::read().map_err(io)?
            && key.kind == KeyEventKind::Press
        {
            match app.on_key(key) {
                Effect::None => {}
                effect => return Ok(effect),
            }
        }
    }
}
