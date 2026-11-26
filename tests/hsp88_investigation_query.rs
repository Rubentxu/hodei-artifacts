//! HSP-88: the read-only investigation query.
//!
//! The investigation is a READ: the body names the subject and, if
//! wanted, the question, and the response is the investigator's report.
//! Here what the contract promises is checked:
//!
//! - the authorized query returns a report with its citations and its
//!   coverage;
//! - it works without a model: the findings are those of the tools and the
//!   question is declared unanswered;
//! - what is not authorized does not get to read anything, and what does not fit in
//!   the budget is DECLARED omitted;
//! - the surface writes nothing: it does not promote, does not change a policy,
//!   does not approve an exception and does not write a decision.

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
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::domain::graph::{GraphNode, GraphRelation, NodeKind, RelationType};
use hodei_evidence::ports::EvidenceStore;
use hodei_hub::blob::{BlobStore, FileBlobStore};
use hodei_investigator::claim::{AIClaim, ClaimKind, InvestigationTrace};
use hodei_investigator::report::InvestigationReport;
use hodei_investigator::scope::Coverage;
use hodei_investigator::{Investigation, ReadSet};
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
///
/// The default template is `hrn:hodei:artifacts:{region}:{account}:artifact/{account}`,
/// and the subject lives in `eu-west-1` and in the `acme` account.
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
    scopes: Arc<std::sync::Mutex<Vec<hodei_investigator::scope::RetrievalScope>>>,
}

impl CountingInvestigation {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            scopes: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn the_scope(&self) -> hodei_investigator::scope::RetrievalScope {
        self.scopes
            .lock()
            .expect("el cerrojo del alcance")
            .first()
            .cloned()
            .expect("hubo una llamada")
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
    ) -> Result<Investigation, hodei_investigator::investigator::InvestigationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.scopes
            .lock()
            .expect("el cerrojo del alcance")
            .push(scope.clone());
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
    ) -> Result<
        hodei_investigator::freshness::Freshness,
        hodei_investigator::investigator::InvestigationError,
    > {
        Ok(hodei_investigator::freshness::Freshness::Current)
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

/// The stores of the investigation over the Dgraph namespace of the
/// deployment: the same adapters the graph profile wires, handed to the test
/// so it can seed the world before asking.
struct Stores {
    evidence: Arc<hodei_evidence_storage::DgraphEvidenceStore>,
    graph: Arc<hodei_evidence_storage::DgraphGraphStore>,
    ledger: Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
    blobs: Arc<FileBlobStore>,
}

async fn stores_of(tmp: &Workdir) -> Stores {
    let client = tmp.tenant().client().await;
    let frontier = Arc::new(
        hodei_evidence_storage::DgraphProjectionFrontier::open(
            client.clone(),
            &tmp.tenant().namespace.to_string(),
        )
        .await
        .expect("the frontier opens"),
    );
    Stores {
        evidence: Arc::new(
            hodei_evidence_storage::DgraphEvidenceStore::open(client.clone())
                .await
                .expect("the evidence store opens"),
        ),
        graph: Arc::new(hodei_evidence_storage::DgraphGraphStore::new(client)),
        ledger: Arc::new(hodei_evidence_storage::DgraphDeltaLedger::new(frontier)),
        blobs: Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the store of content")),
    }
}

/// The request body.
fn body(subject: &str, extra: &str) -> String {
    format!("{{\"subject\":\"{subject}\"{extra}}}")
}

/// The investigation request.
fn request(token: Option<&str>, payload: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/investigations")
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

/// The authorized query returns the investigator's report.
#[tokio::test]
async fn la_consulta_autorizada_devuelve_el_informe() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                r#","question":"why is it affected?""#,
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(investigation.calls(), 1);

    let body = the_json(response).await;
    assert_eq!(body["schema_version"], "1");
    assert_eq!(body["subject"], subject().to_string());
    assert!(body["findings"].is_array());
    assert!(body["read_set"].is_object());
    // The query's scope is that of the subject and of the verified
    // account: the actor is the identity asking.
    let scope = investigation.the_scope();
    assert_eq!(scope.subject, subject());
    assert_eq!(scope.actor.account_id(), ACCOUNT);
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

    let response = app
        .oneshot(request(None, &body(&subject().to_string(), "")))
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
        .oneshot(request(Some("alice"), &body(&foreign.to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(investigation.calls(), 0);
}

/// The subject has to be an artifact: a repository is not investigated.
#[tokio::test]
async fn un_sujeto_que_no_es_un_artefacto_no_esta() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;
    let repository = Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu-west-1".to_string(),
        ACCOUNT.to_string(),
        "repository".to_string(),
        "releases".to_string(),
    );

    let response = app
        .oneshot(request(Some("alice"), &body(&repository.to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(investigation.calls(), 0);
}

/// The subject has to be a canonical HRN.
#[tokio::test]
async fn un_sujeto_que_no_es_un_hrn_no_valida() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;

    let response = app
        .oneshot(request(Some("alice"), &body("no soy un HRN", "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(investigation.calls(), 0);
}

/// Without permission there is no investigation.
#[tokio::test]
async fn sin_permiso_no_se_investiga() {
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
        .oneshot(request(Some("alice"), &body(&subject().to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(investigation.calls(), 0);
}

/// The question has a size, and the size is the deployment's.
#[tokio::test]
async fn una_pregunta_mas_larga_que_el_limite_no_pasa() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;
    let question = "x".repeat(2001);

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                &format!(r#","question":"{question}""#),
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(investigation.calls(), 0);
}

/// The request NARROWS the deployment's budget, never widens it.
#[tokio::test]
async fn la_peticion_solo_acota_el_presupuesto() {
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                r#","max_evidence":5,"max_relations":7,"max_bytes":11"#,
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let scope = investigation.the_scope();
    assert_eq!(scope.budget.max_objects, 5);
    assert_eq!(scope.budget.max_relations, 7);
    assert_eq!(scope.budget.max_bytes, 11);

    // And what is requested beyond that is the deployment's limit.
    let investigation = Arc::new(CountingInvestigation::new());
    let (app, _tmp) = app_with(
        cedar(),
        Some(
            investigation.clone() as Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>
        ),
        true,
    )
    .await;
    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                r#","max_evidence":100000,"max_relations":100000,"max_bytes":100000000"#,
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);

    let scope = investigation.the_scope();
    assert_eq!(scope.budget.max_objects, 100);
    assert_eq!(scope.budget.max_relations, 400);
    assert_eq!(scope.budget.max_bytes, 256 * 1024);
}

/// The surface is a read: it has no other method.
#[tokio::test]
async fn la_superficie_no_tiene_otros_metodos() {
    let (app, _tmp) = app_with(cedar(), None, true).await;

    for method in ["PUT", "DELETE", "PATCH", "GET"] {
        let request = Request::builder()
            .method(method)
            .uri("/api/v1/investigations")
            .header("authorization", "Bearer alice")
            .header("content-type", "application/json")
            .body(Body::from(body(&subject().to_string(), "")))
            .expect("a request");
        let response = app.clone().oneshot(request).await.expect("the request");
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "the surface only answers POST"
        );
    }
}

/// A deployment that does not declare the surface does not mount it.
#[tokio::test]
async fn un_despliegue_que_no_declara_la_superficie_no_la_monta() {
    let (app, _tmp) = app_with(cedar(), None, false).await;

    let response = app
        .oneshot(request(Some("alice"), &body(&subject().to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The deployment wires the investigation over its stores.
///
/// In the graph profile the content store is mandatory and the
/// investigation is always wired: a deployment without a usable store does not
/// get to serve, so the surface can never return an empty report
/// that looks like an answer. Startup refuses, saying which
/// directory cannot be opened.
#[tokio::test]
async fn un_despliegue_sin_almacen_de_contenido_no_arranca() {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.investigation_api_enabled = true;
    tmp.apply(&mut config);
    // The content directory is a path under a regular file: it cannot exist.
    let file = tmp.path().join("not-a-directory");
    std::fs::write(&file, b"a file").expect("the file is written");
    config.content.cas_path = file.join("cas").to_string_lossy().into_owned();

    let error = match bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    {
        Ok(_) => panic!("a deployment without a usable content store must not boot"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.to_lowercase().contains("content directory"),
        "the refusal names the content store: {error}"
    );
}

/// The real investigation: the report cites the evidence and declares the coverage.
///
/// here the world is real: there is a piece of evidence with its content in
/// the CAS, and a graph relation that supports it. The report has to
/// say both things.
#[tokio::test]
async fn el_informe_cita_la_evidencia_y_declara_la_cobertura() {
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

    // The world of the investigation: the stores of the graph profile over
    // the Dgraph namespace of the deployment and its content store.
    let Stores {
        evidence,
        graph,
        ledger,
        blobs,
    } = stores_of(&tmp).await;

    // The evidence: its content in the CAS, with the digest that names it.
    let content = EVIDENCE_TEXT.as_bytes().to_vec();
    let digest = blobs.put(&content).await.expect("the content is stored");
    let record = EvidenceRecord::builder(EvidenceId::new("ev-hsp88-1"), tenant())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject()])
        .source(SourceRef::by_external_id("external-hsp88-1"))
        .content(ContentRef::by_digest(digest.clone()))
        .observed_at(chrono::Utc::now())
        .recorded_at(chrono::Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek").with_version("1.0.0"))
        .build();
    evidence.put(record).await.expect("the evidence is stored");

    // The graph: the relation the evidence supports.
    use hodei_evidence::ports::graph::GraphWriter;
    let now = chrono::Utc::now();
    let changes = hodei_evidence::domain::graph::GraphChangeSet::empty()
        .with_node(GraphNode::new(subject(), tenant(), NodeKind::Artifact, now))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(
                &tenant(),
                &hodei_evidence::EvidenceId::new("ev-hsp88-1"),
            ),
            tenant(),
            NodeKind::Evidence,
            now,
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            subject(),
            Hrn::with_region(
                "hodei".to_string(),
                "artifacts".to_string(),
                "eu-west-1".to_string(),
                ACCOUNT.to_string(),
                "artifact".to_string(),
                "libs/sha256-0123456789abcdef".to_string(),
            ),
            tenant(),
            hodei_evidence::EvidenceId::new("ev-hsp88-1"),
            now,
        ));
    graph.apply(changes).await.expect("the graph is written");

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(DgraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        ledger,
        authorize,
        config.investigation.clone(),
    )));
    let app = build_router(state, &config);

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                r#","question":"why is it affected?""#,
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_json(response).await;

    // The report declares what was read, with the digest that verifies it.
    let stamps = body["read_set"]["stamps"]
        .as_array()
        .expect("the read set has stamps");
    let evidence_stamp = stamps
        .iter()
        .find(|stamp| stamp["resource"] == "ev-hsp88-1")
        .expect("the evidence that was read is in the manifest");
    assert_eq!(evidence_stamp["kind"], "evidence");
    assert_eq!(
        evidence_stamp["verified_by"],
        digest.to_string(),
        "the manifest carries the digest, not the content"
    );

    // The coverage is declared: what was not read is not hidden.
    assert!(
        body["coverage"] == "complete" || body["coverage"]["partial"].is_object(),
        "the coverage is declared"
    );

    // And the report carries no decision: it describes.
    let report = body.as_object().expect("the report is an object");
    for key in report.keys() {
        assert!(
            !key.contains("decision"),
            "the report describes and does not decide"
        );
    }
}

/// What does not fit in the budget is DECLARED omitted.
///
/// With a budget of zero pieces of evidence, the evidence is not read
/// and the report says so: coverage is partial and says how many things
/// were left out.
#[tokio::test]
async fn lo_que_no_cabe_en_el_presupuesto_se_declara_omitido() {
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

    // The world of the investigation: the stores of the graph profile over
    // the Dgraph namespace of the deployment and its content store.
    let Stores {
        evidence,
        graph,
        ledger,
        blobs,
    } = stores_of(&tmp).await;

    let content = EVIDENCE_TEXT.as_bytes().to_vec();
    let digest = blobs.put(&content).await.expect("the content is stored");
    let record = EvidenceRecord::builder(EvidenceId::new("ev-hsp88-1"), tenant())
        .evidence_type(EvidenceType::Sbom)
        .subjects(vec![subject()])
        .source(SourceRef::by_external_id("external-hsp88-1"))
        .content(ContentRef::by_digest(digest))
        .observed_at(chrono::Utc::now())
        .recorded_at(chrono::Utc::now())
        .verification(EvidenceVerification::verified("dsse"))
        .normalization(Normalization::new("cyclonedx", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek").with_version("1.0.0"))
        .build();
    evidence.put(record).await.expect("the evidence is stored");

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(DgraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        ledger,
        authorize,
        config.investigation.clone(),
    )));
    let app = build_router(state, &config);

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(&subject().to_string(), r#","max_evidence":0"#),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_json(response).await;

    assert_eq!(
        body["coverage"]["partial"]["omitted"], 1,
        "the evidence that did not fit is declared as omitted"
    );
}

/// Without a model, the question is declared unanswered.
///
/// The report says the question has no answer, and says it as an
/// uncertainty: an answer that pretends to have answered it is a
/// fabrication.
#[tokio::test]
async fn sin_modelo_la_pregunta_se_declara_sin_responder() {
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

    // The world of the investigation: the stores of the graph profile over
    // the Dgraph namespace of the deployment and its content store.
    let Stores {
        evidence,
        graph,
        ledger,
        blobs,
    } = stores_of(&tmp).await;

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(DgraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        ledger,
        authorize,
        config.investigation.clone(),
    )));
    let app = build_router(state, &config);

    let response = app
        .oneshot(request(
            Some("alice"),
            &body(
                &subject().to_string(),
                r#","question":"why is it affected?""#,
            ),
        ))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_json(response).await;

    let findings = body["findings"].as_array().expect("the findings");
    let uncertainty = findings
        .iter()
        .find(|finding| finding["kind"] == "uncertainty")
        .expect("the uncertainty is declared");
    assert!(
        uncertainty["citations"]
            .as_array()
            .expect("the citations")
            .is_empty(),
        "an uncertainty cites nothing: it says what is not known"
    );
}
