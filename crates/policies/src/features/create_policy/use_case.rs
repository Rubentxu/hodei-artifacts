use std::sync::Arc;

use cedar_policy::Policy;

use crate::shared::application::PolicyStore;
use super::dto::CreatePolicyCommand;

#[derive(Debug, thiserror::Error)]
pub enum CreatePolicyError {
    #[error("invalid_policy: {0}")]
    InvalidPolicy(String),
    #[error("storage_error: {0}")]
    Storage(String),
}

pub struct CreatePolicyUseCase {
    store: Arc<PolicyStore>,
}

impl CreatePolicyUseCase {
    pub fn new(store: Arc<PolicyStore>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, cmd: &CreatePolicyCommand) -> Result<(), CreatePolicyError> {
        // Validate command
        cmd.validate().map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;

        let policy: Policy = cmd.policy_src
            .parse::<Policy>()
            .map_err(|e| CreatePolicyError::InvalidPolicy(e.to_string()))?;
        self
            .store
            .add_policy(policy)
            .await
            .map_err(CreatePolicyError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::application::EngineBuilder;
    use crate::shared::infrastructure::surreal::SurrealMemStorage;
    use crate::shared::domain::principals;
    use std::sync::Arc;

    #[tokio::test]
    async fn create_policy_persists_in_surreal_mem() {
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

        let uc = CreatePolicyUseCase::new(store);
        let cmd = crate::features::create_policy::dto::CreatePolicyCommand::new(
            r#"permit(principal, action, resource);"#,
        );
        uc.execute(&cmd).await.expect("create policy");

        // Ensure it's in the current set
        let pset = engine
            .store
            .get_current_policy_set()
            .await
            .expect("policy set");
        assert!(pset.to_cedar().is_some());
    }
}
