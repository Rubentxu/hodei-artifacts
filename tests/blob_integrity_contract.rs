//! The content integrity probe, wired into startup.
//!
//! The `BlobIntegrityProbe` port exists and the catalog consumes it, but in
//! production **nobody wires it**: nothing calls `with_probe` and the
//! content store is mounted without a probe. As a result, a blob
//! that is not where the catalog says it is **is served as if it were healthy**,
//! and corruption is mistaken for absence.
//!
//! These tests run against the **real** process: a real binary, a real
//! on-disk byte store and a real catalog. The defect does not show up
//! in a test double: it shows up when the stored byte is not the one the
//! catalog says is there.

mod support;

use std::path::PathBuf;

use hodei_catalog::{BlobIntegrityProbe, CatalogReader, CatalogWriter};
use hodei_hub::blob::BlobStore;
use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// The catalog seeder.
///
/// The deployment seeds the catalog in a separate process, and this test is
/// the one that does it when the process asks: the harness passes the path of the
/// database through the `HODEI_SEED_DB` variable, and this is what writes it.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Where a blob lives in the byte store.
///
/// The store spreads blobs over two levels by the start of the
/// digest, so the path comes from the digest itself and not from where it
/// was saved.
fn blob_path(root: &std::path::Path, digest: &str) -> PathBuf {
    root.join(&digest[..2]).join(&digest[2..4]).join(digest)
}

/// The content path inside the repository.
fn the_path(path: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{path}")
}

/// The content that gets published.
fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// A blob that is not where the catalog says it is.
///
/// The catalog claims the blob has those bytes; the store does not
/// have them. With the probe wired, that contradiction is answered as
/// **corrupt**, which means "something is there and it cannot be read properly". Without it,
/// the catalog answers that the asset is present, and the failure shows up later,
/// as a 404: the client sees that it does not exist, and deletes its copy.
///
/// A `Corrupt` where there was a `Missing` makes the client delete something
/// that was not theirs. A `Missing` where there was a `Corrupt` makes the
/// client retry. Confusing them is worse than not answering.
#[test]
fn a_blob_that_is_not_where_the_catalog_says_is_corrupt_and_not_missing() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");
    let digest = hodei_evidence::ContentDigest::of_bytes(&the_content()).value;

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published: status={} body={:?}\n{}",
        published.status,
        published.body,
        deployment.the_log()
    );

    // And now the blob disappears from the byte store: the catalog still
    // says the asset is there, with its bytes and its length.
    let blob = blob_path(&PathBuf::from(&deployment.cas), &digest);
    std::fs::remove_file(&blob).expect("the blob is removed from the store");

    let answer = support::curl_download(service.port, &the_path("app.bin"), &token, None);

    // What is wanted is not a 404: a 404 tells the client its
    // copy does not exist, and the client deletes it. What is wanted is a
    // 503: the server has the asset and cannot serve it, and the client
    // retries.
    assert_eq!(
        answer.status,
        503,
        "a blob that is not in the store must not be said to be missing:\n{}",
        deployment.the_log()
    );

    support::stop(&mut service);
}

/// A blob whose bytes are not the published ones is answered as corrupt.
///
/// The corrupt byte has **the same length** as the good one: the length
/// does not give it away, and the probe does not recompute the digest -- that is the cheap part, and
/// the deep check is the scrub's job --. What gives it away is that the bytes are no
/// longer those its digest says, and that is why the read reports it.
#[test]
fn a_blob_whose_bytes_are_not_the_published_ones_is_not_served_as_healthy() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = the_content();
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, &content).expect("the source is written");
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published:\n{}",
        deployment.the_log()
    );

    // The same digest, the same length, other bytes.
    let mut other = content.clone();
    other[0] = other[0].wrapping_add(1);
    let blob = blob_path(&PathBuf::from(&deployment.cas), &digest);
    std::fs::write(&blob, &other).expect("the blob is altered on disk");

    let answer = support::curl_download(service.port, &the_path("app.bin"), &token, None);

    // The content must not be served as if it were the published one.
    assert_ne!(
        answer.status, 200,
        "the corrupted content was served as the published one"
    );
    assert!(
        answer.body != other,
        "the bytes served are the corrupted ones"
    );

    support::stop(&mut service);
}

/// The probe checks the length, and does not recompute the digest.
///
/// That is the cheap part: reading a file's **metadata** does not cost what
/// reading its content costs, and recomputing the digest on every read would be
/// a full read per request. Deep verification -- that the bytes
/// are those its digest says -- is the scrub's job, which does it when it
/// can and not on every request.
///
/// The blob in this test has the length the catalog declares and
/// bytes that are not those its digest says: the probe accepts it, because what
/// it looks at is the length. That is not a probe failure: it is its
/// contract, and a contract that is met is better than a guarantee that
/// nobody notices.
#[tokio::test]
async fn the_probe_checks_the_length_and_not_the_digest() {
    let deployment = Deployment::new();
    let cas = std::path::PathBuf::from(&deployment.cas);
    let content = the_content();
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let store = hodei_hub::blob::FileBlobStore::new(&cas).expect("the store");
    store.put(&content).await.expect("the content is stored");

    // Same length, other bytes.
    let mut other = content.clone();
    other[0] = other[0].wrapping_add(1);
    std::fs::write(blob_path(&cas, &digest), &other).expect("the blob is altered");

    let probe = hodei_artifacts_api::blob_probe::BlobStoreProbe::new(std::sync::Arc::new(
        hodei_hub::blob::FileBlobStore::new(&cas).expect("the store"),
    ));
    let location = hodei_catalog::BlobLocation::new(
        FIRST_ACCOUNT,
        hodei_catalog::DigestScope::Global,
        hodei_evidence::ContentDigest::sha256(digest.clone()),
    );
    let length = probe.length_of(&location).await.expect("the store answers");
    assert_eq!(
        length,
        content.len() as u64,
        "the probe answers with the length the file has, not with the digest it should have"
    );
}

/// The catalog without a probe keeps working.
///
/// The probe **improves** the answer, it is not a requirement to start: a
/// catalog without a probe keeps reading and publishing, and what it cannot do is
/// assert that the bytes are where it says.
#[tokio::test]
async fn a_catalog_without_a_probe_still_publishes_and_reads() {
    let deployment = Deployment::new();
    let cas = std::path::PathBuf::from(&deployment.cas);
    let content = the_content();
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let store = hodei_hub::blob::FileBlobStore::new(&cas).expect("the store");
    store.put(&content).await.expect("the content is stored");

    // The real Dgraph catalog store of the deployment's own namespace.
    let catalog = deployment
        .graph
        .as_ref()
        .expect("the harness always runs on the graph profile")
        .catalog_store()
        .await;
    catalog
        .create_repository(
            FIRST_ACCOUNT,
            hodei_catalog::RepositoryDescriptor {
                hrn: hodei_catalog::repository_hrn(
                    "hodei",
                    "artifacts",
                    "eu",
                    FIRST_ACCOUNT,
                    FIRST_REPOSITORY,
                ),
                account: FIRST_ACCOUNT.to_string(),
                repo_key: FIRST_REPOSITORY.to_string(),
                format: hodei_catalog::RepositoryFormat::Raw,
                mode: hodei_catalog::RepositoryMode::Hosted,
                config: hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .expect("the repository is created");
    catalog
        .publish(
            FIRST_ACCOUNT,
            hodei_catalog::Publication {
                repository: FIRST_REPOSITORY.to_string(),
                asset_path: "app.bin".to_string(),
                coordinates: hodei_catalog::PackageCoordinates {
                    package_name: "app.bin".to_string(),
                    package_version: digest.clone(),
                },
                digest: hodei_evidence::ContentDigest::sha256(digest.clone()),
                size_bytes: content.len() as u64,
                tags: Vec::new(),
                actor: "alice".to_string(),
                correlation_id: "no-probe".to_string(),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("the content is published");

    // And the asset is read without a probe, with nobody asking about the bytes.
    let asset = catalog
        .asset(FIRST_ACCOUNT, FIRST_REPOSITORY, "app.bin")
        .await
        .expect("the asset is read");
    assert_eq!(asset.size_bytes, content.len() as u64);
}

/// The probe does not change isolation between accounts.
///
/// The probe asks for the length of a blob, and the question carries the
/// account of the asker: the same digest can be in the global slot
/// or in an account's partitioned one, and asking for the digest
/// alone would only find the first.
#[test]
fn the_probe_does_not_change_the_isolation_between_accounts() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let other = support::token("other", "bob");

    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published:\n{}",
        deployment.the_log()
    );

    // The other account does not see the first one's repository.
    let answer = support::curl_download(service.port, &the_path("app.bin"), &other, None);
    assert_ne!(
        answer.status, 200,
        "the other account saw the content of the first"
    );

    support::stop(&mut service);
}
