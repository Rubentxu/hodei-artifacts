use policies::domain::{actions, principals, HodeiEntityType, EngineBuilder, PolicyStore, PolicyStorage, StorageError};
use cedar_policy::{Schema, SchemaFragment};
use std::sync::Arc;
use async_trait::async_trait;

#[test]
fn partial_schemas_exist_and_compose() {
    // Base schema with Principal/Resource required by actions and entities
    let base = r#"
    entity Principal { };
    entity Resource { name: String };
    "#;
    let (base_frag, _) = SchemaFragment::from_cedarschema_str(base).expect("base schema");

    // Partials from each HodeiEntityType in principals
    let user = principals::User::partial_schema().expect("user partial");
    let group = principals::Group::partial_schema().expect("group partial");
    let sa = principals::ServiceAccount::partial_schema().expect("serviceaccount partial");
    let ns = principals::Namespace::partial_schema().expect("namespace partial");

    // Actions for feature directories
    let actions_frag = actions::build_feature_actions_fragment().expect("actions fragment");

    // Compose full schema
    let schema = Schema::from_schema_fragments(vec![
        base_frag, user, group, sa, ns, actions_frag,
    ]);

    assert!(schema.is_ok(), "composed schema should be valid: {:?}", schema.err());
}

struct DummyStorage;

#[async_trait]
impl PolicyStorage for DummyStorage {
    async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> { Ok(()) }
    async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> { Ok(true) }
    async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> { Ok(vec![]) }
}

#[tokio::test]
async fn engine_builder_registers_all_entities_and_builds() {
    let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);

    let mut builder = EngineBuilder::new();
    builder
        .register_entity_type::<principals::User>().expect("register user")
        .register_entity_type::<principals::Group>().expect("register group")
        .register_entity_type::<principals::ServiceAccount>().expect("register sa")
        .register_entity_type::<principals::Namespace>().expect("register ns");

    let res = builder.build(storage);
    assert!(res.is_ok(), "engine build should succeed: {:?}", res.err());
}
