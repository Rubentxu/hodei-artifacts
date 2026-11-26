//! HSP-89: the freshness of a delivered investigation.
//!
//! A report is a dated assertion: it says what it read and in which version
//! of the world. This check answers whether that report still rests
//! on the world, and it has three outcomes: **current**, **stale** (saying which
//! reference moved) or **unknown**.
//!
//! What is checked here:
//!
//! - the authorized query says whether the report is still current;
//! - a support that was withdrawn leaves the report stale, and says which;
//! - a change unrelated to the report does not invalidate it;
//! - what is not authenticated, belongs to another account or lacks permission
//!   **reads nothing**;
//! - a report that does not say which world it spoke of **does not say it is still
//!   current**: it says it is unknown.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::investigation::CasContentStore;
use hodei_artifacts_api::investigation_dgraph::DgraphInvestigation;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_evidence::ContentRef;
use hodei_evidence::domain::delta::RelationDeltaLedger;
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence::application::projection::{ProjectionDelivery, ProjectionFrontier};
use hodei_hub::blob::{BlobStore, FileBlobStore};
use hodei_investigator::claim::{AIClaim, ClaimKind, InvestigationTrace};
use hodei_investigator::freshness::{Freshness, GRAPH_SOURCE};
use hodei_investigator::read_set::{ReadSet, ReadStamp};
use hodei_investigator::report::InvestigationReport;
use hodei_investigator::scope::Coverage;
use hodei_investigator::{Investigation, InvestigationError};
use kernel::Hrn;
use tower::ServiceExt;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

/// The test's account.
const ACCOUNT: &str = "acme";

/// The subject of the investigation: an artifact's canonical HRN.
fn subject() -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        ACCOUNT.to_string(),
        "artifact".to_string(),
        "libs/sha256-0123456789abcdef".to_string(),
    )
}

/// The graph's tenant: the one the deployment template states.
fn tenant() -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        ACCOUNT.to_string(),
        "artifact".to_string(),
        ACCOUNT.to_string(),
    )
}

/// The other end of the relation.
fn component() -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        ACCOUNT.to_string(),
        "artifact".to_string(),
        "libs/sha256-000000000000".to_string(),
    )
}

/// The evidence content: third-party text, and therefore data.
const EVIDENCE_TEXT: &str = "the sbom of the artefact declares the component lib-vuln";

struct TestAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TestAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
            "carol" => ("other", "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// The authorizer that grants the investigation.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"Investigate",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

/// The authorizer that grants something else: the permission to investigate is not there.
fn cedar_without_investigation() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadCapsule",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

/// The investigation that counts how many times it is called.
///
/// It serves to test what the gate does BEFORE reading: a rejected
/// request does not get here, and that is seen in the counter.
struct CountingInvestigation {
    calls: Arc<AtomicUsize>,
    freshness: Arc<std::sync::Mutex<Option<Freshness>>>,
}

impl CountingInvestigation {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            freshness: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn the_freshness(&self) -> Freshness {
        self.freshness
            .lock()
            .expect("el cerrojo de la frescura")
            .clone()
            .unwrap_or(Freshness::Current)
    }
}

#[async_trait]
impl hodei_artifacts_api::investigation::InvestigationPort for CountingInvestigation {
    async fn investigate(
        &self,
        _investigation: &str,
        _context: &hodei_authz::VerifiedContext,
        scope: &hodei_investigator::scope::RetrievalScope,
        _question: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Investigation, InvestigationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let trace = InvestigationTrace::new(
            "inv-1",
            "deterministic",
            "none",
            None,
            "none",
            Vec::new(),
            Coverage::Complete,
            ReadSet::empty(),
            Vec::new(),
            Vec::new(),
            now,
        );
        Ok(Investigation {
            claims: vec![AIClaim::new(
                "nothing was read",
                ClaimKind::Unknown,
                Vec::new(),
            )],
            trace,
            report: InvestigationReport {
                schema_version: "1".to_string(),
                subject: scope.subject.to_string(),
                findings: Vec::new(),
                coverage: Coverage::Complete,
                read_set: ReadSet::empty(),
            },
        })
    }

    async fn freshness_of(
        &self,
        _scope: &hodei_investigator::scope::RetrievalScope,
        _report: &ReadSet,
    ) -> Result<Freshness, InvestigationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.the_freshness())
    }
}

/// The application with the investigation surface mounted.
async fn app_with(
    authorizer: Arc<dyn AuthorizerPort>,
    investigation: Option<Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>>,
    investigation_enabled: bool,
) -> (axum::Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.investigation_api_enabled = investigation_enabled;
    tmp.apply(&mut config);

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap");
    let state = state.with_authorization(authorizer).with_repository_auth(
        Arc::new(TestAuthenticator),
        Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
    );
    let state = match investigation {
        Some(investigation) => state.with_investigation(investigation),
        None => state,
    };
    (build_router(state, &config), tmp)
}

/// The freshness request body: the subject and the manifest the
/// client has.
fn the_body(subject: &str, read_set: &ReadSet) -> String {
    serde_json::json!({
        "subject": subject,
        "read_set": read_set,
    })
    .to_string()
}

/// The freshness request.
fn request(token: Option<&str>, payload: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/investigations/freshness")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder
        .body(Body::from(payload.to_string()))
        .expect("a request")
}

/// The response body, as JSON.
async fn the_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .expect("the body is JSON")
}

/// The test's world: its database, its graph, its evidence and its registry.
struct TheWorld {
    _tmp: Workdir,
    app: axum::Router,
    frontier: Arc<hodei_evidence_storage::DgraphProjectionFrontier>,
    ledger: Arc<hodei_evidence_storage::DgraphDeltaLedger>,
    next_position: std::sync::atomic::AtomicU64,
    digest: hodei_evidence::ContentDigest,
}

impl TheWorld {
    /// Say a change of the graph the way the projection does: through the
    /// frontier, which is what writes the relation deltas the freshness
    /// check reads (the plain graph writer emits none).
    async fn say(&self, changes: GraphChangeSet) {
        let position = self.next_position.fetch_add(1, Ordering::SeqCst);
        self.frontier
            .apply_and_checkpoint(
                changes,
                &ProjectionDelivery {
                    consumer: "hsp89".to_string(),
                    stream_id: "freshness".to_string(),
                    position,
                    event_id: format!("evt-{position}"),
                },
            )
            .await
            .expect("the change goes through the frontier");
    }
}

/// The test's world, mounted with the surface.
async fn the_world() -> TheWorld {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.investigation_api_enabled = true;
    tmp.apply(&mut config);

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap");
    let state = state.with_authorization(cedar()).with_repository_auth(
        Arc::new(TestAuthenticator),
        Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
    );

    let client = tmp.tenant().client().await;
    let evidence = Arc::new(
        hodei_evidence_storage::DgraphEvidenceStore::open(client.clone())
            .await
            .expect("the evidence store opens"),
    );
    let graph = Arc::new(hodei_evidence_storage::DgraphGraphStore::new(
        client.clone(),
    ));
    let frontier = Arc::new(
        hodei_evidence_storage::DgraphProjectionFrontier::open(
            client.clone(),
            &tmp.tenant().namespace.to_string(),
        )
        .await
        .expect("the frontier opens"),
    );
    let ledger = Arc::new(hodei_evidence_storage::DgraphDeltaLedger::new(
        frontier.clone(),
    ));
    let blobs = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the store of content"));

    // The evidence: its content in the CAS, with the digest that names it.
    let content = EVIDENCE_TEXT.as_bytes().to_vec();
    let digest = blobs.put(&content).await.expect("the content is stored");
    let record = EvidenceRecord::builder(EvidenceId::new("ev-hsp89-1"), tenant())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject()])
        .source(SourceRef::by_external_id("external-hsp89-1"))
        .content(ContentRef::by_digest(digest.clone()))
        .observed_at(chrono::Utc::now())
        .recorded_at(chrono::Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek").with_version("1.0.0"))
        .build();
    evidence.put(record).await.expect("the evidence is stored");

    // The graph: the relation the evidence supports, with the cause that
    // makes the registry say what changed.
    let relation = GraphRelation::new(
        RelationType::Contains,
        subject(),
        component(),
        tenant(),
        EvidenceId::new("ev-hsp89-1"),
        chrono::Utc::now(),
    );
    let changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(
            subject(),
            tenant(),
            NodeKind::Artifact,
            chrono::Utc::now(),
        ))
        .with_node(GraphNode::new(
            component(),
            tenant(),
            NodeKind::Artifact,
            chrono::Utc::now(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(
                &tenant(),
                &EvidenceId::new("ev-hsp89-1"),
            ),
            tenant(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ))
        .with_relation(relation)
        .with_cause("evt-1");

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(DgraphInvestigation::new(
        evidence.clone() as Arc<dyn EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph.clone(),
        ledger.clone() as Arc<dyn RelationDeltaLedger>,
        authorize,
        config.investigation.clone(),
    )));
    let app = build_router(state, &config);

    let world = TheWorld {
        _tmp: tmp,
        app,
        frontier,
        ledger,
        next_position: std::sync::atomic::AtomicU64::new(0),
        digest,
    };
    world.say(changes).await;
    world
}

/// A read's manifest: what the report would say it read.
async fn the_report(world: &TheWorld) -> ReadSet {
    let version = world
        .ledger
        .version_of(&tenant())
        .await
        .expect("the version runs");
    let mut read_set = ReadSet::empty();
    read_set.generation(GRAPH_SOURCE, version);
    read_set.read(ReadStamp::relation(
        format!("contains|{}|{}", subject(), component()),
        "ev-hsp89-1",
    ));
    read_set.read(ReadStamp::evidence("ev-hsp89-1", world.digest.to_string()));
    read_set
}

/// A report whose world did not move REMAINS CURRENT.
#[tokio::test]
async fn a_report_whose_world_did_not_move_is_still_current() {
    let world = the_world().await;
    let report = the_report(&world).await;

    let response = world
        .app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &report),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body = the_json(response).await;
    assert_eq!(body["status"], "current");
    assert!(
        body.get("changed").is_none(),
        "nada cambio: no hay lista: {body}"
    );
}

/// The support the report read is gone: the report is STALE, and says
/// which reference was the one that moved.
#[tokio::test]
async fn the_support_that_is_gone_makes_the_report_stale() {
    let world = the_world().await;
    let report = the_report(&world).await;

    // The evidence withdraws its support.
    let withdrawal = GraphChangeSet::empty()
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Contains,
            subject(),
            component(),
            tenant(),
            EvidenceId::new("ev-hsp89-1"),
            chrono::Utc::now(),
        ))
        .with_cause("evt-2");
    world.say(withdrawal).await;

    let response = world
        .app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &report),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body = the_json(response).await;
    assert_eq!(body["status"], "stale");
    let changed = body["changed"].as_array().expect("the changed references");
    assert_eq!(changed.len(), 1, "una referencia cambio: {body}");
    assert_eq!(changed[0]["kind"], "relation");
    assert_eq!(changed[0]["evidence"], "ev-hsp89-1");
}

/// A change UNRELATED to the report does not invalidate it: what was not read cannot
/// expire what was read.
#[tokio::test]
async fn a_change_away_from_the_report_does_not_invalidate_it() {
    let world = the_world().await;
    let report = the_report(&world).await;

    // Another relation, of another pair of nodes.
    let other = GraphRelation::new(
        RelationType::Contains,
        subject(),
        Hrn::with_region(
            "hodei".to_string(),
            "artifacts".to_string(),
            "eu-west-1".to_string(),
            ACCOUNT.to_string(),
            "artifact".to_string(),
            "libs/sha256-999999999999".to_string(),
        ),
        tenant(),
        EvidenceId::new("ev-hsp89-2"),
        chrono::Utc::now(),
    );
    let changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(
            other.to.clone(),
            tenant(),
            NodeKind::Artifact,
            chrono::Utc::now(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(
                &tenant(),
                &EvidenceId::new("ev-hsp89-2"),
            ),
            tenant(),
            NodeKind::Evidence,
            chrono::Utc::now(),
        ))
        .with_relation(other)
        .with_cause("evt-3");
    world.say(changes).await;

    let response = world
        .app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &report),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body = the_json(response).await;
    assert_eq!(body["status"], "current");
}

/// A report that does not say which world it spoke of does NOT say it is still current:
/// it says it is unknown.
#[tokio::test]
async fn a_report_without_a_version_does_not_say_that_it_is_current() {
    let world = the_world().await;
    let mut report = ReadSet::empty();
    report.read(ReadStamp::relation("contains|a|b", "ev-hsp89-1"));

    let response = world
        .app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &report),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let body = the_json(response).await;
    assert_eq!(body["status"], "unknown");
    assert!(
        body["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()),
        "el motivo se dice: {body}"
    );
}

/// What is not authenticated does not get to read anything.
#[tokio::test]
async fn sin_credencial_no_se_consulta_nada() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;
    let report = ReadSet::empty();

    let response = app
        .oneshot(request(None, &the_body(&subject().to_string(), &report)))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        investigation.calls(),
        0,
        "the rejection happens before anything is read"
    );
}

/// The subject of another account is not there, and is not evaluated.
#[tokio::test]
async fn el_sujeto_de_otra_cuenta_no_esta() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;
    let foreign = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        "contoso".to_string(),
        "artifact".to_string(),
        "libs/sha256-0123456789abcdef".to_string(),
    );

    let response = app
        .oneshot(request(
            Some("alice"),
            &the_body(&foreign.to_string(), &ReadSet::empty()),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(investigation.calls(), 0);
}

/// Without permission there is no check.
#[tokio::test]
async fn sin_permiso_no_se_comprueba() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar_without_investigation(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;

    let response = app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &ReadSet::empty()),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(investigation.calls(), 0);
}

/// A deployment that does not declare the surface does not mount it.
#[tokio::test]
async fn un_despliegue_que_no_declara_la_superficie_no_la_monta() {
    let (app, _tmp) = app_with(cedar(), None, false).await;

    let response = app
        .oneshot(request(
            Some("alice"),
            &the_body(&subject().to_string(), &ReadSet::empty()),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
