//! HRN generator implementations
//!
//! This module provides HRN generator implementations for infrastructure.

use kernel::Hrn;
use uuid::Uuid;

/// UUID-based HRN generator
///
/// This generator creates HRNs using UUIDs for uniqueness.
pub struct UuidHrnGenerator {
    partition: String,
    service: String,
    account_id: String,
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

    /// Generate a new user HRN
    pub fn new_user_hrn(&self, _name: &str) -> Hrn {
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
    pub fn new_group_hrn(&self, _name: &str) -> Hrn {
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
