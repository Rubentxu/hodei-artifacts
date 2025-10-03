use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::GetPolicyQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum GetPolicyError {
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct GetPolicyUseCase {
    store: Arc<PolicyStore>,
}

impl GetPolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, query: &GetPolicyQuery) -> Result<Policy, GetPolicyError> {
        // Validate query
        query
            .validate()
            .map_err(|e| GetPolicyError::InvalidQuery(e.to_string()))?;

        // Get policy from store
        let policy = self
            .store
            .get_policy(&query.policy_id)
            .await
            .map_err(GetPolicyError::Storage)?;

        // Return policy or error if not found
        policy.ok_or_else(|| GetPolicyError::NotFound(query.policy_id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::di_helpers;

    #[tokio::test]
    async fn get_policy_returns_policy_when_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        // First, create a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();

        store.add_policy(policy.clone()).await.expect("add policy");

        // Now get it
        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new(policy_id.clone());
        let retrieved = uc.execute(&query).await.expect("get policy");

        assert_eq!(retrieved.id().to_string(), policy_id);
        assert_eq!(retrieved.to_string(), policy.to_string());
    }

    #[tokio::test]
    async fn get_policy_returns_none_when_not_exists() {
        // Build engine/store with real mem storage (no entities registered - domain agnostic)
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("nonexistent_policy_id");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn get_policy_validates_empty_id() {
        let (_engine, store) = di_helpers::build_engine_mem(di_helpers::test_helpers::test_entities_configurator)
            .await
            .expect("build engine");

        let uc = GetPolicyUseCase::new(store);
        let query = GetPolicyQuery::new("");
        let result = uc.execute(&query).await;

        assert!(result.is_err());
        match result {
            Err(GetPolicyError::InvalidQuery(_)) => {}
            _ => panic!("Expected InvalidQuery error"),
        }
    }
}
