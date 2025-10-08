//! Internal Authorization Engine
//! Internal Engine Module
//!
//! This module contains the internal implementation of the authorization engine.
//! It includes the Cedar policy engine integration and related utilities.

pub mod builder;
pub mod core;
pub mod translator;
pub mod types;

// Re-export main types for convenience
pub use core::AuthorizationEngine;
