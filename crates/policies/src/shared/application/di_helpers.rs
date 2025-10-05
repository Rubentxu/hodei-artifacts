// di_helpers.rs (LEGACY)
//
// Este módulo se marca como legacy mientras se refactoriza el crate `policies`.
// - Todo el contenido original que dependía de tipos antiguos (AuthorizationEngine, PolicyStore, etc.)
//   queda detrás de la feature flag `legacy_infra`.
// - Para compilaciones normales (sin `legacy_infra`) se exponen únicamente stubs mínimos usados
//   por tests o módulos que todavía referencian `test_helpers::test_entities_configurator`.
//
// Próximos pasos:
// 1. Eliminar dependencias a Cedar directas desde aquí.
// 2. Reintroducir un builder alineado con los nuevos traits del kernel si sigue siendo necesario.

#[cfg(feature = "legacy_infra")]
use crate::shared::application::{AuthorizationEngine, EngineBuilder, PolicyStore};
#[cfg(feature = "legacy_infra")]
use crate::shared::infrastructure::surreal::SurrealMemStorage;
#[cfg(feature = "legacy_infra")]
use anyhow::Result;
#[cfg(feature = "legacy_infra")]
use kernel::PolicyStorage;
#[cfg(feature = "legacy_infra")]
use std::sync::Arc;

#[cfg(all(feature = "legacy_infra", feature = "embedded"))]
use crate::shared::infrastructure::surreal::SurrealEmbeddedStorage;

#[cfg(feature = "legacy_infra")]
/// Build an AuthorizationEngine with a custom EngineBuilder configurator (LEGACY)
pub async fn build_engine_mem<F>(
    _configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    // Implementación legacy deshabilitada temporalmente
    unimplemented!("legacy_infra: build_engine_mem deshabilitado durante refactor");
}

#[cfg(all(feature = "legacy_infra", feature = "embedded"))]
/// Build an AuthorizationEngine with embedded storage (LEGACY)
pub async fn build_engine_embedded<F>(
    _path: &str,
    _configurator: F,
) -> Result<(Arc<AuthorizationEngine>, Arc<PolicyStore>)>
where
    F: FnOnce(EngineBuilder) -> Result<EngineBuilder>,
{
    unimplemented!("legacy_infra: build_engine_embedded deshabilitado durante refactor");
}

#[cfg(feature = "legacy_infra")]
pub fn no_entities_configurator(builder: EngineBuilder) -> anyhow::Result<EngineBuilder> {
    Ok(builder)
}

#[cfg(feature = "legacy_infra")]
pub mod test_helpers {
    use super::*;
    use anyhow::Result;
    use cedar_policy::{EntityTypeName, EntityUid, RestrictedExpression};
    use kernel::{
        ActionTrait, AttributeType, HodeiEntity, HodeiEntityType, Hrn, Principal, Resource,
    };
    use std::collections::HashMap;
    use std::str::FromStr;

    // Version legacy — mantiene firmas antiguas, pero marcadas como no operativas
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
            vec![("email", AttributeType::string())]
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
            vec![("name", AttributeType::string())]
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

    pub struct TestAccessAction;
    impl ActionTrait for TestAccessAction {
        fn name() -> &'static str {
            "access"
        }
        fn applies_to() -> (EntityTypeName, EntityTypeName) {
            let principal_type = EntityTypeName::from_str("Test::Principal").expect("principal");
            let resource_type = EntityTypeName::from_str("Test::Resource").expect("resource");
            (principal_type, resource_type)
        }
    }

    pub fn test_entities_configurator(
        builder: crate::shared::application::EngineBuilder,
    ) -> Result<crate::shared::application::EngineBuilder> {
        Ok(builder)
    }
}

#[cfg(not(feature = "legacy_infra"))]
pub mod test_helpers {
    // Stubs mínimos para poder compilar mientras se elimina infraestructura legacy.
    use crate::shared::application::EngineBuilder;
    use anyhow::Result;

    /// Configurador vacío (no registra entidades) – usado por tests hasta que se reemplace.
    pub fn test_entities_configurator(builder: EngineBuilder) -> Result<EngineBuilder> {
        Ok(builder)
    }
}
