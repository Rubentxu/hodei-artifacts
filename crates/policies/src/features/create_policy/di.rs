//! # Dependency Injection Configuration for Create Policy Feature
//!
//! This module provides the dependency injection container configuration for the
//! `create_policy` feature. It centralizes the wiring of all dependencies required
//! by the `CreatePolicyUseCase`, including ID generators, validators, and persisters.
//!
//! ## Design Principles
//!
//! - **Type Safety**: All dependencies are strongly typed and validated at compile time.
//! - **Flexibility**: Supports both production and test configurations.
//! - **Explicit Wiring**: All dependencies are explicitly configured, making the
//!   dependency graph clear and auditable.
//! - **Trait-Based**: Depends on traits (ports), not concrete implementations, allowing
//!   easy substitution for testing or different deployment scenarios.
//!
//! ## Usage
//!
//! ### Production Configuration
//!
//! ```rust,ignore
//! use crate::features::create_policy::di::CreatePolicyContainer;
//!
//! let container = CreatePolicyContainer::new_production();
//! let use_case = container.create_use_case();
//! ```
//!
//! ### Test Configuration
//!
//! ```rust,ignore
//! use crate::features::create_policy::di::CreatePolicyContainer;
//! use crate::features::create_policy::mocks::*;
//!
//! let id_gen = MockPolicyIdGenerator::new_with_id("test-id");
//! let validator = MockPolicyValidator::new_accepting_all();
//! let persister = MockPolicyPersister::new();
//!
//! let container = CreatePolicyContainer::new_with_deps(id_gen, validator, persister);
//! let use_case = container.create_use_case();
//! ```

use crate::features::create_policy::adapter::{
    CedarPolicyValidator, InMemoryPolicyPersister, UuidPolicyIdGenerator,
};
use crate::features::create_policy::ports::{PolicyIdGenerator, PolicyPersister, PolicyValidator};
use crate::features::create_policy::use_case::CreatePolicyUseCase;
use std::sync::Arc;

/// Dependency injection container for the Create Policy feature.
///
/// This container encapsulates all the dependencies required to instantiate and
/// configure a `CreatePolicyUseCase`. It supports both production configurations
/// (with real adapters) and test configurations (with mocks or stubs).
///
/// The container uses `Arc` to allow shared ownership of dependencies across
/// multiple use case instances, which is typical in a multi-threaded server
/// environment.
#[derive(Clone)]
pub struct CreatePolicyContainer<G, V, P>
where
    G: PolicyIdGenerator + Clone,
    V: PolicyValidator + Clone,
    P: PolicyPersister + Clone,
{
    id_generator: G,
    validator: V,
    persister: P,
}

impl<G, V, P> CreatePolicyContainer<G, V, P>
where
    G: PolicyIdGenerator + Clone,
    V: PolicyValidator + Clone,
    P: PolicyPersister + Clone,
{
    /// Creates a new container with custom dependencies.
    ///
    /// This constructor is primarily intended for testing, where you want to inject
    /// mock implementations of the ports.
    ///
    /// # Arguments
    ///
    /// * `id_generator` - An implementation of `PolicyIdGenerator`
    /// * `validator` - An implementation of `PolicyValidator`
    /// * `persister` - An implementation of `PolicyPersister`
    ///
    /// # Returns
    ///
    /// A fully configured `CreatePolicyContainer` ready to create use case instances.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use crate::features::create_policy::di::CreatePolicyContainer;
    /// use crate::features::create_policy::mocks::*;
    ///
    /// let id_gen = MockPolicyIdGenerator::new_with_id("test-123");
    /// let validator = MockPolicyValidator::new_accepting_all();
    /// let persister = MockPolicyPersister::new();
    ///
    /// let container = CreatePolicyContainer::new_with_deps(id_gen, validator, persister);
    /// ```
    pub fn new_with_deps(id_generator: G, validator: V, persister: P) -> Self {
        Self {
            id_generator,
            validator,
            persister,
        }
    }

    /// Creates a new `CreatePolicyUseCase` instance with the configured dependencies.
    ///
    /// This method wires all the dependencies together and returns a fully initialized
    /// use case ready to execute.
    ///
    /// # Returns
    ///
    /// A `CreatePolicyUseCase` with all dependencies injected.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let container = CreatePolicyContainer::new_production();
    /// let use_case = container.create_use_case();
    ///
    /// // Now you can execute commands
    /// let command = CreatePolicyCommand::new(content, description, tags)?;
    /// let policy_id = use_case.execute(command).await?;
    /// ```
    pub fn create_use_case(&self) -> CreatePolicyUseCase<G, V, P> {
        CreatePolicyUseCase::new(
            self.id_generator.clone(),
            self.validator.clone(),
            self.persister.clone(),
        )
    }

    /// Returns a reference to the ID generator.
    ///
    /// This is useful for inspecting or testing the container configuration.
    pub fn id_generator(&self) -> &G {
        &self.id_generator
    }

    /// Returns a reference to the validator.
    ///
    /// This is useful for inspecting or testing the container configuration.
    pub fn validator(&self) -> &V {
        &self.validator
    }

    /// Returns a reference to the persister.
    ///
    /// This is useful for inspecting or testing the container configuration.
    pub fn persister(&self) -> &P {
        &self.persister
    }
}

// --- Production Configuration ---

impl CreatePolicyContainer<UuidPolicyIdGenerator, CedarPolicyValidator, InMemoryPolicyPersister> {
    /// Creates a production-ready container with real adapters.
    ///
    /// This configuration uses:
    /// - `UuidPolicyIdGenerator` for generating unique policy IDs
    /// - `CedarPolicyValidator` for validating policies using the Cedar framework
    /// - `InMemoryPolicyPersister` for in-memory storage (suitable for development)
    ///
    /// **Note**: In a real production environment, you would typically replace
    /// `InMemoryPolicyPersister` with a database-backed implementation (e.g.,
    /// `SurrealDbPolicyPersister`).
    ///
    /// # Returns
    ///
    /// A container configured with production adapters.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use crate::features::create_policy::di::CreatePolicyContainer;
    ///
    /// let container = CreatePolicyContainer::new_production();
    /// let use_case = container.create_use_case();
    /// ```
    pub fn new_production() -> Self {
        Self {
            id_generator: UuidPolicyIdGenerator::new(),
            validator: CedarPolicyValidator::new(),
            persister: InMemoryPolicyPersister::new(),
        }
    }
}

// --- Arc-Wrapped Production Configuration ---

/// Type alias for an Arc-wrapped production container.
///
/// This is the most common configuration for use in web servers or other
/// multi-threaded applications where the container needs to be shared across
/// multiple threads or async tasks.
pub type ProductionContainer = Arc<
    CreatePolicyContainer<UuidPolicyIdGenerator, CedarPolicyValidator, InMemoryPolicyPersister>,
>;

/// Creates a production container wrapped in an `Arc` for shared ownership.
///
/// This is a convenience function for creating a shareable production container
/// that can be cloned cheaply and passed to multiple async tasks or threads.
///
/// # Returns
///
/// An `Arc`-wrapped production container.
///
/// # Example
///
/// ```rust,ignore
/// use crate::features::create_policy::di::create_production_container;
///
/// let container = create_production_container();
///
/// // Can be cloned cheaply and shared across threads
/// let container_clone = container.clone();
/// tokio::spawn(async move {
///     let use_case = container_clone.create_use_case();
///     // ...
/// });
/// ```
pub fn create_production_container() -> ProductionContainer {
    Arc::new(CreatePolicyContainer::new_production())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::mocks::{
        MockPolicyIdGenerator, MockPolicyPersister, MockPolicyValidator,
    };

    #[test]
    fn production_container_can_be_created() {
        let container = CreatePolicyContainer::new_production();
        let _use_case = container.create_use_case();
        // If this compiles and runs, the wiring is correct
    }

    #[test]
    fn arc_production_container_can_be_created() {
        let container = create_production_container();
        let _use_case = container.create_use_case();
        // If this compiles and runs, the wiring is correct
    }

    #[test]
    fn arc_production_container_can_be_cloned() {
        let container = create_production_container();
        let clone = container.clone();
        let _use_case1 = container.create_use_case();
        let _use_case2 = clone.create_use_case();
        // If this compiles and runs, Arc cloning works as expected
    }

    #[test]
    fn custom_container_can_be_created_with_mocks() {
        let id_gen = MockPolicyIdGenerator::new_with_id("test-id");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let container = CreatePolicyContainer::new_with_deps(id_gen, validator, persister);
        let _use_case = container.create_use_case();
        // If this compiles and runs, custom wiring works
    }

    #[tokio::test]
    async fn use_case_from_container_can_execute() {
        let id_gen = MockPolicyIdGenerator::new_with_id("container-test-id");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let container = CreatePolicyContainer::new_with_deps(id_gen, validator, persister.clone());
        let use_case = container.create_use_case();

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            Some("Container Test".to_string()),
            None,
        )
        .unwrap();

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "container-test-id");
        assert_eq!(persister.save_count(), 1);
    }

    #[test]
    fn container_provides_access_to_dependencies() {
        let id_gen = MockPolicyIdGenerator::new_with_id("test-id");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let container = CreatePolicyContainer::new_with_deps(
            id_gen.clone(),
            validator.clone(),
            persister.clone(),
        );

        // Verify we can access the dependencies
        let _id_gen_ref = container.id_generator();
        let _validator_ref = container.validator();
        let _persister_ref = container.persister();
    }

    #[tokio::test]
    async fn multiple_use_cases_from_same_container_share_state() {
        let id_gen =
            MockPolicyIdGenerator::new_with_sequence(vec!["id-1".to_string(), "id-2".to_string()]);
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let container = CreatePolicyContainer::new_with_deps(id_gen, validator, persister.clone());

        let use_case1 = container.create_use_case();
        let use_case2 = container.create_use_case();

        let command1 = CreatePolicyCommand::new("policy1".to_string(), None, None).unwrap();
        let command2 = CreatePolicyCommand::new("policy2".to_string(), None, None).unwrap();

        let result1 = use_case1.execute(command1).await;
        let result2 = use_case2.execute(command2).await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());

        // Both policies should be saved in the shared persister
        assert_eq!(persister.save_count(), 2);
    }
}
