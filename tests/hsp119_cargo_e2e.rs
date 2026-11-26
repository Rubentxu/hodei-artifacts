//! Hosted sparse Cargo of HSP-119, at the HTTP level.
//!
//! What is tested: the index configuration, the deterministic sparse
//! path, the `PUT api/v1/crates/new` publication, the byte-for-byte
//! download, the read ETag, the immutability of the version and the
//! idempotence of an identical republication.
//!
//! The real client (`cargo`) is out of scope here: the sparse protocol
//! is spoken against the service the same way the client would speak it.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The sha256 in lowercase hex: the same `cksum` the client verifies.
fn the_cksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The body of a publication: JSON header and file, each with its
/// u32 little-endian length.
fn the_publish_body(header: &Value, file: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(header).expect("the header is JSON");
    let mut body = Vec::new();
    body.extend_from_slice(&(json.len() as u32).to_le_bytes());
    body.extend_from_slice(&json);
    body.extend_from_slice(&(file.len() as u32).to_le_bytes());
    body.extend_from_slice(file);
    body
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// A request to the service.
fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
    extra: &[(&str, &str)],
) -> Answer {
    // A `HEAD` is requested with `--head`: with `-X HEAD` `curl` hangs
    // waiting for the body that a `HEAD` never sends.
    if method == "HEAD" {
        return Curl::describe(port, path, token).run();
    }
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = extra.to_vec();
    if sent.is_some() {
        headers.push(("content-type", "application/octet-stream"));
    }
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &headers,
        Some(&into),
    )
    .run()
}

/// Publishes a crate: header and file along the protocol path.
fn publish(port: u16, repository: &str, token: &str, header: &Value, file: &[u8]) -> Answer {
    call(
        port,
        "PUT",
        &format!("/cargo/{repository}/api/v1/crates/new"),
        token,
        Some(&the_publish_body(header, file)),
        &[],
    )
}

/// The text of a response.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

fn the_header(name: &str, version: &str) -> Value {
    json!({
        "name": name,
        "vers": version,
        "deps": [{"name": "libc", "optional": false}],
        "cksum": "",
        "features": {"std": []},
        "yanked": false,
    })
}

#[test]
fn a_crate_is_published_and_served_through_the_sparse_protocol() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // The index configuration says where to look for the download and the API.
    let config = call(
        port,
        "GET",
        "/cargo/cargo-hosted/config.json",
        &token,
        None,
        &[],
    );
    assert_eq!(config.status, 200, "{}", text_of(&config));
    let config: Value = serde_json::from_slice(&config.body).expect("the config");
    assert_eq!(
        config["dl"].as_str().expect("the dl"),
        format!(
            "http://127.0.0.1:{port}/cargo/cargo-hosted/api/v1/crates/{{crate}}/{{version}}/download"
        )
    );
    assert_eq!(
        config["api"].as_str().map(str::to_string),
        Some(format!("http://127.0.0.1:{port}/cargo/cargo-hosted"))
    );
    assert_eq!(
        config["auth-required"], true,
        "cargo solo manda el token a las descargas si el registro lo declara"
    );

    let file = b"not really a gzipped crate, but bytes".to_vec();
    let digest = the_cksum(&file);

    // Publish version 0.1.0.
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The sparse index: the deterministic path of a four-character name.
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200, "{}", text_of(&index));
    let line: Value = serde_json::from_slice(&index.body).expect("the NDJSON line");
    assert_eq!(line["name"], json!("hodei-pkg"));
    assert_eq!(line["vers"], json!("0.1.0"));
    assert_eq!(line["cksum"], json!(digest));
    assert_eq!(line["yanked"], json!(false));

    // The download delivers the exact bytes that were published.
    let download = call(
        port,
        "GET",
        "/cargo/cargo-hosted/api/v1/crates/hodei-pkg/0.1.0/download",
        &token,
        None,
        &[],
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, file);

    // Publish 0.2.0: the index orders the versions by semver.
    let second = b"a different blob for the next version".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.2.0"),
        &second,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200);
    let lines: Vec<Value> = String::from_utf8_lossy(&index.body)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["vers"], json!("0.1.0"));
    assert_eq!(lines[1]["vers"], json!("0.2.0"));

    // Republishing the same bytes is the echo of an interrupted
    // publication: it is not a rewrite.
    let again = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        &file,
    );
    assert_eq!(again.status, 200, "{}", text_of(&again));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(String::from_utf8_lossy(&index.body).lines().count(), 2);

    // Republishing the same version with other bytes is a conflict: the
    // published version is immutable.
    let conflict = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        b"the same version with different bytes".as_slice(),
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));
}

#[test]
fn the_index_serves_a_conditional_read_and_a_head() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let file = b"bytes behind an etag".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("etag-pkg", "1.0.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[],
    );
    let etag = index.header("etag").expect("the index has an ETag");

    // If-None-Match with the current ETag: a 304 without a body.
    let not_modified = call(
        port,
        "GET",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[("if-none-match", &etag)],
    );
    assert_eq!(not_modified.status, 304, "{}", text_of(&not_modified));

    // HEAD: the same index, without the body, announcing its size.
    let head = call(
        port,
        "HEAD",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(head.status, 200, "{}", text_of(&head));
    assert!(head.body.is_empty());
    assert_eq!(head.size, 0);
    assert_eq!(
        head.header("content-length").as_deref(),
        Some(index.body.len().to_string().as_str())
    );
    assert_eq!(head.header("etag").as_deref(), Some(etag.as_str()));
}

#[test]
fn names_are_normalized_and_the_unknown_crate_is_not_found() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // A name with uppercase letters is normalized to lowercase on publish.
    let file = b"an uppercase crate publishes normalized".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("Case-Pkg", "1.0.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ca/se/case-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200, "{}", text_of(&index));
    let line: Value = serde_json::from_slice(&index.body).expect("the line");
    assert_eq!(line["name"], json!("case-pkg"));

    // A crate that nobody published is not in the index.
    let missing = call(
        port,
        "GET",
        "/cargo/cargo-hosted/mi/ss/missing",
        &token,
        None,
        &[],
    );
    assert_eq!(missing.status, 404, "{}", text_of(&missing));

    // A publication without a credential is rejected.
    let anonymous = publish(
        port,
        "cargo-hosted",
        "",
        &the_header("hodei-pkg", "9.9.9"),
        &file,
    );
    assert_eq!(anonymous.status, 401, "{}", text_of(&anonymous));
}

#[test]
fn a_wild_path_is_not_part_of_the_cargo_protocol() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for hostile in ["../se/serde", "se/../serde", "api/v1/crates/../../etc"] {
        let answer = call(
            port,
            "GET",
            &format!("/cargo/cargo-hosted/{hostile}"),
            &token,
            None,
            &[],
        );
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }
}
