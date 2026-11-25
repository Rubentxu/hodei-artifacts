//! A prefix scan must stop at the segment boundary.
//!
//! The additional formats list what lives under a coordinate by scanning
//! the catalog for a path prefix. A prefix without its trailing slash also
//! matches the sibling that merely starts with the same letters: the
//! channel `c2` under `c`, the model `org/bert` under `org/ber`, the
//! collection `foobar` under `foo`. These tests publish both siblings and
//! ask for one of them.

mod support;

use std::io::{Read, Write};
use std::path::PathBuf;

use sha2::Digest as _;
use support::{Curl, Deployment, FIRST_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn get(deployment: &Deployment, port: u16, path: &str, token: &str) -> (u16, String) {
    let into = deployment.directory.path().join("response.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::call(port, "GET", path, token, None, None, Some(&into)).run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

fn put(deployment: &Deployment, port: u16, path: &str, token: &str, body: &[u8]) -> u16 {
    let sent = deployment.directory.path().join("request.bin");
    std::fs::write(&sent, body).expect("the body");
    let into = deployment.directory.path().join("put.bin");
    Curl::call(
        port,
        "PUT",
        path,
        token,
        Some(sent.as_path()),
        Some("application/octet-stream"),
        Some(&into),
    )
    .run()
    .status
}

#[test]
fn a_conan_channel_does_not_list_the_revisions_of_a_longer_channel() {
    let deployment = Deployment::new();
    deployment.seed_conan(&[("conan-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let upload = |channel: &str, revision: &str| {
        put(
            &deployment,
            port,
            &format!(
                "/conan/conan-hosted/v2/conans/a/1.0/_/{channel}/revisions/{revision}/files/conanfile.py"
            ),
            &token,
            channel.as_bytes(),
        )
    };
    assert_eq!(upload("c", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), 200);
    assert_eq!(upload("c2", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"), 200);

    let (status, body) = get(
        &deployment,
        port,
        "/conan/conan-hosted/v2/conans/a/1.0/_/c/revisions",
        &token,
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), "{body}");
    assert!(
        !body.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        "the revisions of c must not include the ones of c2: {body}"
    );
    support::stop(&mut service);
}

#[test]
fn an_hf_model_does_not_list_the_files_of_a_longer_model_name() {
    let deployment = Deployment::new();
    let directory = deployment.directory.path().join("hf-files");
    std::fs::create_dir_all(&directory).expect("the directory");
    let short: PathBuf = directory.join("short.json");
    let long: PathBuf = directory.join("long.json");
    std::fs::write(&short, "{\"model\":\"ber\"}").expect("the file");
    std::fs::write(&long, "{\"model\":\"bert\"}").expect("the file");
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/org/ber/config.json", short),
            ("artifacts/org/bert/config_of_bert.json", long),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let (status, body) = get(
        &deployment,
        port,
        "/hf/hf-hosted/api/models/org/ber/revision/main",
        &token,
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("config.json"), "{body}");
    assert!(
        !body.contains("config_of_bert"),
        "the model ber must not list the files of bert: {body}"
    );
    // A model that is only the start of another name is not a model.
    let (status, body) = get(
        &deployment,
        port,
        "/hf/hf-hosted/api/models/org/be/revision/main",
        &token,
    );
    assert_eq!(status, 404, "org/be is not org/ber nor org/bert: {body}");
    support::stop(&mut service);
}

/// A ustar member: a 512-byte header, the bytes, and the padding.
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

fn collection(namespace: &str, name: &str) -> Vec<u8> {
    let manifest = serde_json::json!({
        "collection_info": {
            "namespace": namespace, "name": name, "version": "1.0.0", "dependencies": {}
        },
        "format": 1,
    });
    let mut tar = tar_member("MANIFEST.json", manifest.to_string().as_bytes());
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).expect("the gzip");
    gzip.finish().expect("the gzip end")
}

fn publish(port: u16, token: &str, bytes: &[u8]) -> u16 {
    let sha256: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let boundary = "hodei-hsp297";
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{sha256}\r\n"
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"c.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(bytes);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    let head = format!(
        "POST /galaxy/galaxy-hosted/api/v3/artifacts/collections/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Token {token}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line")
}

#[test]
fn a_galaxy_collection_does_not_list_the_versions_of_a_longer_name() {
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    assert_eq!(publish(port, &token, &collection("acme", "foo")), 200);
    assert_eq!(publish(port, &token, &collection("acme", "foobar")), 200);

    let into = deployment.directory.path().join("versions.bin");
    let authorization = format!("Token {token}");
    let answer = Curl::call_with_headers(
        port,
        "GET",
        "/galaxy/galaxy-hosted/api/v3/collections/acme/foo/versions/",
        "",
        None,
        &[("authorization", authorization.as_str())],
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200);
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&into).expect("the body")).expect("json");
    assert_eq!(
        document["meta"]["count"], 1,
        "foo has one version; foobar is another collection: {document}"
    );
    let text = document.to_string();
    assert!(!text.contains("foobar"), "{text}");

    // A name that is only the start of another one is not a collection.
    let into = deployment.directory.path().join("versions-fo.bin");
    let answer = Curl::call_with_headers(
        port,
        "GET",
        "/galaxy/galaxy-hosted/api/v3/collections/acme/fo/",
        "",
        None,
        &[("authorization", authorization.as_str())],
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 404, "acme.fo is neither foo nor foobar");
    support::stop(&mut service);
}
