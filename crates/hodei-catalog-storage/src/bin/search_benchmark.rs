//! The measurement of the catalog search.
//!
//! The question it answers: **how much does searching cost** with a
//! hundred thousand assets, and whether the engine that is already there
//! is enough. "Enough" is a word that has to be measured, not one that is
//! assumed.
//!
//! The measurement is taken **by hand**: seeding a hundred thousand assets
//! takes minutes, and the pipeline cannot pay that on every push. The
//! numbers of the search section of the UAT come from here.
//!
//! ```
//! cargo run --release -p hodei-catalog-storage --bin search_benchmark
//! ```
//!
//! The size can be changed with `HODEI_SEARCH_ASSETS` (100000 by default)
//! and the directory of the database with `HODEI_SEARCH_DB`, to leave the
//! database standing and measure again.
//!
//! # What is measured
//!
//! - **The real searches**, as the adapter makes them: the walk, the
//!   repository, the package name, the version, the digest, the tag and
//!   the free text.
//! - **The price of the indexes**: the same searches with the indexes of
//!   the search removed. An index that is not noticed is an index that
//!   only costs, and an index that is noticed a lot is an index that has
//!   to be kept.
//!
//! # What is not measured
//!
//! The write path. The load is seeded with bulk inserts because what is
//! measured is the read: seeding a hundred thousand assets through the
//! publish path of the catalog (which writes version, asset, reference
//! and audit, and checks the byte store) would take hours and would
//! measure something else.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_catalog::search::SearchQuery;
use hodei_catalog::{
    AllowAllAuthorizer, CatalogSearch, CatalogWriter, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use serde::Serialize;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use surrealdb::sql::Datetime;

/// How many times each query is measured.
///
/// It is a small number on purpose: the searches that walk the catalog
/// cost seconds each, and a measurement that takes an hour is not a
/// measurement.
const ROUNDS: usize = 5;

/// How many times each query is measured when the indexes are gone.
///
/// Without the indexes every search walks the whole catalog, so two
/// rounds are enough: the first one warms the cache and the second one
/// is the measurement.
const ROUNDS_WITHOUT_INDEXES: usize = 2;

/// How many assets are seeded by default.
const DEFAULT_ASSETS: usize = 100_000;

/// How many repositories the load account has.
const REPOSITORIES: usize = 4;

/// The account of the load.
const ACCOUNT: &str = "acme";

/// How many versions each package has.
///
/// A package is four versions, and each version one asset: that is the
/// shape a real repository has, and the one that makes the search by
/// package return more than one row.
const VERSIONS_PER_PACKAGE: usize = 4;

/// The row of an asset, as the catalog writes it.
#[derive(Debug, Serialize)]
struct AssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    subject: Option<String>,
    updated_at: Datetime,
}

/// The row of a version, as the catalog writes it.
#[derive(Debug, Serialize)]
struct VersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    published_at: Datetime,
}

/// The row of a tag, as the catalog writes it.
#[derive(Debug, Serialize)]
struct TagRow {
    account: String,
    repository: String,
    tag_name: String,
    package_name: String,
    package_version: String,
    updated_at: Datetime,
}

/// The summary of some measurements.
#[derive(Debug, Clone)]
struct Measurement {
    /// The median.
    p50: Duration,
    /// The 95th percentile.
    p95: Duration,
    /// The slowest.
    max: Duration,
}

impl Measurement {
    /// The summary of a list of times.
    fn of(values: &mut [Duration]) -> Self {
        values.sort();
        let index = ((values.len() as f64 - 1.0) * 0.50).round() as usize;
        let p95 = ((values.len() as f64 - 1.0) * 0.95).round() as usize;
        Self {
            p50: values[index],
            p95: values[p95],
            max: values[values.len() - 1],
        }
    }

    /// The summary, in milliseconds.
    fn millis(&self) -> (f64, f64, f64) {
        (
            self.p50.as_secs_f64() * 1000.0,
            self.p95.as_secs_f64() * 1000.0,
            self.max.as_secs_f64() * 1000.0,
        )
    }
}

/// How many assets are seeded.
fn the_assets() -> usize {
    std::env::var("HODEI_SEARCH_ASSETS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_ASSETS)
}

/// The coordinates of an asset.
///
/// The package is spread over the versions, so that a package has
/// several versions and each of them has its asset.
fn the_coordinates(index: usize) -> (String, String) {
    let package = index / VERSIONS_PER_PACKAGE;
    let version = index % VERSIONS_PER_PACKAGE;
    (format!("pkg-{package}"), format!("1.{version}.0"))
}

/// The path of an asset.
fn the_path(index: usize) -> String {
    let (package, version) = the_coordinates(index);
    format!("{package}/{version}/{package}-{version}.tar.gz")
}

/// The digest of an asset, in hexadecimal.
fn the_digest(index: usize) -> String {
    format!("{index:064x}")
}

/// A repository of the load.
fn descriptor(repository: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", ACCOUNT, repository),
        account: ACCOUNT.to_string(),
        repo_key: repository.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// Seeds the repositories of the load.
async fn the_repositories(store: &SurrealCatalogStore) -> Vec<RepositoryDescriptor> {
    let mut repositories = Vec::with_capacity(REPOSITORIES);
    for index in 0..REPOSITORIES {
        let key = format!("repo-{index}");
        let created = store
            .create_repository(ACCOUNT, descriptor(&key))
            .await
            .expect("the repository is created");
        repositories.push(created);
    }
    repositories
}

/// Seeds the assets, the versions and the tags of the load.
///
/// The inserts are bulk on purpose: what is measured is the read.
async fn the_load(
    store: &SurrealCatalogStore,
    repositories: &[RepositoryDescriptor],
    assets: usize,
) -> Duration {
    let started = Instant::now();
    let now = Datetime::from(chrono::Utc::now());
    let batch = 2_000;
    let mut index = 0;
    while index < assets {
        let end = (index + batch).min(assets);
        let mut asset_rows = Vec::with_capacity(end - index);
        let mut version_rows = Vec::with_capacity(end - index);
        for current in index..end {
            let repository = repositories[current % repositories.len()].repo_key.clone();
            let (package, version) = the_coordinates(current);
            let digest = the_digest(current);
            asset_rows.push(AssetRow {
                account: ACCOUNT.to_string(),
                repository: repository.clone(),
                asset_path: the_path(current),
                package_name: package.clone(),
                package_version: version.clone(),
                digest_algorithm: "sha256".to_string(),
                digest: digest.clone(),
                size_bytes: 1024,
                subject: None,
                updated_at: now.clone(),
            });
            version_rows.push(VersionRow {
                account: ACCOUNT.to_string(),
                repository,
                package_name: package,
                package_version: version,
                digest_algorithm: "sha256".to_string(),
                digest,
                size_bytes: 1024,
                published_at: now.clone(),
            });
        }
        store
            .db()
            .query("INSERT INTO catalog_asset $rows")
            .bind(("rows", asset_rows))
            .await
            .expect("the assets are seeded")
            .check()
            .expect("the assets are seeded");
        store
            .db()
            .query("INSERT INTO catalog_version $rows")
            .bind(("rows", version_rows))
            .await
            .expect("the versions are seeded")
            .check()
            .expect("the versions are seeded");
        index = end;
    }
    // One tag per repository, which is how they are used: the tag
    // points at a version and the search by tag narrows by its
    // coordinates.
    let mut tag_rows = Vec::with_capacity(repositories.len());
    for (position, repository) in repositories.iter().enumerate() {
        let (package, version) = the_coordinates(position);
        tag_rows.push(TagRow {
            account: ACCOUNT.to_string(),
            repository: repository.repo_key.clone(),
            tag_name: "latest".to_string(),
            package_name: package,
            package_version: version,
            updated_at: now.clone(),
        });
    }
    store
        .db()
        .query("INSERT INTO catalog_tag $rows")
        .bind(("rows", tag_rows))
        .await
        .expect("the tags are seeded")
        .check()
        .expect("the tags are seeded");
    started.elapsed()
}

/// Measures a search.
async fn measure(
    store: &SurrealCatalogStore,
    repositories: &[RepositoryDescriptor],
    query: &SearchQuery,
    rounds: usize,
) -> (Measurement, usize) {
    let mut times = Vec::with_capacity(rounds);
    let mut found = 0usize;
    for _ in 0..rounds {
        let at = Instant::now();
        let page = store
            .search(ACCOUNT, repositories, query)
            .await
            .expect("the search");
        times.push(at.elapsed());
        found = page.results.len();
    }
    (Measurement::of(&mut times), found)
}

/// Removes the indexes of the search, to measure what they are worth.
async fn without_the_indexes(store: &SurrealCatalogStore) -> bool {
    let mut removed = true;
    for (index, table) in [
        ("catalog_asset_unique", "catalog_asset"),
        ("catalog_asset_package", "catalog_asset"),
        ("catalog_asset_digest", "catalog_asset"),
        ("catalog_asset_subject", "catalog_asset"),
    ] {
        let response = store
            .db()
            .query(format!("REMOVE INDEX {index} ON {table}"))
            .await;
        if let Err(error) = response {
            println!("  el indice {index} no se pudo quitar: {error}");
            removed = false;
        }
    }
    removed
}

/// Declares the indexes of the search again.
async fn with_the_indexes(store: &SurrealCatalogStore) {
    for statement in [
        "DEFINE INDEX IF NOT EXISTS catalog_asset_unique ON catalog_asset \
         FIELDS account, repository, asset_path UNIQUE",
        "DEFINE INDEX IF NOT EXISTS catalog_asset_package ON catalog_asset \
         FIELDS account, repository, package_name, package_version",
        "DEFINE INDEX IF NOT EXISTS catalog_asset_digest ON catalog_asset \
         FIELDS account, repository, digest_algorithm, digest",
        "DEFINE INDEX IF NOT EXISTS catalog_asset_subject ON catalog_asset \
         FIELDS account, repository, subject",
    ] {
        store
            .db()
            .query(statement)
            .await
            .expect("the index is declared")
            .check()
            .expect("the index is declared");
    }
}

#[tokio::main]
async fn main() {
    let assets = the_assets();

    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = match std::env::var("HODEI_SEARCH_DB") {
        Ok(path) => path,
        Err(_) => tmp.keep().to_string_lossy().into_owned(),
    };
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.clone())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei").use_db("artifacts").await.expect("the namespace");

    let store = SurrealCatalogStore::with_authorizer((*db).clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("the schema is applied");

    let repositories = the_repositories(&store).await;
    let seeding = the_load(&store, &repositories, assets).await;
    println!("=== La carga ===");
    println!("assets: {assets}");
    println!("repositorios: {REPOSITORIES}");
    println!("tiempo de siembra: {:.1} s", seeding.as_secs_f64());

    // The searches that measure the cost of the search.
    let package = the_coordinates(assets / 2).0;
    let version = the_coordinates(assets / 2).1;
    let by_repository = SearchQuery::new().with_repository("repo-0");
    let by_package = SearchQuery::new().with_package_name(package.clone());
    let by_version = SearchQuery::new()
        .with_package_name(package.clone())
        .with_package_version(version.clone());
    let by_digest = SearchQuery::new()
        .with_digest_text(&format!("sha256:{}", the_digest(assets / 2)))
        .expect("the digest is a digest");
    let by_tag = SearchQuery::new().with_tag("latest");
    // A text that appears in many paths, and another that appears in
    // none: the first stops early, the second walks the
    // whole catalog.
    let by_text = SearchQuery::new().with_text("pkg-1");
    let by_missing_text = SearchQuery::new().with_text("no-existe-este-paquete");
    let second_page = SearchQuery::new().with_limit(20).with_offset(100);

    // The label, the query, and whether the search walks the catalog: the
    // ones that do are measured with fewer rounds, because each of them
    // costs seconds.
    let queries: [(&str, &SearchQuery, bool); 9] = [
        ("el recorrido", &SearchQuery::new(), false),
        ("por el repositorio", &by_repository, false),
        ("por el nombre del paquete", &by_package, false),
        ("por la version", &by_version, false),
        ("por el digest", &by_digest, false),
        ("por la etiqueta", &by_tag, false),
        ("por el texto", &by_text, false),
        ("por un texto que no existe", &by_missing_text, true),
        ("la pagina del Offset 100", &second_page, false),
    ];

    println!("\n=== Con los indices ({ROUNDS} rondas cada una) ===");
    let mut with_indexes = Vec::new();
    for (label, query, walks) in &queries {
        let rounds = if *walks { 2 } else { ROUNDS };
        let (measurement, found) = measure(&store, &repositories, query, rounds).await;
        let (p50, p95, max) = measurement.millis();
        println!("{label}: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms ({found} resultados)");
        with_indexes.push(measurement);
    }

    let removed = without_the_indexes(&store).await;
    if removed {
        println!("\n=== Sin los indices de la busqueda ({ROUNDS_WITHOUT_INDEXES} rondas cada una) ===");
        for ((label, query, walks), with_index) in queries.iter().zip(with_indexes.iter()) {
            if *walks {
                // The text search does not use an index with or without
                // them: it is a walk either way.
                continue;
            }
            let (measurement, found) =
                measure(&store, &repositories, query, ROUNDS_WITHOUT_INDEXES).await;
            let (p50, p95, max) = measurement.millis();
            let (indexed, _, _) = with_index.millis();
            println!(
                "{label}: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms \
                (con el indice: {indexed:.2} ms, {found} resultados)"
            );
        }
        with_the_indexes(&store).await;
    }

    println!("\nLa base queda en {path}");
}
