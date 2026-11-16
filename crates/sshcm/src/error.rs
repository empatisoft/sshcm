//! The one error type of the command line, and its exit codes.

use inquire::InquireError;
use sshcm_core::{DocError, Error, ValidationError};

use crate::i18n::{self, tr};

#[derive(Debug)]
pub enum AppError {
    /// The user pressed Esc in a prompt.
    Cancelled,
    /// The user pressed Ctrl+C.
    Interrupted,
    /// Anything else; the message is shown as is.
    Message(String),
}

impl AppError {
    pub fn msg(text: impl Into<String>) -> AppError {
        AppError::Message(text.into())
    }

    /// `1` for errors, `130` when the user cancelled.
    pub fn exit_code(&self) -> u8 {
        match self {
            AppError::Cancelled | AppError::Interrupted => 130,
            AppError::Message(_) => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

impl From<InquireError> for AppError {
    fn from(err: InquireError) -> AppError {
        match err {
            InquireError::OperationCanceled => AppError::Cancelled,
            InquireError::OperationInterrupted => AppError::Interrupted,
            InquireError::NotTTY => AppError::msg(tr(
                "This needs an interactive terminal. In scripts, pass the values as flags and add --yes.",
                "Bunun için etkileşimli bir terminal gerekir. Betiklerde değerleri bayraklarla verin ve --yes ekleyin.",
            )),
            other => AppError::Message(other.to_string()),
        }
    }
}

impl From<Error> for AppError {
    fn from(err: Error) -> AppError {
        AppError::Message(i18n::store_error(&err))
    }
}

impl From<DocError> for AppError {
    fn from(err: DocError) -> AppError {
        AppError::Message(i18n::doc_error(&err))
    }
}

impl From<ValidationError> for AppError {
    fn from(err: ValidationError) -> AppError {
        AppError::Message(i18n::validation(&err))
    }
}
