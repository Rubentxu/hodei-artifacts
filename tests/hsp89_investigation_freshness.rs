//! HSP-89: la frescura de una investigación entregada.
//!
//! Un informe es una afirmación con fecha: dice qué leyó y en qué versión
//! del mundo. Esta comprobación responde si ese informe sigue apoyándose
//! en el mundo, y es de tres: **vigente**, **obsoleto** (diciendo qué
//! referencia se movió) o **no se sabe**.
//!
//! Lo que se comprueba aquí:
//!
//! - la consulta autorizada dice si el informe sigue vigente;
//! - un apoyo que se retiró deja el informe obsoleto, y dice cuál;
//! - un cambio ajeno al informe no lo invalida;
//! - lo que no está autenticado, es de otra cuenta o no tiene permiso
//!   **no lee nada**;
//! - un informe que no dice de qué mundo habló **no dice que sigue
//!   vigente**: dice que no se sabe.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::investigation::{CasContentStore, GraphInvestigation};
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
use hodei_evidence::ports::graph::GraphWriter;
use hodei_hub::blob::{BlobStore, FileBlobStore};
use hodei_investigator::claim::{AIClaim, ClaimKind, InvestigationTrace};
use hodei_investigator::freshness::{Freshness, GRAPH_SOURCE};
use hodei_investigator::read_set::{ReadSet, ReadStamp};
use hodei_investigator::report::InvestigationReport;
use hodei_investigator::scope::Coverage;
use hodei_investigator::{Investigation, InvestigationError};
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tempfile::TempDir;
use tower::ServiceExt;

/// La cuenta de la prueba.
const ACCOUNT: &str = "acme";

/// El sujeto de la investigación: el HRN canónico de un artefacto.
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

/// El tenant del grafo: el que dice la plantilla del despliegue.
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

/// El otro extremo de la relación.
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

/// El contenido de la evidencia: texto de un tercero, y por tanto dato.
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

/// El autorizador que concede la investigación.
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

/// El autorizador que concede otra cosa: el permiso de investigar no está.
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

/// La investigación que cuenta las veces que la llaman.
///
/// Sirve para probar lo que la puerta hace ANTES de leer: una peticion
/// rechazada no llega aqui, y eso se ve en el contador.
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

/// La aplicación con la superficie de investigación montada.
async fn app_with(
    authorizer: Arc<dyn AuthorizerPort>,
    investigation: Option<Arc<dyn hodei_artifacts_api::investigation::InvestigationPort>>,
    investigation_enabled: bool,
) -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.investigation_api_enabled = investigation_enabled;
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

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

/// El cuerpo de la petición de frescura: el sujeto y el manifiesto que el
/// cliente tiene.
fn the_body(subject: &str, read_set: &ReadSet) -> String {
    serde_json::json!({
        "subject": subject,
        "read_set": read_set,
    })
    .to_string()
}

/// La petición de frescura.
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

/// El cuerpo de la respuesta, como JSON.
async fn the_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .expect("the body is JSON")
}

/// El mundo de la prueba: su base, su grafo, su evidencia y su registro.
struct TheWorld {
    _tmp: TempDir,
    app: axum::Router,
    graph: Arc<hodei_evidence_storage::SurrealGraphStore<surrealdb::engine::local::Db>>,
    ledger: Arc<hodei_evidence_storage::SurrealRelationDeltaLedger<surrealdb::engine::local::Db>>,
    digest: hodei_evidence::ContentDigest,
}

/// El mundo de la prueba, montado con la superficie.
async fn the_world() -> TheWorld {
    let tmp = tempfile::tempdir().expect("temp dir");
    let cas = tmp.path().join("cas");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.investigation_api_enabled = true;
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

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

    let db = Arc::new(
        Surreal::new::<RocksDb>(
            tmp.path()
                .join("evidence.rocksdb")
                .to_string_lossy()
                .to_string(),
        )
        .await
        .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("artifacts")
        .await
        .expect("the ns/db");

    let evidence = Arc::new(hodei_evidence_storage::SurrealEvidenceStore::new(
        db.clone(),
    ));
    let graph = Arc::new(hodei_evidence_storage::SurrealGraphStore::new(db.clone()));
    let ledger = Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(
        db.clone(),
    ));
    let blobs = Arc::new(FileBlobStore::new(&cas).expect("the store of content"));

    // La evidencia: su contenido en el CAS, con el digest que lo nombra.
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

    // El grafo: la relacion que la evidencia sostiene, con la causa que
    // hace que el registro diga lo que cambio.
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
    graph.apply(changes).await.expect("the graph is written");

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(GraphInvestigation::new(
        evidence.clone() as Arc<dyn EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph.clone(),
        ledger.clone() as Arc<dyn RelationDeltaLedger>,
        authorize,
        config.investigation.clone(),
    )));
    let app = build_router(state, &config);

    TheWorld {
        _tmp: tmp,
        app,
        graph,
        ledger,
        digest,
    }
}

/// El manifiesto de una lectura: lo que el informe diria haber leido.
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

/// Un informe cuyo mundo no se movio SIGUE VIGENTE.
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

/// El apoyo que el informe leyo ya no esta: el informe OBSOLETO, y dice
/// que referencia fue la que se movio.
#[tokio::test]
async fn the_support_that_is_gone_makes_the_report_stale() {
    let world = the_world().await;
    let report = the_report(&world).await;

    // La evidencia retira su apoyo.
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
    world
        .graph
        .apply(withdrawal)
        .await
        .expect("the withdrawal is written");

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

/// Un cambio AJENO al informe no lo invalida: lo que no se leyo no puede
/// caducar lo que se leyo.
#[tokio::test]
async fn a_change_away_from_the_report_does_not_invalidate_it() {
    let world = the_world().await;
    let report = the_report(&world).await;

    // Otra relacion, de otro par de nodos.
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
    world
        .graph
        .apply(changes)
        .await
        .expect("the graph is written");

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

/// Un informe que no dice de que mundo hablo NO dice que sigue vigente:
/// dice que no se sabe.
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

/// Lo que no esta autenticado no llega a leer nada.
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

/// El sujeto de otra cuenta no esta, y no se evalua.
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

/// Sin permiso no se comprueba.
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

/// Un despliegue que no declara la superficie no la monta.
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
