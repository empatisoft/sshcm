//! `sshcm config`: sshcm's own settings.

use super::Ctx;
use crate::cli::ConfigCmd;
use crate::error::Result;
use crate::settings::{KEYS, Settings};
use crate::style;

pub fn run(ctx: &Ctx, action: Option<ConfigCmd>) -> Result<()> {
    let mut settings = ctx.settings.clone();
    match action {
        None => {
            for key in KEYS {
                println!("{key} = {}", settings.get(key)?);
            }
            println!(
                "{}",
                style::dim(&Settings::path(&ctx.home).display().to_string())
            );
        }
        Some(ConfigCmd::Get { key }) => println!("{}", settings.get(&key)?),
        Some(ConfigCmd::Set { key, value }) => {
            settings.set(&key, &value)?;
            settings.save(&ctx.home)?;
            println!("{} {key} = {}", style::green("✓"), settings.get(&key)?);
        }
    }
    Ok(())
}
