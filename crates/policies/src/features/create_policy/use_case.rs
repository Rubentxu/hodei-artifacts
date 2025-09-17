use super::dto::{CreatePolicyCommand, CreatePolicyResponse};
use super::error::CreatePolicyError;
use super::ports::{PolicyCreationAuditor, PolicyCreationValidator, PolicyCreator, PolicyExistenceChecker, PolicyCreationStorage};
use crate::domain::ids::{OrganizationId, HodeiPolicyId};
use crate::domain::policy::{Policy, PolicyVersion};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use shared::hrn::Hrn;
use std::sync::Arc;
use tracing::{error, info};

/// Use case for creating policies
pub struct CreatePolicyUseCase<PCV, PEC, PCS, PCA> {
    validator: Arc<PCV>,
    existence_checker: Arc<PEC>,
    storage: Arc<PCS>,
    auditor: Arc<PCA>,
}

impl<PCV, PEC, PCS, PCA> CreatePolicyUseCase<PCV, PEC, PCS, PCA> {
    pub fn new(
        validator: Arc<PCV>,
        existence_checker: Arc<PEC>,
        storage: Arc<PCS>,
        auditor: Arc<PCA>,
    ) -> Self {
        Self {
            validator,
            existence_checker,
            storage,
            auditor,
        }
    }

    /// Execute the create policy use case
    pub async fn execute(&self, command: CreatePolicyCommand) -> Result<CreatePolicyResponse, CreatePolicyError> {
        info!("Creating new policy: {}", command.policy_id);

        // Validate policy ID
        self.validator.validate_policy_id(&command.policy_id).await?;
        
        // Check if policy exists
        if self.existence_checker.exists(&command.policy_id).await? {
            error!("Policy already exists: {}", command.policy_id);
            return Err(CreatePolicyError::policy_already_exists(command.policy_id));
        }
        
        // Validate policy content
        self.validator.validate_policy_content(&command.content).await?;
        self.validator.validate_policy_syntax(&command.content).await?;
        self.validator.validate_policy_semantics(&command.content, &command.policy_id).await?;
        
        // Create policy entity
        let now = Utc::now();
        let policy = Policy {
            id: command.policy_id.clone(),
            name: command.name.clone(),
            description: command.description.clone(),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new(&format!("{}/versions/1", command.policy_id)).unwrap(),
                policy_id: command.policy_id.clone(),
                version: 1,
                content: command.content.clone(),
                created_at: now,
                created_by: command.created_by.clone(),
            },
        };

        // Save policy and version
        self.storage.save(&policy).await?;
        self.storage.create_version(&policy.current_version).await?;

        // Log the creation
        self.auditor.log_policy_creation(&policy.id, &command.created_by).await?;

        info!("Policy created successfully: {}", policy.id);

        Ok(CreatePolicyResponse {
            policy_id: policy.id,
            name: policy.name,
            description: policy.description,
            status: policy.status,
            version: policy.version,
            organization_id: command.organization_id,
            created_at: policy.created_at.to_rfc3339(),
            created_by: command.created_by,
        })
    }
}

#[async_trait]
impl<PCV, PEC, PCS, PCA> PolicyCreator for CreatePolicyUseCase<PCV, PEC, PCS, PCA>
where
    PCV: PolicyCreationValidator + Send + Sync,
    PEC: PolicyExistenceChecker + Send + Sync,
    PCS: PolicyCreationStorage + Send + Sync,
    PCA: PolicyCreationAuditor + Send + Sync,
{
    async fn create_policy(&self, command: CreatePolicyCommand) -> Result<Policy, CreatePolicyError> {
        // Convert response to policy for the interface
        let response = self.execute(command).await?;

        // Reconstruct policy from response (simplified)
        Ok(Policy {
            id: response.policy_id,
            name: response.name,
            description: response.description,
            status: response.status,
            version: response.version,
            created_at: DateTime::parse_from_rfc3339(&response.created_at)
                .map_err(|_| CreatePolicyError::storage_error("Invalid timestamp"))?
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&response.created_at)
                .map_err(|_| CreatePolicyError::storage_error("Invalid timestamp"))?
                .with_timezone(&Utc),
            current_version: PolicyVersion {
                id: Hrn::new(&format!("{}/versions/{}", response.policy_id, response.version)).unwrap(),
                policy_id: response.policy_id,
                version: response.version,
                content: "".to_string(), // TODO: Need to retrieve from storage
                created_at: DateTime::parse_from_rfc3339(&response.created_at)
                    .map_err(|_| CreatePolicyError::storage_error("Invalid timestamp"))?
                    .with_timezone(&Utc),
                created_by: response.created_by,
            },
        })
    }
}
