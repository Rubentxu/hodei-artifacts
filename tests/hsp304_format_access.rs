//! HSP-304 (AC3): who may read the Terraform mirror and who may write to
//! Conan, Galaxy and Hugging Face, plus the HTTP 413 of Galaxy.
//!
//! The format suites prove the happy path with real clients. The goldfish
//! review of the format stack left the access matrix unproven on the wire,
//! so every case here is a real HTTP request against the real binary:
//!
//! - **401**: no credential, or one the hub cannot verify (and, for Galaxy,
//!   a `Bearer` where the dialect wants `Token`);
//! - **403**: a verified principal of the owning account that no policy
//!   permits (`carol`; the deployment policy only names `alice` and `bob`);
//! - **foreign account**: a verified, permitted principal of ANOTHER account
//!   (`bob`, account `other`) gets a 404 that does not say the repository
//!   exists, and a write attempt leaves nothing behind;
//! - **413**: a Galaxy publish whose gzip is tiny on the wire but expands past
//!   the 256 MiB limit of the manifest reader (a decompression bomb made of
//!   zeros, a few hundred KiB compressed);
//! - Hugging Face has no write surface at all: its writes are 405 for the
//!   owner and 401 / 404 for the rest;
//! - residual: `versions` of a collection that does not exist is a 404, not
//!   an empty 200.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use flate2::write::GzEncoder;
use serde_json::json;
use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// A raw HTTP exchange: the status line and the body, nothing interpreted.
struct Reply {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    authorization: Option<&str>,
    content_type: Option<&str>,
    body: &[u8],
) -> Reply {
    let mut head =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(value) = authorization {
        head.push_str(&format!("Authorization: {value}\r\n"));
    }
    if let Some(value) = content_type {
        head.push_str(&format!("Content-Type: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(60)))
        .expect("the timeout");
    stream.write_all(head.as_bytes()).expect("the head");
    // The server may answer 413 and close before the whole body is read.
    let _ = stream.write_all(body);
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no HTTP head in {} bytes", raw.len()));
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// The principals of the matrix.
struct Principals {
    /// The owner: acme, permitted.
    alice: String,
    /// A verified principal of the owning account that no policy permits.
    carol: String,
    /// A permitted principal of another account.
    bob: String,
}

fn the_principals() -> Principals {
    Principals {
        alice: support::token(FIRST_ACCOUNT, "alice"),
        carol: support::token(FIRST_ACCOUNT, "carol"),
        bob: support::token(SECOND_ACCOUNT, "bob"),
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn galaxy_token(token: &str) -> String {
    format!("Token {token}")
}

/// A one-page upstream for the Terraform mirror.
fn start_upstream(path: String, body: &'static [u8]) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let body = Arc::new(body);
    let path = Arc::new(path);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = Arc::clone(&body);
            let path = Arc::clone(&path);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = String::new();
                while !read.contains("\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => read.push_str(&String::from_utf8_lossy(&buffer[..n])),
                    }
                }
                let asked = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default();
                let answer = if asked == path.as_str() {
                    let mut bytes = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    bytes.extend_from_slice(&body);
                    bytes
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                };
                let _ = stream.write_all(&answer);
            });
        }
    });
    port
}

/// A ustar member: header, content and the padding of the record.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:011o}", content.len());
    bytes[124..124 + size.len()].copy_from_slice(size.as_bytes());
    bytes[148..156].copy_from_slice(b"        ");
    bytes[156] = b'0';
    bytes[257..262].copy_from_slice(b"ustar");
    let checksum: u32 = bytes.iter().map(|byte| *byte as u32).sum();
    bytes[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    let mut member = bytes;
    member.extend_from_slice(content);
    member.extend(std::iter::repeat_n(0u8, (512 - content.len() % 512) % 512));
    member
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("the gzip write");
    encoder.finish().expect("the gzip finish")
}

/// A readable collection artifact: a gzip tarball with a `MANIFEST.json`.
fn a_collection(namespace: &str, name: &str, version: &str) -> Vec<u8> {
    let manifest = json!({
        "collection_info": { "namespace": namespace, "name": name, "version": version },
        "format": 1,
    });
    let mut tar = the_member_bytes("MANIFEST.json", manifest.to_string().as_bytes());
    tar.extend_from_slice(&[0u8; 1024]);
    gzip(&tar)
}

/// A decompression bomb: a gzip stream of `inflated` zero bytes. Zeros
/// deflate about 1000:1, so 257 MiB are a few hundred KiB on the wire.
fn a_bomb(inflated: usize) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let block = vec![0u8; 1024 * 1024];
    let mut written = 0usize;
    while written < inflated {
        let take = block.len().min(inflated - written);
        encoder.write_all(&block[..take]).expect("the zeros");
        written += take;
    }
    encoder.finish().expect("the bomb is closed")
}

/// The multipart form of `ansible-galaxy collection publish`.
fn the_publish_form(artifact: &[u8]) -> (String, Vec<u8>) {
    let boundary = "hodei-hsp304";
    let digest = format!("{:x}", sha2::Sha256::digest(artifact));
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{digest}\r\n"
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"demo.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(artifact);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn publish_to_galaxy(port: u16, authorization: Option<&str>, artifact: &[u8]) -> Reply {
    let (content_type, body) = the_publish_form(artifact);
    request(
        port,
        "POST",
        "/galaxy/galaxy-hosted/api/v3/artifacts/collections/",
        authorization,
        Some(&content_type),
        &body,
    )
}

fn a_model(root: &std::path::Path) -> PathBuf {
    let file = root.join("config.json");
    std::fs::write(&file, "{\"model_type\":\"bert\"}\n").expect("the config");
    file
}

const PROVIDER: &str = "registry.terraform.io/hashicorp/null";
const CONAN_FILE: &str =
    "/conan/conan-hosted/v2/conans/hello/1.0.0/hodei/stable/revisions/abc123/files/conanfile.py";

#[test]
fn the_terraform_mirror_enforces_401_403_and_the_account_boundary() {
    let index_path = format!("/{PROVIDER}/index.json");
    let upstream = start_upstream(index_path, br#"{"versions":{"3.2.1":{}}}"#);
    let deployment = Deployment::new();
    deployment.seed_terraform_proxies(&[(
        "tf-mirror",
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{upstream}/"),
                "scopes": ["/"],
                "max_bytes": 65536,
            },
        }),
    )]);
    let allowed = format!("127.0.0.1:{upstream};private;http");
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;
    let who = the_principals();
    let path = format!("/terraform/tf-mirror/{PROVIDER}/index.json");

    let read = |authorization: Option<&str>| request(port, "GET", &path, authorization, None, b"");

    let alice = read(Some(&bearer(&who.alice)));
    assert_eq!(alice.status, 200, "the owner reads the mirror");

    assert_eq!(read(None).status, 401, "no credential: 401");
    let anonymous = read(None);
    assert_eq!(
        anonymous
            .headers
            .get("www-authenticate")
            .map(String::as_str),
        Some("Bearer"),
        "the 401 names the scheme"
    );
    assert_eq!(
        read(Some("Bearer not-a-token")).status,
        401,
        "a credential the hub cannot verify: 401"
    );
    assert_eq!(
        read(Some(&bearer(&who.carol))).status,
        403,
        "a verified principal of the account that no policy permits: 403"
    );
    let stranger = read(Some(&bearer(&who.bob)));
    assert_eq!(
        stranger.status, 404,
        "another account does not see the mirror, and the 404 says nothing about it"
    );
    assert_ne!(stranger.body, alice.body);

    // The mirror is read-only for everybody who is allowed to read.
    let put = request(port, "PUT", &path, Some(&bearer(&who.alice)), None, b"x");
    assert_eq!(put.status, 405, "a mirror has no writes");
    support::stop(&mut service);
}

#[test]
fn the_write_surfaces_enforce_401_403_and_the_account_boundary() {
    let deployment = Deployment::new();
    deployment.seed_repositories(&[
        json!({ "key": "conan-hosted", "conan": null, "format": "conan" }),
        json!({ "key": "galaxy-hosted", "galaxy": null, "format": "galaxy" }),
    ]);
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![(
            "artifacts/hodei/demo-model/config.json",
            a_model(deployment.directory.path()),
        )],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let who = the_principals();
    let recipe = b"from conan import ConanFile\n".as_slice();

    // ---- Conan: PUT of a recipe file ------------------------------------
    let put = |authorization: Option<&str>| {
        request(
            port,
            "PUT",
            CONAN_FILE,
            authorization,
            Some("application/octet-stream"),
            recipe,
        )
    };
    assert_eq!(put(None).status, 401, "conan: no credential");
    assert_eq!(put(Some("Bearer garbage")).status, 401, "conan: bad token");
    assert_eq!(
        put(Some(&bearer(&who.carol))).status,
        403,
        "conan: verified but not permitted"
    );
    assert_eq!(
        put(Some(&bearer(&who.bob))).status,
        404,
        "conan: another account cannot write into the repository of the first"
    );
    let missing = request(
        port,
        "GET",
        CONAN_FILE,
        Some(&bearer(&who.alice)),
        None,
        b"",
    );
    assert_eq!(
        missing.status, 404,
        "conan: none of the rejected writes left a file behind"
    );
    assert_eq!(put(Some(&bearer(&who.alice))).status, 200, "conan: owner");
    let stored = request(
        port,
        "GET",
        CONAN_FILE,
        Some(&bearer(&who.alice)),
        None,
        b"",
    );
    assert_eq!(stored.status, 200);
    assert_eq!(stored.body, recipe);
    // The credential exchange of the dialect: a wrong password is 401.
    let basic = |password: &str| {
        use base64::Engine as _;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("alice:{password}"))
        )
    };
    let login = |authorization: &str| {
        request(
            port,
            "GET",
            "/conan/conan-hosted/v2/users/authenticate",
            Some(authorization),
            None,
            b"",
        )
    };
    assert_eq!(login(&basic("wrong")).status, 401, "conan: wrong password");
    assert_eq!(login(&basic(&who.alice)).status, 200, "conan: right one");

    // ---- Galaxy: publish -------------------------------------------------
    let artifact = a_collection("hodei", "demo", "1.0.0");
    assert_eq!(
        publish_to_galaxy(port, None, &artifact).status,
        401,
        "galaxy: no credential"
    );
    assert_eq!(
        publish_to_galaxy(port, Some(&galaxy_token("garbage")), &artifact).status,
        401,
        "galaxy: bad key"
    );
    assert_eq!(
        publish_to_galaxy(port, Some(&bearer(&who.alice)), &artifact).status,
        401,
        "galaxy: the dialect wants `Token`, a Bearer is not the galaxy credential"
    );
    assert_eq!(
        publish_to_galaxy(port, Some(&galaxy_token(&who.carol)), &artifact).status,
        403,
        "galaxy: verified but not permitted"
    );
    assert_eq!(
        publish_to_galaxy(port, Some(&galaxy_token(&who.bob)), &artifact).status,
        404,
        "galaxy: another account cannot publish into the repository of the first"
    );
    let versions = "/galaxy/galaxy-hosted/api/v3/collections/hodei/demo/versions/";
    let galaxy_alice = galaxy_token(&who.alice);
    let listing = |path: &str| request(port, "GET", path, Some(&galaxy_alice), None, b"");
    assert_eq!(
        listing(versions).status,
        404,
        "galaxy: the rejected publishes left no collection behind, and the versions of a \
         collection that does not exist are a 404, not an empty 200"
    );
    assert_eq!(
        publish_to_galaxy(port, Some(&galaxy_alice), &artifact).status,
        200,
        "galaxy: owner"
    );
    assert_eq!(listing(versions).status, 200, "galaxy: now it is there");

    // ---- Galaxy: the 413 of the decompression bomb -------------------------
    let limit = 256 * 1024 * 1024;
    let bomb = a_bomb(limit + 1024 * 1024);
    assert!(
        bomb.len() < 4 * 1024 * 1024,
        "the bomb is small on the wire: {} bytes",
        bomb.len()
    );
    let refused = publish_to_galaxy(port, Some(&galaxy_alice), &bomb);
    assert_eq!(
        refused.status,
        413,
        "galaxy: a gzip that expands past 256 MiB is refused as too large: {}",
        String::from_utf8_lossy(&refused.body)
    );
    // Just under the limit is not a bomb: it is merely not a collection.
    let heavy = a_bomb(limit / 2);
    assert_eq!(
        publish_to_galaxy(port, Some(&galaxy_alice), &heavy).status,
        400,
        "galaxy: below the limit the artifact is read, and it is not a collection"
    );

    // ---- Hugging Face: there is no write surface -----------------------------
    let hf = "/hf/hf-hosted/hodei/demo-model/resolve/main/config.json";
    for method in ["PUT", "POST", "DELETE"] {
        let write = |authorization: Option<&str>| {
            request(
                port,
                method,
                hf,
                authorization,
                Some("application/json"),
                b"{}",
            )
        };
        assert_eq!(write(None).status, 401, "hf {method}: no credential");
        assert_eq!(
            write(Some(&bearer(&who.alice))).status,
            405,
            "hf {method}: not even the owner writes"
        );
        assert_eq!(
            write(Some(&bearer(&who.bob))).status,
            404,
            "hf {method}: another account does not even see the repository"
        );
    }
    assert_eq!(
        request(port, "GET", hf, Some(&bearer(&who.carol)), None, b"").status,
        403,
        "hf: a read by a verified but not permitted principal"
    );
    assert_eq!(
        request(port, "GET", hf, Some("Bearer garbage"), None, b"").status,
        401,
        "hf: a read with a bad token"
    );
    assert_eq!(
        request(port, "GET", hf, Some(&bearer(&who.bob)), None, b"").status,
        404,
        "hf: a read by another account"
    );
    assert_ne!(
        request(port, "GET", hf, Some(&bearer(&who.alice)), None, b"").status,
        404,
        "hf: the owner does see the model"
    );
    support::stop(&mut service);
}
