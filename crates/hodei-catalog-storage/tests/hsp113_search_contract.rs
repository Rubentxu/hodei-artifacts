//! The contract of the search against a real SurrealDB.
//!
//! What is checked: that the search finds what was published by its
//! coordinates, that it is bounded to the account, that it neither counts
//! nor names what the principal cannot read, that the page is stable and
//! ordered, and that the channel that is read is the one of the
//! relationship and not a copy.
//!
//! And one more thing, which is not of the catalog but of the engine: the
//! same query repeated has to return the same thing. In SurrealDB 2.7 with
//! RocksDB, an `IN` filter over an array in the outer filter sometimes
//! returns zero rows, and that is why the queries of the search do not
//! use it.

use std::sync::Arc;

use hodei_catalog::search::{MAX_COUNTED, RepositoryAttribute, SearchQuery};
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

/// An account with three repositories and three assets.
///
/// The third one is served on the `trusted` channel: what has no grant
/// there does not come out, and it is not counted either.
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
                "tools",
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
        "acme",
        "tools",
        "toolpkg",
        "1.0.0",
        "toolpkg-1.0.0.tgz",
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

/// The digest of an asset that was published with this path.
fn the_digest_of(path: &str) -> ContentDigest {
    ContentDigest::of_bytes(path.as_bytes())
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

    assert_eq!(page.total, Some(2), "the two versions of the package");
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

    // The other account has an asset with the same name and the same path.
    assert_eq!(
        page.total,
        Some(2),
        "the account of the other is not counted"
    );
    assert!(
        page.results.iter().all(|hit| hit.repository != "secrets"),
        "the repository of the other account does not appear"
    );
}

#[tokio::test]
async fn the_search_does_not_see_a_repository_the_principal_cannot_read() {
    let (_tmp, store) = a_catalog().await;
    let all = store.repositories("acme").await.expect("the repositories");
    // The principal can only read one of the two repositories.
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

    assert_eq!(
        page.total,
        Some(2),
        "only what the principal can read is counted"
    );
    assert!(
        page.results.iter().all(|hit| hit.repository == "libs"),
        "the unreadable repository does not appear"
    );

    // And the total does not count it either: a total that counted what
    // cannot be read would say there is something where there is nothing
    // to see.
    let everything = store
        .search(
            "acme",
            &all,
            &SearchQuery::new().with_package_name("labpkg"),
        )
        .await
        .expect("the search");
    assert_eq!(
        everything.total,
        Some(2),
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

    assert_eq!(page.total, Some(0));
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

    assert_eq!(first.total, Some(3), "the three assets of the account");
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
    // The engine sometimes returns zero rows with some filter shapes: a
    // search that sometimes sees nothing is not a search.
    for page in &pages {
        assert_eq!(page.total, Some(2), "the total is always the same");
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
    assert_eq!(by_repository.total, Some(1));
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
    assert_eq!(by_version.total, Some(1));
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
    assert_eq!(by_digest.total, Some(1));
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
    assert_eq!(
        by_format.total,
        Some(2),
        "the two assets of the PyPI repository"
    );

    let by_mode = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_attribute(RepositoryAttribute::Mode(RepositoryMode::Hosted)),
        )
        .await
        .expect("the search");
    assert_eq!(
        by_mode.total,
        Some(3),
        "the three assets that can be served"
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
        by_channel.total,
        Some(3),
        "the repositories with the open channel"
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
        trusted.total,
        Some(0),
        "the trusted repository has nothing granted"
    );
}

#[tokio::test]
async fn the_search_filters_by_a_tag() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    // A tag that does not exist is not a search without results.
    let missing = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_tag("nothing"),
        )
        .await
        .expect("the search");
    assert_eq!(missing.total, Some(0));
    assert!(missing.results.is_empty());

    // And one that exists narrows by its coordinates.
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
    assert_eq!(tagged.total, Some(1), "the tag points at one version");
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
    assert_eq!(page.total, Some(2), "the text is read without case");
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
    // What was published enters quarantine: nobody promoted it.
    assert!(
        page.results
            .iter()
            .all(|hit| hit.trust_state == TrustState::Quarantined),
        "what nobody promoted is quarantined"
    );
}

#[tokio::test]
async fn the_grant_marks_the_content_and_the_revocation_takes_it_away() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");
    let digest = ContentDigest::of_bytes(b"labpkg-1.0-py3-none-any.whl");
    let query = SearchQuery::new().with_digest(digest.clone());

    // Without a grant, the content is quarantined: it is not that nobody
    // looked at it and it passed, it is that nobody granted it.
    let page = store
        .search("acme", &repositories, &query)
        .await
        .expect("the search");
    assert_eq!(page.results[0].trust_state, TrustState::Quarantined);

    // Granted, the same content is trusted.
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

    // And in a page with more than one asset, only the one that has it
    // is granted: the grant of one does not grant the other.
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

    // And revoked, it goes back to quarantine in the next request: there
    // is no index to rewrite.
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
    assert_eq!(page.total, Some(0));
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

/// An asset seeded by hand, for the cases that need many.
#[derive(Debug, serde::Serialize)]
struct SeededAsset {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    subject: Option<String>,
    updated_at: surrealdb::sql::Datetime,
}

/// An account with one repository and `count` assets.
///
/// The assets share the package name when it is asked for: that is what
/// makes the filter of the package own the order of the rows.
async fn a_catalog_with(
    count: usize,
    package: Option<&str>,
) -> (tempfile::TempDir, SurrealCatalogStore) {
    let (tmp, store) = fresh().await;
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "bulk",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");
    let now = surrealdb::sql::Datetime::from(chrono::Utc::now());
    let rows: Vec<SeededAsset> = (0..count)
        .map(|index| SeededAsset {
            account: "acme".to_string(),
            repository: "bulk".to_string(),
            asset_path: format!("cargado-{index:05}.whl"),
            package_name: match package {
                Some(name) => name.to_string(),
                None => format!("cargado-{index:05}"),
            },
            package_version: format!("1.{index:05}"),
            digest_algorithm: "sha256".to_string(),
            digest: format!("{index:064x}"),
            size_bytes: 10,
            subject: None,
            updated_at: now.clone(),
        })
        .collect();
    store
        .db()
        .query("INSERT INTO catalog_asset $rows")
        .bind(("rows", rows))
        .await
        .expect("the assets are seeded")
        .check()
        .expect("the assets are seeded");
    (tmp, store)
}

#[tokio::test]
async fn the_page_says_whether_there_is_more() {
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
    assert!(first.more, "there are three assets and the page brings two");

    let last = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_limit(2).with_offset(2),
        )
        .await
        .expect("the search");
    assert!(!last.more, "the last page is the last page");
    assert_eq!(last.total, Some(3));
}

#[tokio::test]
async fn a_total_that_does_not_fit_is_not_counted() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    // More assets than are counted: the total does not fit, and a number
    // that was not counted is not said.
    let now = surrealdb::sql::Datetime::from(chrono::Utc::now());
    let rows: Vec<SeededAsset> = (0..=MAX_COUNTED)
        .map(|index| SeededAsset {
            account: "acme".to_string(),
            repository: "libs".to_string(),
            asset_path: format!("cargado-{index:05}.whl"),
            package_name: format!("cargado-{index:05}"),
            package_version: "1.0".to_string(),
            digest_algorithm: "sha256".to_string(),
            digest: format!("{index:064x}"),
            size_bytes: 10,
            subject: None,
            updated_at: now.clone(),
        })
        .collect();
    store
        .db()
        .query("INSERT INTO catalog_asset $rows")
        .bind(("rows", rows))
        .await
        .expect("the assets are seeded")
        .check()
        .expect("the assets are seeded");

    let page = store
        .search("acme", &repositories, &SearchQuery::new().with_limit(10))
        .await
        .expect("the search");

    assert_eq!(
        page.total, None,
        "the total does not fit and is not counted"
    );
    assert!(page.more, "there are more than the page brings");
    assert_eq!(page.results.len(), 10, "the page is still a page");
}

#[tokio::test]
async fn the_trusted_channel_only_shows_what_is_granted() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    // The tool was published and nobody promoted it: it does not come
    // out, and it is not counted either. A listing that showed it would
    // say that the content exists where the download answers that it
    // does not.
    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_repository("tools"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, Some(0), "what is not granted is not counted");
    assert!(page.results.is_empty(), "and it does not come out");

    // Granted, it comes out.
    let digest = the_digest_of("toolpkg-1.0.0.tgz");
    store
        .grant(&[TrustGrant {
            account: "acme".to_string(),
            repository: "tools".to_string(),
            digest: digest.clone(),
            decision_id: "dec-hsp113-tools".to_string(),
            actor: "hrn:hodei:iam:acme:User/alice".to_string(),
            granted_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
        }])
        .await
        .expect("the grant");
    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_repository("tools"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, Some(1), "the granted asset is counted");
    assert_eq!(page.results[0].asset_path, "toolpkg-1.0.0.tgz");
    assert_eq!(page.results[0].trust_state, TrustState::Trusted);

    // And revoked, it goes back to being invisible: there is no index to
    // rewrite.
    store
        .revoke_digest(
            "acme",
            "tools",
            &digest,
            "hrn:hodei:iam:acme:User/alice",
            "an operator withdrew it",
            chrono::Utc::now(),
        )
        .await
        .expect("the revocation");
    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_repository("tools"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, Some(0), "a revoked grant does not serve");
    assert!(page.results.is_empty());
}

#[tokio::test]
async fn the_total_is_exact_until_it_does_not_fit() {
    // The cap is the last total that is exact: 255 fits, 256 is the cap
    // and still fits, and 257 does not.
    for (count, expected) in [(255, Some(255)), (256, Some(256)), (257, None)] {
        let (_tmp, store) = a_catalog_with(count, None).await;
        let repositories = store.repositories("acme").await.expect("the repositories");
        let page = store
            .search("acme", &repositories, &SearchQuery::new().with_limit(10))
            .await
            .expect("the search");
        assert_eq!(page.total, expected, "{count} assets");
    }
}

#[tokio::test]
async fn the_cap_holds_when_the_page_starts_far_away() {
    let (_tmp, store) = a_catalog_with(300, None).await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    // The window is 301 rows and the repository is emptied by it: its
    // total is above the cap, and a total above the cap is not counted.
    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_limit(100).with_offset(200),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, None, "a total above the cap is not counted");
    assert_eq!(page.results.len(), 100, "the page is still a page");
    assert!(!page.more, "there is nothing behind the last page");
}

#[tokio::test]
async fn the_pages_do_not_repeat_or_lose_when_the_repository_is_bigger_than_the_window() {
    let (_tmp, store) = a_catalog_with(30, None).await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let mut seen: Vec<String> = Vec::new();
    for offset in (0..30).step_by(7) {
        let page = store
            .search(
                "acme",
                &repositories,
                &SearchQuery::new().with_limit(7).with_offset(offset),
            )
            .await
            .expect("the search");
        seen.extend(page.results.iter().map(|hit| hit.asset_path.clone()));
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(seen.len(), 30, "every asset comes out once");
    assert_eq!(unique.len(), 30, "and no asset comes out twice");
}

#[tokio::test]
async fn the_pages_do_not_repeat_or_lose_when_the_package_filter_owns_the_order() {
    // Every asset is of the same package, so the index of the package
    // serves the query and the rows come out in its order, which is not
    // the order of the path. The pages still have to be consecutive.
    let (_tmp, store) = a_catalog_with(30, Some("cargado")).await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let mut seen: Vec<String> = Vec::new();
    for offset in (0..30).step_by(7) {
        let page = store
            .search(
                "acme",
                &repositories,
                &SearchQuery::new()
                    .with_package_name("cargado")
                    .with_limit(7)
                    .with_offset(offset),
            )
            .await
            .expect("the search");
        seen.extend(page.results.iter().map(|hit| hit.asset_path.clone()));
    }
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(seen.len(), 30, "every asset comes out once");
    assert_eq!(unique.len(), 30, "and no asset comes out twice");
}

#[tokio::test]
async fn a_repository_without_assets_brings_nothing_and_does_not_break_the_total() {
    let (_tmp, store) = a_catalog().await;
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "empty",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                serde_json::json!({}),
            ),
        )
        .await
        .expect("the repository is created");
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search("acme", &repositories, &SearchQuery::new())
        .await
        .expect("the search");
    assert_eq!(page.total, Some(3), "the three assets of the account");
    assert_eq!(page.results.len(), 3);
}

#[tokio::test]
async fn a_text_that_matches_nothing_says_zero() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_text("no-existe-este-paquete"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, Some(0));
    assert!(page.results.is_empty());
}

#[tokio::test]
async fn a_page_of_the_maximum_size_comes_whole() {
    let (_tmp, store) = a_catalog_with(120, Some("cargado")).await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search("acme", &repositories, &SearchQuery::new().with_limit(100))
        .await
        .expect("the search");
    assert_eq!(page.results.len(), 100, "the biggest page comes whole");
    assert!(page.more, "and there is more behind it");
    assert_eq!(page.total, Some(120));
}

#[tokio::test]
async fn the_version_filter_works_without_the_package() {
    let (_tmp, store) = a_catalog().await;
    let repositories = store.repositories("acme").await.expect("the repositories");

    let page = store
        .search(
            "acme",
            &repositories,
            &SearchQuery::new().with_package_version("1.1"),
        )
        .await
        .expect("the search");
    assert_eq!(page.total, Some(1));
    assert_eq!(page.results[0].package_version, "1.1");
}
