//! # Use Case for Creating a New Policy
//!
//! This module implements the core business logic for the `create_policy` feature.
//! Following Clean Architecture and Vertical Slice Architecture principles, this use case:
//!
//! - Validates the input command
//! - Delegates policy content validation to the validator port
//! - Generates a unique policy ID
//! - Constructs the domain entity
//! - Persists the policy via the persister port
//! - Returns a result DTO
//!
//! ## Design Principles
//!
//! - **Async/Await**: All operations are async for non-blocking I/O
//! - **Port-Based**: Depends only on trait abstractions, not concrete implementations
//! - **Domain-Centric**: Works with domain entities (`Policy`), not DTOs
//! - **Single Responsibility**: Only orchestrates; delegates all specific logic to ports
//! - **Observability**: Instrumented with `tracing` for logging and monitoring
//!
//! ## Error Handling
//!
//! All errors are mapped to feature-specific `CreatePolicyError` variants, providing
//! clear error messages for different failure scenarios.

use crate::features::create_policy::dto::{CreatePolicyCommand, CreatedPolicyDto};
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{PolicyIdGenerator, PolicyPersister, PolicyValidator};
use crate::shared::domain::policy::{Policy, PolicyMetadata};
use tracing::{debug, error, info, instrument};

/// Use case for creating a new policy.
///
/// This struct orchestrates the policy creation workflow by coordinating between
/// different ports (abstractions) without depending on concrete implementations.
/// It can be instantiated with any implementations that satisfy the port traits.
///
/// # Type Parameters
///
/// * `G` - The policy ID generator implementation
/// * `V` - The policy validator implementation
/// * `P` - The policy persister implementation
///
/// # Example
///
/// ```rust,ignore
/// use policies::features::create_policy::*;
///
/// let id_gen = UuidPolicyIdGenerator::new();
/// let validator = CedarPolicyValidator::new();
/// let persister = InMemoryPolicyPersister::new();
///
/// let use_case = CreatePolicyUseCase::new(id_gen, validator, persister);
///
/// let command = CreatePolicyCommand::new(
///     "permit(principal, action, resource);".to_string(),
///     Some("Allow all access".to_string()),
///     None,
/// )?;
///
/// let result = use_case.execute(command).await?;
/// println!("Created policy with ID: {}", result.id);
/// ```
pub struct CreatePolicyUseCase<G, V, P>
where
    G: PolicyIdGenerator,
    V: PolicyValidator,
    P: PolicyPersister,
{
    id_generator: G,
    validator: V,
    persister: P,
}

impl<G, V, P> CreatePolicyUseCase<G, V, P>
where
    G: PolicyIdGenerator,
    V: PolicyValidator,
    P: PolicyPersister,
{
    /// Creates a new instance of the use case with the provided dependencies.
    ///
    /// # Arguments
    ///
    /// * `id_generator` - Implementation of the `PolicyIdGenerator` port
    /// * `validator` - Implementation of the `PolicyValidator` port
    /// * `persister` - Implementation of the `PolicyPersister` port
    ///
    /// # Returns
    ///
    /// A fully configured `CreatePolicyUseCase` ready to execute commands.
    pub fn new(id_generator: G, validator: V, persister: P) -> Self {
        Self {
            id_generator,
            validator,
            persister,
        }
    }

    /// Executes the policy creation workflow.
    ///
    /// This method orchestrates the complete policy creation process:
    ///
    /// 1. **Validate Content**: Delegates to the validator port to check Cedar syntax/semantics
    /// 2. **Generate ID**: Uses the ID generator port to create a unique identifier
    /// 3. **Build Domain Entity**: Constructs a `Policy` with metadata from the command
    /// 4. **Persist**: Saves the policy via the persister port
    /// 5. **Return Result**: Returns a DTO with the policy ID
    ///
    /// # Arguments
    ///
    /// * `command` - The validated `CreatePolicyCommand` with all required data
    ///
    /// # Returns
    ///
    /// - `Ok(CreatedPolicyDto)` - Contains the ID of the successfully created policy
    /// - `Err(CreatePolicyError)` - An error occurred during the workflow
    ///
    /// # Errors
    ///
    /// This method can return the following errors:
    ///
    /// - `CreatePolicyError::ValidationError` - Policy content failed validation
    /// - `CreatePolicyError::Internal` - ID generation failed
    /// - `CreatePolicyError::Conflict` - A policy with the generated ID already exists
    /// - `CreatePolicyError::Internal` - Persistence operation failed
    ///
    /// # Observability
    ///
    /// This method is instrumented with a tracing span that captures:
    /// - Whether the policy has a description
    /// - Number of tags
    /// - All logs and errors within the execution
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let command = CreatePolicyCommand::new(
    ///     "permit(principal, action, resource);".to_string(),
    ///     Some("Test policy".to_string()),
    ///     Some(vec!["test".to_string()]),
    /// )?;
    ///
    /// let result = use_case.execute(command).await?;
    /// assert!(!result.id.is_empty());
    /// ```
    #[instrument(
        name = "create_policy_use_case",
        skip(self, command),
        fields(
            has_description = command.description().is_some(),
            tag_count = command.tags().len()
        )
    )]
    pub async fn execute(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<CreatedPolicyDto, CreatePolicyError> {
        debug!("Starting policy creation workflow");

        // Step 1: Validate policy content
        // Delegates to the validator port to check Cedar syntax and semantics
        debug!("Validating policy content");
        self.validator.validate(command.content()).await?;
        debug!("Policy content validation passed");

        // Step 2: Generate a unique policy ID
        // Delegates to the ID generator port
        debug!("Generating policy ID");
        let policy_id = self.id_generator.generate().await.map_err(|e| {
            error!("Failed to generate policy ID: {:?}", e);
            e
        })?;
        debug!(policy_id = %policy_id, "Policy ID generated");

        // Step 3: Build the domain entity
        // Constructs a Policy with metadata from the command
        debug!("Building policy domain entity");
        let metadata = PolicyMetadata::new(
            command.description().map(|s| s.to_string()),
            command.tags().to_vec(),
        );

        let policy = Policy::new(
            policy_id.clone(),
            command.content().as_ref().to_string(),
            metadata,
        );

        // Validate domain invariants
        if let Err(validation_error) = policy.validate() {
            error!(
                policy_id = %policy_id,
                error = %validation_error,
                "Policy domain validation failed"
            );
            return Err(CreatePolicyError::ValidationError(validation_error));
        }

        debug!(policy_id = %policy_id, "Policy domain entity created");

        // Step 4: Persist the policy
        // Delegates to the persister port
        debug!(policy_id = %policy_id, "Persisting policy");
        self.persister.save(&policy).await.map_err(|e| {
            error!(
                policy_id = %policy_id,
                error = ?e,
                "Failed to persist policy"
            );
            e
        })?;

        info!(
            policy_id = %policy_id,
            description = ?command.description(),
            tag_count = command.tags().len(),
            "Policy created successfully"
        );

        // Step 5: Return the result DTO
        Ok(CreatedPolicyDto::new(policy_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Comprehensive tests are in use_case_test.rs
    // This module only contains basic compilation and structure tests

    #[test]
    fn use_case_can_be_instantiated() {
        // This test verifies that the use case compiles with generic types
        use crate::features::create_policy::mocks::*;

        let id_gen = MockPolicyIdGenerator::new_with_id("test-id");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let _use_case = CreatePolicyUseCase::new(id_gen, validator, persister);
        // If this compiles, the generic constraints are correct
    }

    #[tokio::test]
    async fn use_case_can_execute_with_mocks() {
        use crate::features::create_policy::mocks::*;

        let id_gen = MockPolicyIdGenerator::new_with_id("test-123");
        let validator = MockPolicyValidator::new_accepting_all();
        let persister = MockPolicyPersister::new();

        let use_case = CreatePolicyUseCase::new(id_gen, validator, persister);

        let command = CreatePolicyCommand::new(
            "permit(principal, action, resource);".to_string(),
            None,
            None,
        )
        .unwrap();

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "test-123");
    }
}
