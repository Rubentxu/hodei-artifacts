//! El contrato de Maven 2 contra SurrealDB de verdad.
//!
//! Lo que se comprueba: la lectura por prefijo que usan los metadatos
//! derivados (solo lo que cuelga del prefijo, ordenado, aislado por cuenta
//! y repositorio) y que los ajustes `maven` solo los declara un repositorio
//! Maven 2 y solo si se entienden.

use std::sync::Arc;

use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogReader, CatalogWriter, DigestScope,
    PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, repository_hrn,
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
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format,
        mode: RepositoryMode::Hosted,
        config,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

async fn publish(store: &SurrealCatalogStore, account: &str, repository: &str, path: &str) {
    let digest = ContentDigest::of_bytes(path.as_bytes());
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: path.to_string(),
                coordinates: PackageCoordinates {
                    package_name: path.to_string(),
                    package_version: digest.value.clone(),
                },
                digest,
                size_bytes: path.len() as u64,
                tags: Vec::new(),
                actor: "hrn:hodei:iam:acme:User/alice".to_string(),
                correlation_id: "corr-hsp109".to_string(),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("published");
}

#[tokio::test]
async fn assets_under_lists_only_what_hangs_from_the_prefix() {
    let (_tmp, store) = fresh().await;
    for account in ["acme", "other"] {
        for key in ["releases", "second"] {
            store
                .create_repository(
                    account,
                    descriptor(
                        account,
                        key,
                        RepositoryFormat::Maven2,
                        serde_json::json!({}),
                    ),
                )
                .await
                .expect("a repository");
        }
    }
    for path in [
        "org/acme/widget/1.0/widget-1.0.pom",
        "org/acme/widget/1.0/widget-1.0.jar",
        "org/acme/widget/2.0/widget-2.0.pom",
        "org/acme/widget-extra/1.0/widget-extra-1.0.pom",
        "org/acme/other/1.0/other-1.0.pom",
    ] {
        publish(&store, "acme", "releases", path).await;
    }
    publish(
        &store,
        "acme",
        "second",
        "org/acme/widget/9.9/widget-9.9.pom",
    )
    .await;
    publish(
        &store,
        "other",
        "releases",
        "org/acme/widget/7.7/widget-7.7.pom",
    )
    .await;

    let found = store
        .assets_under("acme", "releases", "org/acme/widget/")
        .await
        .expect("a listing");
    let paths: Vec<&str> = found.iter().map(|a| a.asset_path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "org/acme/widget/1.0/widget-1.0.jar",
            "org/acme/widget/1.0/widget-1.0.pom",
            "org/acme/widget/2.0/widget-2.0.pom",
        ],
        "the prefix ends in a slash: `widget-extra` is another artifact"
    );
    assert!(found.iter().all(|asset| asset.account == "acme"));

    let none = store
        .assets_under("acme", "releases", "org/nothing/")
        .await
        .expect("an empty listing");
    assert!(none.is_empty());
}

#[tokio::test]
async fn the_maven_settings_are_declared_only_by_maven_repositories_and_must_be_understood() {
    let (_tmp, store) = fresh().await;
    let valid = serde_json::json!({
        "maven": {"version_policy": "release", "release_redeploy": true}
    });
    store
        .create_repository(
            "acme",
            descriptor("acme", "mvn", RepositoryFormat::Maven2, valid.clone()),
        )
        .await
        .expect("valid settings");

    for (case, format, settings) in [
        (
            "another format",
            RepositoryFormat::Raw,
            serde_json::json!({"maven": {}}),
        ),
        (
            "unknown policy",
            RepositoryFormat::Maven2,
            serde_json::json!({"maven": {"version_policy": "weekly"}}),
        ),
        (
            "unknown key",
            RepositoryFormat::Maven2,
            serde_json::json!({"maven": {"surprise": true}}),
        ),
    ] {
        let outcome = store
            .create_repository("acme", descriptor("acme", "bad", format, settings))
            .await;
        assert!(
            matches!(outcome, Err(CatalogError::InvalidRequest { .. })),
            "{case}: {outcome:?}"
        );
    }

    // La reconfiguracion cumple lo mismo.
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = serde_json::json!({"maven": {"version_policy": "nope"}});
    let outcome = store.update_config("acme", "mvn", config).await;
    assert!(
        matches!(outcome, Err(CatalogError::InvalidRequest { .. })),
        "{outcome:?}"
    );
    let kept = store.repository("acme", "mvn").await.expect("kept");
    assert_eq!(kept.config.config_settings, valid);
}
