//! Ports for get_effective_policies_for_principal feature
//!
//! This module defines the port (trait) interfaces that the use case depends on.
//! Following the Interface Segregation Principle (SOLID), each port is specific
//! to this feature's needs.

use crate::internal::domain::{Group, User};
use async_trait::async_trait;
use kernel::domain::Hrn;

/// Port for finding users by HRN
///
/// This port abstracts user lookup without exposing repository details.
///
/// # Segregation
/// This port is segregated specifically for user lookup and does not include
/// any create, update, or delete operations.
#[async_trait]
pub trait UserFinderPort: Send + Sync {
    /// Find a user by their HRN
    ///
    /// # Arguments
    /// * `hrn` - The HRN of the user to find
    ///
    /// # Returns
    /// An optional User if found, or an error if lookup fails
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port for finding groups that a user belongs to
///
/// This port abstracts group membership lookup.
///
/// # Segregation
/// This port is segregated specifically for finding groups by user membership
/// and does not include any create, update, or delete operations.
#[async_trait]
pub trait GroupFinderPort: Send + Sync {
    /// Find all groups that a user belongs to
    ///
    /// # Arguments
    /// * `user_hrn` - The HRN of the user
    ///
    /// # Returns
    /// A vector of groups the user belongs to, or an error if lookup fails
    async fn find_groups_by_user_hrn(
        &self,
        user_hrn: &Hrn,
    ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port for finding policy documents associated with a principal
///
/// This port abstracts policy lookup and returns policies as strings
/// to maintain decoupling from Cedar types.
///
/// # Segregation
/// This port is segregated specifically for finding policies by principal
/// and does not include any create, update, or delete operations.
#[async_trait]
pub trait PolicyFinderPort: Send + Sync {
    /// Find all policy documents associated with a principal (user or group)
    ///
    /// Returns policy documents in Cedar format as strings.
    /// The policies crate is responsible for parsing and validating these strings.
    ///
    /// # Arguments
    /// * `principal_hrn` - The HRN of the principal (user or group)
    ///
    /// # Returns
    /// A vector of policy document strings, or an error if lookup fails
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
}
