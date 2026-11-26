//! The conformance of the S3 backend against a **real** S3 server (HSP-139).
//!
//! The HSP-116 series pinned the contract against LocalStack. This file does not
//! rewrite that contract: it **applies** it against RustFS, a real S3
//! server in Rust pinned by image digest. The battery --AC1
//! concurrency and retry with the lost response, AC2 honest conditional
//! write over a corrupt pre-existing object, AC3 multipart boundaries
//! and reaping of orphaned staging, AC4 measured capabilities report--
//! lives in `common::conformity` and runs identically against both
//! benches: what is claimed of the emulator has to be true of the
//! real server, and the report verdicts carry the version note of
//! this image.
//!
//! What is deliberately **not** pretended: >5 GiB is not promised in the
//! product nor fits on this machine's bench with room to spare; the limit
//! that is tested is the one the code declares and the one the deployer
//! can ask for. The AC4 report leaves it written down.

mod common;

use hodei_hub::blob::BlobStore;

/// AC1: concurrent publishers of the same digest converge to one
/// intact object and a single reference.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishers_of_one_digest_converge_to_one_intact_object() {
    let bucket = common::rustfs::a_unique_bucket("ac1-conc");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac1_concurrent_publishers(&client, &bucket).await;
}

/// AC1 (hard half): the retry after a lost response publishes
/// the same object only once.
#[tokio::test]
async fn a_retry_after_a_lost_ack_publishes_the_same_object_once() {
    let bucket = common::rustfs::a_unique_bucket("ac1-retry");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac1_lost_ack_retry(&client, &bucket).await;
}

/// AC2: a corrupt pre-existing blob is not presented as a
/// valid deduplication -- it is called corrupt and repaired.
#[tokio::test]
async fn a_preexisting_corrupt_blob_is_never_presented_as_a_valid_dedupe() {
    let bucket = common::rustfs::a_unique_bucket("ac2-cond");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac2_corrupt_is_never_dedupe(&client, &bucket).await;
}

/// AC2 (session half): the conditional publish of the session
/// honestly deduplicates over an already existing good object.
#[tokio::test]
async fn the_session_publish_dedupes_honestly_over_an_existing_object() {
    let bucket = common::rustfs::a_unique_bucket("ac2-session");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac2_session_dedupes(&client, &bucket).await;
}

/// AC3: multipart at the minimum-part boundary and abort without a trace.
#[tokio::test]
async fn multipart_at_the_frontier_and_abort_leaves_no_trace() {
    let bucket = common::rustfs::a_unique_bucket("ac3-multipart");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac3_multipart_and_abort(&client, &bucket).await;
}

/// AC3 (GC half): the reaping aborts its own orphans, respects
/// the live ones and does not touch foreign keys.
#[tokio::test]
async fn the_staging_reaper_aborts_orphans_and_spares_foreign_keys() {
    let bucket = common::rustfs::a_unique_bucket("ac3-reaper");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    common::conformity::ac3_staging_reaper(&client, &bucket).await;
}

/// AC4: the capabilities report, measured against this version of
/// RustFS. Each line comes from a real call to the server of this
/// run; the assertions pin what the product needs.
#[tokio::test]
async fn the_capability_report_is_measured_not_assumed() {
    let bucket = common::rustfs::a_unique_bucket("ac4-report");
    let client = common::rustfs::a_client().await;
    common::rustfs::a_bucket(&client, &bucket).await;
    for line in common::conformity::ac4_capability_report(&client, &bucket).await {
        println!("CAPACIDAD [{}] {line}", common::rustfs::VERSION_NOTE);
    }
}

/// The parity suite of HSP-116, applied without touch-ups to the real
/// server: the literal of the statement ("apply the existing contract
/// suite ... to a real S3 server").
#[tokio::test]
async fn the_parity_suite_runs_against_the_real_server() {
    let bucket = common::rustfs::a_unique_bucket("parity");
    let store = common::rustfs::a_store(&bucket).await;
    common::the_suite(&store).await;
    // The restart, too: the state lives in the server, not in the
    // client -- a new store over the same bucket finds again the
    // content published by the previous one.
    let bytes = common::the_bytes();
    let digest = store.put(&bytes).await.expect("publicado");
    drop(store);
    let reopened = common::rustfs::a_store(&bucket).await;
    reopened.verify(&digest).await.expect("revive en el real");
    assert_eq!(reopened.get(&digest).await.expect("y se lee"), bytes);
}
