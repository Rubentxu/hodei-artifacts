//! The raw hosted content surface, at the level of a real process (AC5).
//!
//! The contract of [`har04_raw_hosted_contract`] runs in process. This one
//! runs the **binary**: it boots the service on a real port, publishes a
//! file with the real `curl`, downloads it with the real `curl`, compares
//! the two with the real `sha256sum`, stops the process, starts it again
//! over the same storage and checks that the content is still there and is
//! still the same bytes.
//!
//! What the in-process contract cannot say and this one says:
//!
//! - That the content is on **disk**, and not in the memory of a process
//!   that is about to die.
//! - That the routing, the configuration and the boot check work in the
//!   binary and not only in a test harness that builds the router by hand.
//! - That the **bytes** are the bytes: `sha256sum` says so, and it is not
//!   the code under test that says it.
//!
//! The repository is seeded directly in the catalog before the service
//! starts, because the creation of a repository is not part of this
//! surface: this surface serves the repositories that are there.

mod support;

use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// The path of the content, inside the repository.
const CONTENT_PATH: &str = "/repository/releases/dir/file.bin";

/// The seeder of the catalog.
///
/// The embedded engine keeps the lock of the directory while there is a
/// connection open, and the service is another process: the seeding is
/// done by a process that ends right after.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The whole scenario: publish, download, compare, restart, download again.
#[test]
fn the_content_is_published_downloaded_and_survives_a_real_restart() {
    let deployment = Deployment::new();
    deployment.seed();
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The content that is published: some bytes that are not text, so
    // that the comparison is of bytes and not of lines.
    let source = deployment.directory.path().join("source.bin");
    let bytes: Vec<u8> = (0u16..4096).map(|value| (value % 251) as u8).collect();
    std::fs::write(&source, &bytes).unwrap();
    let expected = support::sha256_of(&source);

    // --- The first round: the service publishes and serves the content.
    let mut service = deployment.start(&[]);
    let port = service.port;

    let published = support::curl_upload(port, CONTENT_PATH, &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the publication failed:\n{}",
        deployment.the_log()
    );

    let downloaded = deployment.directory.path().join("downloaded.bin");
    let answer = support::curl_download(port, CONTENT_PATH, &token, Some(&downloaded));
    assert_eq!(answer.status, 200, "the download failed");
    assert_eq!(
        support::sha256_of(&downloaded),
        expected,
        "the bytes that came back are not the bytes that went in"
    );

    // And the header says what the content measures.
    let described = support::curl_describe(port, CONTENT_PATH, &token);
    assert_eq!(described.status, 200, "the description failed");

    support::stop(&mut service);

    // --- The second round: the same storage, the same content.
    let mut service = deployment.start(&[]);
    let port = service.port;

    let after = deployment.directory.path().join("after-restart.bin");
    let answer = support::curl_download(port, CONTENT_PATH, &token, Some(&after));
    assert_eq!(
        answer.status,
        200,
        "the content did not survive the restart:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        support::sha256_of(&after),
        expected,
        "the content that survived the restart is not the content that was published"
    );

    // And republishing the same thing is idempotent, not a conflict.
    let published = support::curl_upload(port, CONTENT_PATH, &token, &source, None);
    assert_eq!(published.status, 200, "the republication failed");

    support::stop(&mut service);
}

/// A path that is not allowed does not publish anything, and the service
/// says so with the status code, not with a content.
#[test]
fn a_traversal_is_refused_by_the_running_service() {
    let deployment = Deployment::new();
    deployment.seed();
    let token = support::token(FIRST_ACCOUNT, "alice");

    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, b"content").unwrap();

    let mut service = deployment.start(&[]);
    let port = service.port;

    let refused = support::curl_upload(
        port,
        &format!("/repository/{FIRST_REPOSITORY}/%2e%2e/escape.bin"),
        &token,
        &source,
        None,
    );
    assert_eq!(refused.status, 400, "a traversal has to be refused");

    let escaped = support::curl_download(
        port,
        &format!("/repository/{FIRST_REPOSITORY}/escape.bin"),
        &token,
        None,
    );
    assert_eq!(
        escaped.status, 404,
        "the refused path did not publish anything"
    );

    // And without a credential nothing is said, not even that the
    // repository exists.
    let anonymous = support::curl_download(port, CONTENT_PATH, "", None);
    assert_eq!(
        anonymous.status, 401,
        "a request without a credential is refused"
    );

    support::stop(&mut service);
}
