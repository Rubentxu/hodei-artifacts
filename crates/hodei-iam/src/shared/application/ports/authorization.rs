//! Authorization ports for IAM
//!
//! This module defines the interfaces (ports) that the IAM bounded context
//! exposes for authorization purposes. These are consumed by the authorizer
//! to retrieve identity-based policies.

use async_trait::async_trait;
use cedar_policy::PolicySet;
use policies::shared::domain::hrn::Hrn;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur when providing IAM policies
#[derive(Debug, Error)]
pub enum IamPolicyProviderError {
    #[error("Failed to retrieve policies for principal {principal}: {reason}")]
    ProviderError { principal: String, reason: String },

    #[error("Principal not found: {0}")]
    PrincipalNotFound(String),

    #[error("Invalid policy format for policy {policy_hrn}: {reason}")]
    InvalidPolicyFormat { policy_hrn: String, reason: String },

    #[error("Repository error: {0}")]
    RepositoryError(String),
}

/// Port for retrieving IAM identity-based policies
///
/// This trait defines the contract for obtaining the effective policy set
/// for a given principal (user). The implementation will:
/// 1. Resolve the user from the principal HRN
/// 2. Collect all groups the user belongs to
/// 3. Gather all policies attached to those groups (and potentially the user directly)
/// 4. Return a consolidated Cedar PolicySet
#[async_trait]
pub trait IamPolicyProvider: Send + Sync {
    /// Get all identity-based policies applicable to a principal
    ///
    /// # Arguments
    /// * `principal_hrn` - The HRN of the principal (user) to get policies for
    ///
    /// # Returns
    /// * `Ok(PolicySet)` - A Cedar PolicySet containing all applicable policies
    /// * `Err(IamPolicyProviderError)` - If retrieval fails
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<PolicySet, IamPolicyProviderError>;
}

/// Blanket implementation for Arc<T> where T: IamPolicyProvider
///
/// This allows passing Arc-wrapped implementations without additional boilerplate
#[async_trait]
impl<T: IamPolicyProvider> IamPolicyProvider for Arc<T> {
    async fn get_identity_policies_for(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<PolicySet, IamPolicyProviderError> {
        (**self).get_identity_policies_for(principal_hrn).await
    }
}
