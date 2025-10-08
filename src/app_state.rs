//! Application State for Hodei Artifacts API
//!
//! This module defines the AppState that holds all use cases and dependencies
//! injected throughout the application. All use cases are stored as trait objects
//! to enable dependency inversion and testability.

use hodei_iam::features::create_policy::use_case::CreatePolicyUseCase;
use hodei_iam::features::delete_policy::use_case::DeletePolicyUseCase;
use hodei_iam::features::get_policy::use_case::GetPolicyUseCase;
use hodei_iam::features::list_policies::use_case::ListPoliciesUseCase;
use hodei_iam::features::update_policy::use_case::UpdatePolicyUseCase;
use hodei_policies::features::build_schema::ports::SchemaStoragePort;
use hodei_policies::features::playground_evaluate::use_case::PlaygroundEvaluateUseCase;
use hodei_policies::features::validate_policy::use_case::ValidatePolicyUseCase;
use std::sync::Arc;

/// Application state containing all use cases and shared dependencies
///
/// This struct is cloned into each Axum handler and provides access to
/// all business logic use cases. Each use case is stored as a trait object
/// to decouple handlers from concrete implementations.
#[derive(Clone)]
pub struct AppState<S: SchemaStoragePort + Clone> {
    /// Schema version currently active in the system
    #[allow(dead_code)]
    pub schema_version: String,

    /// Use case for registering IAM schema
    pub register_iam_schema:
        Arc<hodei_iam::features::register_iam_schema::RegisterIamSchemaUseCase>,

    /// Use case for creating IAM policies
    pub create_policy: Arc<
        CreatePolicyUseCase<
            hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
            ValidatePolicyUseCase<S>,
        >,
    >,

    /// Use case for getting IAM policies
    pub get_policy: Arc<
        GetPolicyUseCase<hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter>,
    >,

    /// Use case for listing IAM policies
    pub list_policies: Arc<
        ListPoliciesUseCase<
            hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
        >,
    >,

    /// Use case for updating IAM policies
    pub update_policy: Arc<
        UpdatePolicyUseCase<
            hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
            ValidatePolicyUseCase<S>,
        >,
    >,

    /// Use case for deleting IAM policies
    pub delete_policy: Arc<
        DeletePolicyUseCase<
            hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
        >,
    >,

    /// Use case for registering entity types
    #[allow(dead_code)]
    pub register_entity_type:
        Arc<hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase>,

    /// Use case for registering action types
    #[allow(dead_code)]
    pub register_action_type:
        Arc<hodei_policies::features::register_action_type::RegisterActionTypeUseCase>,

    /// Use case for building schemas
    pub build_schema: Arc<hodei_policies::features::build_schema::BuildSchemaUseCase<S>>,

    /// Use case for loading schemas
    #[allow(dead_code)]
    pub load_schema: Arc<hodei_policies::features::load_schema::LoadSchemaUseCase<S>>,

    /// Use case for validating policies
    pub validate_policy: Arc<ValidatePolicyUseCase<S>>,

    /// Use case for evaluating policies
    #[allow(dead_code)]
    pub evaluate_policies:
        Arc<hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase>,

    /// Use case for playground policy evaluation
    pub playground_evaluate: Arc<PlaygroundEvaluateUseCase>,
}

impl<S: SchemaStoragePort + Clone> AppState<S> {
    /// Create a new application state
    ///
    /// This is typically called during application bootstrap after all
    /// use cases have been constructed with their dependencies.
    ///
    /// # Arguments
    ///
    /// * `schema_version` - The active schema version identifier
    /// * `register_iam_schema` - Use case for IAM schema registration
    /// * `register_entity_type` - Use case for entity type registration
    /// * `register_action_type` - Use case for action type registration
    /// * `build_schema` - Use case for schema building
    /// * `load_schema` - Use case for schema loading
    /// * `validate_policy` - Use case for policy validation
    /// * `evaluate_policies` - Use case for policy evaluation
    /// * `playground_evaluate` - Use case for playground policy evaluation
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: String,
        register_iam_schema: Arc<
            hodei_iam::features::register_iam_schema::RegisterIamSchemaUseCase,
        >,
        create_policy: Arc<
            CreatePolicyUseCase<
                hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
                ValidatePolicyUseCase<S>,
            >,
        >,
        get_policy: Arc<
            GetPolicyUseCase<
                hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
            >,
        >,
        list_policies: Arc<
            ListPoliciesUseCase<
                hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
            >,
        >,
        update_policy: Arc<
            UpdatePolicyUseCase<
                hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
                ValidatePolicyUseCase<S>,
            >,
        >,
        delete_policy: Arc<
            DeletePolicyUseCase<
                hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter,
            >,
        >,
        register_entity_type: Arc<
            hodei_policies::features::register_entity_type::RegisterEntityTypeUseCase,
        >,
        register_action_type: Arc<
            hodei_policies::features::register_action_type::RegisterActionTypeUseCase,
        >,
        build_schema: Arc<hodei_policies::features::build_schema::BuildSchemaUseCase<S>>,
        load_schema: Arc<hodei_policies::features::load_schema::LoadSchemaUseCase<S>>,
        validate_policy: Arc<ValidatePolicyUseCase<S>>,
        evaluate_policies: Arc<
            hodei_policies::features::evaluate_policies::EvaluatePoliciesUseCase,
        >,
        playground_evaluate: Arc<PlaygroundEvaluateUseCase>,
    ) -> Self {
        Self {
            schema_version,
            register_iam_schema,
            register_entity_type,
            register_action_type,
            build_schema,
            load_schema,
            validate_policy,
            evaluate_policies,
            playground_evaluate,
            create_policy,
            get_policy,
            list_policies,
            update_policy,
            delete_policy,
        }
    }
}
