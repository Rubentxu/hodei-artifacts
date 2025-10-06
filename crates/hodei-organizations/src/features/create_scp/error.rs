use thiserror::Error;

/// Error type for SCP creation operations
#[derive(Debug, Error)]
pub enum CreateScpError {
    #[error("Invalid SCP content: {0}")]
    InvalidScpContent(String),
    #[error("SCP already exists with HRN: {0}")]
    ScpAlreadyExists(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Error type for SCP deletion operations
#[derive(Debug, Error)]
pub enum DeleteScpError {
    #[error("SCP not found with HRN: {0}")]
    ScpNotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("SCP is currently attached and cannot be deleted")]
    ScpAttached,
}

/// Error type for SCP update operations
#[derive(Debug, Error)]
pub enum UpdateScpError {
    #[error("SCP not found with HRN: {0}")]
    ScpNotFound(String),
    #[error("Invalid SCP content: {0}")]
    InvalidScpContent(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("No updates provided")]
    NoUpdatesProvided,
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Error type for SCP retrieval operations
#[derive(Debug, Error)]
pub enum GetScpError {
    #[error("SCP not found with HRN: {0}")]
    ScpNotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),
}

/// Error type for SCP listing operations
#[derive(Debug, Error)]
pub enum ListScpsError {
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid pagination parameters: {0}")]
    InvalidPagination(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_scp_error_display() {
        let err = CreateScpError::InvalidScpContent("missing policy".to_string());
        assert_eq!(err.to_string(), "Invalid SCP content: missing policy");

        let err = CreateScpError::ScpAlreadyExists("hrn:aws:org:scp-123".to_string());
        assert_eq!(
            err.to_string(),
            "SCP already exists with HRN: hrn:aws:org:scp-123"
        );
    }

    #[test]
    fn delete_scp_error_display() {
        let err = DeleteScpError::ScpNotFound("hrn:aws:org:scp-123".to_string());
        assert_eq!(
            err.to_string(),
            "SCP not found with HRN: hrn:aws:org:scp-123"
        );

        let err = DeleteScpError::ScpAttached;
        assert_eq!(
            err.to_string(),
            "SCP is currently attached and cannot be deleted"
        );
    }

    #[test]
    fn update_scp_error_display() {
        let err = UpdateScpError::NoUpdatesProvided;
        assert_eq!(err.to_string(), "No updates provided");
    }

    #[test]
    fn get_scp_error_display() {
        let err = GetScpError::InvalidHrn("invalid format".to_string());
        assert_eq!(err.to_string(), "Invalid HRN format: invalid format");
    }

    #[test]
    fn list_scps_error_display() {
        let err = ListScpsError::InvalidPagination("limit too large".to_string());
        assert_eq!(
            err.to_string(),
            "Invalid pagination parameters: limit too large"
        );
    }
}
