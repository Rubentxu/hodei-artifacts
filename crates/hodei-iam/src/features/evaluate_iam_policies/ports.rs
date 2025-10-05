//! Ports (interfaces) for the evaluate_iam_policies feature
//!
//! This module defines the ports (trait interfaces) that the use case depends on.
//! Following the Interface Segregation Principle (SOLID), each port is specific
//! to this feature's needs.

use async_trait::async_trait;
use kernel::application::ports::authorization::AuthorizationError;
use kernel::domain::Hrn;

/// Port for finding IAM policies associated with a principal
///
/// This port abstracts the retrieval of policies that apply to a given principal.
/// It may include:
/// - Direct policies attached to the user
/// - Policies inherited from groups the user belongs to
/// - Policies attached to roles the user has assumed
///
/// # Segregation
/// This port is segregated specifically for this feature and does not include
/// any CRUD operations or other concerns.
#[async_trait]
pub trait PolicyFinderPort: Send + Sync {
    /// Get all policy documents (as Cedar policy strings) that apply to the principal
    ///
    /// # Arguments
    /// * `principal_hrn` - The HRN of the principal (user, service account, etc.)
    ///
    /// # Returns
    /// A vector of Cedar policy document strings, or an error if retrieval fails
    ///
    /// # Example
    /// ```ignore
    /// let policies = policy_finder.get_policies_for_principal(&user_hrn).await?;
    /// // Returns: vec!["permit(principal == User::\"alice\", action, resource);"]
    /// ```
    async fn get_policies_for_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, AuthorizationError>;
}
