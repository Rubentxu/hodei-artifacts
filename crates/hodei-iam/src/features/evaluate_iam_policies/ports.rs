//! Ports (interfaces) for the evaluate_iam_policies feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (SOLID), these ports are specific
//! to IAM policy evaluation needs.

use async_trait::async_trait;
use kernel::domain::HodeiPolicySet;
use kernel::{HodeiEntity, Hrn};

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

/// Port for resolving a principal entity from its HRN
///
/// This port abstracts the retrieval of principal entities (Users, Service Accounts)
/// needed for Cedar evaluation. It follows ISP by providing only the operation
/// needed for resolving principals during policy evaluation.
///
/// # Responsibilities
///
/// - Resolve a principal HRN to a concrete entity implementing `HodeiEntity`
/// - Return entities ready for Cedar evaluation
///
/// # Segregation
///
/// This port is segregated specifically for principal resolution during evaluation.
/// It does NOT include:
/// - Principal CRUD operations (those are in separate features)
/// - Policy management
/// - Authorization decisions
#[async_trait]
pub trait PrincipalResolverPort: Send + Sync {
    /// Resolve a principal HRN to a concrete entity
    ///
    /// This method retrieves the principal entity (User, ServiceAccount, etc.)
    /// from the HRN, returning it as a trait object ready for Cedar evaluation.
    ///
    /// # Arguments
    ///
    /// * `principal_hrn` - The HRN of the principal to resolve
    ///
    /// # Returns
    ///
    /// A boxed trait object implementing `HodeiEntity`, representing the principal.
    ///
    /// # Errors
    ///
    /// Returns `EntityResolverError` if:
    /// - The principal does not exist
    /// - The HRN is invalid or malformed
    /// - Database/repository errors occur
    async fn resolve_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError>;
}

/// Port for resolving a resource entity from its HRN
///
/// This port abstracts the retrieval of resource entities needed for Cedar evaluation.
/// It follows ISP by providing only the operation needed for resolving resources
/// during policy evaluation.
///
/// # Responsibilities
///
/// - Resolve a resource HRN to a concrete entity implementing `HodeiEntity`
/// - Return entities ready for Cedar evaluation
///
/// # Segregation
///
/// This port is segregated specifically for resource resolution during evaluation.
/// It does NOT include:
/// - Resource CRUD operations (those are in other bounded contexts)
/// - Policy management
/// - Authorization decisions
///
/// # Note
///
/// For IAM policy evaluation, the resource might come from other bounded contexts
/// (artifacts, organizations, etc.). This port provides a unified interface for
/// resolving any resource type needed for evaluation.
#[async_trait]
pub trait ResourceResolverPort: Send + Sync {
    /// Resolve a resource HRN to a concrete entity
    ///
    /// This method retrieves the resource entity from the HRN, returning it
    /// as a trait object ready for Cedar evaluation.
    ///
    /// # Arguments
    ///
    /// * `resource_hrn` - The HRN of the resource to resolve
    ///
    /// # Returns
    ///
    /// A boxed trait object implementing `HodeiEntity`, representing the resource.
    ///
    /// # Errors
    ///
    /// Returns `EntityResolverError` if:
    /// - The resource does not exist
    /// - The HRN is invalid or malformed
    /// - Database/repository errors occur
    /// - The resource type is not supported
    async fn resolve_resource(
        &self,
        resource_hrn: &Hrn,
    ) -> Result<Box<dyn HodeiEntity + Send>, EntityResolverError>;
}

/// Errors that can occur during entity resolution
#[derive(Debug, thiserror::Error)]
pub enum EntityResolverError {
    /// Entity not found in the system
    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    /// Invalid or malformed HRN
    #[error("Invalid HRN: {0}")]
    InvalidHrn(String),

    /// Entity type not supported for resolution
    #[error("Unsupported entity type: {0}")]
    UnsupportedEntityType(String),

    /// Repository/database error
    #[error("Repository error: {0}")]
    RepositoryError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}
