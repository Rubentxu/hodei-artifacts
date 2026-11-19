//! The publish precondition guard revalidates the row by identity, and
//! its cost does not grow with the size of the catalog.
//!
//! The guard inside the publish transaction used to read the row with
//! `SELECT digest FROM catalog_asset WHERE id = $guard_id`. SurrealDB
//! does not optimize that predicate into a record fetch, so it scanned
//! the whole `catalog_asset` table on every publish -- measured at
//! 1.725 s per guarded publish with 100k rows against 248 us for the
//! identity fetch, which is what saturated the npm publish path. The
//! oracle here is the query plan, not the clock: a plan with no
//! full-table scan cannot grow with the catalog, whatever the hardware.

use std::path::Path;
use std::sync::Arc;

use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogWriter as _, PackageCoordinates, Precondition,
    Publication, RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    repository_hrn,
};
use hodei_catalog_storage::{SurrealCatalogStore, the_guard_fetch};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;
use surrealdb::sql::Thing;

const ACCOUNT: &str = "acme";
const REPO: &str = "lab-npm";
/// Distractor rows the test seeds into sibling repositories, so the
/// guard runs against a catalog that holds rows the publish path never
/// touches. The plan does not depend on the count; the count only makes
/// the contrast with a seeded catalog explicit.
const DISTRACTOR_ROWS: usize = 2_000;
const GUARD_ASSET_PATH: &str = "the/guard/row/thing.tgz";

async fn open(path: &Path) -> Surreal<surrealdb::engine::local::Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("open rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hsp175-guard-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn npm_repo() -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", ACCOUNT, REPO),
        account: ACCOUNT.into(),
        repo_key: REPO.into(),
        format: RepositoryFormat::Npm,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(hodei_catalog::DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// Rows seeded like the perf-lab profile does: bulk assets in
/// repositories the publish path never names.
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

/// The record id of the guarded row, read from the base itself instead
/// of rebuilt by hand: the test must not restate how the store names
/// its records.
async fn the_guard_row_id(db: &Surreal<surrealdb::engine::local::Db>) -> Thing {
    let mut response = db
        .query("SELECT VALUE id FROM catalog_asset WHERE account = $account AND repository = $repository AND asset_path = $asset_path LIMIT 1")
        .bind(("account", ACCOUNT.to_string()))
        .bind(("repository", REPO.to_string()))
        .bind(("asset_path", GUARD_ASSET_PATH.to_string()))
        .await
        .expect("read the guard row id")
        .check()
        .expect("read the guard row id check");
    let rows: Vec<Thing> = response.take(0).expect("guard row id rows");
    rows.into_iter()
        .next()
        .unwrap_or_else(|| panic!("the guarded row exists"))
}

/// How many catalog rows each side of the fence holds: the distractor
/// repositories, and the publish repository.
async fn row_census(db: &Surreal<surrealdb::engine::local::Db>) -> (usize, usize) {
    let mut response = db
        .query("SELECT count() FROM catalog_asset WHERE repository != $repo GROUP ALL")
        .bind(("repo", REPO.to_string()))
        .await
        .expect("distractor census")
        .check()
        .expect("distractor census check");
    let rows: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        response.take(0).expect("distractor census rows");
    let distractor = rows
        .first()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    let mut response = db
        .query("SELECT count() FROM catalog_asset WHERE repository = $repo GROUP ALL")
        .bind(("repo", REPO.to_string()))
        .await
        .expect("publish repo census")
        .check()
        .expect("publish repo census check");
    let rows: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        response.take(0).expect("publish repo census rows");
    let publish_repo = rows
        .first()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    (distractor, publish_repo)
}

/// One step of a query plan: the operation the planner picked. The rest
/// of the step is ignored on purpose: the detail of a fetch-by-identity
/// plan carries the record id, which SurrealDB 2.3 cannot hand back as
/// JSON.
#[derive(Debug, serde::Deserialize)]
struct PlanStep {
    operation: String,
}

/// The plan the planner builds for a statement with the guard's record
/// id bound, against whatever the catalog currently holds.
async fn plan_of(db: &Surreal<surrealdb::engine::local::Db>, select: &str) -> Vec<PlanStep> {
    let mut response = db
        .query(format!("{select} EXPLAIN"))
        .bind(("guard_id", the_guard_row_id(db).await))
        .await
        .expect("explain")
        .check()
        .expect("explain check");
    response.take(0).expect("explain rows")
}

/// The guarded publication: one tarball row, overwritten in place.
fn guarded_publication(digest: hodei_evidence::ContentDigest) -> Publication {
    Publication {
        repository: REPO.into(),
        asset_path: GUARD_ASSET_PATH.into(),
        coordinates: PackageCoordinates {
            package_name: "the-guard".into(),
            package_version: "1.0.0".into(),
        },
        digest,
        size_bytes: 8,
        tags: Vec::new(),
        actor: "tester".into(),
        correlation_id: "hsp175".into(),
        overwrite: false,
        precondition: None,
        subject: None,
    }
}

fn seed_digest(seed: u64) -> hodei_evidence::ContentDigest {
    hodei_evidence::ContentDigest::sha256(format!("{seed:064x}"))
}

/// The guard must fetch the row by identity. The plan proves it: the
/// fetch plans the same with an empty table and with the distractor
/// seeded, and never as a full-table scan; the banned `WHERE id =`
/// shape plans as a full-table scan, which is why it was replaced.
#[tokio::test]
async fn the_guard_plan_does_not_grow_with_the_catalog() {
    let dir = scratch("plan");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, npm_repo())
        .await
        .expect("repo");

    // One guarded row, so the record id the plan binds is real.
    let digest = seed_digest(1);
    store
        .publish(ACCOUNT, guarded_publication(digest))
        .await
        .expect("the guarded row exists");

    // With the catalog empty of distractors, the guard fetch plans the
    // one record by identity, never a scan of the table.
    let empty_plan = plan_of(&db, the_guard_fetch()).await;
    let empty_operations: Vec<&str> = empty_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        empty_operations.contains(&"Iterate Thing"),
        "the guard must plan a fetch by identity, got {empty_operations:?}"
    );
    assert!(
        !empty_operations.contains(&"Iterate Table"),
        "the guard must not plan a table scan, got {empty_operations:?}"
    );

    // Seed the distractor: the catalog now holds rows the publish path
    // never touches. The plan of the guard must not move.
    seed_distractor(&db, DISTRACTOR_ROWS).await;
    let seeded_plan = plan_of(&db, the_guard_fetch()).await;
    let seeded_operations: Vec<&str> = seeded_plan.iter().map(|s| s.operation.as_str()).collect();
    assert_eq!(
        empty_operations, seeded_operations,
        "the plan of the guard must not depend on the size of the catalog"
    );

    // The banned shape, for contrast: the same column and the same
    // binding, with one predicate over the id -- and the planner answers
    // with a full-table scan. This is the shape the guard must never
    // carry again.
    let banned_plan = plan_of(&db, "SELECT digest FROM catalog_asset WHERE id = $guard_id").await;
    let banned_operations: Vec<&str> = banned_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        banned_operations.contains(&"Iterate Table"),
        "the banned WHERE id = shape must plan a table scan, got {banned_operations:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The identity fetch reads the same digest the table filter read: the
/// revalidation verdict of the guard is unchanged, for rows that exist
/// and for rows that do not.
#[tokio::test]
async fn the_guard_fetch_reads_the_same_verdict_as_the_filter() {
    let dir = scratch("verdict");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, npm_repo())
        .await
        .expect("repo");

    let digest = seed_digest(1);
    store
        .publish(ACCOUNT, guarded_publication(digest))
        .await
        .expect("the guarded row exists");
    let thing = the_guard_row_id(&db).await;

    async fn read(
        db: &Surreal<surrealdb::engine::local::Db>,
        select: &str,
        thing: &Thing,
    ) -> Vec<serde_json::Value> {
        let mut response = db
            .query(select)
            .bind(("guard_id", thing.clone()))
            .await
            .expect("read")
            .check()
            .expect("read check");
        response.take(0).expect("read rows")
    }

    // The row exists: both reads return the one digest.
    let by_filter = read(
        &db,
        "SELECT digest FROM catalog_asset WHERE id = $guard_id",
        &thing,
    )
    .await;
    let by_identity = read(&db, the_guard_fetch(), &thing).await;
    assert_eq!(by_filter.len(), 1, "the filter reads the existing row");
    assert_eq!(
        by_filter, by_identity,
        "the identity fetch reads the same digest the filter read"
    );

    // The row does not exist: both reads return nothing.
    let missing = Thing::from((
        "catalog_asset".to_string(),
        format!("{ACCOUNT}|{REPO}|no/such/row"),
    ));
    let by_filter = read(
        &db,
        "SELECT digest FROM catalog_asset WHERE id = $guard_id",
        &missing,
    )
    .await;
    let by_identity = read(&db, the_guard_fetch(), &missing).await;
    assert!(
        by_filter.is_empty(),
        "the filter reads nothing for a missing row"
    );
    assert_eq!(
        by_filter, by_identity,
        "the identity fetch reads the same nothing the filter read"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Publishing under a guard writes only the rows of its own path: the
/// distractor the catalog holds stays untouched, whatever its size.
#[tokio::test]
async fn publish_with_a_guard_writes_only_its_own_rows() {
    let dir = scratch("census");
    let db = open(&dir).await;
    let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
    store.initialize().await.expect("schema");
    store
        .create_repository(ACCOUNT, npm_repo())
        .await
        .expect("repo");
    seed_distractor(&db, DISTRACTOR_ROWS).await;

    let digest = seed_digest(1);

    // The first publish of the path, guarded Absent: creates the row.
    let mut create = guarded_publication(digest.clone());
    create.precondition = Some(Precondition::Absent);
    store
        .publish(ACCOUNT, create)
        .await
        .expect("guarded create");

    // The guarded overwrite of the path, guarded Present on its digest.
    let mut republish = guarded_publication(digest.clone());
    republish.coordinates.package_version = "2.0.0".into();
    republish.overwrite = true;
    republish.precondition = Some(Precondition::Present(vec![digest.clone()]));
    store
        .publish(ACCOUNT, republish)
        .await
        .expect("guarded overwrite");

    // A guarded publish that no longer holds: Absent over an existing
    // row, and Present over a digest the row does not carry.
    let mut absent_again = guarded_publication(digest.clone());
    absent_again.coordinates.package_version = "3.0.0".into();
    absent_again.overwrite = true;
    absent_again.precondition = Some(Precondition::Absent);
    let error = store
        .publish(ACCOUNT, absent_again)
        .await
        .expect_err("absent over an existing row does not hold");
    assert!(
        matches!(error, CatalogError::PreconditionFailed { .. }),
        "expected PreconditionFailed, got {error:?}"
    );

    let mut wrong_digest = guarded_publication(seed_digest(2));
    wrong_digest.coordinates.package_version = "4.0.0".into();
    wrong_digest.overwrite = true;
    wrong_digest.precondition = Some(Precondition::Present(vec![seed_digest(2)]));
    let error = store
        .publish(ACCOUNT, wrong_digest)
        .await
        .expect_err("present over another digest does not hold");
    assert!(
        matches!(error, CatalogError::PreconditionFailed { .. }),
        "expected PreconditionFailed, got {error:?}"
    );

    // The census: the distractor holds exactly the rows it was seeded
    // with, and the publish repository holds the rows of the guarded
    // path and nothing else.
    let (distractor, publish_repo) = row_census(&db).await;
    assert_eq!(
        distractor, DISTRACTOR_ROWS,
        "the guarded publishes must not touch the distractor rows"
    );
    assert_eq!(
        publish_repo, 1,
        "the publish repository holds exactly the one guarded asset row"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
