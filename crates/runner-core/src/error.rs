//! Aggregate error type for `runner-core`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Signature(#[from] crate::jobspec::SignatureError),
}

pub type Result<T> = std::result::Result<T, CoreError>;
