//! Ports (interfaces) for the create_policy feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (SOLID), each port is specific
//! to this feature's needs.

use crate::features::create_policy::dto::{
    CreatePolicyCommand, DeletePolicyCommand, GetPolicyQuery, ListPoliciesQuery,
    UpdatePolicyCommand,
};
use crate::features::create_policy::error::{
    CreatePolicyError, DeletePolicyError, GetPolicyError, ListPoliciesError, UpdatePolicyError,
};
use async_trait::async_trait;
use kernel::Hrn;
use policies::shared::domain::Policy;

/// Port for validating IAM policy content
///
/// This port abstracts policy validation, delegating to the policies crate
/// without creating a direct dependency on Cedar types.
///
/// # Segregation
/// This port is segregated specifically for policy validation and does not
/// include persistence or evaluation concerns.
#[async_trait]
pub trait PolicyValidator: Send + Sync {
    /// Validate a policy content string
    ///
    /// # Arguments
    /// * `policy_content` - The Cedar policy text to validate
    ///
    /// # Returns
    /// A validation result containing validity status and any errors/warnings
    async fn validate_policy(
        &self,
        policy_content: &str,
    ) -> Result<ValidationResult, PolicyValidationError>;
}

/// Result of policy validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

/// A validation error with optional location information
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// A validation warning
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: String,
}

/// Errors that can occur during policy validation
#[derive(Debug, thiserror::Error)]
pub enum PolicyValidationError {
    #[error("validation service error: {0}")]
    ServiceError(String),
}

/// Port for persisting IAM policies
///
/// This port defines CRUD operations for IAM policy management.
/// Following ISP (Interface Segregation Principle), this port only includes
/// operations needed by the policy management features.
#[async_trait]
pub trait PolicyPersister: Send + Sync {
    async fn create_policy(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<Policy, CreatePolicyError>;
    async fn delete_policy(&self, command: DeletePolicyCommand) -> Result<(), DeletePolicyError>;
    async fn update_policy(
        &self,
        command: UpdatePolicyCommand,
    ) -> Result<Policy, UpdatePolicyError>;
    async fn get_policy(&self, query: GetPolicyQuery) -> Result<Policy, GetPolicyError>;
    async fn list_policies(
        &self,
        query: ListPoliciesQuery,
    ) -> Result<Vec<Policy>, ListPoliciesError>;
}
