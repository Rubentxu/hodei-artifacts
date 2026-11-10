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

use crate::audit::{AuditSink, TracingAuditSink};
use crate::authorization::{AuthorizeResourceUseCase, AuthorizerPort};
use crate::composition_root::CompositionRoot;
use crate::repository::{
    DenyAllAuthenticator, InMemoryRepositoryStore, RepositoryReader, RepositoryWriter,
};
use hodei_authz::PrincipalAuthenticator;
use hodei_iam::register_iam_schema::ports::RegisterIamSchemaPort;
use hodei_policies::build_schema::ports::BuildSchemaPort;
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use hodei_policies::load_schema::ports::LoadSchemaPort;
use hodei_policies::playground_evaluate::ports::PlaygroundEvaluatePort;
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
#[allow(dead_code)]
pub struct AppState {
    /// Schema version currently active in the system
    #[allow(dead_code)]
    pub schema_version: String,

    // ============================================================
    // Puertos de hodei-policies
    // ============================================================
    /// Port for registering entity types
    #[allow(dead_code)]
    pub register_entity_type: Arc<dyn RegisterEntityTypePort>,

    /// Port for registering action types
    #[allow(dead_code)]
    pub register_action_type: Arc<dyn RegisterActionTypePort>,

    /// Port for building and persisting schemas
    pub build_schema: Arc<dyn BuildSchemaPort>,

    /// Port for loading schemas from storage
    pub load_schema: Arc<dyn LoadSchemaPort>,

    /// Port for validating Cedar policies
    pub validate_policy: Arc<dyn ValidatePolicyPort>,

    /// Port for evaluating authorization policies
    #[allow(dead_code)]
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

    /// Port for evaluating the IAM policies of a principal
    pub evaluate_iam_policies:
        Arc<dyn hodei_iam::features::evaluate_iam_policies::ports::EvaluateIamPoliciesPort>,

    /// Shared authorization port and the real use case that consumes it.
    pub(crate) authorization: Arc<dyn AuthorizerPort>,
    pub authorize_resource: Arc<AuthorizeResourceUseCase>,
    pub(crate) authenticator: Arc<dyn PrincipalAuthenticator>,
    pub(crate) repository_reader: Arc<dyn RepositoryReader>,
    pub(crate) repository_writer: Arc<dyn RepositoryWriter>,
    pub(crate) audit_sink: Arc<dyn AuditSink>,
    /// Whether the admin surface has an injected credential verifier and a
    /// non-default authorizer. The gate keeps the surface closed otherwise.
    admin_authenticator_configured: bool,
    admin_authorizer_configured: bool,
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
    #[allow(clippy::too_many_arguments, dead_code)]
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
        evaluate_iam_policies: Arc<
            dyn hodei_iam::features::evaluate_iam_policies::ports::EvaluateIamPoliciesPort,
        >,
        authorization: Arc<dyn AuthorizerPort>,
        authenticator: Arc<dyn PrincipalAuthenticator>,
        repository_reader: Arc<dyn RepositoryReader>,
        repository_writer: Arc<dyn RepositoryWriter>,
        audit_sink: Arc<dyn AuditSink>,
    ) -> Self {
        let authorize_resource = Arc::new(AuthorizeResourceUseCase::new(authorization.clone()));
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
            evaluate_iam_policies,
            authorization,
            authorize_resource,
            authenticator,
            repository_reader,
            repository_writer,
            audit_sink,
            admin_authenticator_configured: true,
            admin_authorizer_configured: true,
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
        let authorization = root.authorization;
        let authenticate = Arc::new(DenyAllAuthenticator);
        // Reader and writer share one store in the production wiring.
        let repository_store = Arc::new(InMemoryRepositoryStore::empty());
        let repository_reader: Arc<dyn RepositoryReader> = repository_store.clone();
        let repository_writer: Arc<dyn RepositoryWriter> = repository_store;
        let audit_sink = Arc::new(TracingAuditSink);
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
            evaluate_iam_policies: root.iam_ports.evaluate_iam_policies,
            authorize_resource: Arc::new(AuthorizeResourceUseCase::new(authorization.clone())),
            authorization,
            authenticator: authenticate,
            repository_reader,
            repository_writer,
            audit_sink,
            admin_authenticator_configured: false,
            admin_authorizer_configured: false,
        }
    }

    /// Whether admin routes can authenticate and authorize requests.
    pub fn admin_surface_ready(&self) -> bool {
        self.admin_authenticator_configured && self.admin_authorizer_configured
    }

    /// Replace the authorization port and rebuild the use case.
    pub fn with_authorization(mut self, authorization: Arc<dyn AuthorizerPort>) -> Self {
        self.authorize_resource = Arc::new(AuthorizeResourceUseCase::new(authorization.clone()));
        self.authorization = authorization;
        self.admin_authorizer_configured = true;
        self
    }

    /// Inject the credential verifier and repository reader for read/create.
    pub fn with_repository_auth(
        mut self,
        authenticator: Arc<dyn PrincipalAuthenticator>,
        repository_reader: Arc<dyn RepositoryReader>,
    ) -> Self {
        self.authenticator = authenticator;
        self.repository_reader = repository_reader;
        self.admin_authenticator_configured = true;
        self
    }

    /// Inject a store implementing both repository ports (durable adapter).
    pub fn with_repository_store<S>(mut self, store: Arc<S>) -> Self
    where
        S: RepositoryReader + RepositoryWriter + 'static,
    {
        self.repository_reader = store.clone();
        self.repository_writer = store;
        self
    }

    /// Inject the writer used by the create route.
    pub fn with_repository_writer(mut self, repository_writer: Arc<dyn RepositoryWriter>) -> Self {
        self.repository_writer = repository_writer;
        self
    }

    /// Inject an audit sink (tracing by default).
    pub fn with_audit_sink(mut self, audit_sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = audit_sink;
        self
    }

    /// Replace only the credential verifier, keeping the shared default store.
    pub fn with_authenticator(mut self, authenticator: Arc<dyn PrincipalAuthenticator>) -> Self {
        self.authenticator = authenticator;
        self.admin_authenticator_configured = true;
        self
    }
}
