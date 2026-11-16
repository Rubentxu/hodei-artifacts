//! El contrato de la busqueda contra SurrealDB de verdad.
//!
//! Lo que se comprueba: que la busqueda encuentra lo publicado por sus
//! coordenadas, que esta acotada a la cuenta, que no cuenta ni nombra lo
//! que el principal no puede leer, que la pagina es estable y ordenada, y
//! que el canal que se lee es el de la relacion y no una copia.
//!
//! Y una cosa mas, que no es del catalogo sino del motor: la misma
//! consulta repetida tiene que devolver lo mismo. En SurrealDB 2.7 con
//! RocksDB, un filtro `IN` sobre un array en el filtro exterior devuelve
//! a veces cero filas, y por eso las consultas de la busqueda no lo usan.

use std::sync::Arc;

use hodei_catalog::search::{RepositoryAttribute, SearchQuery};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogSearch, CatalogWriter, DigestScope,
    PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, TrustGrant, TrustLedger, TrustState, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

async fn open(path: &std::path::Path) -> Surreal<Db> {
    let mut last = String::new();
    for _ in 0..20 {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    panic!("the database could not be opened: {last}");
}

async fn fresh() -> (tempfile::TempDir, SurrealCatalogStore) {
    let tmp = tempfile::tempdir().expect("tmp");
    let db = open(&tmp.path().join("db")).await;
    let store = SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("initialize");
    (tmp, store)
}

fn descriptor(
    account: &str,
    key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode,
        config,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

async fn publish(
    store: &SurrealCatalogStore,
    account: &str,
    repository: &str,
    package: &str,
    version: &str,
    path: &str,
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
                actor: "hrn:hodei:iam:acme:User/alice".to_string(),
                correlation_id: "corr-hsp113".to_string(),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("published");
    digest
}

/// Una cuenta con dos repositorios y tres assets.
async fn a_catalog() -> (tempfile::TempDir, SurrealCatalogStore) {
    let (tmp, store) = fresh().await;
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "libs",
                RepositoryFormat::PyPi,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "apps",
                RepositoryFormat::Npm,
                RepositoryMode::Hosted,
                serde_json::json!({ "trust": { "serve": "trusted" } }),
            ),
        )
        .await
        .expect("the repository is created");
    store
        .create_repository(
            "other",
            descriptor(
                "other",
                "secrets",
                RepositoryFormat::PyPi,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");

    publish(
        &store,
        "acme",
        "libs",
        "labpkg",
        "1.0",
        "labpkg-1.0-py3-none-any.whl",
    )
    .await;
    publish(
        &store,
        "acme",
        "libs",
        "labpkg",
        "1.1",
        "labpkg-1.1-py3-none-any.whl",
    )
    .await;
    publish(
        &store,
        "acme",
        "apps",
        "left-pad",
        "1.0.0",
        "left-pad-1.0.0.tgz",
    )
    .await;
    publish(
        &store,
        "other",
        "secrets",
        "labpkg",
        "1.0",
        "labpkg-1.0-py3-none-any.whl",
    )
    .await;
    (tmp, store)
}

#[tokio::test]
async fn the_search_finds_an_asset_by_its_coordinates() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");

    assert_eq!(page.total, 2, "the two versions of the package");
    assert_eq!(page.results.len(), 2);
    let first = &page.results[0];
    assert_eq!(first.repository, "libs");
    assert_eq!(first.format, RepositoryFormat::PyPi);
    assert_eq!(first.package_name, "labpkg");
    assert_eq!(first.asset_path, "labpkg-1.0-py3-none-any.whl");
    assert_eq!(first.size_bytes, "labpkg-1.0-py3-none-any.whl".len() as u64);
    assert!(first.digest.is_well_formed());
}

#[tokio::test]
async fn the_search_is_scoped_to_the_account() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");

    // La cuenta ajena tiene un asset con el mismo nombre y la misma ruta.
    assert_eq!(page.total, 2, "the account of the other is not counted");
    assert!(
        page.results.iter().all(|hit| hit.repository != "secrets"),
        "the repository of the other account does not appear"
    );
}

#[tokio::test]
async fn the_search_does_not_see_a_repository_the_principal_cannot_read() {
    let (_tmp, store) = a_catalog().await;
    let all = store.repositories("acme").await.expect("the repositories");
    // El principal solo puede leer uno de los dos repositorios.
    let readable: Vec<_> = all
        .iter()
        .filter(|repository| repository.repo_key == "libs")
        .cloned()
        .collect();

    let page = store
        .search(
            "acme",
            &readable,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");

    assert_eq!(page.total, 2, "only what the principal can read is counted");
    assert!(
        page.results.iter().all(|hit| hit.repository == "libs"),
        "the unreadable repository does not appear"
    );

    // Y el total tampoco lo cuenta: un total que contara lo que no se
    // puede leer diria que hay algo donde no hay nada que ver.
    let everything = store
        .search(
            "acme",
            &all,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");
    assert_eq!(
        everything.total, 2,
        "the count is the same in both accounts"
    );
}

#[tokio::test]
async fn a_principal_that_cannot_read_anything_sees_nothing() {
    let (_tmp, store) = a_catalog().await;

    let page = store
        .search("acme", &[], &SearchQuery::new().with_package_name("labpkg"))
        .await
        .expect("the search");

    assert_eq!(page.total, 0);
    assert!(page.results.is_empty());
}

#[tokio::test]
async fn the_page_is_ordered_and_does_not_repeat_or_lose() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let first = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_limit(2).with_offset(0),
        )
        .await
        .expect("the search");
    let second = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_limit(2).with_offset(2),
        )
        .await
        .expect("the search");

    assert_eq!(first.total, 3, "the three assets of the account");
    assert_eq!(first.results.len(), 2);
    assert_eq!(second.results.len(), 1);

    let paths: Vec<&str> = first
        .results
        .iter()
        .chain(second.results.iter())
        .map(|hit| hit.asset_path.as_str())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(paths.len(), sorted.len(), "no asset is repeated");
    assert_eq!(
        paths,
        vec![
            "left-pad-1.0.0.tgz",
            "labpkg-1.0-py3-none-any.whl",
            "labpkg-1.1-py3-none-any.whl",
        ],
        "the order is (repository, asset_path)"
    );
}

#[tokio::test]
async fn the_same_search_returns_the_same_page() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");
    let query = SearchQuery::new().with_package_name("labpkg");

    let mut pages = Vec::new();
    for _ in 0..25 {
        pages.push(
            store
                .search("acme", &repositories, &query)
                .await
                .expect("the search"),
        );
    }
    // El motor devuelve a veces cero filas con algunas formas de filtro:
    // una busqueda que a veces no ve nada no es una busqueda.
    for page in &pages {
        assert_eq!(page.total, 2, "the total is always the same");
        assert_eq!(page.results.len(), 2, "the page is always the same");
    }
}

#[tokio::test]
async fn the_search_filters_by_repository_version_and_digest() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let by_repository = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_repository("apps"),
        )
        .await
        .expect("the search");
    assert_eq!(by_repository.total, 1);
    assert_eq!(by_repository.results[0].repository, "apps");

    let by_version = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new()
                .with_package_name("labpkg")
                .with_package_version("1.1"),
        )
        .await
        .expect("the search");
    assert_eq!(by_version.total, 1);
    assert_eq!(by_version.results[0].package_version, "1.1");

    let digest = ContentDigest::of_bytes(b"labpkg-1.0-py3-none-any.whl");
    let by_digest = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_digest(digest),
        )
        .await
        .expect("the search");
    assert_eq!(by_digest.total, 1);
    assert_eq!(
        by_digest.results[0].asset_path,
        "labpkg-1.0-py3-none-any.whl"
    );
}

#[tokio::test]
async fn the_search_filters_by_the_attributes_of_the_repository() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let by_format = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_attribute(RepositoryAttribute::Format(RepositoryFormat::PyPi)),
        )
        .await
        .expect("the search");
    assert_eq!(by_format.total, 2, "the two assets of the PyPI repository");

    let by_mode = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_attribute(RepositoryAttribute::Mode(RepositoryMode::Hosted)),
        )
        .await
        .expect("the search");
    assert_eq!(
        by_mode.total, 3,
        "every repository of the account is hosted"
    );

    let by_channel = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_attribute(RepositoryAttribute::Channel(
                hodei_catalog::ServeChannel::Open,
            )),
        )
        .await
        .expect("the search");
    assert_eq!(
        by_channel.total, 2,
        "the repository with the trusted channel is not open"
    );

    let trusted = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_attribute(RepositoryAttribute::Channel(
                hodei_catalog::ServeChannel::TrustedOnly,
            )),
        )
        .await
        .expect("the search");
    assert_eq!(
        trusted.total, 1,
        "only the repository with the trusted channel"
    );
}

#[tokio::test]
async fn the_search_filters_by_a_tag() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    // Una etiqueta que no existe no es una busqueda sin resultados.
    let missing = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_tag("nothing"),
        )
        .await
        .expect("the search");
    assert_eq!(missing.total, 0);
    assert!(missing.results.is_empty());

    // Y una que existe acota por sus coordenadas.
    store
        .move_tag(
            "acme",
            "libs",
            "latest",
            PackageCoordinates {
                package_name: "labpkg".to_string(),
                package_version: "1.1".to_string(),
            },
        )
        .await
        .expect("the tag moves");

    let tagged = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_tag("latest"),
        )
        .await
        .expect("the search");
    assert_eq!(tagged.total, 1, "the tag points at one version");
    assert_eq!(tagged.results[0].package_version, "1.1");
}

#[tokio::test]
async fn the_search_finds_by_text() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search("acme", &repositories, &SearchQuery::new().with_text("LAB"))
        .await
        .expect("the search");
    assert_eq!(page.total, 2, "the text is read without case");
}

#[tokio::test]
async fn the_search_reads_the_channel_of_the_asset() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");
    // Lo publicado entra en cuarentena: nadie lo ha promovido.
    assert!(
        page.results
            .iter()
            .all(|hit| hit.trust_state == TrustState::Quarantined),
        "what nobody promoted is quarantined"
    );
}

#[tokio::test]
async fn la_concesion_marca_el_contenido_y_la_revocacion_lo_quita() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");
    let digest = ContentDigest::of_bytes(b"labpkg-1.0-py3-none-any.whl");
    let query = SearchQuery::new().with_digest(digest.clone());

    // Sin concesion, el contenido esta en cuarentena: no es que nadie lo
    // haya mirado y no le haya pasado, es que nadie lo ha concedido.
    let page = store
        .search("acme", &repositories, &query)
        .await
        .expect("the search");
    assert_eq!(page.results[0].trust_state, TrustState::Quarantined);

    // Concedido, el mismo contenido es de fiar.
    store
        .grant(&[TrustGrant {
            account: "acme".to_string(),
            repository: "libs".to_string(),
            digest: digest.clone(),
            decision_id: "dec-hsp113".to_string(),
            actor: "hrn:hodei:iam:acme:User/alice".to_string(),
            granted_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
        }])
        .await
        .expect("the grant");
    let page = store
        .search("acme", &repositories, &query)
        .await
        .expect("the search");
    assert_eq!(page.results[0].trust_state, TrustState::Trusted);

    // Y en una pagina con mas de un asset, solo se concede el que lo
    // esta: la concesion de uno no concede el otro.
    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");
    let trusted: Vec<&str> = page
        .results
        .iter()
        .filter(|hit| hit.trust_state == TrustState::Trusted)
        .map(|hit| hit.asset_path.as_str())
        .collect();
    assert_eq!(
        trusted,
        vec!["labpkg-1.0-py3-none-any.whl"],
        "the concession of one asset is not the concession of the other"
    );

    // Y revocado, vuelve a la cuarentena en la peticion siguiente: no hay
    // indice que reescribir.
    store
        .revoke_digest(
            "acme",
            "libs",
            &digest,
            "hrn:hodei:iam:acme:User/alice",
            "an operator withdrew it",
            chrono::Utc::now(),
        )
        .await
        .expect("the revocation");
    let page = store
        .search("acme", &repositories, &query)
        .await
        .expect("the search");
    assert_eq!(page.results[0].trust_state, TrustState::Quarantined);
}

#[tokio::test]
async fn a_filter_that_matches_nothing_says_zero() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_name("nothing"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, 0);
    assert!(page.results.is_empty());
}

#[tokio::test]
async fn the_search_refuses_a_query_that_does_not_say_anything() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let error = store
        .search("acme", &repositories, &SearchQuery::new().with_limit(0))
        .await
        .expect_err("a page of zero is not a page");
    assert!(matches!(error, CatalogError::InvalidRequest { .. }));
}
