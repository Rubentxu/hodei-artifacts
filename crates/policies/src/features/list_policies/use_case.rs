use std::sync::Arc;

use cedar_policy::Policy;

use super::dto::ListPoliciesQuery;
use crate::shared::application::PolicyStore;

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesError {
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct ListPoliciesUseCase {
    store: Arc<PolicyStore>,
}

impl ListPoliciesUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        query: &ListPoliciesQuery,
    ) -> Result<Vec<Policy>, ListPoliciesError> {
        // Validate query
        query
            .validate()
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Get all policies from store
        let policy_set = self
            .store
            .get_current_policy_set()
            .await
            .map_err(|e| ListPoliciesError::Storage(e.to_string()))?;

        // Convert PolicySet to Vec<Policy>
        let mut policies: Vec<Policy> = policy_set.policies().cloned().collect();

        // Apply filter if specified
        if let Some(filter_id) = &query.filter_id {
            policies.retain(|p| p.id().to_string().contains(filter_id));
        }

        // Apply pagination if specified
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(policies.len());

        // Skip offset items and take limit items
        let paginated_policies: Vec<Policy> =
            policies.into_iter().skip(offset).take(limit).collect();

        Ok(paginated_policies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::EngineBuilder;
    use crate::shared::domain::principals;
    use crate::shared::infrastructure::surreal::SurrealMemStorage;
    use std::sync::Arc;

    #[tokio::test]
    async fn list_policies_returns_empty_when_no_policies() {
        // Build engine/store with real mem storage and schema
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_returns_single_policy_after_adding() {
        // Build engine/store with real mem storage and schema
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        // List policies - should have 1
        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1, "Expected 1 policy after adding one");
        assert_eq!(policies[0].id().to_string(), policy_id);
        assert_eq!(policies[0].to_string().trim(), policy.to_string().trim());
    }

    #[tokio::test]
    async fn list_policies_works_with_valid_cedar_policies() {
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        // Add a policy with conditions
        let conditional_policy_src = r#"
            permit(
                principal,
                action,
                resource
            ) when {
                principal has email
            };
        "#;
        let conditional_policy: Policy = conditional_policy_src
            .parse()
            .expect("parse conditional policy");
        store
            .add_policy(conditional_policy.clone())
            .await
            .expect("add conditional policy");

        let uc = ListPoliciesUseCase::new(store);
        let query = ListPoliciesQuery::new();
        let policies = uc.execute(&query).await.expect("list policies");

        assert_eq!(policies.len(), 1);
        assert_eq!(
            policies[0].id().to_string(),
            conditional_policy.id().to_string()
        );
    }

    #[tokio::test]
    async fn list_policies_with_pagination() {
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with limit
        let query = ListPoliciesQuery::with_pagination(0, 10);
        let policies = uc.execute(&query).await.expect("list policies");
        assert_eq!(policies.len(), 1);

        // Test with offset that skips all
        let query_skip = ListPoliciesQuery::with_pagination(1, 10);
        let policies_skip = uc
            .execute(&query_skip)
            .await
            .expect("list policies with skip");
        assert_eq!(policies_skip.len(), 0);
    }

    #[tokio::test]
    async fn list_policies_validates_limit() {
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        let uc = ListPoliciesUseCase::new(store);

        // Test with invalid limit (0)
        let query_zero = ListPoliciesQuery::with_pagination(0, 0);
        let result_zero = uc.execute(&query_zero).await;
        assert!(result_zero.is_err());

        // Test with invalid limit (> 1000)
        let query_large = ListPoliciesQuery::with_pagination(0, 1001);
        let result_large = uc.execute(&query_large).await;
        assert!(result_large.is_err());
    }

    #[tokio::test]
    async fn list_policies_with_filter() {
        let (engine, _store) = {
            let mut builder = EngineBuilder::new();
            builder
                .register_entity_type::<principals::User>()
                .expect("user")
                .register_entity_type::<principals::Group>()
                .expect("group");
            let storage = Arc::new(SurrealMemStorage::new("ns", "db").await.expect("mem db"));
            builder.build(storage).expect("engine build")
        };
        let store = Arc::new(engine.store.clone());

        // Add a policy
        let policy_src = r#"permit(principal, action, resource);"#;
        let policy: Policy = policy_src.parse().expect("parse policy");
        let policy_id = policy.id().to_string();
        store.add_policy(policy.clone()).await.expect("add policy");

        let uc = ListPoliciesUseCase::new(store);

        // Test with matching filter
        let query_match = ListPoliciesQuery::with_filter(policy_id.clone());
        let policies_match = uc
            .execute(&query_match)
            .await
            .expect("list policies with filter");
        assert_eq!(policies_match.len(), 1);

        // Test with non-matching filter
        let query_no_match = ListPoliciesQuery::with_filter("nonexistent".to_string());
        let policies_no_match = uc
            .execute(&query_no_match)
            .await
            .expect("list policies with no match");
        assert_eq!(policies_no_match.len(), 0);
    }
}
