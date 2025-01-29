//! `monoutils` is a library containing general utilities for the monocore project.

#![warn(missing_docs)]
#![allow(clippy::module_inception)]

pub mod config;
pub mod error;
pub mod log;
pub mod path;
pub mod seekable;
pub mod runtime;

//--------------------------------------------------------------------------------------------------
// Exports
//--------------------------------------------------------------------------------------------------

pub use config::*;
pub use error::*;
pub use log::*;
pub use path::*;
pub use seekable::*;
pub use runtime::*;
