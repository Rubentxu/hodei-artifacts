//! The declared memory budget and the benchmark numbers
//! (HSP-116, acceptance 4): same contract, same content, two backends.
//!
//! What matters in the measurement is that the S3 session **does not accumulate the blob in
//! memory**: with the server in another process, the resident memory of the
//! test process can only grow by what the session budget is worth,
//! not by what the content is worth.

mod common;

use std::time::{Duration, Instant};

use hodei_hub::blob::{BlobStore, CommitExpectation};
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

/// The process resident memory, in bytes (VmRSS from /proc).
fn the_rss() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/status").expect("/proc existe en linux");
    for line in statm.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse()
                .expect("VmRSS es un numero");
            return kb * 1024;
        }
    }
    unreachable!("el kernel siempre dice VmRSS")
}

#[tokio::test]
async fn a_streaming_session_never_holds_the_whole_blob() {
    let bucket = common::localstack::a_unique_bucket("budget");
    let config = S3BlobConfig {
        bucket: bucket.clone(),
        part_size: 8 * 1024 * 1024,
        max_inflight_parts: 2,
    };
    let budget = config.the_memory_budget();
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    let store = S3BlobStore::new(client, config).expect("el almacen abre");

    // Six times the budget: if the session accumulated, this would be
    // 48 MiB of client heap; real streaming is ~16 MiB.
    let blob_size: usize = 6 * budget;
    let pattern: Vec<u8> = (0u8..=255).cycle().take(blob_size).collect();

    let before = the_rss();
    let mut session = store.begin().await.expect("la sesion abre");
    let mut sent = 0usize;
    while sent < blob_size {
        let end = (sent + 1024 * 1024).min(blob_size);
        session
            .append(&pattern[sent..end])
            .await
            .expect("el megabyte entra");
        sent = end;
    }
    let digest = session
        .commit(CommitExpectation::of(&pattern))
        .await
        .expect("el commit cuadra");
    let after = the_rss();

    // The pattern lives in `pattern` throughout the test: the process
    // delta does NOT include the source blob (it was already accounted for) and
    // can only be due to the session machinery. The ceiling, generous:
    // two budgets. Accumulating the whole blob would cost six.
    let growth = after.saturating_sub(before);
    assert!(
        growth < 2 * budget as u64,
        "la sesion de {blob_size} bytes crecio el proceso {growth} bytes, y el presupuesto declarado es {budget}"
    );

    // And what was published is readable by its digest: streaming broke nothing.
    assert_eq!(
        store.length(&digest).await.expect("el blob mide"),
        blob_size as u64
    );
}

/// The comparative numbers of acceptance 4: whole publication,
/// publication by session and range reads, on both backends.
#[tokio::test]
async fn the_benchmarks_say_what_each_backend_costs() {
    let size: usize = 16 * 1024 * 1024;
    let bytes: Vec<u8> = (0u8..=255).cycle().take(size).collect();
    let bucket = common::localstack::a_unique_bucket("bench");
    let s3: S3BlobStore = common::localstack::a_store(&bucket).await;
    let directory = tempfile::tempdir().expect("un directorio de usar y tirar");
    let fs = hodei_hub::blob::FileBlobStore::new(directory.path()).expect("el fs abre");

    let mut rows: Vec<(&str, u128)> = Vec::new();

    let started = Instant::now();
    fs.put(&bytes).await.expect("el fs guarda");
    rows.push(("put 16 MiB (fs)", started.elapsed().as_millis()));

    let started = Instant::now();
    s3.put(&bytes).await.expect("el s3 guarda");
    rows.push(("put 16 MiB (s3)", started.elapsed().as_millis()));

    let started = Instant::now();
    let mut session = fs.begin().await.expect("el fs abre sesion");
    session.append(&bytes).await.expect("entran");
    session
        .commit(CommitExpectation::of(&bytes))
        .await
        .expect("commitea");
    rows.push(("stream 16 MiB (fs)", started.elapsed().as_millis()));

    let started = Instant::now();
    let mut session = s3.begin().await.expect("el s3 abre sesion");
    session.append(&bytes).await.expect("entran");
    session
        .commit(CommitExpectation::of(&bytes))
        .await
        .expect("commitea");
    rows.push((
        "stream 16 MiB (s3 multipart)",
        started.elapsed().as_millis(),
    ));

    let digest = hodei_evidence::ContentDigest::of_bytes(&bytes);
    let started = Instant::now();
    for _ in 0..100 {
        let _ = fs
            .read_range(&digest, 4096, 1024)
            .await
            .expect("el fs sirve tramos");
    }
    rows.push(("100 range reads (fs)", started.elapsed().as_millis()));

    let started = Instant::now();
    for _ in 0..100 {
        let _ = s3
            .read_range(&digest, 4096, 1024)
            .await
            .expect("el s3 sirve tramos");
    }
    rows.push(("100 range reads (s3)", started.elapsed().as_millis()));

    for (case, millis) in &rows {
        println!("{case}: {millis} ms");
    }

    // Sanity ceilings, not competitiveness ones: if something goes beyond
    // them the backend is doing something wrong, not the machine running slowly.
    let ceiling = Duration::from_secs(120);
    for (case, millis) in &rows {
        assert!(
            Duration::from_millis(*millis as u64) < ceiling,
            "{case} tardo {millis} ms: fuera de todo presupuesto"
        );
    }
}
