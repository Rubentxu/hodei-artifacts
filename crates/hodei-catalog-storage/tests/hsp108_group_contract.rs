//! El contrato de los repositorios de grupo contra SurrealDB de verdad.
//!
//! Lo que se comprueba: un grupo solo se guarda con miembros que existen,
//! del mismo formato y sin ciclos; la reconfiguracion cumple lo mismo; un
//! repositorio que no es grupo no puede declarar `group`; y un grupo no
//! guarda contenido propio (ni publicaciones ni etiquetas).

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
    key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    settings: serde_json::Value,
) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = settings;
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", "acme", key),
        account: "acme".to_string(),
        repo_key: key.to_string(),
        format,
        mode,
        config,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn members(keys: &[&str]) -> serde_json::Value {
    serde_json::json!({ "group": { "members": keys } })
}

async fn hosted(store: &SurrealCatalogStore, key: &str, format: RepositoryFormat) {
    store
        .create_repository(
            "acme",
            descriptor(key, format, RepositoryMode::Hosted, serde_json::json!({})),
        )
        .await
        .expect("a hosted repository");
}

async fn group(
    store: &SurrealCatalogStore,
    key: &str,
    keys: &[&str],
) -> Result<RepositoryDescriptor, CatalogError> {
    store
        .create_repository(
            "acme",
            descriptor(
                key,
                RepositoryFormat::Raw,
                RepositoryMode::Group,
                members(keys),
            ),
        )
        .await
}

#[tokio::test]
async fn a_group_is_created_with_members_that_exist() {
    let (_tmp, store) = fresh().await;
    hosted(&store, "internal", RepositoryFormat::Raw).await;
    hosted(&store, "extra", RepositoryFormat::Raw).await;

    let created = group(&store, "all", &["internal", "extra"])
        .await
        .expect("a valid group");
    assert_eq!(created.mode, RepositoryMode::Group);
    assert_eq!(
        created.config.config_settings,
        members(&["internal", "extra"])
    );
}

#[tokio::test]
async fn a_group_without_valid_members_is_refused() {
    let (_tmp, store) = fresh().await;
    hosted(&store, "internal", RepositoryFormat::Raw).await;
    hosted(&store, "images", RepositoryFormat::Oci).await;

    for (case, outcome) in [
        ("no members declared", {
            store
                .create_repository(
                    "acme",
                    descriptor(
                        "g1",
                        RepositoryFormat::Raw,
                        RepositoryMode::Group,
                        serde_json::json!({}),
                    ),
                )
                .await
        }),
        ("empty list", group(&store, "g2", &[]).await),
        (
            "member that does not exist",
            group(&store, "g3", &["ghost"]).await,
        ),
        (
            "member of another format",
            group(&store, "g4", &["images"]).await,
        ),
        ("itself", group(&store, "g5", &["g5"]).await),
        (
            "repeated member",
            group(&store, "g6", &["internal", "internal"]).await,
        ),
    ] {
        assert!(
            matches!(outcome, Err(CatalogError::InvalidRequest { .. })),
            "{case}: {outcome:?}"
        );
    }
    // Y ninguno de los intentos dejo un repositorio a medias.
    for key in ["g1", "g2", "g3", "g4", "g5", "g6"] {
        assert!(matches!(
            store.repository("acme", key).await,
            Err(CatalogError::Missing { .. })
        ));
    }
}

#[tokio::test]
async fn a_member_of_another_account_does_not_exist_for_the_group() {
    let (_tmp, store) = fresh().await;
    store
        .create_repository(
            "other",
            RepositoryDescriptor {
                account: "other".to_string(),
                hrn: repository_hrn("hodei", "artifacts", "eu", "other", "theirs"),
                ..descriptor(
                    "theirs",
                    RepositoryFormat::Raw,
                    RepositoryMode::Hosted,
                    serde_json::json!({}),
                )
            },
        )
        .await
        .expect("their repository");
    let outcome = group(&store, "all", &["theirs"]).await;
    assert!(matches!(outcome, Err(CatalogError::InvalidRequest { .. })));
}

#[tokio::test]
async fn reconfiguring_a_group_cannot_close_a_cycle() {
    let (_tmp, store) = fresh().await;
    hosted(&store, "internal", RepositoryFormat::Raw).await;
    group(&store, "inner", &["internal"]).await.expect("inner");
    group(&store, "outer", &["inner"]).await.expect("outer");

    // inner -> outer -> inner
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = members(&["internal", "outer"]);
    let outcome = store.update_config("acme", "inner", config).await;
    assert!(
        matches!(outcome, Err(CatalogError::InvalidRequest { .. })),
        "{outcome:?}"
    );

    // Y la configuracion anterior sigue siendo la vigente.
    let kept = store.repository("acme", "inner").await.expect("inner");
    assert_eq!(kept.config.version, 1);
    assert_eq!(kept.config.config_settings, members(&["internal"]));

    // Un cambio valido si sube la version.
    hosted(&store, "second", RepositoryFormat::Raw).await;
    let mut config = RepositoryConfig::new(DigestScope::Global);
    config.config_settings = members(&["second", "internal"]);
    let updated = store
        .update_config("acme", "inner", config)
        .await
        .expect("a valid change");
    assert_eq!(updated.config.version, 2);
}

#[tokio::test]
async fn only_a_group_declares_members_and_only_a_proxy_declares_an_upstream() {
    let (_tmp, store) = fresh().await;
    let outcome = store
        .create_repository(
            "acme",
            descriptor(
                "plain",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                members(&["x"]),
            ),
        )
        .await;
    assert!(matches!(outcome, Err(CatalogError::InvalidRequest { .. })));

    let outcome = store
        .create_repository(
            "acme",
            descriptor(
                "g",
                RepositoryFormat::Raw,
                RepositoryMode::Group,
                serde_json::json!({
                    "group": { "members": ["x"] },
                    "proxy": { "upstream": { "url": "http://127.0.0.1:1" } }
                }),
            ),
        )
        .await;
    assert!(matches!(outcome, Err(CatalogError::InvalidRequest { .. })));
}

#[tokio::test]
async fn a_group_stores_no_content_of_its_own() {
    let (_tmp, store) = fresh().await;
    hosted(&store, "internal", RepositoryFormat::Raw).await;
    group(&store, "all", &["internal"]).await.expect("group");

    let digest = ContentDigest::of_bytes(b"x");
    let publication = Publication {
        repository: "all".to_string(),
        asset_path: "a.bin".to_string(),
        coordinates: PackageCoordinates {
            package_name: "a.bin".to_string(),
            package_version: "1".to_string(),
        },
        digest,
        size_bytes: 1,
        tags: Vec::new(),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        correlation_id: "corr-hsp108".to_string(),
        overwrite: false,
        precondition: None,
        subject: None,
    };
    let outcome = store.publish("acme", publication).await;
    assert!(
        matches!(outcome, Err(CatalogError::ModeNotImplemented { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_group_cannot_declare_a_trust_policy_that_would_never_run() {
    let (_tmp, store) = fresh().await;
    hosted(&store, "internal", RepositoryFormat::Raw).await;
    let outcome = store
        .create_repository(
            "acme",
            descriptor(
                "all",
                RepositoryFormat::Raw,
                RepositoryMode::Group,
                serde_json::json!({
                    "group": { "members": ["internal"] },
                    "trust": { "serve": "trusted" },
                }),
            ),
        )
        .await;
    assert!(
        matches!(outcome, Err(CatalogError::InvalidRequest { .. })),
        "the trust policy belongs to the member that serves: {outcome:?}"
    );
}
