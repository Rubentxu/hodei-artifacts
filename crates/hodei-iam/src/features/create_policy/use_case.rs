//! Use case for creating IAM policies
//!
//! This module implements the business logic for creating new IAM policies.
//! Following Clean Architecture and Vertical Slice Architecture (VSA) principles,
//! this use case is self-contained and depends only on abstract ports.
//!
//! # Flow
//!
//! 1. Receive `CreatePolicyCommand` from the caller
//! 2. Validate policy content through `PolicyValidator` port
//! 3. If valid, persist through `CreatePolicyPort`
//! 4. Return `PolicyView` DTO with created policy details
//!
//! # Dependencies
//!
//! - `PolicyValidator`: Abstract port for Cedar policy validation
//! - `CreatePolicyPort`: Abstract port for policy persistence (ISP - only create)

use crate::features::create_policy::dto::{CreatePolicyCommand, PolicyView};
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{
    CreatePolicyPort, CreatePolicyUseCasePort, PolicyValidator,
};
use async_trait::async_trait;
use kernel::Hrn;
use std::sync::Arc;
use tracing::{info, instrument, warn};

/// The IAM service the attached principals must belong to.
const IAM_SERVICE: &str = "iam";

/// The IAM resource type that denotes a principal.
///
/// `User` is the only IAM type whose entity type declares itself a principal
/// (`HodeiEntityType::is_principal_type()`); a `Group` is not one, and the
/// group-derived policies are a separate, deferred capability.
const IAM_PRINCIPAL_TYPE: &str = "User";

/// Use case for creating IAM policies
///
/// This use case orchestrates the policy creation process:
/// 1. Validates the Cedar policy syntax and semantics
/// 2. Persists the policy if validation succeeds
/// 3. Returns a view of the created policy
///
/// # Architecture Note
///
/// This struct uses trait objects (Arc<dyn Trait>) instead of generics for simplicity.
/// This is the idiomatic Rust approach for dependency injection without frameworks.
pub struct CreatePolicyUseCase {
    /// Port for persisting policies (only create operation)
    policy_port: Arc<dyn CreatePolicyPort>,

    /// Port for validating Cedar policy content
    validator: Arc<dyn PolicyValidator>,
}

impl CreatePolicyUseCase {
    /// Create a new instance of the use case
    ///
    /// # Arguments
    ///
    /// * `policy_port` - Implementation of `CreatePolicyPort` for persistence
    /// * `validator` - Implementation of `PolicyValidator` for validation
    pub fn new(
        policy_port: Arc<dyn CreatePolicyPort>,
        validator: Arc<dyn PolicyValidator>,
    ) -> Self {
        Self {
            policy_port,
            validator,
        }
    }

    /// Execute the create policy use case (internal implementation)
    ///
    /// # Arguments
    ///
    /// * `command` - Command containing policy details
    ///
    /// # Returns
    ///
    /// On success, returns `Ok(PolicyView)` with the created policy information.
    ///
    /// # Errors
    ///
    /// - `CreatePolicyError::EmptyPolicyContent` - Policy content is empty
    /// - `CreatePolicyError::InvalidPolicyContent` - Policy fails Cedar validation
    /// - `CreatePolicyError::PolicyAlreadyExists` - Policy ID already in use
    /// - `CreatePolicyError::RepositoryError` - Database or storage failure
    #[instrument(skip(self, command), fields(policy_id = %command.policy_id))]
    async fn execute_impl(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<PolicyView, CreatePolicyError> {
        let mut command = command;

        // Validate policy id
        let normalized_policy_id = command.policy_id.trim();
        if normalized_policy_id.is_empty() {
            warn!("Policy creation failed: empty policy id");
            return Err(CreatePolicyError::InvalidPolicyId(
                "Policy ID cannot be empty".to_string(),
            ));
        }
        command.policy_id = normalized_policy_id.to_string();

        info!("Creating policy with id: {}", command.policy_id);

        // Validate input
        if command.policy_content.trim().is_empty() {
            warn!("Policy creation failed: empty content");
            return Err(CreatePolicyError::EmptyPolicyContent);
        }

        // Validate and canonicalize the attachment.
        //
        // The attachment decides *for whom* the policy is effective: the IAM
        // evaluation only consults the policies attached to the principal it
        // is deciding about. It is therefore stored as the canonical HRN — the
        // very string the query binds — so that the persisted value and the
        // evaluated principal are compared by identity and not by spelling.
        command.attached_principals =
            canonicalize_attached_principals(std::mem::take(&mut command.attached_principals))?;

        // Validate policy syntax using hodei-policies
        let validation_command =
            hodei_policies::features::validate_policy::dto::ValidatePolicyCommand {
                content: command.policy_content.clone(),
            };

        let validation_result = self
            .validator
            .validate(validation_command)
            .await
            .map_err(|e| CreatePolicyError::ValidationFailed(e.to_string()))?;

        if !validation_result.is_valid || !validation_result.errors.is_empty() {
            warn!(
                "Policy validation failed with {} errors",
                validation_result.errors.len()
            );
            let error_messages = validation_result.errors.join(", ");
            return Err(CreatePolicyError::InvalidPolicyContent(error_messages));
        }

        info!("Policy validation successful, persisting policy");

        // Create the policy through the port
        let policy = self.policy_port.create(command.clone()).await?;

        info!("Policy created successfully: {}", policy.id());

        // Convert to view DTO
        let now = chrono::Utc::now();

        // Build HRN from policy ID.
        //
        // The resource type must be spelled `Policy` (PascalCase): that is the
        // spelling the domain uses everywhere else (`User`, `Group`) and the one
        // `GetPolicyUseCase` validates against. Emitting `policy` here produced
        // an HRN that the domain rejected as "not a Policy HRN".
        let policy_hrn = kernel::Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(), // TODO: Get from context
            "Policy".to_string(),
            policy.id().to_string(),
        );

        let view = PolicyView {
            id: policy_hrn,
            content: policy.content().to_string(),
            description: command.description.clone(),
            attached_principals: command.attached_principals.clone(),
            created_at: now,
            updated_at: now,
        };

        Ok(view)
    }
}

/// Validate the attached principals and return them canonicalized.
///
/// Every entry must be a well-formed HRN of an IAM principal. The canonical
/// form is returned so that what is persisted is exactly what the evaluation
/// compares against: two spellings of the same identity cannot end up as two
/// different attachments, and a principal that cannot be named cannot be
/// silently attached to nothing.
fn canonicalize_attached_principals(
    attached: Vec<String>,
) -> Result<Vec<String>, CreatePolicyError> {
    let mut canonical: Vec<String> = Vec::with_capacity(attached.len());

    for raw in attached {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(CreatePolicyError::InvalidHrn(
                "Attached principal cannot be empty".to_string(),
            ));
        }

        let hrn = Hrn::from_string(trimmed).ok_or_else(|| {
            CreatePolicyError::InvalidHrn(format!(
                "Attached principal is not a valid HRN: {trimmed}"
            ))
        })?;

        if hrn.service() != IAM_SERVICE || hrn.resource_type() != IAM_PRINCIPAL_TYPE {
            return Err(CreatePolicyError::InvalidHrn(format!(
                "Attached principal must be an {IAM_SERVICE}::{IAM_PRINCIPAL_TYPE} HRN: {trimmed}"
            )));
        }

        let canonical_hrn = hrn.to_string();
        if !canonical.contains(&canonical_hrn) {
            canonical.push(canonical_hrn);
        }
    }

    Ok(canonical)
}

// Implement CreatePolicyUseCasePort trait for the use case
#[async_trait]
impl CreatePolicyUseCasePort for CreatePolicyUseCase {
    async fn execute(&self, command: CreatePolicyCommand) -> Result<PolicyView, CreatePolicyError> {
        self.execute_impl(command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::mocks::{MockCreatePolicyPort, MockPolicyValidator};

    #[tokio::test]
    async fn test_create_policy_success() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
            attached_principals: Vec::new(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_policy_empty_content() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "   ".to_string(),
            description: None,
            attached_principals: Vec::new(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(CreatePolicyError::EmptyPolicyContent)));
    }

    #[tokio::test]
    async fn test_create_policy_validation_failure() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::with_errors(vec![
            "Syntax error".to_string(),
        ]));
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "invalid policy".to_string(),
            description: None,
            attached_principals: Vec::new(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(CreatePolicyError::InvalidPolicyContent(_))
        ));
    }

    #[tokio::test]
    async fn test_create_policy_canonicalizes_the_attachment() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
            attached_principals: vec!["  hrn:aws:iam::account-a:User/alice  ".to_string()],
        };

        let view = use_case
            .execute(command)
            .await
            .expect("the policy is valid");
        assert_eq!(
            view.attached_principals,
            vec!["hrn:aws:iam::account-a:User/alice".to_string()],
            "the attachment must be persisted as the canonical HRN"
        );
    }

    #[tokio::test]
    async fn test_create_policy_rejects_a_malformed_attachment() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
            attached_principals: vec!["not-an-hrn".to_string()],
        };

        let result = use_case.execute(command).await;
        assert!(
            matches!(result, Err(CreatePolicyError::InvalidHrn(_))),
            "a principal that cannot be named must not be attached to nothing"
        );
    }

    #[tokio::test]
    async fn test_create_policy_rejects_a_non_principal_attachment() {
        let policy_port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = CreatePolicyUseCase::new(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: None,
            attached_principals: vec!["hrn:aws:iam::account-a:Group/admins".to_string()],
        };

        let result = use_case.execute(command).await;
        assert!(
            matches!(result, Err(CreatePolicyError::InvalidHrn(_))),
            "a group is not a principal: attaching to it would never be evaluated"
        );
    }
}
