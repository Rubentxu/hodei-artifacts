use async_trait::async_trait;
use cedar_policy::{EntityUid, RestrictedExpression};
use kernel::Hrn;
use kernel::{
    AttributeType, HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStorageError as StorageError,
    Principal, Resource,
};
use policies::shared::application::EngineBuilder;
use std::collections::HashMap;
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

// Tipos de prueba locales (sustituyen a principals::{User, Group} que ahora viven en IAM)
struct TestUser {
    hrn: Hrn,
}

struct TestGroup {
    hrn: Hrn,
}

// Implementación de HodeiEntityType para TestUser
impl HodeiEntityType for TestUser {
    fn service_name() -> &'static str {
        "iam" // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "User"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("email", AttributeType::Primitive("String")),
        ]
    }
    fn is_principal_type() -> bool {
        true
    }
}

// Implementación de HodeiEntity para TestUser
impl HodeiEntity for TestUser {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Principal para TestUser
impl Principal for TestUser {}

// Implementación de HodeiEntityType para TestGroup
impl HodeiEntityType for TestGroup {
    fn service_name() -> &'static str {
        "iam" // Debe estar en minúsculas según la convención
    }
    fn resource_type_name() -> &'static str {
        "Group"
    }
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![("name", AttributeType::Primitive("String"))]
    }
}

// Implementación de HodeiEntity para TestGroup
impl HodeiEntity for TestGroup {
    fn hrn(&self) -> &kernel::Hrn {
        &self.hrn
    }
    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        HashMap::new()
    }
    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

// Marker trait Resource para TestGroup
impl Resource for TestGroup {}

#[tokio::test]
async fn engine_builder_registers_dummy_entities_and_builds() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_principal::<TestUser>()
        .expect("register TestUser")
        .register_resource::<TestGroup>()
        .expect("register TestGroup");

    let res = builder.build(storage);
    assert!(res.is_ok(), "engine build should succeed: {:?}", res.err());
}
