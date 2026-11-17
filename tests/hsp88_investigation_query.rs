//! HSP-88: la consulta de investigación de solo lectura.
//!
//! La investigación es una LECTURA: el cuerpo nombra el sujeto y, si se
//! quiere, la pregunta, y la respuesta es el informe del investigador.
//! Aquí se comprueba lo que el contrato promete:
//!
//! - la consulta autorizada devuelve un informe con sus citas y su
//!   cobertura;
//! - funciona sin modelo: los hallazgos son los de las herramientas y la
//!   pregunta se declara sin responder;
//! - lo que no está autorizado no llega a leer nada, y lo que no cabe en
//!   el presupuesto se DECLARA omitido;
//! - la superficie no escribe nada: no promueve, no cambia una política,
//!   no aprueba una excepción y no escribe una decisión.

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
///
/// La plantilla por defecto es `hrn:hodei:artifacts:{region}:{account}:artifact/{account}`,
/// y el sujeto vive en `eu-west-1` y en la cuenta `acme`.
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

/// La investigacion que cuenta las veces que la llaman.
///
/// Sirve para probar lo que la puerta hace ANTES de leer: una peticion
/// rechazada no llega aqui, y eso se ve en el contador.
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

/// La aplicacion con la superficie de investigacion montada.
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

/// El cuerpo de la peticion.
fn body(subject: &str, extra: &str) -> String {
    format!("{{\"subject\":\"{subject}\"{extra}}}")
}

/// La peticion de la investigacion.
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

/// El cuerpo de la respuesta, como JSON.
async fn the_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .expect("the body is JSON")
}

/// La consulta autorizada devuelve el informe del investigador.
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
    // El alcance de la consulta es el del sujeto y el de la cuenta
    // verificada: el actor es la identidad que pregunta.
    let scope = investigation.the_scope();
    assert_eq!(scope.subject, subject());
    assert_eq!(scope.actor.account_id(), ACCOUNT);
}

/// Lo que no está autenticado no llega a leer nada.
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

/// El sujeto de otra cuenta no está, y no se evalúa.
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

/// El sujeto tiene que ser un artefacto: un repositorio no se investiga.
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

/// El sujeto tiene que ser un HRN canonico.
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

/// Sin permiso no se investiga.
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

/// La pregunta tiene un tamaño, y el tamaño es el del despliegue.
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

/// La peticion ACOTA el presupuesto del despliegue, nunca lo amplía.
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

    // Y lo que se pide de mas es el limite del despliegue.
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

/// La superficie es una lectura: no tiene ningun otro metodo.
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

/// Un despliegue que no declara la superficie no la monta.
#[tokio::test]
async fn un_despliegue_que_no_declara_la_superficie_no_la_monta() {
    let (app, _tmp) = app_with(cedar(), None, false).await;

    let response = app
        .oneshot(request(Some("alice"), &body(&subject().to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// El despliegue cablea la investigacion sobre sus almacenes.
///
/// Sin almacen de contenido no hay investigacion, y la superficie lo dice
/// con un `503` en vez de devolver un informe vacio que parece una
/// respuesta.
#[tokio::test]
async fn un_despliegue_sin_almacen_de_contenido_dice_que_no_esta() {
    let (app, _tmp) = app_with(cedar(), None, true).await;

    let response = app
        .oneshot(request(Some("alice"), &body(&subject().to_string(), "")))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = the_json(response).await;
    assert_eq!(body["error"], "unavailable");
}

/// La investigacion real: el informe cita la evidencia y declara la cobertura.
///
/// aqui el mundo es real: hay una pieza de evidencia con su contenido en
/// el CAS, y una relacion del grafo que la sostiene. El informe tiene que
/// decir las dos cosas.
#[tokio::test]
async fn el_informe_cita_la_evidencia_y_declara_la_cobertura() {
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

    // El mundo de la investigacion: su propia base, su propio CAS.
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
    let blobs = Arc::new(FileBlobStore::new(&cas).expect("the store of content"));

    // La evidencia: su contenido en el CAS, con el digest que lo nombra.
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

    // El grafo: la relacion que la evidencia sostiene.
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
    let state = state.with_investigation(Arc::new(GraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(
            db.clone(),
        )) as Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
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

    // El informe declara lo que se leyo, con el digest que lo verifica.
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

    // La cobertura se declara: lo que no se leyo no se esconde.
    assert!(
        body["coverage"] == "complete" || body["coverage"]["partial"].is_object(),
        "the coverage is declared"
    );

    // Y el informe no lleva ninguna decision: describe.
    let report = body.as_object().expect("the report is an object");
    for key in report.keys() {
        assert!(
            !key.contains("decision"),
            "the report describes and does not decide"
        );
    }
}

/// Lo que no cabe en el presupuesto se DECLARA omitido.
///
/// Con un presupuesto de cero piezas de evidencia, la evidencia no se lee
/// y el informe lo dice: la cobertura es parcial y dice cuantas cosas se
/// quedaron fuera.
#[tokio::test]
async fn lo_que_no_cabe_en_el_presupuesto_se_declara_omitido() {
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
    let blobs = Arc::new(FileBlobStore::new(&cas).expect("the store of content"));

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
    let state = state.with_investigation(Arc::new(GraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(
            db.clone(),
        )) as Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
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

/// Sin modelo, la pregunta se declara sin responder.
///
/// El informe dice que la pregunta no tiene respuesta, y lo dice como una
/// incertidumbre: una respuesta que finge haberla respondido es una
/// fabricacion.
#[tokio::test]
async fn sin_modelo_la_pregunta_se_declara_sin_responder() {
    let tmp = tempfile::tempdir().expect("temp dir");
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
    let blobs = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the store of content"));

    let authorize = state.authorize_resource.clone();
    let state = state.with_investigation(Arc::new(GraphInvestigation::new(
        evidence.clone() as Arc<dyn hodei_evidence::ports::EvidenceStore>,
        Arc::new(CasContentStore::new(blobs.clone() as Arc<dyn BlobStore>)),
        graph,
        Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(
            db.clone(),
        )) as Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger>,
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
