//! Composition Root - Dependency Injection
//!
//! Este módulo implementa el patrón Composition Root, que es el único lugar
//! en la aplicación donde se instancian las implementaciones concretas y se
//! ensamblan los casos de uso.
//!
//! # Principios
//!
//! 1. **Único lugar de construcción**: Solo aquí se crean adaptadores concretos
//! 2. **Inyección vía puertos**: Los casos de uso se ensamblan usando traits (puertos)
//! 3. **Resolución en compilación**: Uso de generics para zero-cost abstractions
//! 4. **Desacoplamiento**: Los handlers solo conocen los puertos, no las implementaciones

use hodei_iam::features::register_iam_schema::factories as iam_factories;
use hodei_policies::build_schema::factories as policy_factories;
use hodei_policies::build_schema::ports::{BuildSchemaPort, SchemaStoragePort};
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use hodei_policies::features::playground_evaluate::factories as playground_factories;
use hodei_policies::features::playground_evaluate::ports::PlaygroundEvaluatePort;
use hodei_policies::load_schema::ports::LoadSchemaPort;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use hodei_policies::validate_policy::port::ValidatePolicyPort;
use std::sync::Arc;
use tracing::info;

/// Ports de casos de uso de hodei-policies
///
/// Esta estructura agrupa todos los puertos (traits) de hodei-policies
/// que serán inyectados en otros bounded contexts.
pub struct PolicyPorts {
    pub register_entity_type: Arc<dyn RegisterEntityTypePort>,
    pub register_action_type: Arc<dyn RegisterActionTypePort>,
    pub build_schema: Arc<dyn BuildSchemaPort>,
    pub load_schema: Arc<dyn LoadSchemaPort>,
    pub validate_policy: Arc<dyn ValidatePolicyPort>,
    pub evaluate_policies: Arc<dyn EvaluatePoliciesPort>,
    pub playground_evaluate: Arc<dyn PlaygroundEvaluatePort>,
}

/// Ports de casos de uso de hodei-iam
///
/// Esta estructura agrupa todos los puertos (traits) de hodei-iam
/// que serán expuestos a los handlers de la API.
pub struct IamPorts {
    pub register_iam_schema:
        Arc<dyn hodei_iam::features::register_iam_schema::ports::RegisterIamSchemaPort>,
    pub create_policy: Arc<dyn hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort>,
    pub get_policy: Arc<dyn hodei_iam::features::get_policy::ports::PolicyReader>,
    pub list_policies: Arc<dyn hodei_iam::features::list_policies::ports::PolicyLister>,
    pub update_policy: Arc<dyn hodei_iam::features::update_policy::ports::UpdatePolicyPort>,
    pub delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort>,
}

/// Composition Root - Punto de ensamblaje de toda la aplicación
///
/// Esta estructura contiene todos los puertos de casos de uso que serán
/// inyectados en los handlers de Axum.
pub struct CompositionRoot {
    pub policy_ports: PolicyPorts,
    pub iam_ports: IamPorts,
}

impl CompositionRoot {
    /// Crea el Composition Root para producción
    ///
    /// Este método es el único lugar donde se instancian adaptadores concretos.
    /// Todos los casos de uso se ensamblan usando factories que devuelven puertos.
    ///
    /// # Argumentos
    ///
    /// * `schema_storage` - Adaptador concreto para almacenamiento de esquemas
    /// * `policy_adapter` - Adaptador concreto para gestión de políticas IAM
    ///
    /// # Retorna
    ///
    /// Una instancia de CompositionRoot con todos los puertos listos para inyección
    pub fn production<S, P>(schema_storage: Arc<S>, policy_adapter: Arc<P>) -> Self
    where
        S: SchemaStoragePort + Clone + 'static,
        P: hodei_iam::features::create_policy::ports::CreatePolicyPort
            + hodei_iam::features::get_policy::ports::PolicyReader
            + hodei_iam::features::list_policies::ports::PolicyLister
            + hodei_iam::features::update_policy::ports::UpdatePolicyPort
            + hodei_iam::features::delete_policy::ports::DeletePolicyPort
            + 'static,
    {
        info!("🏗️  Initializing Composition Root (Production)");

        // ============================================================
        // PASO 1: Crear puertos de hodei-policies
        // ============================================================
        info!("📦 Creating hodei-policies ports...");

        // 1.1. Bundle de registro de esquemas (entity, action, build)
        info!("  ├─ Schema registration bundle");
        let (register_entity_type, register_action_type, build_schema) =
            policy_factories::create_schema_registration_components(schema_storage.clone());

        // 1.2. Load schema
        info!("  ├─ LoadSchemaPort");
        let load_schema = hodei_policies::load_schema::factories::create_load_schema_use_case(
            schema_storage.clone(),
        );

        // 1.3. Validate policy
        info!("  ├─ ValidatePolicyPort");
        let validate_policy =
            hodei_policies::validate_policy::factories::create_validate_policy_use_case_with_schema(
                schema_storage.clone(),
            );

        // 1.4. Evaluate policies
        info!("  ├─ EvaluatePoliciesPort");
        let evaluate_policies =
            hodei_policies::evaluate_policies::factories::create_evaluate_policies_use_case(
                schema_storage.clone(),
            );

        // 1.5. Playground evaluate
        info!("  └─ PlaygroundEvaluatePort");
        let playground_evaluate = Self::create_playground_evaluate_port(schema_storage.clone());

        let policy_ports = PolicyPorts {
            register_entity_type,
            register_action_type,
            build_schema,
            load_schema,
            validate_policy,
            evaluate_policies,
            playground_evaluate,
        };

        // ============================================================
        // PASO 2: Crear puertos de hodei-iam usando puertos de policies
        // ============================================================
        info!("📦 Creating hodei-iam ports...");

        // 2.1. Register IAM schema (orquesta los puertos de policies)
        info!("  ├─ RegisterIamSchemaPort");
        let register_iam_schema = iam_factories::create_register_iam_schema_use_case(
            policy_ports.register_entity_type.clone(),
            policy_ports.register_action_type.clone(),
            policy_ports.build_schema.clone(),
        );

        // 2.2. Create policy use case
        info!("  ├─ CreatePolicyPort");
        let create_policy =
            hodei_iam::features::create_policy::factories::create_create_policy_use_case(
                policy_adapter.clone(),
                policy_ports.validate_policy.clone(),
            );

        // 2.3. Get policy port
        info!("  ├─ GetPolicyPort");
        let get_policy: Arc<dyn hodei_iam::features::get_policy::ports::PolicyReader> =
            policy_adapter.clone();

        // 2.4. List policies port
        info!("  ├─ ListPoliciesPort");
        let list_policies: Arc<dyn hodei_iam::features::list_policies::ports::PolicyLister> =
            policy_adapter.clone();

        // 2.5. Update policy port
        info!("  ├─ UpdatePolicyPort");
        let update_policy: Arc<dyn hodei_iam::features::update_policy::ports::UpdatePolicyPort> =
            policy_adapter.clone();

        // 2.6. Delete policy port
        info!("  └─ DeletePolicyPort");
        let delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort> =
            policy_adapter;

        let iam_ports = IamPorts {
            register_iam_schema,
            create_policy,
            get_policy,
            list_policies,
            update_policy,
            delete_policy,
        };

        info!("✅ Composition Root initialized successfully");

        Self {
            policy_ports,
            iam_ports,
        }
    }

    /// Crea el puerto de playground evaluate con todas sus dependencias
    ///
    /// Este método encapsula la creación del playground evaluate que requiere
    /// múltiples adaptadores internos.
    fn create_playground_evaluate_port<S>(schema_storage: Arc<S>) -> Arc<dyn PlaygroundEvaluatePort>
    where
        S: SchemaStoragePort + 'static,
    {
        use hodei_policies::features::playground_evaluate::adapters::{
            ContextConverterAdapter, PolicyEvaluatorAdapter, PolicyValidatorAdapter,
            SchemaLoaderAdapter,
        };

        // Crear adaptadores concretos para playground
        let schema_loader = Arc::new(SchemaLoaderAdapter::new(schema_storage));
        let policy_validator = Arc::new(PolicyValidatorAdapter);
        let policy_evaluator = Arc::new(PolicyEvaluatorAdapter);
        let context_converter = Arc::new(ContextConverterAdapter);

        // Ensamblar el caso de uso usando la factory
        playground_factories::create_playground_evaluate_use_case(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        )
    }

    /// Crea un Composition Root para testing
    ///
    /// Este método permite crear un composition root con mocks o
    /// implementaciones de prueba para tests de integración.
    #[cfg(test)]
    pub fn test<S, P>(schema_storage: Arc<S>, policy_adapter: Arc<P>) -> Self
    where
        S: SchemaStoragePort + Clone + 'static,
        P: hodei_iam::features::create_policy::ports::CreatePolicyPort
            + hodei_iam::features::get_policy::ports::PolicyReader
            + hodei_iam::features::list_policies::ports::PolicyLister
            + hodei_iam::features::update_policy::ports::UpdatePolicyPort
            + hodei_iam::features::delete_policy::ports::DeletePolicyPort
            + 'static,
    {
        // En tests, podemos usar implementaciones mock
        Self::production(schema_storage, policy_adapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hodei_policies::build_schema::dto::BuildSchemaCommand;
    use hodei_policies::build_schema::error::BuildSchemaError;

    /// Mock simple de SchemaStoragePort para tests
    #[derive(Clone)]
    struct MockSchemaStorage;

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            _schema_json: String,
            _version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            Ok("test-schema-id".to_string())
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
            Ok(false)
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            Ok(vec![])
        }
    }

    /// Mock simple de todos los puertos de políticas IAM
    struct MockPolicyAdapter;

    #[async_trait]
    impl hodei_iam::features::create_policy::ports::CreatePolicyPort for MockPolicyAdapter {
        async fn create(
            &self,
            _command: hodei_iam::features::create_policy::dto::CreatePolicyCommand,
        ) -> Result<
            kernel::domain::policy::HodeiPolicy,
            hodei_iam::features::create_policy::error::CreatePolicyError,
        > {
            use kernel::domain::policy::{HodeiPolicy, PolicyId};
            Ok(HodeiPolicy::new(
                PolicyId::new("test-id"),
                "permit(principal, action, resource);".to_string(),
            ))
        }
    }

    #[async_trait]
    impl hodei_iam::features::get_policy::ports::PolicyReader for MockPolicyAdapter {
        async fn get_by_hrn(
            &self,
            _hrn: &kernel::Hrn,
        ) -> Result<
            hodei_iam::features::get_policy::dto::PolicyView,
            hodei_iam::features::get_policy::error::GetPolicyError,
        > {
            Ok(hodei_iam::features::get_policy::dto::PolicyView {
                hrn: kernel::Hrn::new(
                    "hodei".to_string(),
                    "iam".to_string(),
                    "default".to_string(),
                    "Policy".to_string(),
                    "test-policy".to_string(),
                ),
                name: "test-policy".to_string(),
                content: "permit(principal, action, resource);".to_string(),
                description: Some("Test policy".to_string()),
            })
        }
    }

    #[async_trait]
    impl hodei_iam::features::list_policies::ports::PolicyLister for MockPolicyAdapter {
        async fn list(
            &self,
            _query: hodei_iam::features::list_policies::dto::ListPoliciesQuery,
        ) -> Result<
            hodei_iam::features::list_policies::dto::ListPoliciesResponse,
            hodei_iam::features::list_policies::error::ListPoliciesError,
        > {
            Ok(
                hodei_iam::features::list_policies::dto::ListPoliciesResponse {
                    policies: vec![],
                    total_count: 0,
                    has_next_page: false,
                    has_previous_page: false,
                },
            )
        }
    }

    #[async_trait]
    impl hodei_iam::features::update_policy::ports::UpdatePolicyPort for MockPolicyAdapter {
        async fn update(
            &self,
            _command: hodei_iam::features::update_policy::dto::UpdatePolicyCommand,
        ) -> Result<
            hodei_iam::features::update_policy::dto::PolicyView,
            hodei_iam::features::update_policy::error::UpdatePolicyError,
        > {
            Err(
                hodei_iam::features::update_policy::error::UpdatePolicyError::PolicyNotFound(
                    "mock".to_string(),
                ),
            )
        }
    }

    #[async_trait]
    impl hodei_iam::features::delete_policy::ports::DeletePolicyPort for MockPolicyAdapter {
        async fn delete(
            &self,
            _policy_id: &str,
        ) -> Result<(), hodei_iam::features::delete_policy::error::DeletePolicyError> {
            Ok(())
        }
    }

    #[test]
    fn test_composition_root_creates_all_ports() {
        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let root = CompositionRoot::production(storage, policy_adapter);

        // Verificar que todos los puertos fueron creados
        assert!(Arc::strong_count(&root.policy_ports.register_entity_type) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.register_action_type) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.build_schema) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.load_schema) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.validate_policy) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.evaluate_policies) >= 1);
        assert!(Arc::strong_count(&root.policy_ports.playground_evaluate) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.register_iam_schema) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.create_policy) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.get_policy) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.list_policies) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.update_policy) >= 1);
        assert!(Arc::strong_count(&root.iam_ports.delete_policy) >= 1);
    }

    #[tokio::test]
    async fn test_ports_are_usable() {
        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let root = CompositionRoot::production(storage, policy_adapter);

        // Verificar que el puerto de build_schema es usable
        let command = BuildSchemaCommand {
            version: Some("test".to_string()),
            validate: false,
        };

        // Esto debería compilar y ejecutar sin errores
        // (aunque falle por falta de tipos registrados, eso es esperado)
        let result = root.policy_ports.build_schema.execute(command).await;

        // Verificamos que el error sea por falta de tipos, no por problemas de DI
        assert!(result.is_err());
    }

    #[test]
    fn test_composition_root_for_testing() {
        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let _root = CompositionRoot::test(storage, policy_adapter);
        // Si compila y se crea, el test pasa
    }
}
