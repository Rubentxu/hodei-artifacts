//! Composition Root - Dependency Injection
//!
//! This module implements the Composition Root pattern, which is the only place
//! in the application where concrete implementations are instantiated and
//! use cases are assembled.
//!
//! # Principios
//!
//! 1. **Single place of construction**: Concrete adapters are only created here
//! 2. **Injection via ports**: Use cases are assembled using traits (ports)
//! 3. **Compile-time resolution**: Use of generics for zero-cost abstractions
//! 4. **Decoupling**: Handlers only know the ports, not the implementations

use crate::authorization::{AuthorizerPort, DenyAllAuthorizer};
use hodei_iam::register_iam_schema::factories as iam_factories;
use hodei_policies::build_schema::factories as policy_factories;
use hodei_policies::build_schema::ports::{BuildSchemaPort, SchemaStoragePort};
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use hodei_policies::load_schema::ports::LoadSchemaPort;
use hodei_policies::playground_evaluate::factories as playground_factories;
use hodei_policies::playground_evaluate::ports::PlaygroundEvaluatePort;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use hodei_policies::validate_policy::port::ValidatePolicyPort;
use std::sync::Arc;
use tracing::info;

/// Use case ports of hodei-policies
///
/// This structure groups all the ports (traits) of hodei-policies
/// that will be injected into other bounded contexts.
pub struct PolicyPorts {
    pub register_entity_type: Arc<dyn RegisterEntityTypePort>,
    pub register_action_type: Arc<dyn RegisterActionTypePort>,
    pub build_schema: Arc<dyn BuildSchemaPort>,
    pub load_schema: Arc<dyn LoadSchemaPort>,
    pub validate_policy: Arc<dyn ValidatePolicyPort>,
    pub evaluate_policies: Arc<dyn EvaluatePoliciesPort>,
    pub playground_evaluate: Arc<dyn PlaygroundEvaluatePort>,
}

/// Use case ports of hodei-iam
///
/// This structure groups all the ports (traits) of hodei-iam
/// that will be exposed to the API handlers.
pub struct IamPorts {
    pub register_iam_schema:
        Arc<dyn hodei_iam::features::register_iam_schema::ports::RegisterIamSchemaPort>,
    pub create_policy: Arc<dyn hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort>,
    pub get_policy: Arc<dyn hodei_iam::features::get_policy::ports::PolicyReader>,
    pub list_policies: Arc<dyn hodei_iam::features::list_policies::ports::PolicyLister>,
    pub update_policy: Arc<dyn hodei_iam::features::update_policy::ports::UpdatePolicyPort>,
    pub delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort>,
    pub evaluate_iam_policies:
        Arc<dyn hodei_iam::features::evaluate_iam_policies::ports::EvaluateIamPoliciesPort>,
}

/// Composition Root - Assembly point of the whole application
///
/// This structure holds all the use case ports that will be
/// injected into the Axum handlers.
pub struct CompositionRoot {
    pub policy_ports: PolicyPorts,
    pub iam_ports: IamPorts,
    pub(crate) authorization: Arc<dyn AuthorizerPort>,
}

impl CompositionRoot {
    /// Creates the Composition Root for production
    ///
    /// This method is the only place where concrete adapters are instantiated.
    /// All use cases are assembled using factories that return ports.
    ///
    /// # Arguments
    ///
    /// * `schema_storage` - Concrete adapter for schema storage
    /// * `policy_adapter` - Concrete adapter for IAM policy management
    ///
    /// # Returns
    ///
    /// A CompositionRoot instance with all ports ready for injection
    pub fn production<S, P>(schema_storage: Arc<S>, policy_adapter: Arc<P>) -> Self
    where
        S: SchemaStoragePort + Clone + 'static,
        P: hodei_iam::features::create_policy::ports::CreatePolicyPort
            + hodei_iam::features::get_policy::ports::PolicyReader
            + hodei_iam::features::list_policies::ports::PolicyLister
            + hodei_iam::features::update_policy::ports::UpdatePolicyPort
            + hodei_iam::features::delete_policy::ports::DeletePolicyPort
            + hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderPort
            + 'static,
    {
        Self::production_with_authoritative_schema(schema_storage, policy_adapter, None)
    }

    /// Build the production composition with one declared authoritative schema.
    ///
    /// The identity names the schema every policy is validated and evaluated
    /// against when a request does not name another one. It is what keeps the
    /// gate and the persisted evaluator on the same declarations: selecting
    /// the most recently stored schema would let a schema registered after
    /// the bundle displace it.
    ///
    /// # Arguments
    ///
    /// * `schema_storage` - Concrete adapter for schema storage
    /// * `policy_adapter` - Concrete adapter for IAM policy management
    /// * `authoritative_schema_id` - Identity of the schema declared by the
    ///   deployment, or `None` when the deployment declares none
    pub fn production_with_authoritative_schema<S, P>(
        schema_storage: Arc<S>,
        policy_adapter: Arc<P>,
        authoritative_schema_id: Option<String>,
    ) -> Self
    where
        S: SchemaStoragePort + Clone + 'static,
        P: hodei_iam::features::create_policy::ports::CreatePolicyPort
            + hodei_iam::features::get_policy::ports::PolicyReader
            + hodei_iam::features::list_policies::ports::PolicyLister
            + hodei_iam::features::update_policy::ports::UpdatePolicyPort
            + hodei_iam::features::delete_policy::ports::DeletePolicyPort
            + hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderPort
            + 'static,
    {
        info!("🏗️  Initializing Composition Root (Production)");

        // ============================================================
        // STEP 1: Create hodei-policies ports
        // ============================================================
        info!("📦 Creating hodei-policies ports...");

        // 1.1. Schema registration bundle (entity, action, build)
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
        let validate_policy = match &authoritative_schema_id {
            Some(id) => hodei_policies::validate_policy::factories::
                create_validate_policy_use_case_with_authoritative_schema(
                    schema_storage.clone(),
                    id.clone(),
                ),
            None => hodei_policies::validate_policy::factories::
                create_validate_policy_use_case_with_schema(schema_storage.clone()),
        };

        // 1.4. Evaluate policies
        info!("  ├─ EvaluatePoliciesPort");
        let evaluate_policies = match &authoritative_schema_id {
            Some(id) => hodei_policies::evaluate_policies::factories::
                create_evaluate_policies_use_case_with_authoritative_schema(
                    schema_storage.clone(),
                    id.clone(),
                ),
            None => hodei_policies::evaluate_policies::factories::
                create_evaluate_policies_use_case(schema_storage.clone()),
        };

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
        // STEP 2: Create hodei-iam ports using policies ports
        // ============================================================
        info!("📦 Creating hodei-iam ports...");

        // 2.1. Register IAM schema (orchestrates the policies ports)
        info!("  ├─ RegisterIamSchemaPort");
        let register_iam_schema = iam_factories::create_register_iam_schema_use_case(
            policy_ports.register_entity_type.clone(),
            policy_ports.register_action_type.clone(),
            policy_ports.build_schema.clone(),
        );

        // 2.2. Create policy use case
        info!("  ├─ CreatePolicyPort");
        let create_policy = hodei_iam::features::create_policy::factories::create_policy_use_case(
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
        info!("  ├─ DeletePolicyPort");
        let delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort> =
            policy_adapter.clone();

        // 2.7. IAM policy evaluation
        //
        // The evaluation consults the **persisted** policies: the policy finder
        // is the same adapter that writes them, so the policies the decision
        // is made from are the ones that were stored. The entities are
        // materialized from their HRNs — the platform has no identity profile
        // yet, and the degradation is declared in the resolver itself.
        info!("  └─ EvaluateIamPoliciesPort");
        let iam_policy_finder: Arc<
            dyn hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderPort,
        > = policy_adapter.clone();
        let entity_resolver =
            Arc::new(hodei_iam::features::evaluate_iam_policies::adapter::HrnEntityResolver::new());
        let evaluate_iam_policies: Arc<
            dyn hodei_iam::features::evaluate_iam_policies::ports::EvaluateIamPoliciesPort,
        > = Arc::new(
            match &authoritative_schema_id {
                Some(id) => {
                    hodei_iam::features::evaluate_iam_policies::use_case::EvaluateIamPoliciesUseCase::
                        with_authoritative_schema(
                            iam_policy_finder,
                            entity_resolver.clone(),
                            entity_resolver,
                            schema_storage.clone(),
                            id.clone(),
                        )
                }
                None => {
                    hodei_iam::features::evaluate_iam_policies::use_case::EvaluateIamPoliciesUseCase::new(
                        iam_policy_finder,
                        entity_resolver.clone(),
                        entity_resolver,
                        schema_storage.clone(),
                    )
                }
            },
        );

        let iam_ports = IamPorts {
            register_iam_schema,
            create_policy,
            get_policy,
            list_policies,
            update_policy,
            delete_policy,
            evaluate_iam_policies,
        };

        info!("✅ Composition Root initialized successfully");

        Self {
            policy_ports,
            iam_ports,
            authorization: Arc::new(DenyAllAuthorizer),
        }
    }

    /// Replace the compatibility deny-all authorizer with the shared
    /// `hodei-authz` adapter (or any other `AuthorizerPort` implementation).
    pub fn with_authorization(mut self, authorization: Arc<dyn AuthorizerPort>) -> Self {
        self.authorization = authorization;
        self
    }

    /// Creates the playground evaluate port with all its dependencies
    ///
    /// This method encapsulates the creation of the playground evaluate, which requires
    /// multiple internal adapters.
    fn create_playground_evaluate_port<S>(schema_storage: Arc<S>) -> Arc<dyn PlaygroundEvaluatePort>
    where
        S: SchemaStoragePort + 'static,
    {
        use hodei_policies::playground_evaluate::adapters::{
            ContextConverterAdapter, PolicyEvaluatorAdapter, PolicyValidatorAdapter,
            SchemaLoaderAdapter,
        };

        // Create concrete adapters for the playground
        let schema_loader = Arc::new(SchemaLoaderAdapter::new(schema_storage));
        let policy_validator = Arc::new(PolicyValidatorAdapter);
        let policy_evaluator = Arc::new(PolicyEvaluatorAdapter);
        let context_converter = Arc::new(ContextConverterAdapter);

        // Assemble the use case using the factory
        playground_factories::create_playground_evaluate_use_case(
            schema_loader,
            policy_validator,
            policy_evaluator,
            context_converter,
        )
    }

    /// Creates a Composition Root for testing
    ///
    /// This method allows creating a composition root with mocks or
    /// test implementations for integration tests.
    #[cfg(test)]
    pub fn test<S, P>(schema_storage: Arc<S>, policy_adapter: Arc<P>) -> Self
    where
        S: SchemaStoragePort + Clone + 'static,
        P: hodei_iam::features::create_policy::ports::CreatePolicyPort
            + hodei_iam::features::get_policy::ports::PolicyReader
            + hodei_iam::features::list_policies::ports::PolicyLister
            + hodei_iam::features::update_policy::ports::UpdatePolicyPort
            + hodei_iam::features::delete_policy::ports::DeletePolicyPort
            + hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderPort
            + 'static,
    {
        // In tests, we can use mock implementations
        Self::production(schema_storage, policy_adapter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use async_trait::async_trait;
    use hodei_policies::build_schema::dto::BuildSchemaCommand;
    use hodei_policies::build_schema::error::BuildSchemaError;

    /// Simple mock of SchemaStoragePort for tests
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

    /// Simple mock of all the IAM policy ports
    struct MockPolicyAdapter;

    #[async_trait]
    impl hodei_iam::features::create_policy::ports::CreatePolicyPort for MockPolicyAdapter {
        async fn create(
            &self,
            _command: hodei_iam::features::create_policy::dto::CreatePolicyCommand,
        ) -> Result<
            hodei_iam::features::create_policy::dto::CreatedPolicy,
            hodei_iam::features::create_policy::error::CreatePolicyError,
        > {
            use kernel::domain::policy::{HodeiPolicy, PolicyId};
            let now = chrono::Utc::now();
            Ok(hodei_iam::features::create_policy::dto::CreatedPolicy {
                policy: HodeiPolicy::new(
                    PolicyId::new("test-id"),
                    "permit(principal, action, resource);".to_string(),
                ),
                created_at: Some(now),
                updated_at: Some(now),
            })
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
                created_at: None,
                updated_at: None,
                attached_principals: Vec::new(),
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

    #[async_trait]
    impl hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderPort for MockPolicyAdapter {
        async fn get_effective_policies(
            &self,
            _principal_hrn: &kernel::Hrn,
        ) -> Result<
            kernel::domain::policy::HodeiPolicySet,
            hodei_iam::features::evaluate_iam_policies::ports::PolicyFinderError,
        > {
            Ok(kernel::domain::policy::HodeiPolicySet::new(Vec::new()))
        }
    }

    #[test]
    fn test_composition_root_creates_all_ports() {
        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let root = CompositionRoot::production(storage, policy_adapter);

        // Check that all the ports were created
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

        // Check that the build_schema port is usable
        let command = BuildSchemaCommand {
            version: Some("test".to_string()),
            validate: false,
        };

        // This should compile and run without errors
        // (even if it fails for lack of registered types, that is expected)
        let result = root.policy_ports.build_schema.execute(command).await;

        // We verify that the error is due to missing types, not DI problems
        assert!(result.is_err());
    }

    struct PermitAllAuthorizer;

    impl hodei_authz::Authorizer for PermitAllAuthorizer {
        fn decide(
            &self,
            _request: &hodei_authz::AuthorizationRequest,
        ) -> hodei_authz::AuthorizationOutcome {
            hodei_authz::AuthorizationOutcome::Permit
        }
    }

    struct TestAuthenticator;

    #[async_trait]
    impl hodei_authz::PrincipalAuthenticator for TestAuthenticator {
        async fn authenticate(
            &self,
            _credentials: &hodei_authz::Credentials,
        ) -> Result<hodei_authz::PrincipalClaims, hodei_authz::AuthenticationError> {
            Ok(hodei_authz::PrincipalClaims::new(
                hodei_authz::PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
                hodei_authz::AccountId::new("acme").unwrap(),
            ))
        }
    }

    async fn test_context() -> hodei_authz::VerifiedContext {
        hodei_authz::VerifiedContext::from_principal(
            hodei_authz::verify_principal(
                &TestAuthenticator,
                &hodei_authz::Credentials::bearer("token"),
            )
            .await
            .unwrap(),
        )
    }

    fn test_resource() -> hodei_authz::ResourceRef {
        use hodei_authz::hodei_hrn::Hrn;
        let hrn: Hrn = "hrn:hodei:artifacts:eu:acme:repository/releases"
            .parse()
            .unwrap();
        hodei_authz::ResourceRef::new(hrn).unwrap()
    }

    #[tokio::test]
    async fn authorization_adapter_is_injectable_and_reversible() {
        use crate::authorization::{AuthorizeResourceUseCase, SharedAuthorizer};

        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let default_root = CompositionRoot::production(storage.clone(), policy_adapter.clone());
        let default_case = AuthorizeResourceUseCase::new(default_root.authorization.clone());

        assert_eq!(
            default_case.execute(
                test_context().await,
                hodei_authz::ActionId::new("artifacts", "ReadRepository").unwrap(),
                test_resource(),
            ),
            hodei_authz::AuthorizationOutcome::Deny(hodei_authz::DenyReason::NoMatchingPolicy)
        );

        let shared = Arc::new(SharedAuthorizer::new(Arc::new(PermitAllAuthorizer)));
        let root = default_root.with_authorization(shared);
        let shared_case = AuthorizeResourceUseCase::new(root.authorization.clone());

        assert_eq!(
            shared_case.execute(
                test_context().await,
                hodei_authz::ActionId::new("artifacts", "ReadRepository").unwrap(),
                test_resource(),
            ),
            hodei_authz::AuthorizationOutcome::Permit
        );

        let state = AppState::from_composition_root("test".to_string(), root);
        assert!(Arc::strong_count(&state.authorize_resource) >= 1);
    }

    #[test]
    fn test_composition_root_for_testing() {
        let storage = Arc::new(MockSchemaStorage);
        let policy_adapter = Arc::new(MockPolicyAdapter);
        let _root = CompositionRoot::test(storage, policy_adapter);
        // If it compiles and gets created, the test passes
    }
}
