use std::sync::Arc;

use cedar_policy::Policy;

use crate::shared::application::PolicyStore;
use super::dto::UpdatePolicyCommand;

#[derive(Debug, thiserror::Error)]
pub enum UpdatePolicyError {
    #[error("invalid_command: {0}")]
    InvalidCommand(String),
    #[error("policy_not_found: {0}")]
    NotFound(String),
    #[error("policy_parse_error: {0}")]
    ParseError(String),
    #[error("validation_error: {0}")]
    ValidationError(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct UpdatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl UpdatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        cmd: &UpdatePolicyCommand,
    ) -> Result<Policy, UpdatePolicyError> {
        // 1. Validar comando
        cmd.validate()
            .map_err(|e| UpdatePolicyError::InvalidCommand(e.to_string()))?;

        // 2. Verificar que la política existe
        let existing = self
            .store
            .get_policy(&cmd.policy_id)
            .await
            .map_err(UpdatePolicyError::Storage)?;

        if existing.is_none() {
            return Err(UpdatePolicyError::NotFound(cmd.policy_id.clone()));
        }

        // 3. Parsear nueva política
        let new_policy: Policy = cmd
            .new_policy_content
            .parse()
            .map_err(|e| UpdatePolicyError::ParseError(format!("{}", e)))?;

        // 4. Actualizar política (esto valida automáticamente)
        self.store
            .update_policy(&cmd.policy_id, new_policy.clone())
            .await
            .map_err(UpdatePolicyError::ValidationError)?;

        Ok(new_policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::EngineBuilder;
    use crate::shared::domain::principals;
    use crate::shared::infrastructure::surreal::SurrealMemStorage;

    #[tokio::test]
    async fn update_policy_successfully_updates_existing_policy() {
        // Arrange: Create engine/store and add a policy
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

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        // Act: Update the policy
        let uc = UpdatePolicyUseCase::new(store.clone());
        let new_content = r#"forbid(principal, action, resource);"#;
        let cmd = UpdatePolicyCommand::new(policy_id.clone(), new_content.to_string());
        let result = uc.execute(&cmd).await;

        // Assert: Should succeed
        assert!(result.is_ok());
        let updated_policy = result.unwrap();
        assert_eq!(updated_policy.to_string().trim(), new_content.trim());

        // Verify the policy was actually updated in storage
        let retrieved = store.get_policy(&policy_id).await.expect("get policy");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().to_string().trim(), new_content.trim());
    }

    #[tokio::test]
    async fn update_policy_returns_not_found_for_nonexistent_policy() {
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

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "nonexistent_policy_id".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::NotFound(id)) => {
                assert_eq!(id, "nonexistent_policy_id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_id() {
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

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            "".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_empty_content() {
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

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new("policy_id".to_string(), "".to_string());
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::InvalidCommand(_)) => {}
            _ => panic!("Expected InvalidCommand error"),
        }
    }

    #[tokio::test]
    async fn update_policy_validates_new_policy_syntax() {
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

        // Add original policy
        let original_policy_src = r#"permit(principal, action, resource);"#;
        let original_policy: Policy = original_policy_src.parse().expect("parse original");
        let policy_id = original_policy.id().to_string();
        store
            .add_policy(original_policy.clone())
            .await
            .expect("add original policy");

        let uc = UpdatePolicyUseCase::new(store);
        let cmd = UpdatePolicyCommand::new(
            policy_id,
            "this is not valid cedar syntax".to_string(),
        );
        let result = uc.execute(&cmd).await;

        assert!(result.is_err());
        match result {
            Err(UpdatePolicyError::ParseError(_)) => {}
            _ => panic!("Expected ParseError"),
        }
    }
}
