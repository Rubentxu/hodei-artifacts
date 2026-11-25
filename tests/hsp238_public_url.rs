//! HSP-238: the public address of the deployment decides the public URLs.
//!
//! What is proved here, against the **real service**:
//!
//! - behind a TLS reverse proxy that the deployment trusts, npm, PyPI,
//!   NuGet, Cargo and the OCI token announce the address the client used;
//! - a forwarded header that a client sends on its own changes nothing;
//! - a `Host` that is not an authority is never reflected in a URL, in a
//!   metadata document or in the challenge of the OCI token.
//!
//! The service is the one the CI builds, and the requests are made with a
//! `curl` of the system: what is asserted is what a client receives, not
//! what a function returns.

mod support;

use std::path::Path;

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// The catalog seeder.
///
/// The harness seeds the catalog by running this very test with the
/// environment that says what to seed, so the name is not free.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The separator of the PyPI upload.
const BOUNDARY: &str = "hodeipypiboundary";

/// A request with headers of its own; it returns the answer and its body.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> Answer {
    // A pair of files per request: several tests call at the same time.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let into = dir.join(format!("answer-{n}.bin"));
    let sent = body.map(|body| {
        let sent = dir.join(format!("request-{n}.json"));
        std::fs::write(&sent, serde_json::to_vec(body).expect("the body")).expect("the body file");
        sent
    });
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        headers,
        Some(&into),
    )
    .run()
}

/// A PyPI upload, as `twine` sends it.
fn the_upload(name: &str, version: &str, filename: &str, file: &[u8], sha256: &str) -> Vec<u8> {
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

/// The body of an npm publication: a version and its tarball in base64.
fn the_publication(name: &str, version: &str, tarball: &[u8]) -> Value {
    let file = format!("{name}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": version },
        "versions": { version: { "name": name, "version": version, "_id": format!("{name}@{version}") } },
        "_attachments": { file: {
            "content_type": "application/octet-stream",
            "data": base64::engine::general_purpose::STANDARD.encode(tarball),
            "length": tarball.len(),
        } },
        "access": name,
    })
}

/// The JSON of an answer.
fn json_of(answer: &Answer) -> Value {
    serde_json::from_slice(&answer.body).unwrap_or_else(|error| {
        panic!(
            "the body is not JSON ({error}): {}",
            String::from_utf8_lossy(&answer.body)
        )
    })
}

/// The body of an answer, as text.
fn the_text(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The scheme and the authority of an absolute URL.
fn the_origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("an absolute URL");
    let authority = rest.split('/').next().expect("an authority");
    format!("{scheme}://{authority}")
}

/// The environment of a deployment that says where it lives.
fn the_environment(base: Option<&str>, trusted: Option<&str>) -> Vec<(String, String)> {
    let mut environment = Vec::new();
    if let Some(base) = base {
        environment.push((
            "HODEI_PUBLIC_URL__PUBLIC_BASE_URL".to_string(),
            base.to_string(),
        ));
    }
    if let Some(trusted) = trusted {
        environment.push((
            "HODEI_PUBLIC_URL__TRUSTED_PROXIES".to_string(),
            trusted.to_string(),
        ));
    }
    environment
}

/// A deployment with one repository of every format that answers with a
/// public URL, and with the content that those answers describe.
fn the_deployment(environment: &[(&str, &str)]) -> (Deployment, support::Service, u16) {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let service = deployment.start(environment);
    let port = service.port;
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The npm package: its packument announces the tarball.
    let tarball = b"not a real tarball, the registry does not open it".to_vec();
    let published = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget",
        &token,
        &[],
        Some(&the_publication("widget", "1.0.0", &tarball)),
    );
    assert_eq!(published.status, 201, "{}", the_text(&published));

    // The PyPI distribution: its index announces the file.
    let wheel = support::a_wheel("labpkg", "1.0", Some(">=3.9"));
    let hash = the_sha256(&wheel);
    let body = the_upload(
        "labpkg",
        "1.0",
        "labpkg-1.0-py3-none-any.whl",
        &wheel,
        &hash,
    );
    let sent = dir.join("upload.bin");
    let into = dir.join("upload-answer.bin");
    std::fs::write(&sent, body).expect("the upload body is written");
    let published = Curl::call(
        port,
        "POST",
        "/pypi/pypi-hosted/",
        &token,
        Some(&sent),
        Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
        Some(&into),
    )
    .run();
    assert_eq!(published.status, 200, "{}", the_text(&published));

    // The service stays alive as long as the guard does: dropping it
    // kills the process, and the requests would talk to nobody.
    (deployment, service, port)
}

/// The origin that npm announces in the tarball of a package.
fn the_npm_origin(dir: &Path, port: u16, token: &str, headers: &[(&str, &str)]) -> String {
    let answer = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget",
        token,
        headers,
        None,
    );
    assert_eq!(answer.status, 200, "{}", the_text(&answer));
    let document = json_of(&answer);
    let tarball = document["versions"]["1.0.0"]["dist"]["tarball"]
        .as_str()
        .expect("the tarball");
    the_origin_of(tarball)
}

/// The origin that PyPI announces in the file of a distribution.
fn the_pypi_origin(dir: &Path, port: u16, token: &str, headers: &[(&str, &str)]) -> String {
    let mut asked = headers.to_vec();
    asked.push((
        "accept",
        "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
    ));
    let answer = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        token,
        &asked,
        None,
    );
    assert_eq!(answer.status, 200, "{}", the_text(&answer));
    let document = json_of(&answer);
    let url = document["files"][0]["url"].as_str().expect("the file url");
    the_origin_of(url)
}

/// The origin that NuGet announces in its service index.
///
/// The service index is anonymous by contract: the client loads it before
/// it has a credential, so it is asked without one.
fn the_nuget_origin(dir: &Path, port: u16, headers: &[(&str, &str)]) -> String {
    let answer = call(
        dir,
        port,
        "GET",
        "/nuget/nuget-hosted/index.json",
        "",
        headers,
        None,
    );
    assert_eq!(answer.status, 200, "{}", the_text(&answer));
    let document = json_of(&answer);
    let id = document["resources"][0]["@id"]
        .as_str()
        .expect("the address");
    the_origin_of(id)
}

/// The origin that Cargo announces in its configuration.
fn the_cargo_origin(dir: &Path, port: u16, token: &str, headers: &[(&str, &str)]) -> String {
    let answer = call(
        dir,
        port,
        "GET",
        "/cargo/cargo-hosted/config.json",
        token,
        headers,
        None,
    );
    assert_eq!(answer.status, 200, "{}", the_text(&answer));
    let document = json_of(&answer);
    let dl = document["dl"].as_str().expect("the download address");
    the_origin_of(dl)
}

/// The realm that the OCI token announces in its challenge.
fn the_oci_realm(dir: &Path, port: u16, headers: &[(&str, &str)]) -> String {
    let answer = call(dir, port, "GET", "/v2/", "", headers, None);
    assert_eq!(answer.status, 401, "{}", the_text(&answer));
    let challenge = answer
        .header("www-authenticate")
        .expect("the challenge of the token");
    let realm = challenge
        .split("realm=\"")
        .nth(1)
        .expect("the realm of the challenge");
    realm.split('"').next().expect("the realm").to_string()
}

/// A forwarded header that a client sends on its own changes nothing.
///
/// The scheme and the authority of the public address are decided by the
/// deployment. A client that says "https" does not get to say it, and a
/// client that says which host it is talking to does not get to say that
/// either.
#[test]
fn a_forwarded_header_of_a_client_does_not_change_the_public_url() {
    let (deployment, _service, port) = the_deployment(&[]);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let hostile = [
        ("x-forwarded-proto", "https"),
        ("x-forwarded-host", "evil.example"),
    ];
    let expected = format!("http://127.0.0.1:{port}");

    assert_eq!(the_npm_origin(dir, port, &token, &hostile), expected);
    assert_eq!(the_pypi_origin(dir, port, &token, &hostile), expected);
    assert_eq!(the_nuget_origin(dir, port, &hostile), expected);
    assert_eq!(the_cargo_origin(dir, port, &token, &hostile), expected);
    assert_eq!(
        the_oci_realm(dir, port, &hostile),
        format!("{expected}/token")
    );
}

/// A trusted proxy says where the deployment lives.
///
/// The proxy is trusted because the deployment says so, and then its
/// forwarded headers are the address the client used.
#[test]
fn a_trusted_proxy_says_where_the_deployment_lives() {
    let environment = the_environment(None, Some("127.0.0.1/32"));
    let pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let (deployment, _service, port) = the_deployment(&pairs);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let forwarded = [
        ("x-forwarded-proto", "https"),
        ("x-forwarded-host", "registry.example"),
    ];
    let expected = "https://registry.example";

    assert_eq!(the_npm_origin(dir, port, &token, &forwarded), expected);
    assert_eq!(the_pypi_origin(dir, port, &token, &forwarded), expected);
    assert_eq!(the_nuget_origin(dir, port, &forwarded), expected);
    assert_eq!(the_cargo_origin(dir, port, &token, &forwarded), expected);
    assert_eq!(
        the_oci_realm(dir, port, &forwarded),
        "https://registry.example/token"
    );
}

/// The declared base is the public address, and the `Host` is not reflected.
///
/// The `Host` of a request is written by the client, so a deployment that
/// declares where it lives does not repeat it in the URLs it hands out.
#[test]
fn the_declared_base_is_the_public_address() {
    let environment = the_environment(Some("https://artifacts.example"), None);
    let pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let (deployment, _service, port) = the_deployment(&pairs);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let hostile = [("host", "evil.example")];
    let expected = "https://artifacts.example";

    assert_eq!(the_npm_origin(dir, port, &token, &hostile), expected);
    assert_eq!(the_pypi_origin(dir, port, &token, &hostile), expected);
    assert_eq!(the_nuget_origin(dir, port, &hostile), expected);
    assert_eq!(the_cargo_origin(dir, port, &token, &hostile), expected);
    assert_eq!(
        the_oci_realm(dir, port, &hostile),
        "https://artifacts.example/token"
    );
}

/// A `Host` that is not an authority is never reflected.
///
/// The negative case, per format: an authority with a path, an authority
/// with credentials and an authority with a control character are not
/// authorities, and what is not an authority is not reflected in a URL nor
/// in the challenge of the OCI token.
#[test]
fn a_host_that_is_not_an_authority_is_not_reflected() {
    let (deployment, _service, port) = the_deployment(&[]);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");

    for hostile in [
        "evil.example/path",
        "user:secret@evil.example",
        "evil.example?token=secret",
    ] {
        let headers = [("host", hostile)];
        // The NuGet service index is anonymous, and the others need the
        // credential: with it, the request reaches the handler that
        // builds the URL.
        for (path, credential) in [
            ("/npm/npm-hosted/widget", token.clone()),
            ("/pypi/pypi-hosted/simple/labpkg/", token.clone()),
            ("/nuget/nuget-hosted/index.json", String::new()),
            ("/cargo/cargo-hosted/config.json", token.clone()),
        ] {
            let answer = call(dir, port, "GET", path, &credential, &headers, None);
            assert_eq!(
                answer.status,
                400,
                "the host {hostile} was answered in {path}: {}",
                the_text(&answer)
            );
            assert!(
                !the_text(&answer).contains("evil.example"),
                "the host {hostile} was reflected in {path}"
            );
        }
        // El reto del token OCI: o no hay reto, o dice `localhost`, y en
        // ningun caso dice el host que el cliente escribio.
        let answer = call(dir, port, "GET", "/v2/", "", &headers, None);
        let challenge = answer.header("www-authenticate").unwrap_or_default();
        assert!(
            !challenge.contains("evil.example"),
            "the host {hostile} reached the challenge: {challenge}"
        );
    }
}

/// A credential in the query of a request does not travel in the URLs.
///
/// The URLs the registry hands out are built out of the deployment and the
/// format, never out of the query the client sent.
#[test]
fn a_query_with_a_credential_does_not_travel_in_the_urls() {
    let (deployment, _service, port) = the_deployment(&[]);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let hostile = [
        ("x-forwarded-host", "evil.example"),
        ("x-forwarded-proto", "https"),
    ];
    let answer = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget?token=secret",
        &token,
        &hostile,
        None,
    );
    assert_eq!(answer.status, 200, "{}", the_text(&answer));
    let document = json_of(&answer);
    let tarball = document["versions"]["1.0.0"]["dist"]["tarball"]
        .as_str()
        .expect("the tarball");
    assert!(
        !tarball.contains("secret"),
        "the credential travelled in the tarball: {tarball}"
    );
    assert!(
        !tarball.contains("evil.example"),
        "the host of the client travelled in the tarball: {tarball}"
    );
}

/// The query of a request that carries a credential, and a hostile origin.
const THE_SECRET_QUERY: &str = "?token=secret&access_token=secret";

/// A credential in the query does not travel in the metadata of PyPI, NuGet
/// and Cargo.
///
/// The same negative that npm has, for the other formats that announce URLs:
/// the document is read whole, so a secret reflected anywhere in it (and not
/// only in the field that holds the URL) turns the test red.
#[test]
fn a_query_with_a_credential_does_not_travel_in_pypi_nuget_and_cargo() {
    let (deployment, _service, port) = the_deployment(&[]);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let hostile = [
        ("x-forwarded-host", "evil.example"),
        ("x-forwarded-proto", "https"),
        (
            "accept",
            "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
        ),
    ];
    let expected = format!("http://127.0.0.1:{port}");

    for (format, path, credential) in [
        (
            "pypi",
            format!("/pypi/pypi-hosted/simple/labpkg/{THE_SECRET_QUERY}"),
            token.clone(),
        ),
        (
            "nuget",
            format!("/nuget/nuget-hosted/index.json{THE_SECRET_QUERY}"),
            String::new(),
        ),
        (
            "cargo",
            format!("/cargo/cargo-hosted/config.json{THE_SECRET_QUERY}"),
            token.clone(),
        ),
    ] {
        let answer = call(dir, port, "GET", &path, &credential, &hostile, None);
        assert_eq!(answer.status, 200, "{format}: {}", the_text(&answer));
        let text = the_text(&answer);
        assert!(
            text.contains(&expected),
            "{format}: the metadata does not announce {expected}: {text}"
        );
        for leaked in ["secret", "token=", "evil.example"] {
            assert!(
                !text.contains(leaked),
                "{format}: the metadata reflects `{leaked}`: {text}"
            );
        }
    }
}

/// The trace of a request holds neither its query nor its `Host`.
///
/// The service logs every request, and a log is a place where a credential
/// stays: the span names the path and nothing else.
#[test]
fn the_trace_holds_neither_the_query_nor_the_hostile_host() {
    let (deployment, _service, port) = the_deployment(&[]);
    let dir = deployment.directory.path();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let hostile = [
        ("host", "evil.example"),
        ("x-forwarded-host", "evil.example"),
    ];

    // The request must reach the trace layer even if it is refused.
    for path in [
        "/npm/npm-hosted/widget?token=secret",
        "/pypi/pypi-hosted/simple/labpkg/?token=secret",
        "/cargo/cargo-hosted/config.json?token=secret",
        "/v2/?token=secret",
    ] {
        call(dir, port, "GET", path, &token, &hostile, None);
    }
    // And a request with a Host that is an authority, so that it is served.
    let served = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget?token=secret",
        &token,
        &[("x-forwarded-host", "evil.example")],
        None,
    );
    assert_eq!(served.status, 200, "{}", the_text(&served));

    // The trace is written as the response goes out: wait for the last line.
    let mut log = String::new();
    for _ in 0..50 {
        log = support::without_the_colors(&deployment.the_log());
        if log.matches("uri: /npm/npm-hosted/widget").count() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        log.contains("uri: /npm/npm-hosted/widget"),
        "the trace does not name the request, so the test proves nothing:\n{log}"
    );
    for leaked in ["secret", "token=", "evil.example"] {
        assert!(!log.contains(leaked), "the trace holds `{leaked}`:\n{log}");
    }
}

/// A raw request, as a socket sees it: the answer in full.
fn raw_request(port: u16, request: &[u8]) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the connection");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("the timeout");
    stream.write_all(request).expect("the request");
    let mut answer = Vec::new();
    // The service closes the connection after a malformed request, and the
    // others carry `Connection: close`; a timeout ends what is left.
    let _ = stream.read_to_end(&mut answer);
    String::from_utf8_lossy(&answer).into_owned()
}

/// The status line of a raw answer.
fn the_status_of(answer: &str) -> u16 {
    answer
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse().ok())
        .unwrap_or_else(|| panic!("no status line in: {answer:?}"))
}

/// A `Host` with a control character is a 400 and is not reflected.
///
/// `curl` does not send a header with a line break, so the request is written
/// to the socket by hand. The challenge of the OCI token, for a `Host` that is
/// not an authority, says the address that the deployment knows: `localhost`.
#[test]
fn a_host_with_a_line_break_is_refused_without_reflection() {
    let (deployment, _service, port) = the_deployment(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // A bare CR, a CRLF folded into the value (obs-fold) and an encoded CRLF
    // inside the value of the header. A bare LF or an unfolded CRLF ends the
    // header, so what follows is another header and the `Host` is whole.
    for host in [
        "evil.example\rX-Injected: 1",
        "evil.example\r\n X-Injected: 1",
        "evil.example%0d%0aX-Injected: 1",
    ] {
        for path in ["/npm/npm-hosted/widget", "/cargo/cargo-hosted/config.json"] {
            let request = format!(
                "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\n\
                 Connection: close\r\n\r\n"
            );
            let answer = raw_request(port, request.as_bytes());
            assert_eq!(
                the_status_of(&answer),
                400,
                "the host {host:?} was not refused in {path}: {answer}"
            );
            assert!(
                !answer.contains("evil.example") && !answer.contains("X-Injected"),
                "the host {host:?} was reflected in {path}: {answer}"
            );
        }
    }

    // A `Host` that the parser lets through but that is not an authority: the
    // challenge says the exact address of the fallback.
    for host in ["evil.example/path", "evil.example%0d%0aX-Injected: 1"] {
        let request = format!("GET /v2/ HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        let answer = raw_request(port, request.as_bytes());
        assert_eq!(the_status_of(&answer), 401, "{host}: {answer}");
        assert!(!answer.contains("evil.example"), "{host}: {answer}");
        let challenge = answer
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("www-authenticate:"))
            .unwrap_or_else(|| panic!("no challenge in: {answer}"));
        assert!(
            challenge.contains("realm=\"http://localhost/token\""),
            "{host}: the realm of the challenge is not the fallback: {challenge}"
        );
    }
    drop(deployment);
}
