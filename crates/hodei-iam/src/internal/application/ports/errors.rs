//! Application-level errors for hodei-iam
//!
//! This module defines standardized error types for all repository operations
//! using thiserror for better error handling and type safety.

use kernel::Hrn;
use thiserror::Error;

/// Errors that can occur during User repository operations
#[derive(Debug, Error, Clone)]
pub enum UserRepositoryError {
    #[error("User not found: {0}")]
    NotFound(Hrn),

    #[error("User already exists: {0}")]
    AlreadyExists(Hrn),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Connection pool exhausted")]
    ConnectionPoolExhausted,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    #[error("Internal repository error: {0}")]
    InternalError(String),
}

/// Errors that can occur during Group repository operations
#[derive(Debug, Error, Clone)]
pub enum GroupRepositoryError {
    #[error("Group not found: {0}")]
    NotFound(Hrn),

    #[error("Group already exists: {0}")]
    AlreadyExists(Hrn),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Connection pool exhausted")]
    ConnectionPoolExhausted,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    #[error("User not found in group: user={user_hrn}, group={group_hrn}")]
    UserNotInGroup { user_hrn: Hrn, group_hrn: Hrn },

    #[error("Internal repository error: {0}")]
    InternalError(String),
}

/// Errors that can occur during Policy repository operations
#[allow(dead_code)]
#[derive(Debug, Error, Clone)]
pub enum PolicyRepositoryError {
    #[error("Policy not found: {0}")]
    NotFound(Hrn),

    #[error("Policy already exists: {0}")]
    AlreadyExists(Hrn),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Connection pool exhausted")]
    ConnectionPoolExhausted,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    #[error("Invalid policy format: {0}")]
    InvalidPolicyFormat(String),

    #[error("Policy parsing error: {0}")]
    PolicyParsingError(String),

    #[error("Internal repository error: {0}")]
    InternalError(String),
}

/// Conversion from anyhow::Error to UserRepositoryError
///
/// This is provided for gradual migration from anyhow to typed errors.
/// New code should use specific error variants directly.
impl From<anyhow::Error> for UserRepositoryError {
    fn from(err: anyhow::Error) -> Self {
        UserRepositoryError::InternalError(err.to_string())
    }
}

/// Conversion from anyhow::Error to GroupRepositoryError
///
/// This is provided for gradual migration from anyhow to typed errors.
/// New code should use specific error variants directly.
impl From<anyhow::Error> for GroupRepositoryError {
    fn from(err: anyhow::Error) -> Self {
        GroupRepositoryError::InternalError(err.to_string())
    }
}

/// Conversion from anyhow::Error to PolicyRepositoryError
///
/// This is provided for gradual migration from anyhow to typed errors.
/// New code should use specific error variants directly.
impl From<anyhow::Error> for PolicyRepositoryError {
    fn from(err: anyhow::Error) -> Self {
        PolicyRepositoryError::InternalError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_repository_error_display() {
        let hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test").unwrap();
        let err = UserRepositoryError::NotFound(hrn.clone());
        assert!(err.to_string().contains("User not found"));
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_group_repository_error_display() {
        let hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:group/admins").unwrap();
        let err = GroupRepositoryError::AlreadyExists(hrn.clone());
        assert!(err.to_string().contains("Group already exists"));
        assert!(err.to_string().contains("admins"));
    }

    #[test]
    fn test_policy_repository_error_display() {
        let hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:policy/admin-policy").unwrap();
        let err = PolicyRepositoryError::NotFound(hrn.clone());
        assert!(err.to_string().contains("Policy not found"));
        assert!(err.to_string().contains("admin-policy"));
    }

    #[test]
    fn test_user_repository_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Something went wrong");
        let repo_err: UserRepositoryError = anyhow_err.into();
        assert!(matches!(repo_err, UserRepositoryError::InternalError(_)));
        assert!(repo_err.to_string().contains("Something went wrong"));
    }

    #[test]
    fn test_group_repository_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Database connection failed");
        let repo_err: GroupRepositoryError = anyhow_err.into();
        assert!(matches!(repo_err, GroupRepositoryError::InternalError(_)));
        assert!(repo_err.to_string().contains("Database connection failed"));
    }

    #[test]
    fn test_policy_repository_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Serialization failed");
        let repo_err: PolicyRepositoryError = anyhow_err.into();
        assert!(matches!(repo_err, PolicyRepositoryError::InternalError(_)));
        assert!(repo_err.to_string().contains("Serialization failed"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UserRepositoryError>();
        assert_send_sync::<GroupRepositoryError>();
        assert_send_sync::<PolicyRepositoryError>();
    }

    #[test]
    fn test_error_is_cloneable() {
        let hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/test").unwrap();
        let err = UserRepositoryError::NotFound(hrn);
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }

    #[test]
    fn test_constraint_violation_errors() {
        let err = UserRepositoryError::ConstraintViolation("Unique constraint failed".to_string());
        assert!(err.to_string().contains("Constraint violation"));
        assert!(err.to_string().contains("Unique constraint failed"));
    }

    #[test]
    fn test_serialization_errors() {
        let err = GroupRepositoryError::SerializationError("Invalid JSON".to_string());
        assert!(err.to_string().contains("Serialization error"));
        assert!(err.to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_user_not_in_group_error() {
        let user_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:user/john").unwrap();
        let group_hrn = Hrn::from_string("hrn:hodei:iam:us-east-1:default:group/admins").unwrap();
        let err = GroupRepositoryError::UserNotInGroup {
            user_hrn: user_hrn.clone(),
            group_hrn: group_hrn.clone(),
        };
        let msg = err.to_string();
        assert!(msg.contains("User not found in group"));
        assert!(msg.contains("john"));
        assert!(msg.contains("admins"));
    }
}
