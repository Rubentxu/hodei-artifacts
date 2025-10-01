use crate::domain::actions;
use crate::domain::{HodeiEntity, HodeiEntityType, PolicyStorage, PolicyStore};
use cedar_policy::{
    Context, Entities, PolicySet, Request, Response, Schema, SchemaError, SchemaFragment,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crate::shared::generate_fragment_for_type;

pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: cedar_policy::EntityUid,
    pub resource: &'a dyn HodeiEntity,
    pub context: Context,
    pub entities: Vec<&'a dyn HodeiEntity>,
}

#[derive(Clone)]
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response {
        let entity_vec: Vec<cedar_policy::Entity> = request
            .entities
            .iter()
            .map(|entity| {
                let attrs = entity.attributes();
                let parents: HashSet<_> = entity.parents().into_iter().collect();
                cedar_policy::Entity::new(entity.euid(), attrs, parents)
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to create entities");

        let entities = Entities::from_entities(entity_vec, None)
            .expect("Failed to create Entities collection");

        let cedar_request = Request::new(
            request.principal.euid(),
            request.action.clone(),
            request.resource.euid(),
            request.context.clone(),
            None,
        )
            .expect("Failed to create Cedar request");

        let policies = self
            .store
            .get_current_policy_set()
            .await
            .unwrap_or_else(|_| PolicySet::new());
        cedar_policy::Authorizer::new().is_authorized(&cedar_request, &policies, &entities)
    }
}



#[derive(Default)]
pub struct EngineBuilder {
    partials: HashMap<&'static str, SchemaFragment>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_entity_type<T: HodeiEntityType + 'static>(
        &mut self,
    ) -> Result<&mut Self, Box<SchemaError>> {
        let frag = generate_fragment_for_type::<T>()?;
        self.partials.insert(T::entity_type_name(), frag);
        Ok(self)
    }

    pub fn build(
        self,
        storage: Arc<dyn PolicyStorage>,
    ) -> Result<(AuthorizationEngine, PolicyStore), Box<SchemaError>> {
        // Compose schema from base + registered partials + feature actions
        // Base provides fundamental types referenced by partials/actions
        let base = r#"
        entity Principal { };
        entity Resource { name: String };
        "#;
        let (base_frag, _) =
            SchemaFragment::from_cedarschema_str(base).expect("Base schema should be valid");

        let mut fragments: Vec<SchemaFragment> = Vec::new();
        fragments.push(base_frag);
        let has_partials = !self.partials.is_empty();
        fragments.extend(self.partials.into_values());

        // Add actions derived from feature directories only when there are registered partials
        if !fragments.is_empty() && has_partials {
            let actions_frag = actions::build_feature_actions_fragment()
                .expect("actions fragment should be valid");
            fragments.push(actions_frag);
        }

        let schema = Arc::new(Schema::from_schema_fragments(fragments)?);

        let store = PolicyStore::new(schema.clone(), storage);
        let engine = AuthorizationEngine {
            schema,
            store: store.clone(),
        };
        Ok((engine, store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::principals;
    use crate::domain::{Hrn, PolicyStorage, StorageError};
    use async_trait::async_trait;
    use serde_json::json;

    struct DummyStorage;

    #[async_trait]
    impl PolicyStorage for DummyStorage {
        async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn build_engine_with_minimal_schema() {
        let storage: Arc<dyn PolicyStorage> = Arc::new(DummyStorage);
        let builder = EngineBuilder::new();
        let res = builder.build(storage);
        assert!(res.is_ok());
    }

    #[derive(Debug)]
    struct TestEntity {
        hrn: Hrn,
        attrs: std::collections::HashMap<String, cedar_policy::RestrictedExpression>,
        parents: Vec<cedar_policy::EntityUid>,
    }

    impl TestEntity {
        fn new(hrn: Hrn) -> Self {
            Self {
                hrn,
                attrs: Default::default(),
                parents: vec![],
            }
        }
        fn with_attr(mut self, k: &str, v: &str) -> Self {
            self.attrs.insert(
                k.to_string(),
                cedar_policy::RestrictedExpression::new_string(v.to_string()),
            );
            self
        }
    }

    impl crate::domain::HodeiEntity for TestEntity {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(
            &self,
        ) -> std::collections::HashMap<String, cedar_policy::RestrictedExpression> {
            self.attrs.clone()
        }
        fn parents(&self) -> Vec<cedar_policy::EntityUid> {
            self.parents.clone()
        }
    }

    // Helper: build engine composing base + principals partials + feature actions
    fn build_engine_with_registered_entities_and_actions(
        storage: Arc<dyn PolicyStorage>,
    ) -> (AuthorizationEngine, PolicyStore) {
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
        builder.build(storage).expect("engine build")
    }

    #[tokio::test]
    async fn end_to_end_authorization_with_actions_principals_resources_and_context() {
        // Build a schema with User, Resource and an action evaluate_policy
        let schema_src = r#"
        entity User { name: String, email: String };
        entity Resource { name: String };
        action evaluate_policy appliesTo { principal: User, resource: Resource };
        "#;
        let (frag, _) =
            cedar_policy::SchemaFragment::from_cedarschema_str(schema_src).expect("schema");
        let schema = Arc::new(
            cedar_policy::Schema::from_schema_fragments(vec![frag]).expect("schema build"),
        );

        // Storage returns a policy permitting alice evaluate_policy on res1
        struct PolicyStorageAllow;
        #[async_trait]
        impl PolicyStorage for PolicyStorageAllow {
            async fn save_policy(
                &self,
                _policy: &cedar_policy::Policy,
            ) -> Result<(), StorageError> {
                Ok(())
            }
            async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
                Ok(true)
            }
            async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
                let policy_src = r#"permit(
                    principal == User::"alice",
                    action == Action::"evaluate_policy",
                    resource == Resource::"res1"
                );"#;
                let p: cedar_policy::Policy = policy_src.parse().expect("policy parse");
                Ok(vec![p])
            }
        }

        let storage: Arc<dyn PolicyStorage> = Arc::new(PolicyStorageAllow);
        let store = PolicyStore::new(schema.clone(), storage);
        let engine = AuthorizationEngine {
            schema: schema.clone(),
            store: store.clone(),
        };

        // Build principal and resource entities
        let principal = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "User".into(),
            "alice".into(),
        ))
            .with_attr("name", "Alice")
            .with_attr("email", "alice@example.com");
        let resource = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "Resource".into(),
            "res1".into(),
        ))
            .with_attr("name", "Res1");

        // Action via HRN helper
        let action_uid = Hrn::action("", "evaluate_policy").euid();

        // Context empty
        let ctx = cedar_policy::Context::empty();

        let request = AuthorizationRequest {
            principal: &principal,
            action: action_uid,
            resource: &resource,
            context: ctx,
            entities: vec![&principal, &resource],
        };

        let resp = engine.is_authorized(&request).await;
        assert_eq!(resp.decision(), cedar_policy::Decision::Allow);
    }

    // Storage that allows create_policy on res1 and requires context.ip==127.0.0.1
    struct StorageAllowWithContext;
    #[async_trait]
    impl PolicyStorage for StorageAllowWithContext {
        async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
            let policy_src = r#"permit(
                principal == User::"alice",
                action == Action::"create_policy",
                resource == Resource::"res1"
            ) when { context.ip == "127.0.0.1" };"#;
            let p: cedar_policy::Policy = policy_src.parse().expect("policy parse");
            Ok(vec![p])
        }
    }

    // Storage that allows create_policy on res1 without context condition
    struct StorageAllowCreatePolicy;
    #[async_trait]
    impl PolicyStorage for StorageAllowCreatePolicy {
        async fn save_policy(&self, _policy: &cedar_policy::Policy) -> Result<(), StorageError> {
            Ok(())
        }
        async fn delete_policy(&self, _id: &str) -> Result<bool, StorageError> {
            Ok(true)
        }
        async fn load_all_policies(&self) -> Result<Vec<cedar_policy::Policy>, StorageError> {
            let policy_src = r#"permit(
                principal == User::"alice",
                action == Action::"create_policy",
                resource == Resource::"res1"
            );"#;
            let p: cedar_policy::Policy = policy_src.parse().expect("policy parse");
            Ok(vec![p])
        }
    }

    #[tokio::test]
    async fn engine_with_domain_fragments_allows_matching_request() {
        let (engine, _store) =
            build_engine_with_registered_entities_and_actions(Arc::new(StorageAllowCreatePolicy));

        let principal = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "User".into(),
            "alice".into(),
        ))
            .with_attr("name", "Alice");
        let resource = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "Resource".into(),
            "res1".into(),
        ))
            .with_attr("name", "Res1");
        let action_uid = Hrn::action("", "create_policy").euid();

        let req = AuthorizationRequest {
            principal: &principal,
            action: action_uid,
            resource: &resource,
            context: cedar_policy::Context::empty(),
            entities: vec![&principal, &resource],
        };

        let resp = engine.is_authorized(&req).await;
        assert_eq!(resp.decision(), cedar_policy::Decision::Allow);
    }

    #[tokio::test]
    async fn engine_with_domain_fragments_denies_mismatched_resource() {
        let (engine, _store) =
            build_engine_with_registered_entities_and_actions(Arc::new(StorageAllowCreatePolicy));

        let principal = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "User".into(),
            "alice".into(),
        ));
        let resource = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "Resource".into(),
            "res2".into(),
        ));
        let action_uid = Hrn::action("", "create_policy").euid();

        let req = AuthorizationRequest {
            principal: &principal,
            action: action_uid,
            resource: &resource,
            context: cedar_policy::Context::empty(),
            entities: vec![&principal, &resource],
        };

        let resp = engine.is_authorized(&req).await;
        assert_eq!(resp.decision(), cedar_policy::Decision::Deny);
    }

    #[tokio::test]
    async fn engine_allows_when_context_condition_matches() {
        let (engine, _store) =
            build_engine_with_registered_entities_and_actions(Arc::new(StorageAllowWithContext));

        let principal = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "User".into(),
            "alice".into(),
        ));
        let resource = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "Resource".into(),
            "res1".into(),
        ));
        let action_uid = Hrn::action("", "create_policy").euid();

        let ctx =
            cedar_policy::Context::from_json_value(json!({"ip": "127.0.0.1"}), None).expect("ctx");

        let req = AuthorizationRequest {
            principal: &principal,
            action: action_uid,
            resource: &resource,
            context: ctx,
            entities: vec![&principal, &resource],
        };

        let resp = engine.is_authorized(&req).await;
        assert_eq!(resp.decision(), cedar_policy::Decision::Allow);
    }

    #[tokio::test]
    async fn engine_denies_when_context_condition_not_met() {
        let (engine, _store) =
            build_engine_with_registered_entities_and_actions(Arc::new(StorageAllowWithContext));

        let principal = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "User".into(),
            "alice".into(),
        ));
        let resource = TestEntity::new(Hrn::new(
            "default".into(),
            "hodei".into(),
            "".into(),
            "Resource".into(),
            "res1".into(),
        ));
        let action_uid = Hrn::action("", "create_policy").euid();

        let ctx =
            cedar_policy::Context::from_json_value(json!({"ip": "10.0.0.1"}), None).expect("ctx");

        let req = AuthorizationRequest {
            principal: &principal,
            action: action_uid,
            resource: &resource,
            context: ctx,
            entities: vec![&principal, &resource],
        };

        let resp = engine.is_authorized(&req).await;
        assert_eq!(resp.decision(), cedar_policy::Decision::Deny);
    }
}
