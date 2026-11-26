//! HSP-121's GOPROXY, at the HTTP level.
//!
//! Covered: `PUT` publication of a version's three artifacts,
//! the listing assembled in semver order, `@latest`, escaped paths,
//! version immutability, rejection of what the protocol does not
//! know how to do, and the proxy cache with the service afterwards offline.
//!
//! The real client (`go`) is tested in its own file; here the
//! protocol is spoken the way the client would speak it.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// A request to the service.
fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = Vec::new();
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

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The canonical `.info` document of a version.
fn info_document(version: &str) -> Vec<u8> {
    json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" })
        .to_string()
        .into_bytes()
}

/// A `go.mod` that declares the module unescaped: the `!x` in the path is
/// only path encoding, the go.mod lives inside and tells the truth.
fn go_mod(module: &str) -> Vec<u8> {
    let declared = hodei_catalog::gomod::unescape(module);
    format!("module {declared}\n\ngo 1.22\n").into_bytes()
}

/// The zip bytes: at the HTTP level they are opaque; the real client
/// unpacks them in its own test.
fn module_zip(tag: &str) -> Vec<u8> {
    format!("the module zip of {tag}").into_bytes()
}

/// Publishes the whole version: `.info`, `.mod` and `.zip`.
fn publish_version(
    port: u16,
    repository: &str,
    token: &str,
    module: &str,
    version: &str,
    zip: &[u8],
) {
    for (extension, body) in [
        ("info", info_document(version)),
        ("mod", go_mod(module)),
        ("zip", zip.to_vec()),
    ] {
        let answer = call(
            port,
            "PUT",
            &format!("/go/{repository}/{module}/@v/{version}.{extension}"),
            token,
            Some(&body),
        );
        assert_eq!(
            answer.status,
            200,
            "publish {version}.{extension}: {}",
            text_of(&answer)
        );
    }
}

#[test]
fn a_version_is_published_and_served_through_goproxy() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Publish 1.0.0: the three artifacts by their protocol path.
    let zip = module_zip("hello-1.0.0");
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.0.0",
        &zip,
    );

    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    let info = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.info",
        &token,
        None,
    );
    assert_eq!(info.status, 200, "{}", text_of(&info));
    assert_eq!(
        info.header("content-type").as_deref(),
        Some("application/json"),
        "el .info es JSON"
    );

    let served_mod = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.status, 200, "{}", text_of(&served_mod));
    assert_eq!(served_mod.body, go_mod("example.com/hello"));

    // The zip comes out byte for byte: the checksum the client computes over
    // these bytes is the same as the published one.
    let download = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, zip);
    assert_eq!(
        download.header("content-type").as_deref(),
        Some("application/zip")
    );

    // More versions: the listing sorts by semver, not by text.
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.2.0",
        &module_zip("hello-1.2.0"),
    );
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.10.0",
        &module_zip("hello-1.10.0"),
    );
    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(text_of(&list), "v1.0.0\nv1.2.0\nv1.10.0\n");

    // `@latest` points to the latest one, with its document.
    let latest = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@latest",
        &token,
        None,
    );
    assert_eq!(latest.status, 200, "{}", text_of(&latest));
    let document: Value = serde_json::from_slice(&latest.body).expect("the latest info");
    assert_eq!(document["Version"], json!("v1.10.0"));

    // Republishing the same bytes is the echo of an interrupted
    // publication.
    let again = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        Some(&zip),
    );
    assert_eq!(again.status, 200, "{}", text_of(&again));

    // Republishing the same version with other bytes is a conflict.
    let conflict = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        Some(&module_zip("hello-1.0.0-tampered")),
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));

    // An `.info` that lies about its version is not accepted.
    let liar = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v2.0.0.info",
        &token,
        Some(&info_document("v1.9.9")),
    );
    assert_eq!(liar.status, 400, "{}", text_of(&liar));

    // A `.mod` that declares another module is not accepted either.
    let wrong = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v2.0.0.mod",
        &token,
        Some(&go_mod("example.com/other")),
    );
    assert_eq!(wrong.status, 400, "{}", text_of(&wrong));

    // What nobody published is not there.
    let missing_list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/nobody/@v/list",
        &token,
        None,
    );
    assert_eq!(missing_list.status, 404, "{}", text_of(&missing_list));
    let missing_latest = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@latest",
        &token,
        None,
    );
    assert_eq!(missing_latest.status, 200);
    let missing_version = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v9.9.9.info",
        &token,
        None,
    );
    assert_eq!(missing_version.status, 404, "{}", text_of(&missing_version));

    // Without a credential not even the path is looked at.
    let anonymous = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        "",
        None,
    );
    assert_eq!(anonymous.status, 401, "{}", text_of(&anonymous));
}

#[test]
fn escaped_modules_are_addressed_by_their_canonical_path() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // The protocol's canonical escaping: uppercase is `!lowercase`.
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/!burnt!sushi/toml",
        "v1.0.0",
        &module_zip("toml-1.0.0"),
    );
    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/!burnt!sushi/toml/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    // The `go.mod` declares the module unescaped, and that is the check
    // done at publish time.
    let served_mod = call(
        port,
        "GET",
        "/go/go-hosted/example.com/!burnt!sushi/toml/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.body, go_mod("example.com/BurntSushi/toml"));

    // The unescaped form is NOT a path: the protocol speaks in escaped form.
    let unescaped = call(
        port,
        "GET",
        "/go/go-hosted/example.com/BurntSushi/toml/@v/list",
        &token,
        None,
    );
    assert_eq!(unescaped.status, 404, "{}", text_of(&unescaped));
}

#[test]
fn the_protocol_says_no_to_what_it_does_not_know() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    deployment.seed_go_proxies(&[(
        "go-proxy",
        json!({"upstream": {"id": "lab", "url": "http://127.0.0.1:1/", "scopes": ["/"], "max_bytes": 4096}}),
    )]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // A method the protocol does not have.
    let post = call(
        port,
        "POST",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        Some(b""),
    );
    assert_eq!(post.status, 405, "{}", text_of(&post));
    let delete = call(
        port,
        "DELETE",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(delete.status, 405, "{}", text_of(&delete));

    // The list is not published: only the three pieces of a version.
    let put_list = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        Some(b"v1.0.0\n"),
    );
    assert_eq!(put_list.status, 405, "{}", text_of(&put_list));

    // Paths that are not GOPROXY, not even with path tricks.
    for hostile in [
        "/go/go-hosted/random/stuff",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.exe",
        "/go/go-hosted/../../etc/passwd",
        "/go/go-hosted/example.com/hello/notat@v/v1.0.0.info",
    ] {
        let answer = call(port, "GET", hostile, &token, None);
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }

    // A proxy does not accept publishing: it is not a hosted under another name.
    let publish_into_proxy = call(
        port,
        "PUT",
        "/go/go-proxy/example.com/hello/@v/v1.0.0.info",
        &token,
        Some(&info_document("v1.0.0")),
    );
    assert_eq!(
        publish_into_proxy.status,
        405,
        "{}",
        text_of(&publish_into_proxy)
    );

    // A repository of another format is not a GOPROXY, and is not served
    // as Raw by equivalence: it does not exist for this surface.
    let other_format = call(
        port,
        "GET",
        "/go/registry/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// A fake-for-real GOPROXY upstream: serves whatever is put at
/// a path and counts every request.
struct Upstream {
    /// The port it listens on.
    port: u16,
    /// How many requests it has served, per path.
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter of the upstream")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.asked.lock().expect("the counter").values().sum()
    }
}

fn start_upstream(module: &str, version: &str, zip: Vec<u8>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        format!("/{module}/@v/list"),
        format!("{version}\n").into_bytes(),
    );
    pages.insert(
        format!("/{module}/@v/{version}.info"),
        info_document(version),
    );
    pages.insert(format!("/{module}/@v/{version}.mod"), go_mod(module));
    pages.insert(format!("/{module}/@v/{version}.zip"), zip);
    pages.insert(format!("/{module}/@latest"), info_document(version));
    let pages = Arc::new(pages);
    let counter = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let pages = Arc::clone(&pages);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = String::new();
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            read.push_str(String::from_utf8_lossy(&buffer[..n]).as_ref());
                            if read.contains("\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let Some(first) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    return;
                };
                let path = first.to_string();
                *counter
                    .lock()
                    .expect("the counter")
                    .entry(path.clone())
                    .or_default() += 1;
                let answer = match pages.get(&path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body.clone())
                    .collect::<Vec<u8>>(),
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    Upstream { port, asked }
}

#[test]
fn the_proxy_caches_the_module_and_survives_without_network() {
    let zip = module_zip("proxied-1.0.0");
    let upstream = start_upstream("example.com/proxied/hello", "v1.0.0", zip.clone());

    let deployment = Deployment::new();
    deployment.seed_go_proxies(&[(
        "go-central",
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["/"],
                "max_bytes": 65536,
            },
        }),
    )]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);

    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let first = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(first.status, 200, "{}", text_of(&first));
    assert_eq!(first.body, zip, "el proxy reescribe el zip");
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));

    // The second time it does not go out: the cache serves the same bytes, and the
    // checksum the client computes cannot be any other.
    let second = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(second.status, 200, "{}", text_of(&second));
    assert_eq!(second.body, zip);
    assert_eq!(second.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(
        upstream.asked_for("/example.com/proxied/hello/@v/v1.0.0.zip"),
        1
    );

    // The list and the go.mod, same as the zip: by the protocol path.
    let list = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    let served_mod = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.body, go_mod("example.com/proxied/hello"));

    let before = upstream.total();
    support::stop(&mut service);

    // Without network, the cached module is still served as is; what was not
    // cached is a 503 that says so.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;
    let offline = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(offline.status, 200, "{}", text_of(&offline));
    assert_eq!(offline.body, zip);
    assert_eq!(offline.header("x-hodei-cache").as_deref(), Some("offline"));
    let unknown = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v9.9.9.zip",
        &token,
        None,
    );
    assert_eq!(unknown.status, 503, "{}", text_of(&unknown));
    support::stop(&mut service);
    assert_eq!(upstream.total(), before, "offline toco la red");
}

/// A go group is a go group: it assembles the union of its members'
/// versions and does **not** turn into a raw repository serving loose
/// paths.
///
/// A group with no published module answers with the go protocol's `404`
/// -- "there is no module with that name" -, and not with the `405`
/// of a modality the release does not know how to serve: HSP-232 served it.
#[test]
fn a_go_group_answers_in_the_dialect_of_go_and_never_degrades_to_raw() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-member", Value::Null)]);
    deployment.seed_go_group("go-all", &["go-member"]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let answer = call(
        port,
        "GET",
        "/go/go-all/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(answer.status, 404, "{}", text_of(&answer));
    let body = text_of(&answer);
    assert!(
        body.contains("module"),
        "el rechazo debe hablar de modulos: {body}"
    );

    // And it is not the raw surface speaking: the same path through `/raw/`
    // has no asset to serve, and the go-group is not registered as a
    // raw repository.
    let raw = call(
        port,
        "GET",
        "/raw/go-all/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(raw.status, 404, "{}", text_of(&raw));
}

/// The `go` client authenticates with Basic from its `.netrc` and the credential
/// travels in the password. The surface normalizes it to Bearer before
/// asking; this pins the dialect at the protocol level, which the real
/// client test can only exercise over TLS.
#[test]
fn the_basic_credentials_of_the_go_client_are_understood() {
    use base64::Engine;
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-basic", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    publish_version(
        port,
        "go-basic",
        &token,
        "example.com/hello",
        "v1.0.0",
        b"opaque zip bytes",
    );

    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("alice:{token}"));
    let answer = Curl::call_with_headers(
        port,
        "GET",
        "/go/go-basic/example.com/hello/@v/v1.0.0.info",
        "",
        None,
        &[("authorization", &format!("Basic {encoded}"))],
        Some(&next("basic")),
    )
    .run();
    assert_eq!(answer.status, 200, "basic: {}", text_of(&answer));
    let body: Value = serde_json::from_slice(&answer.body).expect("the info document");
    assert_eq!(body["Version"], "v1.0.0");

    // Basic is not a free pass: without a valid credential you do not get through.
    let junk = base64::engine::general_purpose::STANDARD.encode("alice:not-a-token");
    let denied = Curl::call_with_headers(
        port,
        "GET",
        "/go/go-basic/example.com/hello/@v/v1.0.0.info",
        "",
        None,
        &[("authorization", &format!("Basic {junk}"))],
        Some(&next("basic-denied")),
    )
    .run();
    assert_eq!(denied.status, 401, "junk basic: {}", text_of(&denied));
}
