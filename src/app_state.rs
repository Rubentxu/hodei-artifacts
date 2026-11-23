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
use crate::evidence::{EmptyEvidenceReader, EvidenceReader};
use crate::repository::{
    DenyAllAuthenticator, InMemoryRepositoryStore, RepositoryReader, RepositoryWriter,
};
use hodei_authz::PrincipalAuthenticator;
use hodei_catalog::{CatalogReader, CatalogSearch, CatalogWriter};
use hodei_hub::blob::BlobStore;
use hodei_iam::register_iam_schema::ports::RegisterIamSchemaPort;
use hodei_policies::build_schema::ports::BuildSchemaPort;
use hodei_policies::evaluate_policies::ports::EvaluatePoliciesPort;
use hodei_policies::load_schema::ports::LoadSchemaPort;
use hodei_policies::playground_evaluate::ports::PlaygroundEvaluatePort;
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use hodei_policies::validate_policy::port::ValidatePolicyPort;
use kernel::application::ports::clock::Identity;
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
    /// The reader of the accepted evidence.
    ///
    /// The trust capsule is assembled from the evidence that is there, and
    /// the evidence is read through a port and not through a concrete
    /// store.
    pub(crate) evidence_reader: Arc<dyn EvidenceReader>,
    /// The facts of risk the promotion decision measures against.
    ///
    /// It is a port and not a store: a deployment without the impact
    /// graph answers with UNMEASURED facts, and the policy that requires
    /// the risk does not grant on them.
    pub(crate) risk_facts: Arc<dyn hodei_risk_storage::subject_risk::RiskFactsReader>,
    pub(crate) audit_sink: Arc<dyn AuditSink>,
    /// The catalog: repositories, assets, versions and references.
    ///
    /// They are optional because a deployment can serve the administrative
    /// surface without serving content: without a catalog, the raw surface
    /// answers that it is not available, and not that the content does not
    /// exist -- a 404 would be a lie.
    pub(crate) catalog_reader: Option<Arc<dyn CatalogReader>>,
    pub(crate) catalog_writer: Option<Arc<dyn CatalogWriter>>,
    /// The search of the catalog.
    ///
    /// It is optional for the same reason the catalog is: a deployment
    /// that serves content without an index answers that the search is
    /// not available, and not that there is nothing to find.
    pub(crate) catalog_search: Option<Arc<dyn CatalogSearch>>,
    /// The store of content by digest.
    ///
    /// Optional for the same reason the catalog is: without it there are no
    /// bytes to serve.
    pub(crate) content_store: Option<Arc<dyn BlobStore>>,
    /// The garbage collection of the store of content.
    ///
    /// Optional: it needs the durable catalog (the roots) and a store that
    /// can reclaim. Without it the surface answers 501 and nothing is
    /// deleted.
    pub(crate) gc: Option<Arc<crate::gc::GcService>>,
    /// The integrity scrub of the registry: it reads the store and the
    /// catalog and reports; it never writes.
    ///
    /// Optional for the same reason as the garbage collection.
    pub(crate) scrub: Option<Arc<crate::recovery::Scrubber>>,
    /// The ledger of trust grants: which digest the trusted channel of a
    /// repository may serve.
    ///
    /// Optional, and fail-closed: a repository with the trusted channel
    /// and no ledger serves nothing.
    pub(crate) trust_ledger: Option<Arc<dyn hodei_catalog::TrustLedger>>,
    /// The egress and the provenance ledger of the remote proxy.
    ///
    /// Optional, and fail-closed: a proxy repository with no plane answers
    /// `501` and the service makes no outbound request.
    pub(crate) proxy_plane: Option<Arc<crate::proxy::ProxyPlane>>,
    /// The promotion service and the policy it decides with.
    ///
    /// Optional: without it the promotion routes answer `501` and nothing
    /// is ever granted, so a trusted channel stays empty.
    pub(crate) trust_plane: Option<Arc<crate::trust::promotion::TrustPlane>>,
    /// El ciclo reactivo CVE: admision de advisories, reevaluacion y
    /// barrido de vigencias (HSP-137).
    ///
    /// Optional, and fail-closed: a deployment without it answers `501`
    /// on the advisory entry and never reevaluates on its own -- a
    /// concession revoked by a CVE is revoked by the promotion path, not
    /// invented here.
    pub(crate) reactor: Option<Arc<crate::trust::reaction::VulnerabilityReactor>>,
    /// The investigation of a subject.
    ///
    /// Optional, and fail-closed: a deployment without the evidence and
    /// the graph answers that the investigation is not available, and not
    /// that there is nothing to find -- a 404 would be a lie, and an empty
    /// report would be worse.
    pub(crate) investigation: Option<Arc<dyn crate::investigation::InvestigationPort>>,
    /// The limits of the investigation.
    ///
    /// They are the deployment's, and a request can only narrow them.
    pub(crate) investigation_limits: crate::config::InvestigationConfig,
    /// The public address of the deployment.
    ///
    /// It is what the registry tells its clients: the absolute URL of a
    /// package, of a feed and of the token endpoint. It is decided by the
    /// deployment and not by the request, because a request can be
    /// written by anybody.
    pub(crate) public_url: crate::public_url::PublicUrl,
    /// The registry of the upload sessions.
    ///
    /// It lives here because it is state that **survives a request**: the
    /// protocol of push is by chunks, and between one request and the next
    /// there is a session that has to be remembered. Its identifier is
    /// opaque and its scope is the verified account, so a session of
    /// another account is a session that does not exist.
    pub(crate) uploads: Arc<crate::oci::uploads::UploadRegistry>,
    /// The quota ledger: what every account has reserved and stored.
    ///
    /// It lives here for the same reason the upload registry does: the
    /// reservation of an upload survives its request, and two uploads
    /// that arrive at once must be decided against the **same** state.
    /// A deployment that says nothing about quotas gets the ledger of
    /// no limits, which admits everything and is exactly what it said.
    pub(crate) quota: Arc<crate::quota::QuotaLedger>,
    /// La clave con la que el hub firma la metadata de los repositorios
    /// de sistema: el `Release.gpg` de apt y el `repomd.xml.asc` de yum.
    pub(crate) metadata_signer: Arc<crate::openpgp::MetadataSigner>,
    /// The largest content this service accepts, in bytes.
    ///
    /// A limit that is not written is a limit that is not enforced, and a
    /// service that accepts a body of any size is a service whose memory
    /// use depends on who is calling.
    pub(crate) max_body_size: usize,
    /// The clock and the identifier generator the security trail uses.
    ///
    /// They are seams: the production wiring reads the machine, and a test
    /// pins them so that the trail it compares is reproducible.
    pub(crate) identity: Identity,
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
    ///     // ... the rest of the ports ...
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
        identity: Identity,
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
            evidence_reader: Arc::new(EmptyEvidenceReader),
            risk_facts: Arc::new(crate::risk_facts::UnmeasuredRiskFacts),
            audit_sink,
            catalog_reader: None,
            catalog_writer: None,
            catalog_search: None,
            content_store: None,
            gc: None,
            scrub: None,
            trust_ledger: None,
            proxy_plane: None,
            trust_plane: None,
            reactor: None,
            uploads: Arc::new(crate::oci::uploads::UploadRegistry::default()),
            quota: Arc::new(crate::quota::QuotaLedger::new(
                crate::quota::QuotaLimits::default(),
            )),
            metadata_signer: Arc::new(crate::openpgp::bootstrap_signer()),
            max_body_size: 10 * 1024 * 1024,
            identity,
            admin_authenticator_configured: true,
            admin_authorizer_configured: true,
            investigation: None,
            investigation_limits: crate::config::InvestigationConfig::default(),
            public_url: crate::public_url::PublicUrl::default(),
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
            evidence_reader: Arc::new(EmptyEvidenceReader),
            risk_facts: Arc::new(crate::risk_facts::UnmeasuredRiskFacts),
            audit_sink,
            catalog_reader: None,
            catalog_writer: None,
            catalog_search: None,
            content_store: None,
            gc: None,
            scrub: None,
            trust_ledger: None,
            proxy_plane: None,
            trust_plane: None,
            reactor: None,
            uploads: Arc::new(crate::oci::uploads::UploadRegistry::default()),
            quota: Arc::new(crate::quota::QuotaLedger::new(
                crate::quota::QuotaLimits::default(),
            )),
            metadata_signer: Arc::new(crate::openpgp::bootstrap_signer()),
            max_body_size: 10 * 1024 * 1024,
            identity: Identity::system(),
            admin_authenticator_configured: false,
            admin_authorizer_configured: false,
            investigation: None,
            investigation_limits: crate::config::InvestigationConfig::default(),
            public_url: crate::public_url::PublicUrl::default(),
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
    /// Replace the reader of the accepted evidence.
    pub fn with_evidence_store(mut self, store: Arc<dyn EvidenceReader>) -> Self {
        self.evidence_reader = store;
        self
    }

    /// Wire the investigation of a subject.
    pub fn with_investigation(
        mut self,
        investigation: Arc<dyn crate::investigation::InvestigationPort>,
    ) -> Self {
        self.investigation = Some(investigation);
        self
    }

    /// Set the limits of the investigation.
    pub fn with_investigation_limits(mut self, limits: crate::config::InvestigationConfig) -> Self {
        self.investigation_limits = limits;
        self
    }

    /// Set the public address of the deployment.
    pub fn with_public_url(mut self, public_url: crate::public_url::PublicUrl) -> Self {
        self.public_url = public_url;
        self
    }

    /// Set the reader of the risk facts, once the deployment has an
    /// impact graph to read them from.
    pub fn with_risk_facts(
        mut self,
        risk_facts: Arc<dyn hodei_risk_storage::subject_risk::RiskFactsReader>,
    ) -> Self {
        self.risk_facts = risk_facts;
        self
    }

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

    /// Inject the catalog.
    ///
    /// The reader and the writer are the same store: the catalog is one
    /// thing that is read and written, and splitting it into two objects
    /// that do not share state would invent a catalog that does not exist.
    pub fn with_catalog(
        mut self,
        reader: Arc<dyn CatalogReader>,
        writer: Arc<dyn CatalogWriter>,
    ) -> Self {
        self.catalog_reader = Some(reader);
        self.catalog_writer = Some(writer);
        self
    }

    /// Inject the search of the catalog.
    ///
    /// It is a port of its own and not a method of the reader: "what is
    /// there?" and "what is at this path?" are different questions, with
    /// different bounds and different pages.
    pub fn with_catalog_search(mut self, searcher: Arc<dyn CatalogSearch>) -> Self {
        self.catalog_search = Some(searcher);
        self
    }

    /// Inject the promotion service.
    pub fn with_trust_plane(mut self, plane: Arc<crate::trust::promotion::TrustPlane>) -> Self {
        self.trust_plane = Some(plane);
        self
    }

    /// Inject the reactive cycle of the CVE.
    pub fn with_reactor(
        mut self,
        reactor: Arc<crate::trust::reaction::VulnerabilityReactor>,
    ) -> Self {
        self.reactor = Some(reactor);
        self
    }

    /// Inject the ledger of trust grants.
    pub fn with_trust_ledger(mut self, ledger: Arc<dyn hodei_catalog::TrustLedger>) -> Self {
        self.trust_ledger = Some(ledger);
        self
    }

    /// Inject the egress and provenance ledger of the remote proxy.
    pub fn with_proxy_plane(mut self, plane: Arc<crate::proxy::ProxyPlane>) -> Self {
        self.proxy_plane = Some(plane);
        self
    }

    /// Inject the garbage collection of the store of content.
    pub fn with_gc(mut self, gc: Arc<crate::gc::GcService>) -> Self {
        self.gc = Some(gc);
        self
    }

    /// Inject the integrity scrub of the registry.
    pub fn with_scrub(mut self, scrub: Arc<crate::recovery::Scrubber>) -> Self {
        self.scrub = Some(scrub);
        self
    }

    /// Inject the store of content by digest.
    pub fn with_content_store(mut self, store: Arc<dyn BlobStore>) -> Self {
        self.content_store = Some(store);
        self
    }

    /// Replace the registry of the upload sessions.
    ///
    /// The registry is what remembers a chunked upload between requests,
    /// and its limits --how long a session lives and how many can be
    /// alive at once-- are a decision of the deployment, not of the code.
    pub fn with_uploads(mut self, uploads: Arc<crate::oci::uploads::UploadRegistry>) -> Self {
        self.uploads = uploads;
        self
    }

    /// Set the quotas of the deployment.
    ///
    /// The limits are the deployment's decision, said in configuration;
    /// the ledger is shared by every request and every session, because
    /// a quota decided per request would be a quota with a race.
    pub fn with_quota(mut self, limits: crate::quota::QuotaLimits) -> Self {
        self.quota = Arc::new(crate::quota::QuotaLedger::new(limits));
        self
    }

    /// The largest content this service accepts.
    pub fn with_body_limit(mut self, limit: usize) -> Self {
        self.max_body_size = limit;
        self
    }

    /// Replace the clock and the identifier generator the security trail
    /// uses.
    ///
    /// A test that pins them gets the same moment and the same identifiers on
    /// every execution, and the trail it compares becomes a fact instead of a
    /// recollection.
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// Replace only the credential verifier, keeping the shared default store.
    pub fn with_authenticator(mut self, authenticator: Arc<dyn PrincipalAuthenticator>) -> Self {
        self.authenticator = authenticator;
        self.admin_authenticator_configured = true;
        self
    }
}
