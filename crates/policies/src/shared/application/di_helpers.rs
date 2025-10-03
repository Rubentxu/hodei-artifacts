use crate::shared::application::{AuthorizationEngine, EngineBuilder, PolicyStore};
use crate::shared::domain::PolicyStorage;
use crate::shared::infrastructure::surreal::SurrealMemStorage;
use anyhow::Result;
/// Centralized DI helpers to avoid code duplication across features
/// 
/// This module provides reusable functions for building engines and storage,
/// allowing features to focus on their specific use case construction.

use std::sync::Arc;

#[cfg(feature = "embedded")]
use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses in-memory storage (default dev/test)
pub async fn build_engine_mem<F>(configurator: F) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealMemStorage::new("policies", "policies").await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// Build an AuthorizationEngine with a custom EngineBuilder configurator
/// Uses embedded storage (RocksDB)
#[cfg(feature = "embedded")]
pub async fn build_engine_embedded<F>(
    path: &str,
    configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    let storage: Arc<dyn PolicyStorage> = Arc::new(SurrealEmbeddedStorage::new("policies", "policies", path).await?);
    
    let builder = EngineBuilder::new();
    let builder = configurator(builder)?;
    let (engine, store) = builder.build(storage.clone())?;
    
    Ok((Arc::new(engine), Arc::new(store)))
}

/// No-op configurator - creates an engine with NO entities registered (domain agnostic)
pub fn no_entities_configurator(builder: EngineBuilder) -> Result<EngineBuilder> {
    Ok(builder)
}

/// Test helpers module - provides reusable test entities and configurators
/// Available in both test and non-test builds for integration tests and examples
pub mod test_helpers {
    use super::*;
    use crate::shared::domain::ports::{Action, AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
    use crate::shared::Hrn;
    use cedar_policy::{EntityTypeName, EntityUid, RestrictedExpression};
    use std::collections::HashMap;
    use std::str::FromStr;

    // Test Principal type
    pub struct TestPrincipal {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestPrincipal {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Principal"
        }
        fn is_principal_type() -> bool {
            true
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("email", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestPrincipal {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Principal for TestPrincipal {}

    // Test Resource type
    pub struct TestResource {
        pub hrn: Hrn,
    }

    impl HodeiEntityType for TestResource {
        fn service_name() -> &'static str {
            "test"
        }
        fn resource_type_name() -> &'static str {
            "Resource"
        }
        fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
            vec![("name", AttributeType::Primitive("String"))]
        }
    }

    impl HodeiEntity for TestResource {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }
        fn attributes(&self) -> HashMap<String, RestrictedExpression> {
            HashMap::new()
        }
        fn parents(&self) -> Vec<EntityUid> {
            Vec::new()
        }
    }

    impl Resource for TestResource {}

    // Test Action
    pub struct TestAccessAction;

    impl Action for TestAccessAction {
        fn name() -> &'static str {
            "access"
        }
        fn applies_to() -> (EntityTypeName, EntityTypeName) {
            let principal_type = EntityTypeName::from_str("Test::Principal")
                .expect("Valid principal type");
            let resource_type = EntityTypeName::from_str("Test::Resource")
                .expect("Valid resource type");
            (principal_type, resource_type)
        }
    }

    /// Configurator for tests - registers basic test entities and actions
    pub fn test_entities_configurator(mut builder: EngineBuilder) -> Result<EngineBuilder> {
        builder
            .register_principal::<TestPrincipal>()?
            .register_resource::<TestResource>()?
            .register_action::<TestAccessAction>()?;
        Ok(builder)
    }
}
