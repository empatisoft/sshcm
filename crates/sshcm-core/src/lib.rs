#![doc = include_str!("../README.md")]
//!
//! This crate is the only code that reads or writes the config file. The
//! command line in the `sshcm` crate collects input and shows results.

pub mod diff;
pub mod document;
pub mod model;
pub mod parser;
pub mod store;

pub use diff::{Change, diff_lines};
pub use document::{DocError, Document, Entry, dependents, render_block_text};
pub use model::{Connection, DEFAULT_PORT, Known, Proxy, ValidationError};
pub use store::{Error, SaveOptions, SaveReport, Store, Verification};
