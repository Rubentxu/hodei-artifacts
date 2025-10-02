use async_trait::async_trait;
use policies::domain::{EngineBuilder, PolicyStorage, StorageError, principals};
use std::sync::Arc;

struct DummyStorage;

#[async_trait]
impl PolicyStorage for DummyStorage {
    async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
        Ok(())
    }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn get_policy_by_id(
        &self,
        _id: &str,
    ) -> Result<Option<cedar_policy::Policy>, StorageError> {
        Ok(None)
    }
    async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn engine_builder_registers_all_entities_and_builds() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_entity_type::<principals::User>()
        .expect("register user")
        .register_entity_type::<principals::Group>()
        .expect("register group")
        .register_entity_type::<principals::ServiceAccount>()
        .expect("register sa")
        .register_entity_type::<principals::Namespace>()
        .expect("register ns");

    let res = builder.build(storage);
    assert!(res.is_ok(), "engine build should succeed: {:?}", res.err());
}
