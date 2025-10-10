//! Shared ports for the hodei-iam crate
//!
//! This module contains traits that are shared by multiple features
//! within the hodei-iam crate. Following the Interface Segregation
//! Principle (ISP), these traits should be minimal and focused.

use kernel::Hrn;

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
