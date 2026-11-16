//! `sshcm connect`: hand over to the system ssh.

use sshcm_core::model::validate_alias;

use super::{Ctx, pick};
use crate::error::Result;
use crate::exec;
use crate::i18n::tr;

pub fn run(ctx: &Ctx, alias: Option<String>, ssh_args: &[String]) -> Result<()> {
    // Refuse names that ssh could read as an option before looking them up.
    if let Some(alias) = &alias {
        validate_alias(alias)?;
    }
    let store = ctx.load()?;
    let entry = pick(
        &store,
        alias,
        false,
        tr(
            "Which connection do you want to open?",
            "Hangi bağlantıyı açmak istiyorsunuz?",
        ),
    )?;
    validate_alias(entry.alias())?;
    Err(exec::ssh(ctx.config.as_deref(), entry.alias(), ssh_args))
}
