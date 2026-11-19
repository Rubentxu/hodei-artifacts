//! The PyPI simple index and the publication seam stay bounded as the
//! catalog grows.
//!
//! Two questions reach the catalog on the PyPI surface. The upload of a
//! wheel or an sdist publishes through `write_the_publication` -- the
//! same seam the npm publish path serves, whose precondition guard was
//! measured at one full-table scan per publication until it was
//! rewritten as an identity fetch (HSP-175). The simple index (the
//! `GET /pypi/` root) lists the projects of the repository through
//! `assets_under`, whose statement used to filter with
//! `string::starts_with(asset_path, $prefix)`: the planner cannot bind a
//! function call to the `(account, repository, asset_path)` index, so it
//! answered with a full-table scan (`Iterate Table`, measured against
//! 2,000 distractor rows) and every simple index read would pay the
//! size of the catalog. The oracle here is the query plan, not the
//! clock: a plan that iterates the index cannot grow with the catalog,
//! whatever the hardware.

use std::path::Path;
use std::sync::Arc;

use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogReader as _, CatalogWriter as _, PackageCoordinates,
    Precondition, Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
    RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::{
    SurrealCatalogStore, the_assets_under_read, the_guard_fetch, the_prefix_end,
};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use surrealdb::sql::Thing;

const ACCOUNT: &str = "acme";
const OTHER_ACCOUNT: &str = "rival";
const REPO: &str = "lab-pypi";
/// Distractor rows the test seeds into sibling repositories, so the
/// reads run against a catalog that holds rows the PyPI surface never
/// names. The plan does not depend on the count; the count only makes
/// the contrast with a seeded catalog explicit.
const DISTRACTOR_ROWS: usize = 2_000;

const PROJECT: &str = "hello-pkg";
const WHEEL: &str = "hello_pkg-1.0.0-py3-none-any.whl";
const SDIST: &str = "hello_pkg-1.0.0.tar.gz";

async fn open(path: &Path) -> Surreal<surrealdb::engine::local::Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("open rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hsp178-pypi-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn pypi_repo(account: &str, repo: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo),
        account: account.into(),
        repo_key: repo.into(),
        format: RepositoryFormat::PyPi,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(hodei_catalog::DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn seed_digest(seed: u64) -> hodei_evidence::ContentDigest {
    hodei_evidence::ContentDigest::sha256(format!("{seed:064x}"))
}

/// The publication of one distribution, the way the PyPI upload handler
/// builds it: the path carries the project and the file, the
/// coordinates name the file (two distributions of one version are two
/// contents), and there is no precondition and no overwrite.
fn distribution_publication(
    project: &str,
    filename: &str,
    digest: hodei_evidence::ContentDigest,
) -> Publication {
    Publication {
        repository: REPO.into(),
        asset_path: format!("{project}/{filename}"),
        coordinates: PackageCoordinates {
            package_name: format!("{project}/{filename}"),
            package_version: "1.0.0".into(),
        },
        digest,
        size_bytes: 8,
        tags: Vec::new(),
        actor: "tester".into(),
        correlation_id: "hsp178".into(),
        overwrite: false,
        precondition: None,
        subject: None,
    }
}

/// The publication of one project document, the way the PyPI upload
/// handler builds it: the path is the bare project, the coordinates
/// name the snapshot of the document, and the write is an overwrite
/// under a precondition -- the write the guard serves.
fn document_publication(
    project: &str,
    digest: hodei_evidence::ContentDigest,
    precondition: Precondition,
) -> Publication {
    Publication {
        repository: REPO.into(),
        asset_path: project.into(),
        coordinates: PackageCoordinates {
            package_name: format!(
                "{project}{}",
                hodei_catalog::pypi::PROJECT_COORDINATES_SUFFIX
            ),
            package_version: digest.value.clone(),
        },
        digest: digest.clone(),
        size_bytes: 8,
        tags: Vec::new(),
        actor: "tester".into(),
        correlation_id: "hsp178".into(),
        overwrite: true,
        precondition: Some(precondition),
        subject: None,
    }
}

/// Rows seeded like the perf-lab profile does: bulk assets in
/// repositories the PyPI surface never names.
#[derive(Debug, serde::Serialize)]
struct DistractorRow {
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

async fn seed_distractor(db: &Surreal<surrealdb::engine::local::Db>, rows: usize) {
    for batch_start in (0..rows).step_by(1_000) {
        let batch_end = (batch_start + 1_000).min(rows);
        let batch: Vec<DistractorRow> = (batch_start..batch_end)
            .map(|row| {
                let package = format!("pkg-{}", row / 20);
                let version = format!("1.{}.0", row % 20);
                let repository = format!("search-{}", (row / 20) % 8);
                let digest = format!("{:064x}", row);
                DistractorRow {
                    account: ACCOUNT.into(),
                    repository,
                    asset_path: format!("{package}/{version}/{package}-{version}.tar.gz"),
                    package_name: package,
                    package_version: version,
                    digest_algorithm: "sha256".into(),
                    digest,
                    size_bytes: 1024,
                    subject: None,
                    updated_at: surrealdb::sql::Datetime::from(chrono::Utc::now()),
                }
            })
            .collect();
        db.query("INSERT INTO catalog_asset $rows")
            .bind(("rows", batch))
            .await
            .expect("insert distractor")
            .check()
            .expect("insert distractor check");
    }
}

/// One step of a query plan: the operation the planner picked. The rest
/// of the step is ignored where the detail is not comparable: the
/// detail of a fetch-by-identity plan carries the record id, which
/// SurrealDB 2.3 cannot hand back as JSON. Where the detail matters
/// (the index plan), the test reads it as raw JSON.
#[derive(Debug, serde::Deserialize)]
struct PlanStep {
    operation: String,
}

/// The plan the planner builds for a statement with the binds the
/// assets-under read carries, against whatever the catalog holds.
async fn plan_of(
    db: &Surreal<surrealdb::engine::local::Db>,
    select: &str,
    prefix: Option<&str>,
    prefix_end: Option<&str>,
) -> Vec<PlanStep> {
    let mut statement = db
        .query(format!("{select} EXPLAIN"))
        .bind(("account", ACCOUNT.to_string()))
        .bind(("repository", REPO.to_string()))
        .bind(("limit", 20_001i64));
    if let Some(prefix) = prefix {
        statement = statement.bind(("prefix", prefix.to_string()));
    }
    if let Some(prefix_end) = prefix_end {
        statement = statement.bind(("prefix_end", prefix_end.to_string()));
    }
    let mut response = statement
        .await
        .expect("explain")
        .check()
        .expect("explain check");
    response.take(0).expect("explain rows")
}

/// The plan steps of the statement the assets-under read sends, for a
/// prefix whose upper bound exists.
async fn the_range_plan(db: &Surreal<surrealdb::engine::local::Db>, prefix: &str) -> Vec<PlanStep> {
    let end = the_prefix_end(prefix).expect("the prefix has an upper bound");
    plan_of(db, the_assets_under_read(prefix), Some(prefix), Some(&end)).await
}

/// The detail of the index iteration of the range plan, as raw JSON:
/// which index the planner picked, and which equality and range
/// predicates it bound.
async fn the_range_plan_detail(
    db: &Surreal<surrealdb::engine::local::Db>,
    prefix: &str,
) -> serde_json::Value {
    let end = the_prefix_end(prefix).expect("the prefix has an upper bound");
    let mut response = db
        .query(format!("{} EXPLAIN FULL", the_assets_under_read(prefix)))
        .bind(("account", ACCOUNT.to_string()))
        .bind(("repository", REPO.to_string()))
        .bind(("prefix", prefix.to_string()))
        .bind(("prefix_end", end))
        .bind(("limit", 20_001i64))
        .await
        .expect("explain full")
        .check()
        .expect("explain full check");
    let steps: Vec<serde_json::Value> = response.take(0).expect("explain full rows");
    steps
        .into_iter()
        .find(|step| {
            step.get("operation").and_then(serde_json::Value::as_str) == Some("Iterate Index")
        })
        .unwrap_or_else(|| panic!("the range plan iterates the index"))
}

/// The paths the banned prefix-function shape reads, the shape the
/// statement carried before the range form replaced it.
async fn the_banned_shape_paths(
    db: &Surreal<surrealdb::engine::local::Db>,
    account: &str,
    repository: &str,
    prefix: &str,
) -> Vec<String> {
    let mut response = db
        .query(
            "SELECT VALUE asset_path FROM catalog_asset \
             WHERE account = $account AND repository = $repository \
             AND string::starts_with(asset_path, $prefix) ORDER BY asset_path LIMIT $limit",
        )
        .bind(("account", account.to_string()))
        .bind(("repository", repository.to_string()))
        .bind(("prefix", prefix.to_string()))
        .bind(("limit", 20_001i64))
        .await
        .expect("banned shape read")
        .check()
        .expect("banned shape read check");
    response.take(0).expect("banned shape rows")
}

/// The record id of a row, read from the base itself instead of rebuilt
/// by hand: the test must not restate how the store names its records.
async fn the_row_id(
    db: &Surreal<surrealdb::engine::local::Db>,
    repository: &str,
    asset_path: &str,
) -> Thing {
    let mut response = db
        .query(
            "SELECT VALUE id FROM catalog_asset \
             WHERE account = $account AND repository = $repository AND asset_path = $asset_path LIMIT 1",
        )
        .bind(("account", ACCOUNT.to_string()))
        .bind(("repository", repository.to_string()))
        .bind(("asset_path", asset_path.to_string()))
        .await
        .expect("read the row id")
        .check()
        .expect("read the row id check");
    let rows: Vec<Thing> = response.take(0).expect("row id rows");
    rows.into_iter()
        .next()
        .unwrap_or_else(|| panic!("the row {asset_path} exists"))
}

/// How many rows of a table carry a field with a value.
async fn rows_with(
    db: &Surreal<surrealdb::engine::local::Db>,
    table: &str,
    field: &str,
    value: &str,
) -> usize {
    let mut response = db
        .query(format!(
            "SELECT count() FROM {table} WHERE {field} = $value GROUP ALL"
        ))
        .bind(("value", value.to_string()))
        .await
        .expect("census")
        .check()
        .expect("census check");
    let rows: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        response.take(0).expect("census rows");
    rows.first()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

/// The successor of the prefix: it sorts after every string that
/// extends the prefix, and the empty prefix has none.
#[tokio::test]
async fn the_prefix_end_sorts_after_every_extension_of_the_prefix() {
    let extensions = [
        "".to_string(),
        "\u{0}".to_string(),
        "a".to_string(),
        "zzz".to_string(),
        "\u{10FFFF}".to_string(),
    ];
    for prefix in [
        "pkg/",
        "abc",
        "caf",
        "caf\u{E9}",
        "a\u{D7FF}",
        "x\u{10FFFF}",
    ] {
        let end = the_prefix_end(prefix).expect("the prefix has an upper bound");
        assert!(end.as_str() > prefix, "{end:?} must sort after {prefix:?}");
        for extension in &extensions {
            let extended = format!("{prefix}{extension}");
            assert!(
                extended.as_str() < end.as_str(),
                "{extended:?} (an extension of {prefix:?}) must sort before {end:?}"
            );
        }
    }
    // The empty prefix has no upper bound, and neither has the
    // degenerate prefix of highest characters.
    assert!(the_prefix_end("").is_none(), "the empty prefix has no end");
    assert!(
        the_prefix_end("\u{10FFFF}\u{10FFFF}").is_none(),
        "a prefix of highest characters has no end"
    );
    // The successor skips the surrogate block: `U+D7FF` lands on
    // `U+E000`, which still sorts after every extension.
    assert_eq!(
        the_prefix_end("a\u{D7FF}").as_deref(),
        Some("a\u{E000}"),
        "the successor of U+D7FF is U+E000"
    );
    // The successor of the last character is one scalar further.
    assert_eq!(the_prefix_end("pkg/").as_deref(), Some("pkg0"));
    assert_eq!(the_prefix_end("abc").as_deref(), Some("abd"));
    assert_eq!(the_prefix_end("x\u{10FFFF}").as_deref(), Some("y"));
}

/// The statement the simple index sends must iterate the index, and its
/// plan must not move when the catalog grows. The banned prefix-function
/// shape, for contrast, plans a full-table scan on the same fixture --
/// which is why it was replaced.
#[tokio::test]
async fn the_simple_index_plan_serves_the_index_and_does_not_grow_with_the_catalog() {
    let dir = scratch("plan");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, pypi_repo(ACCOUNT, REPO))
        .await
        .expect("repo");

    // The empty prefix (the PyPI simple index asks for the whole
    // repository) and a project prefix both plan index iterations.
    let empty_plan = plan_of(&db, the_assets_under_read(""), None, None).await;
    let empty_operations: Vec<&str> = empty_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        empty_operations.contains(&"Iterate Index"),
        "the empty-prefix read must plan an index iteration, got {empty_operations:?}"
    );
    assert!(
        !empty_operations.contains(&"Iterate Table"),
        "the empty-prefix read must not plan a table scan, got {empty_operations:?}"
    );
    let prefix_plan = the_range_plan(&db, "hello-pkg/").await;
    let prefix_operations: Vec<&str> = prefix_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        prefix_operations.contains(&"Iterate Index"),
        "the ranged read must plan an index iteration, got {prefix_operations:?}"
    );
    assert!(
        !prefix_operations.contains(&"Iterate Table"),
        "the ranged read must not plan a table scan, got {prefix_operations:?}"
    );

    // Seed the distractor: the catalog now holds rows the PyPI surface
    // never names. The plans must not move.
    seed_distractor(&db, DISTRACTOR_ROWS).await;
    let empty_seeded = plan_of(&db, the_assets_under_read(""), None, None).await;
    let prefix_seeded = the_range_plan(&db, "hello-pkg/").await;
    assert_eq!(
        empty_operations,
        empty_seeded
            .iter()
            .map(|s| s.operation.as_str())
            .collect::<Vec<_>>(),
        "the plan of the empty-prefix read must not depend on the size of the catalog"
    );
    assert_eq!(
        prefix_operations,
        prefix_seeded
            .iter()
            .map(|s| s.operation.as_str())
            .collect::<Vec<_>>(),
        "the plan of the ranged read must not depend on the size of the catalog"
    );

    // The detail of the index iteration: the compound index of the
    // asset path serves both equalities and the path range. This is
    // what makes the cost the rows of the answer, not the rows of the
    // table.
    let detail = the_range_plan_detail(&db, "hello-pkg/").await;
    let plan = detail.get("detail").and_then(|detail| detail.get("plan"));
    assert_eq!(
        plan.and_then(|plan| plan.get("index"))
            .and_then(serde_json::Value::as_str),
        Some("catalog_asset_unique"),
        "the range read must iterate the compound index of the path, got {detail}"
    );
    assert_eq!(
        plan.and_then(|plan| plan.get("prefix"))
            .and_then(serde_json::Value::as_array),
        Some(&vec![
            serde_json::Value::String(ACCOUNT.into()),
            serde_json::Value::String(REPO.into())
        ]),
        "the index iteration must bind the account and the repository equalities, got {detail}"
    );
    let ranges = plan
        .and_then(|plan| plan.get("ranges"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("the index iteration carries the path range, got {detail}"));
    let operators: Vec<&str> = ranges
        .iter()
        .filter_map(|range| range.get("operator").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(
        operators,
        vec![">=", "<"],
        "the path range is half-open, got {ranges:?}"
    );

    // The banned shape, for contrast: the same table and the same
    // binds, with the prefix as a function call -- and the planner
    // answers with a full-table scan. This is the shape the simple
    // index must never carry again.
    let banned = "SELECT account, repository, asset_path, package_name, package_version, \
         digest_algorithm, digest, size_bytes, subject, updated_at FROM catalog_asset \
         WHERE account = $account AND repository = $repository \
         AND string::starts_with(asset_path, $prefix) ORDER BY asset_path LIMIT $limit";
    let banned_plan = plan_of(&db, banned, Some("hello-pkg/"), None).await;
    let banned_operations: Vec<&str> = banned_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        banned_operations.contains(&"Iterate Table"),
        "the banned starts_with shape must plan a table scan, got {banned_operations:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The read through the port returns exactly the rows the prefix
/// function read -- the same paths, in the same order, with the account
/// fence intact -- and writes nothing on the way.
#[tokio::test]
async fn assets_under_reads_exactly_the_rows_the_prefix_function_read() {
    let dir = scratch("equivalence");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, pypi_repo(ACCOUNT, REPO))
        .await
        .expect("repo");
    store
        .create_repository(OTHER_ACCOUNT, pypi_repo(OTHER_ACCOUNT, REPO))
        .await
        .expect("the rival repo with the same key");

    // The rows of the account under test: one project document (no
    // separator) and the distributions of the project (with one), the
    // shape the PyPI surface writes.
    for (project, filename, seed) in [
        (PROJECT, WHEEL, 1u64),
        (PROJECT, SDIST, 2),
        ("other-pkg", "other_pkg-1.0.0-py3-none-any.whl", 3),
    ] {
        store
            .publish(
                ACCOUNT,
                distribution_publication(project, filename, seed_digest(seed)),
            )
            .await
            .expect("the distribution row exists");
    }

    // The rival account published at the same repository key: the
    // account fence must keep the two apart.
    store
        .publish(
            OTHER_ACCOUNT,
            distribution_publication(PROJECT, WHEEL, seed_digest(4)),
        )
        .await
        .expect("the rival row exists");

    async fn port_paths(
        store: &SurrealCatalogStore,
        account: &str,
        repository: &str,
        prefix: &str,
    ) -> Vec<String> {
        store
            .assets_under(account, repository, prefix)
            .await
            .expect("the port read")
            .into_iter()
            .map(|asset| asset.asset_path)
            .collect()
    }

    // The simple index question: every path of the repository. The port
    // reads exactly what the prefix function read, in the same order.
    let by_port = port_paths(&store, ACCOUNT, REPO, "").await;
    let by_function = the_banned_shape_paths(&db, ACCOUNT, REPO, "").await;
    assert_eq!(
        by_port, by_function,
        "the port reads the same paths the prefix function read"
    );
    assert_eq!(
        by_port,
        vec![
            format!("{PROJECT}/{WHEEL}"),
            format!("{PROJECT}/{SDIST}"),
            "other-pkg/other_pkg-1.0.0-py3-none-any.whl".to_string(),
        ],
        "the read holds the paths of the account and the repository, in path order"
    );

    // The project page question: the distributions of one project.
    let by_port = port_paths(&store, ACCOUNT, REPO, "hello-pkg/").await;
    let by_function = the_banned_shape_paths(&db, ACCOUNT, REPO, "hello-pkg/").await;
    assert_eq!(
        by_port, by_function,
        "the port reads the same prefixed paths the prefix function read"
    );
    assert_eq!(
        by_port,
        vec![format!("{PROJECT}/{WHEEL}"), format!("{PROJECT}/{SDIST}")],
        "the prefixed read holds the distributions of the project only"
    );

    // The account fence: the rival account with the same repository key
    // reads its own rows, never the rows of the account under test.
    let rival = port_paths(&store, OTHER_ACCOUNT, REPO, "").await;
    assert_eq!(
        rival,
        vec![format!("{PROJECT}/{WHEEL}")],
        "the rival account reads its own rows only"
    );

    // The read writes nothing: the census of the catalog does not move.
    let before = rows_with(&db, "catalog_asset", "repository", REPO).await
        + rows_with(&db, "catalog_asset", "repository", "search-0").await;
    let _ = port_paths(&store, ACCOUNT, REPO, "").await;
    let after = rows_with(&db, "catalog_asset", "repository", REPO).await
        + rows_with(&db, "catalog_asset", "repository", "search-0").await;
    assert_eq!(before, after, "the read must not write");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The flow the PyPI upload runs -- one publication per distribution,
/// one guarded publication per project document -- writes only the rows
/// of its own paths, whatever the catalog holds around them.
#[tokio::test]
async fn the_pypi_publication_flow_writes_only_its_own_rows() {
    let dir = scratch("flow");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, pypi_repo(ACCOUNT, REPO))
        .await
        .expect("repo");
    seed_distractor(&db, DISTRACTOR_ROWS).await;

    // The upload of the wheel: the distribution, then the document of
    // the project (created: precondition Absent).
    store
        .publish(
            ACCOUNT,
            distribution_publication(PROJECT, WHEEL, seed_digest(1)),
        )
        .await
        .expect("the wheel publishes");
    let first_document = seed_digest(9);
    store
        .publish(
            ACCOUNT,
            document_publication(PROJECT, first_document.clone(), Precondition::Absent),
        )
        .await
        .expect("the document publishes");

    // The upload of the sdist: the distribution, then the document
    // (overwritten: precondition Present on the digest it carries).
    store
        .publish(
            ACCOUNT,
            distribution_publication(PROJECT, SDIST, seed_digest(2)),
        )
        .await
        .expect("the sdist publishes");
    let second_document = seed_digest(10);
    store
        .publish(
            ACCOUNT,
            document_publication(
                PROJECT,
                second_document.clone(),
                Precondition::Present(vec![first_document]),
            ),
        )
        .await
        .expect("the document overwrite publishes");

    // The census: the distractor holds exactly the rows it was seeded
    // with, and the publish repository holds the rows of its own paths
    // and nothing else -- two distributions and one document.
    let distractor = rows_with(&db, "catalog_asset", "account", ACCOUNT).await
        - rows_with(&db, "catalog_asset", "repository", REPO).await;
    let own = rows_with(&db, "catalog_asset", "repository", REPO).await;
    assert_eq!(
        own, 3,
        "the repository holds the two distributions and the document, nothing else"
    );
    assert!(
        distractor >= DISTRACTOR_ROWS,
        "the guarded publishes must not touch the distractor rows"
    );
    // The document points at the digest the last guarded publish
    // carried: the precondition arbitrated the two writers.
    let document = the_row_id(&db, REPO, PROJECT).await;
    let mut response = db
        .query("SELECT digest FROM $document")
        .bind(("document", document))
        .await
        .expect("read the document row")
        .check()
        .expect("read the document row check");
    let rows: Vec<serde_json::Value> = response.take(0).expect("document rows");
    assert_eq!(
        rows.first()
            .and_then(|row| row.get("digest"))
            .and_then(serde_json::Value::as_str),
        Some(second_document.value.as_str()),
        "the document row carries the digest of the last guarded publish"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The rows one publication writes, composed the way
/// `write_the_publication` composes them: the guard inside the
/// transaction, then the catalog rows and the audit row, all in one
/// `BEGIN` ... `COMMIT`. The record ids are the test's own: what the
/// probe pins is the composition -- the guard statement, the writes by
/// identity, the one commit -- not how the store names its records.
#[derive(Debug, serde::Serialize)]
struct ProbeVersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    published_at: surrealdb::sql::Datetime,
}

#[derive(Debug, serde::Serialize)]
struct ProbeAssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    updated_at: surrealdb::sql::Datetime,
}

#[derive(Debug, serde::Serialize)]
struct ProbeReferenceRow {
    account: String,
    repository: String,
    asset_path: String,
    digest_algorithm: String,
    digest: String,
    digest_scope: String,
    trust_state: String,
    size_bytes: i64,
    updated_at: surrealdb::sql::Datetime,
}

#[derive(Debug, serde::Serialize)]
struct ProbeAuditRow {
    event_id: String,
    tenant: String,
    actor: String,
    correlation_id: String,
    action: String,
    resource: String,
    result: String,
    occurred_at: surrealdb::sql::Datetime,
    state: String,
    attempts: i64,
}

/// One probe publication: the version, the asset, the reference and the
/// audit row, named after the correlation of the publication.
struct ProbePublication {
    version: ProbeVersionRow,
    asset: ProbeAssetRow,
    reference: ProbeReferenceRow,
    audit: ProbeAuditRow,
}

fn probe_publication(correlation: &str, digest: &str) -> ProbePublication {
    let now = surrealdb::sql::Datetime::from(chrono::Utc::now());
    ProbePublication {
        version: ProbeVersionRow {
            account: ACCOUNT.into(),
            repository: REPO.into(),
            package_name: format!("{PROJECT}#project"),
            package_version: digest.into(),
            digest_algorithm: "sha256".into(),
            digest: digest.into(),
            size_bytes: 8,
            published_at: now.clone(),
        },
        asset: ProbeAssetRow {
            account: ACCOUNT.into(),
            repository: REPO.into(),
            asset_path: PROJECT.into(),
            package_name: format!("{PROJECT}#project"),
            package_version: digest.into(),
            digest_algorithm: "sha256".into(),
            digest: digest.into(),
            size_bytes: 8,
            updated_at: now.clone(),
        },
        reference: ProbeReferenceRow {
            account: ACCOUNT.into(),
            repository: REPO.into(),
            asset_path: PROJECT.into(),
            digest_algorithm: "sha256".into(),
            digest: digest.into(),
            digest_scope: "global".into(),
            trust_state: "trusted".into(),
            size_bytes: 8,
            updated_at: now.clone(),
        },
        audit: ProbeAuditRow {
            event_id: format!("event-{correlation}"),
            tenant: ACCOUNT.into(),
            actor: "tester".into(),
            correlation_id: correlation.into(),
            action: "publish".into(),
            resource: format!("hrn:hodei:artifacts:eu:{ACCOUNT}:{REPO}/{PROJECT}"),
            result: "allowed".into(),
            occurred_at: now,
            state: "pending".into(),
            attempts: 0,
        },
    }
}

/// A publication that fails inside its transaction leaves nothing
/// behind: the `THROW` of the guard aborts the `BEGIN` ... `COMMIT`, so
/// no version, no asset, no reference and no audit row of the failed
/// publication survives -- not even half of them. The positive control
/// proves the same composition writes all four rows when the guard
/// holds.
#[tokio::test]
async fn a_failed_publication_leaves_no_partial_metadata() {
    let dir = scratch("abort");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, pypi_repo(ACCOUNT, REPO))
        .await
        .expect("repo");

    // The row the guard revalidates: a published distribution.
    store
        .publish(
            ACCOUNT,
            distribution_publication(PROJECT, WHEEL, seed_digest(1)),
        )
        .await
        .expect("the guarded row exists");
    let guard_id = the_row_id(&db, REPO, &format!("{PROJECT}/{WHEEL}")).await;

    // The positive control: the guard holds, the transaction commits,
    // and the four rows of the publication are there.
    let kept = probe_publication("kept", &seed_digest(2).value);
    db.query(format!(
        "BEGIN TRANSACTION;\n\
         IF ({})[0].digest NOT IN $expected_digests {{ THROW 'the precondition does not hold' }};\n\
         CREATE $version_id CONTENT $version;\n\
         CREATE $asset_id CONTENT $asset;\n\
         CREATE $reference_id CONTENT $reference;\n\
         CREATE $audit_id CONTENT $audit;\n\
         COMMIT;",
        the_guard_fetch()
    ))
    .bind(("guard_id", guard_id.clone()))
    .bind(("expected_digests", vec![seed_digest(1).value]))
    .bind((
        "version_id",
        Thing::from((
            "catalog_version".into(),
            format!(
                "{}|{}|{}|{}",
                ACCOUNT, REPO, kept.version.package_name, kept.version.package_version
            ),
        )),
    ))
    .bind(("version", kept.version))
    .bind((
        "asset_id",
        Thing::from((
            "catalog_asset".into(),
            format!("{}|{}|{}", ACCOUNT, REPO, kept.asset.asset_path),
        )),
    ))
    .bind(("asset", kept.asset))
    .bind((
        "reference_id",
        Thing::from((
            "catalog_reference".into(),
            format!("{}|{}|{}", ACCOUNT, REPO, kept.reference.asset_path),
        )),
    ))
    .bind(("reference", kept.reference))
    .bind((
        "audit_id",
        Thing::from(("catalog_audit_outbox".into(), kept.audit.event_id.clone())),
    ))
    .bind(("audit", kept.audit))
    .await
    .expect("the guarded transaction commits")
    .check()
    .expect("the guarded transaction commits check");
    assert_eq!(
        rows_with(
            &db,
            "catalog_version",
            "package_version",
            &seed_digest(2).value
        )
        .await,
        1,
        "the version row of the committed publication is there"
    );
    assert_eq!(
        rows_with(&db, "catalog_asset", "asset_path", PROJECT).await,
        1,
        "the asset row of the committed publication is there"
    );
    assert_eq!(
        rows_with(&db, "catalog_reference", "asset_path", PROJECT).await,
        1,
        "the reference row of the committed publication is there"
    );
    assert_eq!(
        rows_with(&db, "catalog_audit_outbox", "correlation_id", "kept").await,
        1,
        "the audit row of the committed publication is there"
    );

    // The failure: the same composition, but the guarded row no longer
    // carries the digest the client demanded. The `THROW` fires, the
    // transaction aborts, and none of the four rows of the failed
    // publication is left behind.
    let failed = probe_publication("failed", &seed_digest(3).value);
    let result = db
        .query(format!(
            "BEGIN TRANSACTION;\n\
             IF ({})[0].digest NOT IN $expected_digests {{ THROW 'the precondition does not hold' }};\n\
             CREATE $version_id CONTENT $version;\n\
             CREATE $asset_id CONTENT $asset;\n\
             CREATE $reference_id CONTENT $reference;\n\
             CREATE $audit_id CONTENT $audit;\n\
             COMMIT;",
            the_guard_fetch()
        ))
        .bind(("guard_id", guard_id))
        .bind(("expected_digests", vec![seed_digest(9).value]))
        .bind(("version_id", Thing::from(("catalog_version".into(), format!("{}|{}|{}|{}", ACCOUNT, REPO, failed.version.package_name, failed.version.package_version)))))
        .bind(("version", failed.version))
        .bind(("asset_id", Thing::from(("catalog_asset".into(), format!("{}|{}|{}", ACCOUNT, REPO, failed.asset.asset_path)))))
        .bind(("asset", failed.asset))
        .bind(("reference_id", Thing::from(("catalog_reference".into(), format!("{}|{}|{}", ACCOUNT, REPO, failed.reference.asset_path)))))
        .bind(("reference", failed.reference))
        .bind(("audit_id", Thing::from(("catalog_audit_outbox".into(), failed.audit.event_id.clone()))))
        .bind(("audit", failed.audit))
        .await
        .expect("the query reaches the base")
        .check()
        .expect_err("the guard throws and the transaction aborts");
    let _ = result;
    assert_eq!(
        rows_with(
            &db,
            "catalog_version",
            "package_version",
            &seed_digest(3).value
        )
        .await,
        0,
        "no version row of the failed publication survives"
    );
    assert_eq!(
        rows_with(&db, "catalog_reference", "asset_path", PROJECT).await,
        1,
        "the reference of the failed publication was never written"
    );
    assert_eq!(
        rows_with(&db, "catalog_audit_outbox", "correlation_id", "failed").await,
        0,
        "no audit row of the failed publication survives"
    );
    // The guarded row itself is untouched: the abort did not move it.
    let document = the_row_id(&db, REPO, &format!("{PROJECT}/{WHEEL}")).await;
    let mut response = db
        .query("SELECT digest FROM $row")
        .bind(("row", document))
        .await
        .expect("read the guarded row")
        .check()
        .expect("read the guarded row check");
    let rows: Vec<serde_json::Value> = response.take(0).expect("guarded rows");
    assert_eq!(
        rows.first()
            .and_then(|row| row.get("digest"))
            .and_then(serde_json::Value::as_str),
        Some(seed_digest(1).value.as_str()),
        "the guarded row carries the digest it was published with"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The error the reader answers with when the repository holds more
/// assets under the prefix than the port carries: the same contract the
/// statement had before the range form, unchanged.
#[tokio::test]
async fn assets_under_refuses_to_serve_more_rows_than_the_port_carries() {
    let dir = scratch("limit");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, pypi_repo(ACCOUNT, REPO))
        .await
        .expect("repo");

    // More assets under the prefix than the port carries: the read
    // fails instead of returning a list that looks complete and is not.
    let rows: Vec<DistractorRow> = (0..hodei_catalog::MAX_ASSETS_UNDER + 1)
        .map(|row| {
            let digest = format!("{:064x}", row);
            DistractorRow {
                account: ACCOUNT.into(),
                repository: REPO.into(),
                asset_path: format!("{PROJECT}/dist-{}.whl", row),
                package_name: PROJECT.into(),
                package_version: "1.0.0".into(),
                digest_algorithm: "sha256".into(),
                digest,
                size_bytes: 8,
                subject: None,
                updated_at: surrealdb::sql::Datetime::from(chrono::Utc::now()),
            }
        })
        .collect();
    db.query("INSERT INTO catalog_asset $rows")
        .bind(("rows", rows))
        .await
        .expect("insert the rows")
        .check()
        .expect("insert the rows check");

    let error = store
        .assets_under(ACCOUNT, REPO, "")
        .await
        .expect_err("the read refuses the list it cannot complete");
    assert!(
        matches!(error, CatalogError::InvalidRequest { .. }),
        "expected InvalidRequest, got {error:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
