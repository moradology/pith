//! Error types for Pith.

use std::path::PathBuf;

use crate::codemap::CodemapError;
use crate::filter::FilterError;
use crate::output::OutputError;
use crate::walker::WalkError;

/// Top-level error type for Pith operations.
#[derive(Debug, thiserror::Error)]
pub enum PithError {
    #[error("path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),

    #[error("no supported files found in {0}")]
    NoFilesFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("walk error: {0}")]
    Walk(#[from] WalkError),

    #[error("filter error: {0}")]
    Filter(#[from] FilterError),

    #[error("codemap error: {0}")]
    Codemap(#[from] CodemapError),

    #[error("output error: {0}")]
    Output(#[from] OutputError),
}

/// Map an error to its exit code.
pub fn exit_code(error: &PithError) -> i32 {
    match error {
        PithError::PathNotFound(_) => 3,
        PithError::PermissionDenied(_) => 4,
        PithError::NoFilesFound(_) => 5,
        PithError::Io(_) => 1,
        PithError::Walk(_) => 2,
        PithError::Filter(_) => 1,
        PithError::Codemap(_) => 1,
        PithError::Output(_) => 1,
    }
}
