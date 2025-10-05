//! # Adapter Implementations for Create Policy Feature
//!
//! This module provides concrete implementations for the ports defined in the `create_policy`
//! feature. These adapters are responsible for interacting with external systems and libraries,
//! such as UUID generation, policy validation using the Cedar framework, and persisting
//! policy data.
//!
//! ## Implementations
//!
//! - **`UuidPolicyIdGenerator`**: A `PolicyIdGenerator` that uses the `uuid` crate to produce
//!   unique, version 4 UUIDs for new policies.
//!
//! - **`CedarPolicyValidator`**: A `PolicyValidator` that encapsulates the logic for parsing
//!   and validating a policy's syntax using the `cedar-policy` crate. This is a critical
//!   boundary in the architecture, ensuring that Cedar-specific logic is isolated from the
//!   core use case.
//!
//! - **`InMemoryPolicyPersister`**: An in-memory `PolicyPersister` implementation suitable for
//!   testing, development, and scenarios where persistence to a database is not required. It
//!   uses a `DashMap` for thread-safe, concurrent access.
//!
//! These adapters are designed to be injected into the `CreatePolicyUseCase` via a dependency
//! injection container, fulfilling the contracts defined by the feature's ports.

use crate::features::create_policy::dto::PolicyContent;
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{PolicyIdGenerator, PolicyPersister, PolicyValidator};
use crate::shared::domain::policy::{Policy, PolicyId};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use uuid::Uuid;

// --- Policy ID Generator Implementation ---

/// A `PolicyIdGenerator` that generates unique identifiers using the `uuid` crate.
///
/// This implementation creates a new version 4 UUID for each policy, ensuring a high
/// degree of uniqueness. It is stateless and can be cheaply cloned.
#[derive(Debug, Default, Clone)]
pub struct UuidPolicyIdGenerator;

impl UuidPolicyIdGenerator {
    /// Creates a new instance of the generator.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyIdGenerator for UuidPolicyIdGenerator {
    /// Generates a new, unique `PolicyId`.
    ///
    /// # Returns
    ///
    /// A `Result` containing the new `PolicyId` or a `CreatePolicyError` if generation fails,
    /// although this specific implementation is infallible.
    async fn generate(&self) -> Result<PolicyId, CreatePolicyError> {
        let new_id = Uuid::new_v4().to_string();
        Ok(PolicyId::new(new_id))
    }
}

// --- Policy Validator Implementation ---

/// A `PolicyValidator` that uses the `cedar-policy` crate to validate policy syntax.
///
/// This adapter is the primary boundary between the application's domain and the Cedar
/// framework for policy validation. It attempts to parse the policy content, and if
/// successful, the policy is considered valid. Any parsing errors are translated into
/// a feature-specific `CreatePolicyError::ValidationError`.
#[derive(Debug, Default, Clone)]
pub struct CedarPolicyValidator;

impl CedarPolicyValidator {
    /// Creates a new instance of the validator.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidator {
    /// Validates the provided policy content using the Cedar parser.
    ///
    /// # Arguments
    ///
    /// * `content` - A reference to the `PolicyContent` DTO containing the policy string.
    ///
    /// # Returns
    ///
    /// An `Ok(())` if the policy is valid, or a `CreatePolicyError::ValidationError`
    /// containing a descriptive error message if parsing fails.
    async fn validate(&self, content: &PolicyContent) -> Result<(), CreatePolicyError> {
        cedar_policy::Policy::parse(None, content.as_ref()).map_err(|e| {
            CreatePolicyError::ValidationError(format!("Invalid Cedar policy syntax: {}", e))
        })?;
        Ok(())
    }
}

// --- Policy Persister Implementation ---

/// An in-memory `PolicyPersister` for storing policies.
///
/// This implementation uses a `DashMap` to provide a thread-safe, in-memory key-value
/// store for `Policy` objects. It is intended for use in development, testing, or
/// scenarios where durable persistence is not required.
///
/// The `Arc` wrapper makes it shareable across multiple threads, which is typical
/// for application state managed by a DI container.
#[derive(Debug, Default, Clone)]
pub struct InMemoryPolicyPersister {
    /// The underlying thread-safe map storing policies by their ID.
    store: Arc<DashMap<PolicyId, Policy>>,
}

impl InMemoryPolicyPersister {
    /// Creates a new, empty in-memory persister.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PolicyPersister for InMemoryPolicyPersister {
    /// Saves a `Policy` to the in-memory store.
    ///
    /// # Arguments
    ///
    /// * `policy` - The `Policy` domain object to save.
    ///
    /// # Returns
    ///
    /// An `Ok(())` if the policy was saved successfully.
    ///
    /// # Errors
    ///
    /// * `CreatePolicyError::Conflict` - If a policy with the same `PolicyId` already
    ///   exists in the store.
    async fn save(&self, policy: &Policy) -> Result<(), CreatePolicyError> {
        if self.store.contains_key(policy.id()) {
            return Err(CreatePolicyError::Conflict(policy.id().to_string()));
        }
        self.store.insert(policy.id().clone(), policy.clone());
        Ok(())
    }
}
