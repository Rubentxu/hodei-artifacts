//! La busqueda y la vista de detalle (HSP-113), contra el router de verdad.
//!
//! Lo que se mide aqui no es el SQL: eso esta en el contrato del puerto,
//! contra la base de verdad. Lo que se mide es la **superficie**: que la
//! cuenta salga del principal verificado, que Cedar acote lo que se ve, y
//! que la vista de detalle no confunda "nadie ha dicho que sea bueno" con
//! "es bueno".
//!
//! Las dos cuentas son el caso duro: la cuenta ajena tiene un asset con
//! el mismo nombre y la misma ruta, y la busqueda de una cuenta no puede
//! devolver ni el nombre ni el total de la otra.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustGrant,
    TrustLedger, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use tower::ServiceExt;

/// La primera cuenta.
const ACCOUNT: &str = "acme";
/// La segunda cuenta.
const OTHER: &str = "other";
/// El repositorio que la primera cuenta puede leer.
const READABLE: &str = "libs";
/// El repositorio que la primera cuenta **no** puede leer.
const UNREADABLE: &str = "private";
/// El repositorio de la segunda cuenta.
const THEIRS: &str = "secrets";

/// El verificador de credenciales de la prueba.
///
/// El token es el nombre de la persona: no hay un secreto aqui, y la
/// cuenta sale del token y no de una cabecera.
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
            "alice" => (ACCOUNT, "alice"),
            "carol" => (OTHER, "carol"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// La politica de la prueba.
///
/// Alice puede leer dos repositorios de su cuenta y el tercero no: lo que
/// Cedar no concede, la busqueda no lo nombra ni lo cuenta. Carol solo
/// puede leer el suyo.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/libs"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:acme:repository/apps"
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::other:User/carol",
        action == Artifacts::Action::"ReadRepository",
        resource == Artifacts::Repository::"hrn:hodei:artifacts:eu:other:repository/secrets"
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

/// Un repositorio del catalogo.
fn descriptor(account: &str, key: &str, format: RepositoryFormat) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// La aplicacion con un catalogo de verdad detras.
async fn app() -> (axum::Router, Arc<SurrealCatalogStore>, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = Surreal::new::<RocksDb>(tmp.path().join("db"))
        .await
        .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    let store = Arc::new(SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(AllowAllAuthorizer),
    ));
    store.initialize().await.expect("initialize");

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.rocksdb.path = tmp
        .path()
        .join("schema.rocksdb")
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
    let state = state
        .with_authorization(cedar())
        .with_repository_auth(
            Arc::new(TestAuthenticator),
            Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
        )
        .with_catalog(store.clone(), store.clone())
        .with_catalog_search(store.clone())
        .with_trust_ledger(store.clone());

    (build_router(state, &config), store, tmp)
}

/// Publica un asset y devuelve su digest.
async fn publish(
    store: &SurrealCatalogStore,
    account: &str,
    repository: &str,
    package: &str,
    version: &str,
    path: &str,
    subject: Option<ContentDigest>,
) -> ContentDigest {
    let digest = ContentDigest::of_bytes(path.as_bytes());
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: path.to_string(),
                coordinates: PackageCoordinates {
                    package_name: package.to_string(),
                    package_version: version.to_string(),
                },
                digest: digest.clone(),
                size_bytes: path.len() as u64,
                tags: Vec::new(),
                actor: format!("hrn:hodei:iam::{account}:User/alice"),
                correlation_id: "corr-hsp113".to_string(),
                overwrite: false,
                precondition: None,
                subject,
            },
        )
        .await
        .expect("published");
    digest
}

/// Un catalogo con dos cuentas, un repositorio que alice no puede leer y
/// un asset con el mismo nombre en las dos.
async fn a_catalog(store: &SurrealCatalogStore) {
    for (account, key, format) in [
        (ACCOUNT, READABLE, RepositoryFormat::PyPi),
        (ACCOUNT, "apps", RepositoryFormat::Npm),
        (ACCOUNT, UNREADABLE, RepositoryFormat::Raw),
        (OTHER, THEIRS, RepositoryFormat::PyPi),
    ] {
        store
            .create_repository(account, descriptor(account, key, format))
            .await
            .expect("the repository is created");
    }

    publish(
        store,
        ACCOUNT,
        READABLE,
        "labpkg",
        "1.0",
        "labpkg-1.0.whl",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        READABLE,
        "labpkg",
        "1.1",
        "labpkg-1.1.whl",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        "apps",
        "left-pad",
        "1.0.0",
        "left-pad-1.0.0.tgz",
        None,
    )
    .await;
    publish(
        store,
        ACCOUNT,
        UNREADABLE,
        "secretpkg",
        "1.0",
        "secretpkg-1.0.tar.gz",
        None,
    )
    .await;
    // La otra cuenta tiene un asset con el mismo nombre y la misma ruta.
    publish(
        store,
        OTHER,
        THEIRS,
        "labpkg",
        "1.0",
        "labpkg-1.0.whl",
        None,
    )
    .await;
}

/// Una peticion de busqueda.
fn search_request(token: &str, query: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/v1/search{query}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a request")
}

/// Una peticion del detalle de un paquete.
fn package_request(token: &str, repository: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/v1/packages/{repository}/{path}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a request")
}

/// El cuerpo de la respuesta, como JSON.
async fn the_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("the body is JSON")
}

/// La busqueda de una cuenta encuentra lo suyo y no lo ajeno.
#[tokio::test]
async fn la_busqueda_encuentra_lo_suyo_y_no_lo_ajeno() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", "?package=labpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["total"], 2, "the two versions of the package");
    let results = body["results"].as_array().expect("the results");
    assert_eq!(results.len(), 2);
    for result in results {
        assert_eq!(result["repository"], READABLE);
        assert_eq!(result["package_name"], "labpkg");
    }
}

/// La cuenta ajena no sale ni en los nombres ni en los totales.
#[tokio::test]
async fn la_cuenta_ajena_no_sale_en_los_nombres_ni_en_los_totales() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("carol", "?package=labpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    // El total es el de su cuenta: el mismo nombre en la otra cuenta no
    // suma, porque un total que sumara lo ajeno diria que hay algo donde
    // no hay nada que ver.
    assert_eq!(body["total"], 1, "the count of the other account");
    let results = body["results"].as_array().expect("the results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["repository"], THEIRS);

    // Y el cuerpo no nombra nada de la cuenta ajena.
    let text = serde_json::to_string(&body).expect("the body");
    assert!(!text.contains(READABLE), "the other repository is named");
    assert!(!text.contains("apps"), "the other repository is named");
    assert!(!text.contains(UNREADABLE), "the other repository is named");
}

/// El repositorio que Cedar no concede no sale ni cuenta.
#[tokio::test]
async fn un_repositorio_sin_permiso_no_sale_ni_cuenta() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(search_request("alice", "?text=secretpkg"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["total"], 0, "the unreadable repository is counted");
    assert!(body["results"].as_array().expect("the results").is_empty());
}

/// Una busqueda sin credencial no es una busqueda.
#[tokio::test]
async fn la_busqueda_necesita_una_credencial() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/search?package=labpkg")
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Una consulta que no dice lo que dice es un 400.
#[tokio::test]
async fn una_consulta_que_no_dice_lo_que_dice_es_un_400() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    for query in [
        "?pakcage=labpkg",
        "?package=labpkg&package=left-pad",
        "?limit=0",
        "?limit=not-a-number",
        "?digest=sha256:zz",
        "?attribute=colour=red",
    ] {
        let response = app
            .clone()
            .oneshot(search_request("alice", query))
            .await
            .expect("the request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
}

/// La vista de detalle dice donde estan los bytes, que se publico, de
/// quien cuelga, que dice la evidencia y que se ha decidido.
#[tokio::test]
async fn la_vista_de_detalle_enlaza_el_blob_los_metadatos_y_la_decision() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["repository"], READABLE);
    assert_eq!(body["asset_path"], "labpkg-1.0.whl");
    assert_eq!(body["package_name"], "labpkg");
    assert_eq!(body["package_version"], "1.0");

    // El blob: donde estan los bytes.
    assert_eq!(body["blob"]["scope"], "global");
    assert!(body["blob"]["digest"]["value"].is_string());
    assert_eq!(body["blob"]["size_bytes"], body["size_bytes"]);

    // Los metadatos originales: la version tal y como se publico.
    assert_eq!(body["published"]["package_name"], "labpkg");
    assert_eq!(body["published"]["package_version"], "1.0");
    assert!(body["published"]["published_at"].is_string());

    // La linea: de quien cuelga.
    assert!(body["lineage"]["subject"].is_null());
    assert!(
        body["lineage"]["referrers"]
            .as_array()
            .expect("the referrers")
            .is_empty()
    );

    // La capsula: lo que dice la evidencia.
    assert!(
        !body["capsule"]["content_hash"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );

    // Y la decision: sin concesion, en cuarentena. No hay findings, pero
    // eso no es una confianza: es un silencio.
    assert_eq!(body["trust"]["state"], "quarantined");
    assert!(body["trust"]["decision"].is_null());
}

/// Un referrer cuelga del asset y la vista lo dice.
#[tokio::test]
async fn la_vista_dice_de_quien_cuelga_el_contenido() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;
    let subject = ContentDigest::of_bytes(b"labpkg-1.0.whl");
    publish(
        &store,
        ACCOUNT,
        READABLE,
        "sbom",
        "1.0",
        "sbom-1.0.spdx.json",
        Some(subject.clone()),
    )
    .await;

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    let referrers = body["lineage"]["referrers"]
        .as_array()
        .expect("the referrers");
    assert_eq!(referrers.len(), 1, "the referrer is not there");
    assert_eq!(referrers[0]["asset_path"], "sbom-1.0.spdx.json");
    assert_eq!(referrers[0]["subject"]["value"], subject.value);
}

/// La concesion es lo que dice que el contenido es de fiar, y lo dice con
/// su decision.
#[tokio::test]
async fn la_concesion_es_la_decision_que_dice_que_es_de_fiar() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;
    let digest = ContentDigest::of_bytes(b"labpkg-1.0.whl");
    store
        .grant(&[TrustGrant {
            account: ACCOUNT.to_string(),
            repository: READABLE.to_string(),
            digest: digest.clone(),
            decision_id: "dec-hsp113".to_string(),
            actor: "hrn:hodei:iam::acme:User/alice".to_string(),
            granted_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
        }])
        .await
        .expect("the grant");

    let response = app
        .clone()
        .oneshot(package_request("alice", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;

    assert_eq!(body["trust"]["state"], "trusted");
    assert_eq!(body["trust"]["decision"]["decision_id"], "dec-hsp113");

    // Y la busqueda lo dice tambien, sin reescribir nada.
    let response = app
        .oneshot(search_request("alice", "?package=labpkg"))
        .await
        .expect("the request");
    let body = the_body(response).await;
    let states: Vec<&str> = body["results"]
        .as_array()
        .expect("the results")
        .iter()
        .map(|result| result["trust_state"].as_str().unwrap_or(""))
        .collect();
    assert!(
        states.contains(&"trusted"),
        "the promotion is not in the search: {states:?}"
    );
}

/// El detalle de un asset de otra cuenta no existe.
#[tokio::test]
async fn el_detalle_de_un_asset_de_otra_cuenta_no_existe() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    // El repositorio no es de la cuenta de carol: el mismo 404 que si no
    // existiera, porque responder distinto diria que existe.
    let response = app
        .clone()
        .oneshot(package_request("carol", READABLE, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Y el suyo, que existe, se ve.
    let response = app
        .oneshot(package_request("carol", THEIRS, "labpkg-1.0.whl"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
}

/// El detalle de un asset de un repositorio sin permiso no se ve.
#[tokio::test]
async fn el_detalle_de_un_repositorio_sin_permiso_no_se_ve() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let response = app
        .clone()
        .oneshot(package_request("alice", UNREADABLE, "secretpkg-1.0.tar.gz"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// El detalle necesita una credencial.
#[tokio::test]
async fn el_detalle_necesita_una_credencial() {
    let (app, store, _tmp) = app().await;
    a_catalog(&store).await;

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/packages/{READABLE}/labpkg-1.0.whl"))
        .body(Body::empty())
        .expect("a request");
    let response = app.oneshot(request).await.expect("the request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
