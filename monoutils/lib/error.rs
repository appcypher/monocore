//! `monoutils::error` is a module containing error utilities for the monocore project.

use thiserror::Error;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// The result of a monoutils-related operation.
pub type MonoutilsResult<T> = Result<T, MonoutilsError>;

/// An error that occurred during a file system operation.
#[derive(pretty_error_debug::Debug, Error)]
pub enum MonoutilsError {
    /// An error that occurred when validating paths
    #[error("path validation error: {0}")]
    PathValidation(String),

    /// An error that occurred when performing an IO operation
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),    
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Creates an `Ok` `MonoutilsResult`.
#[allow(non_snake_case)]
pub fn Ok<T>(value: T) -> MonoutilsResult<T> {
    Result::Ok(value)
}
