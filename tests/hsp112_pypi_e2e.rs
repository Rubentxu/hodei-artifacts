//! PyPI hosted, proxy and group of HSP-112, at the HTTP level.
//!
//! What is tested: the upload with the protocol `twine` speaks, the
//! index that negotiates HTML and JSON, the normalized name, the hash that is
//! checked against the bytes that arrived, the *yanked* state of a
//! version, the group that joins the projects of its members and the proxy
//! that brings the index from its upstream without returning the upstream's HTML.
//!
//! The real client (`twine` and `pip`) is in `hsp112_pypi_cli.rs`.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The upload separator.
const BOUNDARY: &str = "hodeipypiboundary";

/// The body of a `twine` upload: the fields and the file.
///
/// The fields are those `twine` sends: the action, the protocol
/// version, the name, the version and the hash.
fn the_upload(name: &str, version: &str, filename: &str, file: &[u8], sha256: &str) -> Vec<u8> {
    the_upload_with(name, version, filename, file, sha256, None)
}

/// The body of an upload, signed if requested.
///
/// `twine upload --sign` sends the signature as another part with a file
/// name: it is not a second distribution.
fn the_upload_with(
    name: &str,
    version: &str,
    filename: &str,
    file: &[u8],
    sha256: &str,
    signature: Option<&str>,
) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", name),
        ("version", version),
        ("metadata_version", "2.1"),
        ("requires_python", ">=3.9"),
        ("sha256_digest", sha256),
    ] {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\n\
                 Content-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\n\
             Content-Disposition: form-data; name=\"content\"; filename=\"{filename}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n");
    if let Some(signature) = signature {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\n\
                 Content-Disposition: form-data; name=\"gpg_signature\"; filename=\"{filename}.asc\"\r\n\
                 Content-Type: application/pgp-signature\r\n\r\n{signature}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// The hash of some bytes, in hexadecimal.
fn the_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A request to the service.
///
/// The body is always left in a file: a response without a body does not
/// say **what** the service served, it only says that it said something.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    extra: &[(&str, &str)],
    into: Option<&Path>,
) -> Answer {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let target = match into {
        Some(file) => file.to_path_buf(),
        None => dir.join(format!("answer-{n}.bin")),
    };
    Curl::call_with_headers(port, method, path, token, None, extra, Some(&target)).run()
}

/// An upload to the service.
fn upload(dir: &Path, port: u16, repository: &str, token: &str, body: Vec<u8>) -> Answer {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let sent = dir.join(format!("upload-{n}.bin"));
    let into = dir.join(format!("upload-answer-{n}.bin"));
    std::fs::write(&sent, body).expect("the upload body is written");
    Curl::call(
        port,
        "POST",
        &format!("/pypi/{repository}/"),
        token,
        Some(&sent),
        Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
        Some(&into),
    )
    .run()
}

/// The JSON of a response.
fn json_of(answer: &Answer) -> Value {
    serde_json::from_slice(&answer.body).unwrap_or_else(|error| {
        panic!(
            "the body is not JSON ({error}): {}",
            String::from_utf8_lossy(&answer.body)
        )
    })
}

/// The text of a response.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

#[test]
fn a_distribution_is_published_and_served_in_the_index() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", Some(">=3.9"));
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    assert_eq!(json_of(&published)["ok"], json!(true));

    // The JSON index: the hash, the URL and the Python version.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[(
            "accept",
            "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
        )],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(
        page.header("content-type").as_deref(),
        Some("application/vnd.pypi.simple.v1+json")
    );
    let document = json_of(&page);
    assert_eq!(document["name"], json!("labpkg"));
    assert_eq!(document["meta"]["api-version"], json!("1.0"));
    assert_eq!(document["versions"], json!(["1.0"]));
    let files = document["files"].as_array().expect("the files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], json!("labpkg-1.0-py3-none-any.whl"));
    assert_eq!(files[0]["hashes"]["sha256"], json!(hash));
    assert_eq!(files[0]["requires-python"], json!(">=3.9"));
    assert_eq!(files[0]["yanked"], json!(false));
    let url = files[0]["url"].as_str().expect("the url");
    assert!(url.contains("/pypi/pypi-hosted/files/labpkg/"), "{url}");

    // The HTML index: the same hash, in the fragment.
    let html = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    assert_eq!(html.status, 200);
    assert_eq!(
        html.header("content-type").as_deref(),
        Some("text/html; charset=utf-8")
    );
    let page = text_of(&html);
    assert!(page.contains(&format!("#sha256={hash}")), "{page}");
    assert!(page.contains("data-requires-python=\"&gt;=3.9\""), "{page}");

    // The index carries its ETag, and with it it is not served again.
    let etag = html.header("etag").expect("the index has an ETag");
    let again = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html"), ("if-none-match", &etag)],
        None,
    );
    assert_eq!(again.status, 304);

    // The file is downloaded as it was published.
    let into = dir.join("downloaded.whl");
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200);
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);
}

#[test]
fn the_project_name_is_normalized_the_same_way_everywhere() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    // The metadata name and the file name are not the same: both are
    // the same project.
    let wheel = support::a_wheel("Lab.Pkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "Lab.Pkg",
            "1.0",
            "lab_pkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    for name in ["lab-pkg", "lab_pkg", "Lab.Pkg", "LAB--PKG"] {
        let page = call(
            dir,
            port,
            "GET",
            &format!("/pypi/pypi-hosted/simple/{name}/"),
            &token,
            &[("accept", "application/vnd.pypi.simple.v1+json")],
            None,
        );
        assert_eq!(
            page.status,
            200,
            "the project {name} is not served: {}",
            text_of(&page)
        );
        assert_eq!(json_of(&page)["name"], json!("lab-pkg"));
    }
    // A project that does not exist is not invented.
    let missing = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[],
        None,
    );
    assert_eq!(missing.status, 404);
}

#[test]
fn an_upload_whose_hash_is_not_the_one_of_its_content_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let lying = "0".repeat(64);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &lying,
        ),
    );
    assert_eq!(published.status, 400, "{}", text_of(&published));
    // What was not published does not appear in the index.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[],
        None,
    );
    assert_eq!(page.status, 404);
}

#[test]
fn an_upload_that_carries_two_distributions_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    // Two file parts: one is not chosen, it is refused.
    let mut body = the_upload(
        "labpkg",
        "1.0",
        "labpkg-1.0-py3-none-any.whl",
        &wheel,
        &hash,
    );
    let second = format!(
        "--{BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"content\"; filename=\"labpkg-2.0-py3-none-any.whl\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    );
    let end = body.len() - format!("\r\n--{BOUNDARY}--\r\n").len();
    body.truncate(end);
    body.extend_from_slice(second.as_bytes());
    body.extend_from_slice(&wheel);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    let published = upload(dir, port, "pypi-hosted", &token, body);
    assert_eq!(published.status, 400, "{}", text_of(&published));
}

#[test]
fn a_yanked_version_is_marked_and_still_downloadable() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    for version in ["1.0", "2.0"] {
        let wheel = support::a_wheel("labpkg", version, None);
        let hash = the_sha256(&wheel);
        let published = upload(
            dir,
            port,
            "pypi-hosted",
            &token,
            the_upload(
                "labpkg",
                version,
                &format!("labpkg-{version}-py3-none-any.whl"),
                &wheel,
                &hash,
            ),
        );
        assert_eq!(published.status, 200, "{}", text_of(&published));
    }

    // Yank version 2.0.
    let into = dir.join("yank.json");
    std::fs::write(
        &into,
        serde_json::to_vec(&json!({
            "project": "labpkg",
            "version": "2.0",
            "yanked": true,
            "reason": "the wheel was broken",
        }))
        .expect("the body"),
    )
    .expect("the body is written");
    let yank = Curl::call(
        port,
        "POST",
        "/pypi/pypi-hosted/yank",
        &token,
        Some(&into),
        Some("application/json"),
        Some(&dir.join("yank-answer.bin")),
    )
    .run();
    assert_eq!(yank.status, 200, "{}", text_of(&yank));

    // The index says 2.0 is yanked, and 1.0 is not.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200);
    let document = json_of(&page);
    let files = document["files"].as_array().expect("the files");
    let yanked: Vec<&Value> = files
        .iter()
        .filter(|file| file["yanked"].is_string())
        .collect();
    assert_eq!(yanked.len(), 1, "{document}");
    assert_eq!(yanked[0]["filename"], json!("labpkg-2.0-py3-none-any.whl"));
    assert_eq!(yanked[0]["yanked"], json!("the wheel was broken"));

    // And the HTML says it the other way.
    let html = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    let page = text_of(&html);
    assert!(
        page.contains("data-yanked=\"the wheel was broken\""),
        "{page}"
    );

    // What is yanked can still be downloaded: exactly that version is requested.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-2.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 200);
}

#[test]
fn a_group_joins_the_projects_of_its_members() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-one", Value::Null), ("pypi-two", Value::Null)]);
    deployment.seed_pypi_groups(&[("pypi-all", &["pypi-one", "pypi-two"])]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    for (repository, name) in [("pypi-one", "onepkg"), ("pypi-two", "twopkg")] {
        let wheel = support::a_wheel(name, "1.0", None);
        let hash = the_sha256(&wheel);
        let published = upload(
            dir,
            port,
            repository,
            &token,
            the_upload(
                name,
                "1.0",
                &format!("{name}-1.0-py3-none-any.whl"),
                &wheel,
                &hash,
            ),
        );
        assert_eq!(published.status, 200, "{}", text_of(&published));
    }

    // The group has both projects.
    let index = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-all/simple/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(index.status, 200);
    let document = json_of(&index);
    let names: Vec<&str> = document["projects"]
        .as_array()
        .expect("the projects")
        .iter()
        .filter_map(|project| project["name"].as_str())
        .collect();
    assert!(names.contains(&"onepkg"), "{document}");
    assert!(names.contains(&"twopkg"), "{document}");

    // And the file comes from the member that has it.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-all/files/twopkg/twopkg-1.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
}

#[test]
fn a_proxy_serves_the_index_of_its_upstream_without_its_html() {
    let deployment = Deployment::new();
    let root = deployment.directory.path();

    // The upstream serves the Simple API JSON when JSON is requested,
    // and its own HTML when not. What comes out of here is always
    // ours: the upstream's HTML is not returned, and its values are
    // escaped.
    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let upstream = start_upstream_on(|port| {
        let mut pages: Pages = HashMap::new();
        pages.insert(
            "/simple/labpkg".to_string(),
            (
                serde_json::to_vec(&json!({
                    "meta": {"api-version": "1.0"},
                    "name": "labpkg",
                    "files": [{
                        "filename": "labpkg-1.0-py3-none-any.whl",
                        "url": format!("http://127.0.0.1:{port}/packages/labpkg-1.0-py3-none-any.whl"),
                        "hashes": {"sha256": hash},
                        "requires-python": "\"><script>alert(1)</script>",
                    }],
                }))
                .expect("the document"),
                b"<html><body><script>alert('the upstream')</script></body></html>".to_vec(),
            ),
        );
        pages.insert(
            "/packages/labpkg-1.0-py3-none-any.whl".to_string(),
            (wheel.clone(), wheel.clone()),
        );
        pages
    });
    let port = upstream.port;

    deployment.seed_pypi_proxies(&[(
        "pypi-central",
        json!({
            "upstream": {
                "id": "central",
                "url": format!("http://127.0.0.1:{port}/simple/"),
                "scopes": ["/"],
                "max_bytes": 1048576,
            },
        }),
    )]);
    let allowed = format!("127.0.0.1:{port};private;http");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let service_port = service.port;
    let dir = deployment.directory.path();

    // The proxy index is written by the service, not by the upstream.
    let page = call(
        dir,
        service_port,
        "GET",
        "/pypi/pypi-central/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    let html = text_of(&page);
    assert!(
        !html.contains("<script>"),
        "the upstream wrote HTML: {html}"
    );
    assert!(html.contains("&lt;script&gt;"), "{html}");
    // And the HTML is ours: the URL belongs to this surface.
    assert!(html.contains("/pypi/pypi-central/files/labpkg/"), "{html}");

    // The file comes from the upstream, and its hash is the one the index said.
    let into = root.join("from-proxy.whl");
    let download = call(
        dir,
        service_port,
        "GET",
        "/pypi/pypi-central/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);

    // The upstream received one request per path: the project page was
    // requested once, and the file download did not request it again.
    assert_eq!(upstream.asked_for("/simple/labpkg"), 1);
    assert_eq!(
        upstream.asked_for("/packages/labpkg-1.0-py3-none-any.whl"),
        1
    );
}

#[test]
fn a_write_without_a_credential_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        "",
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 401);
}

#[test]
fn a_proxy_does_not_accept_writes() {
    let deployment = Deployment::new();
    deployment.seed_pypi_proxies(&[(
        "pypi-central",
        json!({
            "upstream": {
                "id": "central",
                "url": "http://127.0.0.1:1/simple/",
                "scopes": ["/"],
            },
        }),
    )]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-central",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 405);
}

/// A distribution of more than two megabytes is published.
///
/// The web server's default limit is two megabytes, and a
/// real distribution does not fit in two. The limit that rules is the
/// configured one, and the service enforces it.
#[test]
fn a_distribution_bigger_than_two_megabytes_is_published() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_big_wheel("bigpkg", "1.0", None, 3 * 1024 * 1024);
    assert!(wheel.len() > 2 * 1024 * 1024, "the wheel is not big enough");
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "bigpkg",
            "1.0",
            "bigpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // And it is downloaded just as it was published.
    let into = dir.join("big.whl");
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/bigpkg/bigpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200);
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);
}

/// A range of the content is served as a range.
///
/// Today's `pip` asks for a range so as not to download the same thing twice, and a
/// range that does not fit is reported as not fitting.
#[test]
fn a_range_asks_for_a_part_of_the_distribution() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The beginning of the file.
    let into = dir.join("part.whl");
    let part = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[("range", "bytes=0-9")],
        Some(&into),
    );
    assert_eq!(part.status, 206);
    assert_eq!(
        part.header("content-range").as_deref(),
        Some(format!("bytes 0-9/{}", wheel.len()).as_str())
    );
    assert_eq!(std::fs::read(&into).expect("the part"), wheel[..10]);

    // A range that does not fit: it is refused.
    let impossible = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[("range", "bytes=99999999-")],
        None,
    );
    assert_eq!(impossible.status, 416);
    // And it says how much there is: RFC 9110 requires the `Content-Range` of an
    // unsatisfiable range to state the length of what exists.
    assert_eq!(
        impossible.header("content-range").as_deref(),
        Some(format!("bytes */{}", wheel.len()).as_str())
    );
}

/// An upload that exceeds the limit is refused.
///
/// The limit is applied in two layers: the transport caps the whole body
/// --the `multipart` envelope included-- and the handler caps the bytes of the
/// file. Both answer `413`.
#[test]
fn an_upload_bigger_than_the_limit_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    // The configured limit is 64 KiB.
    let service = deployment.start(&[("HODEI_SERVER__MAX_BODY_SIZE", "65536")]);
    let port = service.port;
    let dir = deployment.directory.path();

    // A file that exceeds the limit: the handler says so, as it is the one that
    // counts the file's bytes.
    let wheel = support::a_big_wheel("bigpkg", "1.0", None, 128 * 1024);
    let hash = the_sha256(&wheel);
    let refused = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "bigpkg",
            "1.0",
            "bigpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(refused.status, 413, "{}", text_of(&refused));

    // An envelope with a giant header is not counted by the handler--which
    // only counts the field bytes--: the transport cuts it off. Without
    // that cap, the upload would be published: the `413` is the proof that the
    // whole body has a limit.
    let wheel = support::a_wheel("hugepkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let cut = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload_with_a_giant_header(
            "hugepkg",
            "1.0",
            "hugepkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(cut.status, 413, "{}", text_of(&cut));

    // And nothing was published.
    let index = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/",
        &token,
        &[],
        None,
    );
    assert_eq!(index.status, 200);
    assert!(!text_of(&index).contains("bigpkg"), "{}", text_of(&index));
    assert!(!text_of(&index).contains("hugepkg"), "{}", text_of(&index));
}

/// An upload whose envelope carries a giant header.
///
/// The part's header is not a field: the handler does not count it, and
/// without a cap at the transport it would accumulate in memory without limit.
fn the_upload_with_a_giant_header(
    name: &str,
    version: &str,
    filename: &str,
    file: &[u8],
    sha256: &str,
) -> Vec<u8> {
    let body = the_upload(name, version, filename, file, sha256);
    // A part with a huge header is prepended: the envelope has to
    // weigh more than the transport limit.
    let giant = format!(
        "--{BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"{}\"\r\n\r\nx\r\n",
        "x".repeat(1024 * 1024)
    );
    let mut with_giant = Vec::new();
    with_giant.extend_from_slice(giant.as_bytes());
    with_giant.extend_from_slice(&body);
    with_giant
}

/// A wheel name that `pip` installs is published.
///
/// The build tag of a wheel is a digit followed by whatever
/// else: `1.5` is a valid tag and `pip` installs it. A name that
/// `pip` would install is not rejected at the door.
#[test]
fn a_wheel_with_a_build_tag_is_published() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-1.5-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // And the index announces it under that name.
    let index = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[],
        None,
    );
    assert_eq!(index.status, 200);
    assert!(
        text_of(&index).contains("labpkg-1.0-1.5-py3-none-any.whl"),
        "{}",
        text_of(&index)
    );
}

/// A local version is canonicalized as the PEP says.
///
/// `1.0+01` and `1.0+1` are the same version: the client may write the
/// first and the file name carry the second.
#[test]
fn a_local_version_is_canonicalized() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0+01",
            "labpkg-1.0+1-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The index announces the canonical version.
    let index = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(index.status, 200);
    let document = json_of(&index);
    let versions = document["versions"].as_array().expect("the versions");
    assert_eq!(
        versions,
        &vec![json!("1.0+1")],
        "{}",
        String::from_utf8_lossy(&index.body)
    );
}

/// The trust channel does not announce what it does not let serve.
///
/// The `trusted` channel only serves digests with an active grant. An
/// index that announces a distribution that cannot be downloaded is a
/// broken index.
#[test]
fn the_trusted_channel_does_not_announce_what_it_does_not_serve() {
    let deployment = Deployment::new();
    deployment.seed_pypi_trusted(&["pypi-trusted"], &[("pypi-trusted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-trusted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The index does not announce the distribution: the channel does not admit it.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-trusted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    let document = json_of(&page);
    let files = document["files"].as_array().expect("the files");
    assert!(files.is_empty(), "the index announces what it cannot serve");

    // And the distribution is not downloaded.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-trusted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 404);
}

/// The version is compared in its canonical form.
///
/// The client may write the version however it likes and the file
/// name carries it canonical: `1.0-2` and `1.0.post2` are the same version.
#[test]
fn a_version_that_is_not_canonical_is_accepted() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let sdist = support::an_sdist("labpkg", "1.0.post2", None);
    let hash = the_sha256(&sdist);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload("labpkg", "1.0-2", "labpkg-1.0.post2.tar.gz", &sdist, &hash),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // And the index says it with the canonical version.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(json_of(&page)["versions"], json!(["1.0.post2"]));
}

/// A signed upload is not taken for two distributions.
///
/// `twine upload --sign` sends the signature as another part with a file
/// name. It is not a second distribution: the surface does not store
/// signatures, and a signed upload is published like the others.
#[test]
fn a_signed_upload_is_not_taken_for_two_distributions() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload_with(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
            Some("-----BEGIN PGP SIGNATURE-----\nnot-a-signature\n-----END PGP SIGNATURE-----\n"),
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // And the distribution is served like the others.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(json_of(&page)["versions"], json!(["1.0"]));
}

/// The pages of an upstream: the JSON one and the HTML one.
type Pages = HashMap<String, (Vec<u8>, Vec<u8>)>;

/// A laboratory upstream: answers what is asked of it, by path.
struct Upstream {
    /// The port it listens on.
    port: u16,
    /// How many requests it has served, by path.
    asked: Arc<std::sync::Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    /// How many times that path has been requested.
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter of the upstream")
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}

/// An upstream that comes up on a free port and serves the given pages.
///
/// Each page has two forms: the JSON one and the HTML one. The upstream
/// answers with the JSON one when the request asks for JSON, and with the
/// HTML one when not: this way it can be seen that what comes out of the surface is
/// always the republished JSON, never the upstream's HTML.
fn start_upstream_on<F>(pages: F) -> Upstream
where
    F: Fn(u16) -> Pages,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<std::sync::Mutex<HashMap<String, usize>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let counter = Arc::clone(&asked);
    let served = Arc::new(std::sync::Mutex::new(pages(port)));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let served = Arc::clone(&served);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                *counter
                    .lock()
                    .expect("the counter of the upstream")
                    .entry(path.clone())
                    .or_insert(0) += 1;
                let wants_json = request.contains("application/vnd.pypi.simple.v1+json");
                let pages = served.lock().expect("the pages");
                let (json, html) = pages
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| (b"not found".to_vec(), b"not found".to_vec()));
                let body = if wants_json { json } else { html };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            });
        }
    });
    Upstream { port, asked }
}
