//! HRN generator trait for infrastructure implementations
//!
//! This trait provides a unified interface for HRN generation
//! across all bounded contexts in the system.

use crate::Hrn;

/// Shared HRN generator trait for all bounded contexts
///
/// This trait provides a unified interface for HRN generation
/// across all bounded contexts in the system.
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
