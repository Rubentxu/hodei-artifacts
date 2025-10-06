//! SurrealDB infrastructure implementations for hodei-iam
//!
//! This module contains the concrete implementations of repositories
//! and other infrastructure concerns using SurrealDB as the backend.

pub mod group_repository;
pub mod user_repository;

// Legacy modules that depend on Cedar - disabled until refactored
// These modules will be refactored to use the new architecture where
// Cedar types are isolated in the policies crate
#[cfg(feature = "legacy-cedar-infrastructure")]
pub mod policy_repository;

#[cfg(feature = "legacy-cedar-infrastructure")]
pub mod iam_policy_provider;

// Public exports of active repositories
pub use group_repository::SurrealGroupRepository;
pub use user_repository::SurrealUserRepository;

// Legacy exports - only available with feature flag
#[cfg(feature = "legacy-cedar-infrastructure")]
pub use policy_repository::IamPolicyRepository;

#[cfg(feature = "legacy-cedar-infrastructure")]
pub use iam_policy_provider::SurrealIamPolicyProvider;
