//! The listings and the metadata of the additional formats go through the
//! trust gate.
//!
//! In the trusted channel a digest nobody promoted is hidden as if it did
//! not exist. A document that names a digest, a size or the existence of
//! that content -- a version listing, a revision listing, a search, the
//! download metadata, the HEAD of a resolve -- tells what the bytes gate
//! is hiding, so the documents are gated as the bytes are. The content is
//! published (or seeded) into a trusted repository and never promoted.

mod support;

use std::io::{Read, Write};

use serde_json::{Value, json};
use sha2::Digest as _;
use support::{Curl, Deployment, FIRST_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn request(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let file = body.map(|bytes| {
        std::fs::write(&sent, bytes).expect("the body");
        sent.as_path()
    });
    let answer = if method == "HEAD" {
        Curl::describe(port, path, "").run()
    } else {
        Curl::call_with_headers(port, method, path, "", file, headers, Some(&into)).run()
    };
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[test]
fn a_trusted_conan_repository_lists_nothing_that_nobody_promoted() {
    let deployment = Deployment::new();
    deployment.seed_format_trusted("conan", &[("conan-trusted", Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let authorization = bearer(&support::token(FIRST_ACCOUNT, "alice"));
    let headers = [
        ("authorization", authorization.as_str()),
        ("content-type", "application/octet-stream"),
    ];
    let (status, _) = request(
        &deployment,
        port,
        "PUT",
        "/conan/conan-trusted/v2/conans/lib/1.0/_/_/revisions/0123456789abcdef0123456789abcdef/files/conanfile.py",
        &headers,
        Some(b"from conan import ConanFile\n"),
    );
    assert_eq!(
        status, 200,
        "the upload is accepted into the trusted channel"
    );

    for (path, what) in [
        ("/v2/conans/search?q=*", "the search"),
        ("/v2/conans/lib/1.0/_/_/revisions", "the recipe revisions"),
        ("/v2/conans/lib/1.0/_/_/latest", "the latest revision"),
        (
            "/v2/conans/lib/1.0/_/_/revisions/0123456789abcdef0123456789abcdef/files",
            "the files of the revision",
        ),
    ] {
        let (status, body) = request(
            &deployment,
            port,
            "GET",
            &format!("/conan/conan-trusted{path}"),
            &headers[..1],
            None,
        );
        assert!(
            !body.contains("0123456789abcdef") && !body.contains("lib/1.0"),
            "{what} exposes the unpromoted recipe (status {status}): {body}"
        );
    }
    support::stop(&mut service);
}

fn tar_member(name: &str, content: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    let mut member = header.to_vec();
    member.extend_from_slice(content);
    member.extend(std::iter::repeat_n(0u8, (512 - content.len() % 512) % 512));
    member
}

fn gzip(tar: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(tar).expect("the gzip");
    encoder.finish().expect("the gzip end")
}

fn sha256_of(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn raw_post(port: u16, head: &str, body: &[u8]) -> (u16, String) {
    let request = format!(
        "{head}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.write_all(body);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let status = answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    (
        status,
        answer
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string(),
    )
}

#[test]
fn a_trusted_galaxy_repository_does_not_name_an_unpromoted_version() {
    let deployment = Deployment::new();
    deployment.seed_format_trusted("galaxy", &[("galaxy-trusted", Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let manifest = json!({
        "collection_info": {
            "namespace": "acme", "name": "demo", "version": "1.0.0", "dependencies": {}
        },
        "format": 1,
    });
    let mut tar = tar_member("MANIFEST.json", manifest.to_string().as_bytes());
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let artifact = gzip(&tar);
    let boundary = "hodei-hsp297";
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{}\r\n",
        sha256_of(&artifact)
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"c.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(&artifact);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    let (status, answer) = raw_post(
        port,
        &format!(
            "POST /galaxy/galaxy-trusted/api/v3/artifacts/collections/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Token {token}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\n"
        ),
        &body,
    );
    assert_eq!(status, 200, "the publish is accepted: {answer}");

    let authorization = format!("Token {token}");
    let headers = [("authorization", authorization.as_str())];
    let digest = sha256_of(&artifact);
    for path in [
        "/api/v3/collections/acme/demo/versions/1.0.0/",
        "/api/v3/collections/acme/demo/versions/",
    ] {
        let (status, body) = request(
            &deployment,
            port,
            "GET",
            &format!("/galaxy/galaxy-trusted{path}"),
            &headers,
            None,
        );
        assert!(
            !body.contains(&digest) && (status == 404 || !body.contains("\"version\"")),
            "{path} exposes the unpromoted version (status {status}): {body}"
        );
    }
    support::stop(&mut service);
}

#[test]
fn a_trusted_terraform_registry_does_not_name_an_unpromoted_module() {
    let deployment = Deployment::new();
    deployment.seed_format_trusted("terraform", &[("tf-trusted", Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let mut tar = tar_member("main.tf", b"# demo\n");
    tar.extend(std::iter::repeat_n(0u8, 512));
    let artifact = gzip(&tar);
    let (status, answer) = raw_post(
        port,
        &format!(
            "POST /terraform/tf-trusted/publish/hodei/demo/local/1.0.0 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/gzip\r\n"
        ),
        &artifact,
    );
    assert!(status < 300, "the publish is accepted ({status}): {answer}");

    let authorization = bearer(&token);
    let headers = [("authorization", authorization.as_str())];
    let digest = sha256_of(&artifact);
    for path in [
        "/v1/modules/hodei/demo/local/versions",
        "/v1/modules/hodei/demo/local/1.0.0/download",
    ] {
        let (status, body) = request(
            &deployment,
            port,
            "GET",
            &format!("/terraform/tf-trusted{path}"),
            &headers,
            None,
        );
        assert!(
            status == 404 || (!body.contains("1.0.0") && !body.contains(&digest)),
            "{path} exposes the unpromoted module (status {status}): {body}"
        );
        assert_ne!(
            status, 204,
            "{path} announces the download of an unpromoted module"
        );
    }
    support::stop(&mut service);
}

#[test]
fn a_trusted_hf_repository_does_not_resolve_an_unpromoted_file() {
    let deployment = Deployment::new();
    let directory = deployment.directory.path().join("hf-files");
    std::fs::create_dir_all(&directory).expect("the directory");
    let config = directory.join("config.json");
    std::fs::write(&config, "{\"model_type\":\"bert\"}").expect("the file");
    deployment.seed_hf_trusted(
        &["hf-trusted"],
        &[(
            "hf-trusted",
            vec![("artifacts/org/ber/config.json", config)],
        )],
    );
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let (status, body) = request(
        &deployment,
        port,
        "GET",
        "/hf/hf-trusted/api/models/org/ber/revision/main",
        &[("authorization", bearer(&token).as_str())],
        None,
    );
    assert!(
        !body.contains("config.json"),
        "the revision lists an unpromoted file (status {status}): {body}"
    );
    let head = Curl::describe(
        port,
        "/hf/hf-trusted/org/ber/resolve/main/config.json",
        &token,
    )
    .run();
    assert_eq!(
        head.status, 404,
        "the HEAD of the resolve must not redirect to an unpromoted blob"
    );
    support::stop(&mut service);
}
