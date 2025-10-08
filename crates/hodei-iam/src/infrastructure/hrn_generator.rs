//! HRN generator implementations
//!
//! This module provides HRN generator implementations for infrastructure.

use kernel::Hrn;
use uuid::Uuid;

/// Shared HRN generator trait for all features
///
/// This trait provides a unified interface for HRN generation
/// across all features in the hodei-iam crate.
pub trait HrnGenerator: Send + Sync {
    /// Generate a new HRN for a user
    ///
    /// # Arguments
    /// * `name` - The name of the user (used for HRN generation)
    ///
    /// # Returns
    /// * A new HRN for the user
    fn new_user_hrn(&self, name: &str) -> Hrn;

    /// Generate a new HRN for a group
    ///
    /// # Arguments
    /// * `name` - The name of the group (used for HRN generation)
    ///
    /// # Returns
    /// * A new HRN for the group
    fn new_group_hrn(&self, name: &str) -> Hrn;
}

/// UUID-based HRN generator
///
/// This generator creates HRNs using UUIDs for uniqueness.
pub struct UuidHrnGenerator {
    partition: String,
    service: String,
    account_id: String,
}

impl HrnGenerator for UuidHrnGenerator {
    /// Generate a new user HRN
    fn new_user_hrn(&self, _name: &str) -> Hrn {
        let resource_id = Uuid::new_v4().to_string();
        Hrn::new(
            self.partition.clone(),
            self.service.clone(),
            self.account_id.clone(),
            "User".to_string(),
            resource_id,
        )
    }

    /// Generate a new group HRN
    fn new_group_hrn(&self, _name: &str) -> Hrn {
        let resource_id = Uuid::new_v4().to_string();
        Hrn::new(
            self.partition.clone(),
            self.service.clone(),
            self.account_id.clone(),
            "Group".to_string(),
            resource_id,
        )
    }
}

impl UuidHrnGenerator {
    /// Create a new UUID-based HRN generator
    ///
    /// # Arguments
    /// * `partition` - The partition for the HRN (e.g., "hodei")
    /// * `service` - The service for the HRN (e.g., "iam")
    /// * `account_id` - The account ID for the HRN
    pub fn new(partition: String, service: String, account_id: String) -> Self {
        Self {
            partition,
            service,
            account_id,
        }
    }
}
