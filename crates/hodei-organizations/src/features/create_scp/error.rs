use crate::shared::application::ports::scp_repository::ScpRepositoryError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateScpError {
    #[error("SCP repository error: {0}")]
    ScpRepositoryError(#[from] ScpRepositoryError),
    #[error("Invalid SCP name")]
    InvalidScpName,
    #[error("Invalid SCP document")]
    InvalidScpDocument,
    #[error("Transaction error: {0}")]
    TransactionError(String),
}
