//! Application State for Hodei Artifacts API
//!
//! This module defines the AppState that holds all use cases and dependencies
//! injected throughout the application. All use cases are stored as trait objects
//! (ports) to enable dependency inversion and testability.
//!
//! # Architecture
//!
//! AppState sigue el patrón Composition Root:
//! - Solo contiene puertos (traits), no implementaciones concretas
//! - Es construido por el composition_root
//! - Es clonado e inyectado en cada handler de Axum

use crate::composition_root::CompositionRoot;
use hodei_iam::features::register_iam_schema::ports::RegisterIamSchemaPort;
use hodei_policies::build_schema::ports::BuildSchemaPort;
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use hodei_policies::features::playground_evaluate::ports::PlaygroundEvaluatePort;
use hodei_policies::load_schema::ports::LoadSchemaPort;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use hodei_policies::validate_policy::port::ValidatePolicyPort;
use std::sync::Arc;

/// Application state containing all use case ports
///
/// Este struct es clonado en cada handler de Axum y proporciona acceso a
/// toda la lógica de negocio a través de puertos (traits). Esto permite:
/// - Desacoplamiento total entre handlers e implementaciones
/// - Fácil testing con mocks
/// - Cumplimiento del Dependency Inversion Principle
#[derive(Clone)]
pub struct AppState {
    /// Schema version currently active in the system
    pub schema_version: String,

    // ============================================================
    // Puertos de hodei-policies
    // ============================================================
    /// Port for registering entity types
    pub register_entity_type: Arc<dyn RegisterEntityTypePort>,

    /// Port for registering action types
    pub register_action_type: Arc<dyn RegisterActionTypePort>,

    /// Port for building and persisting schemas
    pub build_schema: Arc<dyn BuildSchemaPort>,

    /// Port for loading schemas from storage
    pub load_schema: Arc<dyn LoadSchemaPort>,

    /// Port for validating Cedar policies
    pub validate_policy: Arc<dyn ValidatePolicyPort>,

    /// Port for evaluating authorization policies
    pub evaluate_policies: Arc<dyn EvaluatePoliciesPort>,

    /// Port for playground policy evaluation
    pub playground_evaluate: Arc<dyn PlaygroundEvaluatePort>,

    // ============================================================
    // Puertos de hodei-iam
    // ============================================================
    /// Port for registering IAM schema
    pub register_iam_schema: Arc<dyn RegisterIamSchemaPort>,

    /// Port for creating IAM policies
    pub create_policy: Arc<dyn hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort>,

    /// Port for getting IAM policies
    pub get_policy: Arc<dyn hodei_iam::features::get_policy::ports::PolicyReader>,

    /// Port for listing IAM policies
    pub list_policies: Arc<dyn hodei_iam::features::list_policies::ports::PolicyLister>,

    /// Port for updating IAM policies
    pub update_policy: Arc<dyn hodei_iam::features::update_policy::ports::UpdatePolicyPort>,

    /// Port for deleting IAM policies
    pub delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort>,
}

impl AppState {
    /// Create a new application state with all ports
    ///
    /// This method is called during application bootstrap after the
    /// composition root has assembled all use cases.
    ///
    /// # Arguments
    ///
    /// * `schema_version` - The active schema version identifier
    /// * `register_entity_type` - Port for registering entity types
    /// * `register_action_type` - Port for registering action types
    /// * `build_schema` - Port for building schemas
    /// * `load_schema` - Port for loading schemas
    /// * `validate_policy` - Port for validating policies
    /// * `evaluate_policies` - Port for evaluating policies
    /// * `playground_evaluate` - Port for playground evaluation
    /// * `register_iam_schema` - Port for IAM schema registration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_artifacts::app_state::AppState;
    /// use hodei_artifacts::composition_root::CompositionRoot;
    ///
    /// let root = CompositionRoot::production(schema_storage);
    ///
    /// let app_state = AppState::new(
    ///     "v1.0.0".to_string(),
    ///     root.policy_ports.register_entity_type,
    ///     root.policy_ports.register_action_type,
    ///     root.policy_ports.build_schema,
    ///     root.policy_ports.load_schema,
    ///     root.policy_ports.validate_policy,
    ///     root.policy_ports.evaluate_policies,
    ///     root.policy_ports.playground_evaluate,
    ///     root.iam_ports.register_iam_schema,
    /// );
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: String,
        register_entity_type: Arc<dyn RegisterEntityTypePort>,
        register_action_type: Arc<dyn RegisterActionTypePort>,
        build_schema: Arc<dyn BuildSchemaPort>,
        load_schema: Arc<dyn LoadSchemaPort>,
        validate_policy: Arc<dyn ValidatePolicyPort>,
        evaluate_policies: Arc<dyn EvaluatePoliciesPort>,
        playground_evaluate: Arc<dyn PlaygroundEvaluatePort>,
        register_iam_schema: Arc<dyn RegisterIamSchemaPort>,
        create_policy: Arc<dyn hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort>,
        get_policy: Arc<dyn hodei_iam::features::get_policy::ports::PolicyReader>,
        list_policies: Arc<dyn hodei_iam::features::list_policies::ports::PolicyLister>,
        update_policy: Arc<dyn hodei_iam::features::update_policy::ports::UpdatePolicyPort>,
        delete_policy: Arc<dyn hodei_iam::features::delete_policy::ports::DeletePolicyPort>,
    ) -> Self {
        Self {
            schema_version,
            register_entity_type,
            register_action_type,
            build_schema,
            load_schema,
            validate_policy,
            evaluate_policies,
            playground_evaluate,
            register_iam_schema,
            create_policy,
            get_policy,
            list_policies,
            update_policy,
            delete_policy,
        }
    }

    /// Create AppState from a CompositionRoot
    ///
    /// This is a convenience method that extracts all ports from the
    /// composition root and creates an AppState.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_artifacts::composition_root::CompositionRoot;
    ///
    /// let root = CompositionRoot::production(schema_storage);
    /// let app_state = AppState::from_composition_root("v1.0.0".to_string(), root);
    /// ```
    pub fn from_composition_root(schema_version: String, root: CompositionRoot) -> Self {
        Self {
            schema_version,
            register_entity_type: root.policy_ports.register_entity_type,
            register_action_type: root.policy_ports.register_action_type,
            build_schema: root.policy_ports.build_schema,
            load_schema: root.policy_ports.load_schema,
            validate_policy: root.policy_ports.validate_policy,
            evaluate_policies: root.policy_ports.evaluate_policies,
            playground_evaluate: root.policy_ports.playground_evaluate,
            register_iam_schema: root.iam_ports.register_iam_schema,
            create_policy: root.iam_ports.create_policy,
            get_policy: root.iam_ports.get_policy,
            list_policies: root.iam_ports.list_policies,
            update_policy: root.iam_ports.update_policy,
            delete_policy: root.iam_ports.delete_policy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hodei_iam::features::create_policy::dto::{CreatePolicyCommand, PolicyView};
    use hodei_iam::features::create_policy::error::CreatePolicyError;
    use hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort;
    use hodei_iam::features::delete_policy::dto::DeletePolicyCommand;
    use hodei_iam::features::delete_policy::error::DeletePolicyError;
    use hodei_iam::features::delete_policy::ports::DeletePolicyPort;
    use hodei_iam::features::get_policy::dto::GetPolicyResponse;
    use hodei_iam::features::get_policy::error::GetPolicyError;
    use hodei_iam::features::get_policy::ports::PolicyReader;
    use hodei_iam::features::list_policies::dto::{ListPoliciesCommand, ListPoliciesResponse};
    use hodei_iam::features::list_policies::error::ListPoliciesError;
    use hodei_iam::features::list_policies::ports::PolicyLister;
    use hodei_iam::features::register_iam_schema::dto::{
        RegisterIamSchemaCommand, RegisterIamSchemaResult,
    };
    use hodei_iam::features::register_iam_schema::error::RegisterIamSchemaError;
    use hodei_iam::features::update_policy::dto::{UpdatePolicyCommand, UpdatePolicyResponse};
    use hodei_iam::features::update_policy::error::UpdatePolicyError;
    use hodei_iam::features::update_policy::ports::UpdatePolicyPort;
    use hodei_policies::build_schema::dto::{BuildSchemaCommand, BuildSchemaResult};
    use hodei_policies::build_schema::error::BuildSchemaError;
    use hodei_policies::evaluate_policies::dto::{EvaluatePoliciesCommand, EvaluationDecision};
    use hodei_policies::evaluate_policies::error::EvaluatePoliciesError;
    use hodei_policies::features::playground_evaluate::dto::{
        PlaygroundEvaluateCommand, PlaygroundEvaluateResult,
    };
    use hodei_policies::features::playground_evaluate::error::PlaygroundEvaluateError;
    use hodei_policies::load_schema::dto::{LoadSchemaCommand, LoadSchemaResult};
    use hodei_policies::load_schema::error::LoadSchemaError;
    use hodei_policies::register_action_type::dto::RegisterActionTypeCommand;
    use hodei_policies::register_action_type::error::RegisterActionTypeError;
    use hodei_policies::register_entity_type::dto::RegisterEntityTypeCommand;
    use hodei_policies::register_entity_type::error::RegisterEntityTypeError;
    use hodei_policies::validate_policy::dto::{ValidatePolicyCommand, ValidationResult};
    use hodei_policies::validate_policy::error::ValidatePolicyError;
    use kernel::domain::policy::HodeiPolicy;

    // Mock implementations for testing

    struct MockCreatePolicyPort;
    #[async_trait]
    impl hodei_iam::features::create_policy::ports::CreatePolicyUseCasePort for MockCreatePolicyPort {
        async fn execute(
            &self,
            _command: CreatePolicyCommand,
        ) -> Result<PolicyView, CreatePolicyError> {
            Ok(PolicyView {
                id: kernel::Hrn::new(
                    "hodei".to_string(),
                    "iam".to_string(),
                    "default".to_string(),
                    "Policy".to_string(),
                    "test-policy".to_string(),
                ),
                content: "permit(principal, action, resource);".to_string(),
                description: Some("Test policy".to_string()),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
        }
    }

    struct MockGetPolicyPort;
    #[async_trait]
    impl hodei_iam::features::get_policy::ports::PolicyReader for MockGetPolicyPort {
        async fn get_by_hrn(&self, _hrn: kernel::Hrn) -> Result<GetPolicyResponse, GetPolicyError> {
            Ok(GetPolicyResponse {
                policy: HodeiPolicy::new(
                    kernel::Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        "default".to_string(),
                        "Policy".to_string(),
                        "test-policy".to_string(),
                    ),
                    "permit(principal, action, resource);".to_string(),
                ),
            })
        }
    }

    struct MockListPoliciesPort;
    #[async_trait]
    impl hodei_iam::features::list_policies::ports::PolicyLister for MockListPoliciesPort {
        async fn list(
            &self,
            _command: ListPoliciesCommand,
        ) -> Result<ListPoliciesResponse, ListPoliciesError> {
            Ok(ListPoliciesResponse {
                total_count: 0,
                has_next_page: false,
                has_previous_page: false,
            })
        }
    }

    struct MockUpdatePolicyPort;
    #[async_trait]
    impl hodei_iam::features::update_policy::ports::UpdatePolicyPort for MockUpdatePolicyPort {
        async fn update(
            &self,
            _command: UpdatePolicyCommand,
        ) -> Result<UpdatePolicyResponse, UpdatePolicyError> {
            Ok(UpdatePolicyResponse {
                policy: HodeiPolicy::new(
                    kernel::Hrn::new(
                        "hodei".to_string(),
                        "iam".to_string(),
                        "default".to_string(),
                        "Policy".to_string(),
                        "test-policy".to_string(),
                    ),
                    "permit(principal, action, resource);".to_string(),
                ),
            })
        }
    }

    struct MockDeletePolicyPort;
    #[async_trait]
    impl hodei_iam::features::delete_policy::ports::DeletePolicyPort for MockDeletePolicyPort {
        async fn delete(&self, _command: DeletePolicyCommand) -> Result<(), DeletePolicyError> {
            Ok(())
        }
    }

    struct MockRegisterEntityTypePort;
    #[async_trait]
    impl RegisterEntityTypePort for MockRegisterEntityTypePort {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(
            &self,
            _command: RegisterEntityTypeCommand,
        ) -> Result<(), RegisterEntityTypeError> {
            Ok(())
        }
    }

    struct MockRegisterActionTypePort;
    #[async_trait]
    impl RegisterActionTypePort for MockRegisterActionTypePort {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn execute(
            &self,
            _command: RegisterActionTypeCommand,
        ) -> Result<(), RegisterActionTypeError> {
            Ok(())
        }
    }

    struct MockBuildSchemaPort;
    #[async_trait]
    impl BuildSchemaPort for MockBuildSchemaPort {
        async fn execute(
            &self,
            _command: BuildSchemaCommand,
        ) -> Result<BuildSchemaResult, BuildSchemaError> {
            Ok(BuildSchemaResult::new(
                0,
                0,
                None,
                false,
                "test-id".to_string(),
            ))
        }
    }

    struct MockLoadSchemaPort;
    #[async_trait]
    impl LoadSchemaPort for MockLoadSchemaPort {
        async fn execute(
            &self,
            _command: LoadSchemaCommand,
        ) -> Result<LoadSchemaResult, LoadSchemaError> {
            let schema = cedar_policy::Schema::from_schema_fragments(vec![]).unwrap();
            Ok(LoadSchemaResult::new(schema, None, "test-id".to_string()))
        }
    }

    struct MockValidatePolicyPort;
    #[async_trait]
    impl ValidatePolicyPort for MockValidatePolicyPort {
        async fn validate(
            &self,
            _command: ValidatePolicyCommand,
        ) -> Result<ValidationResult, ValidatePolicyError> {
            Ok(ValidationResult {
                is_valid: true,
                errors: vec![],
            })
        }
    }

    struct MockEvaluatePoliciesPort;
    #[async_trait]
    impl EvaluatePoliciesPort for MockEvaluatePoliciesPort {
        async fn evaluate(
            &self,
            _command: EvaluatePoliciesCommand<'_>,
        ) -> Result<EvaluationDecision, EvaluatePoliciesError> {
            Ok(EvaluationDecision {
                decision: hodei_policies::evaluate_policies::dto::Decision::Allow,
                determining_policies: vec![],
                reasons: vec![],
                used_schema_version: None,
                policy_ids_evaluated: vec![],
                diagnostics: vec![],
            })
        }

        async fn clear_cache(&self) -> Result<(), EvaluatePoliciesError> {
            Ok(())
        }
    }

    struct MockPlaygroundEvaluatePort;
    #[async_trait]
    impl PlaygroundEvaluatePort for MockPlaygroundEvaluatePort {
        async fn evaluate(
            &self,
            _command: PlaygroundEvaluateCommand,
        ) -> Result<PlaygroundEvaluateResult, PlaygroundEvaluateError> {
            Ok(PlaygroundEvaluateResult {
                decision: hodei_policies::features::playground_evaluate::dto::Decision::Allow,
                determining_policies: vec![],
                diagnostics:
                    hodei_policies::features::playground_evaluate::dto::EvaluationDiagnostics {
                        total_policies: 0,
                        matched_policies: 0,
                        schema_validated: false,
                        validation_errors: vec![],
                    },
            })
        }
    }

    struct MockRegisterIamSchemaPort;
    #[async_trait]
    impl RegisterIamSchemaPort for MockRegisterIamSchemaPort {
        async fn register(
            &self,
            _command: RegisterIamSchemaCommand,
        ) -> Result<RegisterIamSchemaResult, RegisterIamSchemaError> {
            Ok(RegisterIamSchemaResult::new(
                0,
                0,
                "test".to_string(),
                "test-id".to_string(),
                false,
            ))
        }
    }

    #[test]
    fn test_app_state_creation() {
        let app_state = AppState::new(
            "v1.0.0".to_string(),
            Arc::new(MockRegisterEntityTypePort),
            Arc::new(MockRegisterActionTypePort),
            Arc::new(MockBuildSchemaPort),
            Arc::new(MockLoadSchemaPort),
            Arc::new(MockValidatePolicyPort),
            Arc::new(MockEvaluatePoliciesPort),
            Arc::new(MockPlaygroundEvaluatePort),
            Arc::new(MockRegisterIamSchemaPort),
            Arc::new(MockCreatePolicyPort),
            Arc::new(MockGetPolicyPort),
            Arc::new(MockListPoliciesPort),
            Arc::new(MockUpdatePolicyPort),
            Arc::new(MockDeletePolicyPort),
        );

        assert_eq!(app_state.schema_version, "v1.0.0");
        assert!(Arc::strong_count(&app_state.register_entity_type) >= 1);
        assert!(Arc::strong_count(&app_state.register_action_type) >= 1);
        assert!(Arc::strong_count(&app_state.build_schema) >= 1);
        assert!(Arc::strong_count(&app_state.load_schema) >= 1);
        assert!(Arc::strong_count(&app_state.validate_policy) >= 1);
        assert!(Arc::strong_count(&app_state.evaluate_policies) >= 1);
        assert!(Arc::strong_count(&app_state.playground_evaluate) >= 1);
        assert!(Arc::strong_count(&app_state.register_iam_schema) >= 1);
    }

    #[test]
    fn test_app_state_is_cloneable() {
        let app_state = AppState::new(
            "v1.0.0".to_string(),
            Arc::new(MockRegisterEntityTypePort),
            Arc::new(MockRegisterActionTypePort),
            Arc::new(MockBuildSchemaPort),
            Arc::new(MockLoadSchemaPort),
            Arc::new(MockValidatePolicyPort),
            Arc::new(MockEvaluatePoliciesPort),
            Arc::new(MockPlaygroundEvaluatePort),
            Arc::new(MockRegisterIamSchemaPort),
            Arc::new(MockCreatePolicyPort),
            Arc::new(MockGetPolicyPort),
            Arc::new(MockListPoliciesPort),
            Arc::new(MockUpdatePolicyPort),
            Arc::new(MockDeletePolicyPort),
        );

        let cloned_state = app_state.clone();
        assert_eq!(cloned_state.schema_version, app_state.schema_version);

        // Arc counts should increase after clone
        assert!(Arc::strong_count(&app_state.register_entity_type) >= 2);
    }
}
