use async_trait::async_trait;
use chrono::Utc;
use shared::hrn::Hrn;
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::dto::{PolicyUpdateChanges, UpdatePolicyCommand, UpdatePolicyResponse};
use super::error::UpdatePolicyError;
use super::ports::{PolicyRetriever, PolicyUpdateAuditor, PolicyUpdateStorage, PolicyUpdateValidator, PolicyUpdater};
use crate::domain::ids::{PolicyId};
use crate::domain::policy::{Policy, PolicyVersion};

/// Use case for updating policies
pub struct UpdatePolicyUseCase<PUV, PR, PUS, PUA> {
    validator: Arc<dyn PolicyUpdateValidator>,
    retriever: Arc<dyn PolicyRetriever>,
    storage: Arc<dyn PolicyUpdateStorage>,
    auditor: Arc<dyn PolicyUpdateAuditor>,
    _phantom: PhantomData<(PUV, PR, PUS, PUA)>,
}

impl<PUV, PR, PUS, PUA> UpdatePolicyUseCase<PUV, PR, PUS, PUA> {
    pub fn new(
        validator: Arc<dyn PolicyUpdateValidator>,
        retriever: Arc<dyn PolicyRetriever>,
        storage: Arc<dyn PolicyUpdateStorage>,
        auditor: Arc<dyn PolicyUpdateAuditor>,
    ) -> Self {
        Self {
            validator,
            retriever,
            storage,
            auditor,
            _phantom: PhantomData,
        }
    }

    /// Execute the update policy use case
    pub async fn execute(&self, policy_id: &PolicyId, command: UpdatePolicyCommand) -> Result<UpdatePolicyResponse, UpdatePolicyError> {
        info!("Updating policy: {}", policy_id);

        // Retrieve existing policy
        let mut existing_policy = self.retriever.get_policy(policy_id).await?
            .ok_or_else(|| UpdatePolicyError::policy_not_found(policy_id.clone()))?;

        // Check version conflict if expected version provided
        if let Some(expected_version) = command.expected_version {
            if existing_policy.version != expected_version {
                error!("Version conflict for policy {}: expected {}, got {}", policy_id, expected_version, existing_policy.version);
                return Err(UpdatePolicyError::version_conflict(expected_version, existing_policy.version));
            }
        }

        // Validate update permissions
        self.validator.validate_update_allowed(&existing_policy, &command.updated_by).await?;

        // Track changes
        let mut changes = PolicyUpdateChanges {
            name_changed: false,
            description_changed: false,
            content_changed: false,
            version_incremented: false,
        };

        // Apply updates
        let mut content_changed = false;
        if let Some(ref new_name) = command.name {
            if existing_policy.name != *new_name {
                existing_policy.name = new_name.clone();
                changes.name_changed = true;
            }
        }

        if let Some(ref new_description) = command.description {
            if existing_policy.description != *new_description {
                existing_policy.description = new_description.clone();
                changes.description_changed = true;
            }
        }

        if let Some(ref new_content) = command.content {
            // Validate new content
            self.validator.validate_policy_content(new_content).await?;
            self.validator.validate_policy_syntax(new_content).await?;
            self.validator.validate_policy_semantics(new_content, policy_id).await?;

            if existing_policy.current_version.content != *new_content {
                // Create new version
                existing_policy.version += 1;
                existing_policy.current_version = PolicyVersion {
                    id: Hrn::new(&format!("{}/versions/{}", policy_id, existing_policy.version)).unwrap(),
                    policy_id: policy_id.clone(),
                    version: existing_policy.version,
                    content: new_content.clone(),
                    created_at: Utc::now(),
                    created_by: command.updated_by.clone(),
                };
                changes.content_changed = true;
                changes.version_incremented = true;
                content_changed = true;
            }
        }

        // Update timestamp
        existing_policy.updated_at = Utc::now();

        // Save updated policy
        self.storage.update(&existing_policy).await?;

        // Save new version if content changed
        if content_changed {
            self.storage.create_version(&existing_policy.current_version).await?;
        }

        // Log the update
        let change_descriptions = self.build_change_descriptions(&changes);
        self.auditor.log_policy_update(policy_id, &command.updated_by, change_descriptions).await?;

        info!("Policy updated successfully: {}", policy_id);

        Ok(UpdatePolicyResponse {
            policy_id: existing_policy.id,
            name: existing_policy.name,
            description: existing_policy.description,
            status: existing_policy.status,
            version: existing_policy.version,
            updated_at: existing_policy.updated_at.to_rfc3339(),
            updated_by: command.updated_by,
        })
    }

    fn build_change_descriptions(&self, changes: &PolicyUpdateChanges) -> Vec<String> {
        let mut descriptions = Vec::new();

        if changes.name_changed {
            descriptions.push("Policy name updated".to_string());
        }
        if changes.description_changed {
            descriptions.push("Policy description updated".to_string());
        }
        if changes.content_changed {
            descriptions.push("Policy content updated".to_string());
        }
        if changes.version_incremented {
            descriptions.push("Policy version incremented".to_string());
        }

        descriptions
    }
}

#[async_trait]
impl<PUV, PR, PUS, PUA> PolicyUpdater for UpdatePolicyUseCase<PUV, PR, PUS, PUA>
where
    PUV: PolicyUpdateValidator + Send + Sync,
    PR: PolicyRetriever + Send + Sync,
    PUS: PolicyUpdateStorage + Send + Sync,
    PUA: PolicyUpdateAuditor + Send + Sync,
{
    async fn update_policy(&self, policy_id: &PolicyId, command: UpdatePolicyCommand) -> Result<Policy, UpdatePolicyError> {
        // Execute the update and convert response back to policy
        let response = self.execute(policy_id, command).await?;

        // Reconstruct policy from response (simplified)
        Ok(Policy {
            id: response.policy_id,
            name: response.name,
            description: response.description,
            status: response.status,
            version: response.version,
            created_at: chrono::DateTime::parse_from_rfc3339(&response.updated_at)
                .map_err(|_| UpdatePolicyError::storage_error("Invalid timestamp"))?
                .with_timezone(&Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&response.updated_at)
                .map_err(|_| UpdatePolicyError::storage_error("Invalid timestamp"))?
                .with_timezone(&Utc),
            current_version: PolicyVersion {
                id: Hrn::new(&format!("{}/versions/{}", response.policy_id, response.version)).unwrap(),
                policy_id: response.policy_id,
                version: response.version,
                content: "".to_string(), // TODO: Need to retrieve from storage
                created_at: chrono::DateTime::parse_from_rfc3339(&response.updated_at)
                    .map_err(|_| UpdatePolicyError::storage_error("Invalid timestamp"))?
                    .with_timezone(&Utc),
                created_by: response.updated_by,
            },
        })
    }
}
