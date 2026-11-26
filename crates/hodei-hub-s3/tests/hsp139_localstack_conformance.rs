//! The same HSP-139 conformance, run against the other bench:
//! LocalStack 4.14, the development S3 server (HSP-116).
//!
//! It is not repetition: it is the **matrix** that AC4 asks for -- the
//! capabilities report per backend and version, with the results measured against
//! each one. Where RustFS is the word of the real server, LocalStack
//! is the word of the bench worked with daily; if some day they
//! differ in a capability the product uses, this pair of files
//! is the place where it shows.
//!
//! The LocalStack harness is the same as HSP-116; the only thing specific
//! here is that each test asks for its bucket and runs the shared battery of
//! `common::conformity`.

mod common;

/// AC1: concurrent publishers of the same digest converge to one
/// intact object and a single reference.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishers_of_one_digest_converge_to_one_intact_object() {
    let bucket = common::localstack::a_unique_bucket("ac1-conc");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac1_concurrent_publishers(&client, &bucket).await;
}

/// AC1 (hard half): the retry after a lost response publishes
/// the same object only once.
#[tokio::test]
async fn a_retry_after_a_lost_ack_publishes_the_same_object_once() {
    let bucket = common::localstack::a_unique_bucket("ac1-retry");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac1_lost_ack_retry(&client, &bucket).await;
}

/// AC2: a corrupt pre-existing blob is not presented as a
/// valid deduplication -- it is called corrupt and repaired.
#[tokio::test]
async fn a_preexisting_corrupt_blob_is_never_presented_as_a_valid_dedupe() {
    let bucket = common::localstack::a_unique_bucket("ac2-cond");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac2_corrupt_is_never_dedupe(&client, &bucket).await;
}

/// AC2 (session half): the conditional publish of the session
/// honestly deduplicates over an already existing good object.
#[tokio::test]
async fn the_session_publish_dedupes_honestly_over_an_existing_object() {
    let bucket = common::localstack::a_unique_bucket("ac2-session");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac2_session_dedupes(&client, &bucket).await;
}

/// AC3: multipart at the minimum-part boundary and abort without a trace.
#[tokio::test]
async fn multipart_at_the_frontier_and_abort_leaves_no_trace() {
    let bucket = common::localstack::a_unique_bucket("ac3-multipart");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac3_multipart_and_abort(&client, &bucket).await;
}

/// AC3 (GC half): the reaping aborts its own orphans, respects
/// the live ones and does not touch foreign keys.
#[tokio::test]
async fn the_staging_reaper_aborts_orphans_and_spares_foreign_keys() {
    let bucket = common::localstack::a_unique_bucket("ac3-reaper");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    common::conformity::ac3_staging_reaper(&client, &bucket).await;
}

/// AC4: the capabilities report of LocalStack 4.14, measured against
/// this bench. The other half of the matrix is in the sibling file
/// for RustFS.
#[tokio::test]
async fn the_capability_report_is_measured_not_assumed() {
    let bucket = common::localstack::a_unique_bucket("ac4-report");
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    for line in common::conformity::ac4_capability_report(&client, &bucket).await {
        println!("CAPACIDAD [{}] {line}", common::localstack::VERSION_NOTE);
    }
}
