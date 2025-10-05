use thiserror::Error;
use crate::shared::application::ports::PolicyStorageError;

#[derive(Debug, Error)]
pub enum CreateScpError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
    #[error("Invalid SCP content")]
    InvalidScpContent,
    #[error("SCP already exists")]
    ScpAlreadyExists,
}

#[derive(Debug, Error)]
pub enum DeleteScpError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
    #[error("SCP not found")]
    ScpNotFound,
}

#[derive(Debug, Error)]
pub enum UpdateScpError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
    #[error("SCP not found")]
    ScpNotFound,
    #[error("Invalid SCP content")]
    InvalidScpContent,
}

#[derive(Debug, Error)]
pub enum GetScpError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
    #[error("SCP not found")]
    ScpNotFound,
}

#[derive(Debug, Error)]
pub enum ListScpsError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
}
