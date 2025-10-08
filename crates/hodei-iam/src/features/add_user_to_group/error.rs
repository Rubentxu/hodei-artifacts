use thiserror::Error;

/// Errors that can occur when adding a user to a group
#[derive(Debug, Error)]
pub enum AddUserToGroupError {
    #[error("Invalid user HRN: {0}")]
    InvalidUserHrn(String),

    #[error("Invalid group HRN: {0}")]
    InvalidGroupHrn(String),

    #[error("Group not found: {0}")]
    GroupNotFound(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Failed to save user: {0}")]
    PersistenceError(String),
}