use crate::domain::{PolicyStorage, StorageError};
use cedar_policy::{Policy, PolicySet, Schema, Validator};
use std::sync::Arc;

#[derive(Clone)]
pub struct PolicyStore {
    storage: Arc<dyn PolicyStorage>,
    validator: Validator,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PolicyStorage;
    use async_trait::async_trait;

    #[derive(Clone)]
    struct DummyStorage;

    #[async_trait]
    impl PolicyStorage for DummyStorage {
        async fn save_policy(&self, _policy: &Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn get_policy_by_id(&self, _id: &str) -> Result<Option<Policy>, StorageError> {
            Ok(None)
        }
        async fn load_all_policies(&self) -> Result<Vec<Policy>, StorageError> {
            Ok(vec![])
        }
    }

    fn minimal_schema() -> Arc<Schema> {
        let minimal_schema = r#"
        entity Principal { };
        action access appliesTo {
            principal: Principal,
            resource: Principal
        };
        "#;
        let (fragment, _) = cedar_policy::SchemaFragment::from_cedarschema_str(minimal_schema)
            .expect("minimal schema valid");
        Arc::new(Schema::from_schema_fragments(vec![fragment]).expect("schema build"))
    }

    #[tokio::test]
    async fn get_current_policy_set_returns_empty_with_dummy_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let pset = store.get_current_policy_set().await.expect("policy set");
        // Rendering should be possible
        let rendered = pset.to_cedar();
        assert!(rendered.is_some());
    }

    #[tokio::test]
    async fn remove_policy_calls_storage() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let store = PolicyStore::new(minimal_schema(), storage);
        let removed = store.remove_policy("any").await.expect("remove ok");
        assert!(removed);
    }
}

impl PolicyStore {
    pub fn new(schema: Arc<Schema>, storage: Arc<dyn PolicyStorage>) -> Self {
        Self {
            storage,
            validator: Validator::new(schema.as_ref().clone()),
        }
    }

    pub async fn add_policy(&self, policy: Policy) -> Result<(), String> {
        // Build a PolicySet containing the single policy to validate
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy to set: {}", e))?;

        // Validate the policy set using Cedar's validator
        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            self.storage
                .save_policy(&policy)
                .await
                .map_err(|e| e.to_string())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Policy validation failed: {}", errors.join(", ")))
        }
    }

    pub async fn remove_policy(&self, id: &str) -> Result<bool, String> {
        self.storage
            .delete_policy(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_current_policy_set(&self) -> Result<PolicySet, StorageError> {
        let policies = self.storage.load_all_policies().await?;
        let mut policy_set = PolicySet::new();
        for policy in policies {
            policy_set
                .add(policy)
                .map_err(|e| StorageError::ParsingError(e.to_string()))?;
        }
        Ok(policy_set)
    }

    pub async fn get_policy(&self, id: &str) -> Result<Option<Policy>, String> {
        self.storage
            .get_policy_by_id(id)
            .await
            .map_err(|e| e.to_string())
    }

    /// Update an existing policy by removing the old one and adding the new one
    pub async fn update_policy(&self, old_id: &str, new_policy: Policy) -> Result<(), String> {
        // Eliminar política antigua
        let removed = self.remove_policy(old_id).await?;
        if !removed {
            return Err(format!("Policy '{}' not found", old_id));
        }

        // Agregar nueva política (esto valida automáticamente)
        self.add_policy(new_policy).await
    }

    /// Validate a policy without persisting it
    pub fn validate_policy(&self, policy: &Policy) -> Result<(), String> {
        let mut pset = PolicySet::new();
        pset.add(policy.clone())
            .map_err(|e| format!("Failed to add policy: {}", e))?;

        let validation_result = self
            .validator
            .validate(&pset, cedar_policy::ValidationMode::default());

        if validation_result.validation_passed() {
            Ok(())
        } else {
            let errors: Vec<String> = validation_result
                .validation_errors()
                .map(|e| e.to_string())
                .collect();
            Err(format!("Validation failed: {}", errors.join(", ")))
        }
    }
}
