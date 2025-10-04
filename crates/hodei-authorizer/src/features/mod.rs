//! Features module for the hodei-authorizer crate
//! 
//! This module contains all authorization-related features organized
//! according to Vertical Slice Architecture principles.

pub mod evaluate_permissions;

// Re-export all features for easier access
pub use evaluate_permissions::*;
