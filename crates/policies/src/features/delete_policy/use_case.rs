use std::sync::Arc;

use super::dto::DeletePolicyCommand;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum DeletePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct DeletePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl DeletePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &DeletePolicyCommand) -> Result<bool, DeletePolicyError> {
        // Validate command
        cmd.validate()
            .map_err(|e| DeletePolicyError::InvalidCommand(e.to_string()))?;

        // Delete policy from store
        let deleted = self
            .store
            .remove_policy(&cmd.policy_id)
            .await
            .map_err(DeletePolicyError::Storage)?;

        // Return error if policy was not found
        if !deleted {
            return Err(DeletePolicyError::NotFound(cmd.policy_id.clone()));
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;
    use cedar_policy::Policy;
    use std::sync::Arc;

    #[tokio::test]
    async fn delete_policy_removes_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Verify it exists
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());

        // Now delete it
        let uc = DeletePolicyUseCase::new(store.clone());
        let cmd = DeletePolicyCommand::new(policy_id.clone());
        let result = uc.execute(&cmd).await.expect("delete policy");

        assert!(result);

        // Verify it's gone
        let retrieved_after = store
            .get_policy(&policy_id)
            .await
            .expect("get policy after delete");
        assert!(retrieved_after.is_none());
    }

    #[tokio::test]
    async fn delete_policy_returns_not_found_for_nonexistent_policy() {
        // Build engine/store with real mem storage and schema
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn delete_policy_validates_whitespace_only_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::no_entities_configurator)
            .await
            .expect("build engine");

        let uc = DeletePolicyUseCase::new(store);
        let cmd = DeletePolicyCommand::new("   ");
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(DeletePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }
}
