//! Error types for the create_policy feature
//!
//! This module defines all error types that can occur during IAM policy
//! management operations.

use crate::shared::application::ports::PolicyStorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreatePolicyError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),

    #[error("Invalid policy content: {0}")]
    InvalidPolicyContent(String),

    #[error("Policy validation failed: {0}")]
    ValidationFailed(String),

    #[error("Policy already exists")]
    PolicyAlreadyExists,
}

#[derive(Debug, Error)]
pub enum DeletePolicyError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),

    #[error("Policy not found")]
    PolicyNotFound,
}

#[derive(Debug, Error)]
pub enum UpdatePolicyError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),

    #[error("Policy not found")]
    PolicyNotFound,

    #[error("Invalid policy content: {0}")]
    InvalidPolicyContent(String),

    #[error("Policy validation failed: {0}")]
    ValidationFailed(String),
}

#[derive(Debug, Error)]
pub enum GetPolicyError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),

    #[error("Policy not found")]
    PolicyNotFound,
}

#[derive(Debug, Error)]
pub enum ListPoliciesError {
    #[error("Policy storage error: {0}")]
    StorageError(#[from] PolicyStorageError),
}
