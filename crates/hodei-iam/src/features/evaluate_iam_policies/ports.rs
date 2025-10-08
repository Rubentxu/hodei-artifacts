//! Ports (interfaces) for the evaluate_iam_policies feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (SOLID), this port is specific
//! to IAM policy evaluation needs.

use async_trait::async_trait;
use kernel::domain::HodeiPolicySet;
use kernel::Hrn;

/// Port for finding and retrieving IAM policies
///
/// This port abstracts the retrieval of effective IAM policies for a principal.
/// It follows ISP by providing only the operations needed for policy evaluation.
///
/// # Responsibilities
///
/// - Retrieve all effective policies for a given principal
/// - Return policies as a PolicySet ready for evaluation
///
/// # Segregation
///
/// This port is segregated specifically for policy retrieval during evaluation.
/// It does NOT include:
/// - Policy CRUD operations (those are in separate features)
/// - Policy validation (that's in create_policy feature)
/// - Entity management (that's in other features)
#[async_trait]
pub trait PolicyFinderPort: Send + Sync {
    /// Get the effective IAM policies for a principal
    ///
    /// This method retrieves all IAM policies that apply to the given principal,
    /// including:
    /// - Policies directly attached to the principal
    /// - Policies attached to groups the principal belongs to
    /// - Policies inherited through organizational hierarchy
    ///
    /// # Arguments
    ///
    /// * `principal_hrn` - The HRN of the principal (user, service account)
    ///
    /// # Returns
    ///
    /// A `PolicySet` containing all effective policies, ready for Cedar evaluation.
    /// Returns an empty `PolicySet` if no policies are found (implicit deny).
    ///
    /// # Errors
    ///
    /// Returns `PolicyFinderError` if:
    /// - The principal does not exist
    /// - Database/repository errors occur
    /// - Policy parsing fails
    async fn get_effective_policies(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<HodeiPolicySet, PolicyFinderError>;
}

/// Errors that can occur during policy retrieval
#[derive(Debug, thiserror::Error)]
pub enum PolicyFinderError {
    /// Principal not found in the system
    #[error("Principal not found: {0}")]
    PrincipalNotFound(String),

    /// Repository/database error
    #[error("Repository error: {0}")]
    RepositoryError(String),

    /// Policy parsing error
    #[error("Policy parsing error: {0}")]
    PolicyParseError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}
