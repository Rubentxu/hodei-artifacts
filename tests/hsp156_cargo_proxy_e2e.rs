//! The cargo proxy of HSP-156, at the HTTP level.
//!
//! It proves the resolution against a local sparse upstream: the index
//! byte for byte, the download verified against the cksum of its line,
//! the yank the TTL reflects, the cache and its ETag, the negative
//! cache, the offline mode, the refused publish, the collision with the
//! hosted repository and the authorization the hosted one applies.
//!
//! The real client (`cargo`) lives in `hsp156_cargo_proxy_cli.rs`.

mod support;

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type SharedPages = Arc<Mutex<HashMap<String, Vec<u8>>>>;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The sha256 in lowercase hex: the `cksum` the client verifies.
fn the_cksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// A GET to the service, with the body in a file.
fn get(port: u16, path: &str, token: &str, extra: &[(&str, &str)]) -> Answer {
    let into = next("answer");
    Curl::call_with_headers(port, "GET", path, token, None, extra, Some(&into)).run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// A sparse index line, the way an upstream would write it.
fn an_index_line(name: &str, version: &str, cksum: &str, yanked: bool) -> String {
    json!({
        "name": name,
        "vers": version,
        "deps": [{"name": "libc", "req": "*", "optional": false, "kind": "normal"}],
        "cksum": cksum,
        "features": {},
        "yanked": yanked,
        "v": 2,
    })
    .to_string()
}

/// The test sparse upstream: it serves the config.json, the index
/// document of a crate and the .crate files, and counts every request.
struct Upstream {
    port: u16,
    asked: Arc<Mutex<HashMap<String, usize>>>,
    /// The current etag of each page: the conditional client uses it.
    etags: Arc<Mutex<HashMap<String, String>>>,
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
}

/// An upstream with the given pages. The index page carries its etag and
/// answers the `If-None-Match` with a `304`. The `dl` template is written
/// with the port the upstream already knows.
fn start_upstream(
    name: &str,
    lines: Vec<String>,
    crate_bytes: Vec<u8>,
    dl_template: impl Fn(u16) -> String,
) -> (Upstream, SharedPages) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let etags: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let index_path = format!(
        "/{}",
        hodei_catalog::cargo::Route::index_path(name).expect("valid")
    );
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        "/config.json".to_string(),
        json!({ "dl": dl_template(port) }).to_string().into_bytes(),
    );
    let index_body = lines.join("\n").into_bytes();
    pages.insert(index_path.clone(), index_body.clone());
    pages.insert(format!("/files/{name}/1.0.0.crate"), crate_bytes);
    let pages = Arc::new(Mutex::new(pages));
    let served_etags = Arc::clone(&etags);
    served_etags.lock().expect("the etags").insert(
        index_path.clone(),
        format!("\"w/{}\"", the_cksum(&index_body)),
    );
    let counter = Arc::clone(&asked);
    let served_pages = Arc::clone(&pages);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let pages = Arc::clone(&served_pages);
            let etags = Arc::clone(&served_etags);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = Vec::new();
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            read.extend_from_slice(&buffer[..n]);
                            if read.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let request = String::from_utf8_lossy(&read).into_owned();
                let Some((request_line, rest)) = request.split_once("\r\n") else {
                    return;
                };
                let Some(path) = request_line.split_whitespace().nth(1) else {
                    return;
                };
                let if_none_match = rest
                    .split("\r\n")
                    .find_map(|line| line.strip_prefix("If-None-Match:"))
                    .map(str::trim)
                    .map(str::to_string);
                *counter
                    .lock()
                    .expect("the counter")
                    .entry(path.to_string())
                    .or_default() += 1;
                let current = pages.lock().expect("the pages").get(path).cloned();
                let answer = match current {
                    Some(body) => {
                        let etag = etags.lock().expect("the etags").get(path).cloned();
                        let not_modified = if_none_match.is_some()
                            && etag.is_some()
                            && if_none_match.as_deref() == etag.as_deref();
                        if not_modified {
                            format!(
                                "HTTP/1.1 304 Not Modified\r\nETag: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                etag.expect("an etag")
                            )
                            .into_bytes()
                        } else {
                            let mut head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nETag: {}\r\nConnection: close\r\n\r\n",
                                body.len(),
                                etag.unwrap_or_else(|| format!("\"w/{}\"", the_cksum(&body)))
                            )
                            .into_bytes();
                            head.extend_from_slice(&body);
                            head
                        }
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    (Upstream { port, asked, etags }, pages)
}

/// Rewrites a page of the upstream and renews its etag.
fn rewrite(
    pages: &Arc<Mutex<HashMap<String, Vec<u8>>>>,
    upstream: &Upstream,
    path: &str,
    body: Vec<u8>,
) {
    pages
        .lock()
        .expect("the pages")
        .insert(path.to_string(), body.clone());
    upstream
        .etags
        .lock()
        .expect("the etags")
        .insert(path.to_string(), format!("\"w/{}\"", the_cksum(&body)));
}

fn the_proxy_config(port: u16, metadata_secs: u64, negative_secs: u64) -> Value {
    json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{port}/"),
            "scopes": ["/"],
            "max_bytes": 1_048_576,
        },
        "cache": {
            "ttl": { "metadata_secs": metadata_secs },
            "negative_secs": negative_secs,
        },
    })
}

fn the_allowed_hosts(port: u16) -> String {
    format!("127.0.0.1:{port};private;http")
}

const NAME: &str = "hodei-pkg";
const INDEX_ROUTE: &str = "/cargo/cargo-proxy/ho/de/hodei-pkg";
const DOWNLOAD_ROUTE: &str = "/cargo/cargo-proxy/api/v1/crates/hodei-pkg/1.0.0/download";

#[test]
fn the_proxy_serves_the_index_and_the_crate_byte_for_byte() {
    let crate_bytes = b"the exact bytes of the crate, nothing more".to_vec();
    let cksum = the_cksum(&crate_bytes);
    let (upstream, pages) = start_upstream(
        NAME,
        vec![
            an_index_line(NAME, "0.9.0", &cksum, false),
            an_index_line(NAME, "1.0.0", &cksum, false),
        ],
        crate_bytes.clone(),
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );
    let _ = pages;

    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 300, 30))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    // The index: the bytes of the upstream, as they came, in the order
    // they came in.
    let index = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(index.status, 200, "{}", text_of(&index));
    // The upstream serves what it has: two lines and the separator the
    // upstream itself chose. The proxy does NOT add or remove a byte.
    let expected: Vec<u8> = [
        an_index_line(NAME, "0.9.0", &cksum, false),
        an_index_line(NAME, "1.0.0", &cksum, false),
    ]
    .join("\n")
    .into_bytes();
    assert_eq!(index.body, expected, "the index is the upstream document");
    assert_eq!(index.header("x-hodei-cache").as_deref(), Some("miss"));

    // The download: the bytes the upstream has, the cksum its index line
    // declares.
    let download = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, crate_bytes, "the crate is byte for byte");
    assert_eq!(download.header("x-hodei-cache").as_deref(), Some("miss"));

    // The second time the cache serves: the upstream sees not one
    // request.
    let index_again = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(index_again.status, 200);
    assert_eq!(index_again.body, expected);
    assert_eq!(index_again.header("x-hodei-cache").as_deref(), Some("hit"));
    let download_again = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(download_again.status, 200);
    assert_eq!(download_again.body, crate_bytes);
    assert_eq!(
        download_again.header("x-hodei-cache").as_deref(),
        Some("hit")
    );

    let index_path = format!(
        "/{}",
        hodei_catalog::cargo::Route::index_path(NAME).expect("valid")
    );
    assert_eq!(
        upstream.asked_for(&index_path),
        1,
        "the index was asked once"
    );
    assert_eq!(
        upstream.asked_for(&format!("/files/{NAME}/1.0.0.crate")),
        1,
        "the crate was asked once"
    );
    support::stop(&mut service);
}

#[test]
fn the_upstream_yank_is_reflected_after_the_ttl() {
    let crate_bytes = b"yanked but still downloadable bytes".to_vec();
    let cksum = the_cksum(&crate_bytes);
    let (upstream, pages) = start_upstream(
        NAME,
        vec![an_index_line(NAME, "1.0.0", &cksum, false)],
        crate_bytes.clone(),
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );

    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 1, 30))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let index_path = format!(
        "/{}",
        hodei_catalog::cargo::Route::index_path(NAME).expect("valid")
    );
    let first = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(first.status, 200);
    assert!(
        text_of(&first).contains("\"yanked\":false"),
        "{}",
        text_of(&first)
    );

    // The upstream yanks the version: the document changes and so does
    // its etag.
    rewrite(
        &pages,
        &upstream,
        &index_path,
        [an_index_line(NAME, "1.0.0", &cksum, true)]
            .join("\n")
            .into_bytes(),
    );

    // Before the TTL the cache serves what it had.
    let fresh = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(fresh.status, 200);
    assert_eq!(fresh.header("x-hodei-cache").as_deref(), Some("hit"));
    assert!(text_of(&fresh).contains("\"yanked\":false"));

    // When the TTL expires, the revalidation brings the yank.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let revalidated = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(revalidated.status, 200, "{}", text_of(&revalidated));
    assert!(
        text_of(&revalidated).contains("\"yanked\":true"),
        "the yank arrives with the revalidation: {}",
        text_of(&revalidated)
    );
    assert_eq!(
        revalidated.header("x-hodei-cache").as_deref(),
        Some("refreshed")
    );
    support::stop(&mut service);
}

#[test]
fn the_etag_revalidates_the_index_without_moving_bytes() {
    let crate_bytes = b"etag revalidation bytes".to_vec();
    let cksum = the_cksum(&crate_bytes);
    let (upstream, _pages) = start_upstream(
        NAME,
        vec![an_index_line(NAME, "1.0.0", &cksum, false)],
        crate_bytes.clone(),
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );

    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 1, 30))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;
    let index_path = format!(
        "/{}",
        hodei_catalog::cargo::Route::index_path(NAME).expect("valid")
    );

    let first = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(first.status, 200);
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));
    let asked_first = upstream.asked_for(&index_path);

    // The TTL expires and the upstream confirms with a 304: the same
    // bytes.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let again = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(again.status, 200, "{}", text_of(&again));
    assert_eq!(again.body, first.body, "a 304 serves the same document");
    assert_eq!(
        again.header("x-hodei-cache").as_deref(),
        Some("revalidated")
    );
    assert_eq!(
        upstream.asked_for(&index_path),
        asked_first + 1,
        "the revalidation asked the upstream"
    );
    support::stop(&mut service);
}

#[test]
fn the_missing_crate_is_remembered_by_the_negative_cache() {
    let (upstream, _pages) = start_upstream(NAME, vec![], b"unused".to_vec(), |port| {
        format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate")
    });
    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 300, 60))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let missing = get(port, "/cargo/cargo-proxy/no/pe/nope", &token, &[]);
    assert_eq!(missing.status, 404, "{}", text_of(&missing));
    let again = get(port, "/cargo/cargo-proxy/no/pe/nope", &token, &[]);
    assert_eq!(again.status, 404);
    assert_eq!(
        upstream.asked_for("/no/pe/nope"),
        1,
        "the negative cache remembered the absence"
    );
    support::stop(&mut service);
}

#[test]
fn a_proxy_does_not_publish_and_a_download_that_lies_about_its_cksum_is_refused() {
    // The upstream serves bytes that are NOT the ones its index line
    // declares.
    let real_bytes = b"the bytes the upstream actually has".to_vec();
    let declared_cksum = the_cksum(b"different bytes than what is served");
    let (upstream, _pages) = start_upstream(
        NAME,
        vec![an_index_line(NAME, "1.0.0", &declared_cksum, false)],
        real_bytes,
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );
    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 300, 30))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let download = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(
        download.status,
        502,
        "the download that does not match its cksum is refused: {}",
        text_of(&download)
    );

    // The proxy never publishes.
    let body = {
        let header = json!({"name": NAME, "vers": "2.0.0", "deps": []});
        let json = serde_json::to_vec(&header).expect("json");
        let mut body = Vec::new();
        body.extend_from_slice(&(json.len() as u32).to_le_bytes());
        body.extend_from_slice(&json);
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"file");
        body
    };
    let into = next("publish");
    std::fs::write(&into, &body).expect("the body is written");
    let refused = Curl::call_with_headers(
        port,
        "PUT",
        "/cargo/cargo-proxy/api/v1/crates/new",
        &token,
        Some(&into),
        &[],
        None,
    )
    .run();
    assert_eq!(
        refused.status,
        405,
        "a proxy does not publish: {}",
        text_of(&refused)
    );
    support::stop(&mut service);
}

#[test]
fn a_name_reserved_by_the_hosted_is_not_resolved_from_upstream() {
    // A hosted repository of the account already knows the crate: the
    // proxy blocks it.
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    deployment.seed_cargo_proxies(&[(
        "cargo-proxy",
        json!({
            "upstream": {
                "id": "lab",
                "url": "http://127.0.0.1:1/",
                "scopes": ["/"],
            },
        }),
    )]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;

    // Publish the crate in the hosted repository.
    let header = json!({"name": NAME, "vers": "1.0.0", "deps": []});
    let json = serde_json::to_vec(&header).expect("json");
    let mut body = Vec::new();
    body.extend_from_slice(&(json.len() as u32).to_le_bytes());
    body.extend_from_slice(&json);
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"file");
    let into = next("publish");
    std::fs::write(&into, &body).expect("the body is written");
    let published = Curl::call_with_headers(
        port,
        "PUT",
        "/cargo/cargo-hosted/api/v1/crates/new",
        &token,
        Some(&into),
        &[],
        None,
    )
    .run();
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The index of the reserved crate: 409, and the upstream never hears
    // about it.
    let index = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(index.status, 409, "{}", text_of(&index));
    assert!(
        text_of(&index).contains("reserved"),
        "the refusal says the name is reserved: {}",
        text_of(&index)
    );
    let download = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(download.status, 409, "{}", text_of(&download));

    // A crate the hosted does not know keeps resolving through the proxy
    // (here the upstream does not exist: the failure is the upstream's,
    // not a 409).
    let other = get(port, "/cargo/cargo-proxy/un/kn/unknown", &token, &[]);
    assert_eq!(other.status, 502, "{}", text_of(&other));
    support::stop(&mut service);
}

#[test]
fn the_proxy_serves_the_cache_when_it_cannot_reach_the_upstream() {
    let crate_bytes = b"offline cache bytes".to_vec();
    let cksum = the_cksum(&crate_bytes);
    let (upstream, _pages) = start_upstream(
        NAME,
        vec![an_index_line(NAME, "1.0.0", &cksum, false)],
        crate_bytes.clone(),
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );
    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 300, 30))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    // Bring everything with network.
    let index = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(index.status, 200);
    let download = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(download.status, 200);
    assert_eq!(download.body, crate_bytes);
    let asked = upstream.asked_for(&format!("/files/{NAME}/1.0.0.crate"));
    support::stop(&mut service);

    // Without egress, the cached content keeps being served.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;
    let offline_index = get(port, INDEX_ROUTE, &token, &[]);
    assert_eq!(offline_index.status, 200, "{}", text_of(&offline_index));
    assert_eq!(offline_index.body, index.body);
    let offline_download = get(port, DOWNLOAD_ROUTE, &token, &[]);
    assert_eq!(
        offline_download.status,
        200,
        "{}",
        text_of(&offline_download)
    );
    assert_eq!(offline_download.body, crate_bytes);
    // What was not brought is a 503 that says so.
    let unknown = get(port, "/cargo/cargo-proxy/un/kn/unknown", &token, &[]);
    assert_eq!(unknown.status, 503, "{}", text_of(&unknown));
    support::stop(&mut service);
    assert_eq!(
        upstream.asked_for(&format!("/files/{NAME}/1.0.0.crate")),
        asked,
        "offline did not reach the upstream"
    );
}

#[test]
fn the_hosted_authorization_applies_to_the_proxy() {
    let (upstream, _pages) = start_upstream(
        NAME,
        vec![an_index_line(
            NAME,
            "1.0.0",
            &the_cksum(b"auth bytes"),
            false,
        )],
        b"auth bytes".to_vec(),
        |port| format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate"),
    );
    let deployment = Deployment::new();
    deployment.seed_cargo_proxies(&[("cargo-proxy", the_proxy_config(upstream.port, 300, 30))]);
    let allowed = the_allowed_hosts(upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    // No credential, no index.
    let anonymous = get(port, INDEX_ROUTE, "", &[]);
    assert_eq!(anonymous.status, 401, "{}", text_of(&anonymous));

    // A principal of another account does not see the repository: for it
    // there is nothing there.
    let stranger = support::token(SECOND_ACCOUNT, "bob");
    let hidden = get(port, INDEX_ROUTE, &stranger, &[]);
    assert_eq!(
        hidden.status,
        404,
        "a package of another account is not visible: {}",
        text_of(&hidden)
    );
    support::stop(&mut service);
}
