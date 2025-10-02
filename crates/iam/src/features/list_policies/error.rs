// crates/iam/src/features/list_policies/error.rs

use thiserror::Error;

/// Specific errors for the list_policies feature
#[derive(Debug, Error)]
pub enum ListPoliciesError {
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),

    #[error("Invalid pagination: {0}")]
    InvalidPagination(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl ListPoliciesError {
    /// Create an invalid filter error
    pub fn invalid_filter(message: impl Into<String>) -> Self {
        Self::InvalidFilter(message.into())
    }

    /// Create an invalid pagination error
    pub fn invalid_pagination(message: impl Into<String>) -> Self {
        Self::InvalidPagination(message.into())
    }

    /// Create a database error
    pub fn database_error(message: impl Into<String>) -> Self {
        Self::DatabaseError(message.into())
    }
}

impl From<ListPoliciesError> for crate::infrastructure::errors::IamError {
    fn from(error: ListPoliciesError) -> Self {
        match error {
            ListPoliciesError::InvalidFilter(msg) => Self::InvalidInput(msg),
            ListPoliciesError::InvalidPagination(msg) => Self::InvalidInput(msg),
            ListPoliciesError::DatabaseError(msg) => Self::DatabaseError(msg),
            ListPoliciesError::InternalError(msg) => Self::InternalError(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_policies_error_display() {
        let error = ListPoliciesError::InvalidFilter("Test filter error".to_string());
        assert_eq!(error.to_string(), "Invalid filter: Test filter error");

        let error = ListPoliciesError::DatabaseError("DB error".to_string());
        assert_eq!(error.to_string(), "Database error: DB error");
    }

    #[test]
    fn test_list_policies_error_convenience_methods() {
        let error = ListPoliciesError::invalid_filter("test filter");
        assert!(matches!(error, ListPoliciesError::InvalidFilter(_)));

        let error = ListPoliciesError::invalid_pagination("test pagination");
        assert!(matches!(error, ListPoliciesError::InvalidPagination(_)));
    }
}
