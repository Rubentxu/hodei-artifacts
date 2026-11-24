//! The writes of the additional formats leave a trace in the audit trail.
//!
//! Conan (`PUT` of a file) and Galaxy (`POST` of a publish) write into the
//! catalog; like every other format, each write is recorded as a security
//! event whether it is accepted or not. The service log is where the
//! tracing sink writes the event, so the test reads it from there.

mod support;

use support::{Curl, Deployment, FIRST_ACCOUNT};

fn request(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: &[u8],
    content_type: &str,
) -> u16 {
    let directory = deployment.directory.path();
    let sent = directory.join("request.bin");
    std::fs::write(&sent, body).expect("the body is written");
    let into = directory.join("response.bin");
    Curl::call(
        port,
        method,
        path,
        token,
        Some(sent.as_path()),
        Some(content_type),
        Some(&into),
    )
    .run()
    .status
}

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn a_conan_upload_leaves_an_audit_event() {
    let deployment = Deployment::new();
    deployment.seed_conan(&[("conan-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let path = "/conan/conan-hosted/v2/conans/audited/1.0/_/_/revisions/0123456789abcdef0123456789abcdef/files/conanfile.py";
    let status = request(
        &deployment,
        port,
        "PUT",
        path,
        &token,
        b"from conan import ConanFile\n",
        "application/octet-stream",
    );
    assert_eq!(status, 200, "the upload is accepted");

    support::stop(&mut service);
    let log = deployment.the_log();
    assert!(
        log.contains("publish") && log.contains("conan-hosted/v2/conans/audited/1.0"),
        "the conan upload left no audit event in the service log:\n{log}"
    );
}

#[test]
fn a_galaxy_publish_leaves_an_audit_event_even_when_it_fails() {
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The galaxy dialect sends the key as `Authorization: Token <key>`.
    let directory = deployment.directory.path();
    let sent = directory.join("request.bin");
    std::fs::write(&sent, b"this is not a publish form").expect("the body is written");
    let into = directory.join("response.bin");
    let authorization = format!("Token {token}");
    let status = Curl::call_with_headers(
        port,
        "POST",
        "/galaxy/galaxy-hosted/api/v3/artifacts/collections/",
        "",
        Some(sent.as_path()),
        &[
            ("authorization", authorization.as_str()),
            ("content-type", "text/plain"),
        ],
        Some(&into),
    )
    .run()
    .status;
    assert!(
        status >= 400,
        "a malformed publish is refused, got {status}"
    );

    support::stop(&mut service);
    let log = deployment.the_log();
    assert!(
        log.contains("publish") && log.contains("galaxy-hosted/api/v3/artifacts/collections"),
        "the galaxy publish left no audit event in the service log:\n{log}"
    );
}
