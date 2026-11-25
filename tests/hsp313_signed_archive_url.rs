//! The provider mirror hands out signed, short-lived archive URLs.
//!
//! Terraform downloads the archive of a network mirror without any
//! credential, so the version document of the mirror names each archive with
//! a URL that carries its own, narrow authority. What is proved here, against
//! the **real service** and a `curl` of the system (what a client receives,
//! not what a function returns):
//!
//! - the version document of an authenticated reader carries signed URLs,
//!   the archive is served to a request WITHOUT credential and byte for byte,
//!   and the unsigned path still needs one;
//! - the URL is bound to its path, repository, account, principal, expiry and
//!   key: any change is refused with the same 403, and the refusal costs the
//!   upstream and the store nothing;
//! - an expired URL is refused;
//! - a deployment without a key fails closed and says so;
//! - a key can be rotated without cutting the URLs in flight;
//! - Cedar and the trust gate still decide at issuance;
//! - neither the secret nor the signature reaches the log or a response.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

const PROVIDER: &str = "registry.terraform.io/hashicorp/null";
const LINUX: &str = "terraform-provider-null_3.2.1_linux_amd64.zip";
const DARWIN: &str = "terraform-provider-null_3.2.1_darwin_arm64.zip";
const LINUX_ZIP: &[u8] = b"PK\x03\x04 the linux provider archive";
const DARWIN_ZIP: &[u8] = b"PK\x03\x04 the darwin provider archive";
const HASH: &str = "zh:2bb3a1c0deadbeef";

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hsp313-{name}-{n}"))
}

/// A fake upstream: serves what it is given by path and counts each ask.
struct Upstream {
    port: u16,
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter")
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}

fn start_upstream(pages: HashMap<String, Vec<u8>>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
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
                let Some(path) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_string)
                else {
                    return;
                };
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
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec(),
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    Upstream { port, asked }
}

fn the_upstream() -> Upstream {
    let version = json!({
        "archives": {
            "linux_amd64": { "url": LINUX, "hashes": [HASH] },
            "darwin_arm64": { "url": DARWIN, "hashes": [HASH] },
        }
    });
    let mut pages = HashMap::new();
    pages.insert(
        format!("/{PROVIDER}/index.json"),
        br#"{"versions":{"3.2.1":{}}}"#.to_vec(),
    );
    pages.insert(
        format!("/{PROVIDER}/3.2.1.json"),
        version.to_string().into_bytes(),
    );
    pages.insert(format!("/{PROVIDER}/{LINUX}"), LINUX_ZIP.to_vec());
    pages.insert(format!("/{PROVIDER}/{DARWIN}"), DARWIN_ZIP.to_vec());
    start_upstream(pages)
}

/// A deployment with three mirrors over one upstream: two open ones and a
/// trusted one nobody promoted anything in.
fn the_deployment(upstream: &Upstream) -> Deployment {
    let deployment = Deployment::new();
    let proxy = json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{}/", upstream.port),
            "scopes": ["/"],
            "max_bytes": 65536,
        },
    });
    deployment.seed_terraform_proxies_trusted(
        &["tf-trusted"],
        &[
            ("tf-mirror", proxy.clone()),
            ("tf-mirror-2", proxy.clone()),
            ("tf-trusted", proxy),
        ],
    );
    deployment
}

fn start(
    deployment: &Deployment,
    upstream: &Upstream,
    extra: &[(String, String)],
) -> support::Service {
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);
    let mut environment: Vec<(&str, &str)> = vec![("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())];
    environment.extend(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    deployment.start(&environment)
}

fn alice() -> String {
    support::token(FIRST_ACCOUNT, "alice")
}

fn authenticated(port: u16, method: &str, path: &str, token: &str) -> Answer {
    let into = next("answer");
    Curl::call_with_headers(port, method, path, token, None, &[], Some(&into)).run()
}

/// A request without any credential, as Terraform makes it for the archive.
fn anonymous(port: u16, method: &str, path_and_query: &str) -> Answer {
    let body = next("anonymous-body");
    let headers = next("anonymous-headers");
    let mut command = Command::new("curl");
    command.args(["-s", "-m", "10", "-o"]).arg(&body);
    command.arg("-D").arg(&headers);
    command.args(["-w", "%{http_code}"]);
    if method == "HEAD" {
        command.arg("--head");
    } else {
        command.args(["-X", method]);
    }
    command.arg(format!("http://127.0.0.1:{port}{path_and_query}"));
    let output = command.output().expect("curl runs");
    let status: u16 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("curl prints a status");
    let body = std::fs::read(&body).unwrap_or_default();
    Answer {
        status,
        size: body.len() as u64,
        headers: std::fs::read_to_string(&headers).unwrap_or_default(),
        body,
    }
}

fn text(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The version document, as an authenticated reader gets it.
fn the_version_document(port: u16, repository: &str) -> Answer {
    authenticated(
        port,
        "GET",
        &format!("/terraform/{repository}/{PROVIDER}/3.2.1.json"),
        &alice(),
    )
}

/// The URL the document declares for `platform`, relative as the protocol has it.
fn the_archive_url(document: &Answer, platform: &str) -> String {
    let value: Value = serde_json::from_slice(&document.body).expect("a JSON document");
    value["archives"][platform]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("no url for {platform}: {}", text(document)))
        .to_string()
}

/// The path of a relative archive URL, resolved against the document.
fn the_path_of(repository: &str, relative: &str) -> String {
    format!("/terraform/{repository}/{PROVIDER}/{relative}")
}

fn the_log_after_the_traffic(deployment: &Deployment, mark: &str) -> String {
    let mut log = String::new();
    for _ in 0..50 {
        log = support::without_the_colors(&deployment.the_log());
        if log.contains(mark) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    log
}

#[test]
fn the_version_document_signs_the_archives_and_the_archive_needs_no_credential() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut service = start(&deployment, &upstream, &support::the_signing_ring());
    let port = service.port;

    let document = the_version_document(port, "tf-mirror");
    assert_eq!(document.status, 200, "{}", text(&document));
    assert_eq!(
        document.header("cache-control").as_deref(),
        Some("private, no-store"),
        "a document that carries capabilities is not cached"
    );
    let parsed: Value = serde_json::from_slice(&document.body).expect("JSON");
    assert_eq!(
        parsed["archives"]["linux_amd64"]["hashes"],
        json!([HASH]),
        "the hashes of the upstream travel untouched"
    );
    let linux = the_archive_url(&document, "linux_amd64");
    let darwin = the_archive_url(&document, "darwin_arm64");
    assert!(linux.starts_with(&format!("{LINUX}?")), "{linux}");
    assert!(darwin.starts_with(&format!("{DARWIN}?")), "{darwin}");
    for url in [&linux, &darwin] {
        for parameter in [
            "hodei_exp=",
            "hodei_kid=k1",
            "hodei_acct=",
            "hodei_sub=",
            "hodei_sig=",
        ] {
            assert!(url.contains(parameter), "{parameter} missing in {url}");
        }
    }
    assert_ne!(
        linux.split_once('?').map(|(_, query)| query),
        darwin.split_once('?').map(|(_, query)| query),
        "each archive has its own signature"
    );

    // Without a credential and without a signature the archive is not served.
    let unsigned = anonymous(port, "GET", &the_path_of("tf-mirror", LINUX));
    assert_eq!(unsigned.status, 401, "{}", text(&unsigned));
    assert_eq!(upstream.asked_for(&format!("/{PROVIDER}/{LINUX}")), 0);

    // With the signature, and no credential, the zip comes byte for byte.
    let served = anonymous(port, "GET", &the_path_of("tf-mirror", &linux));
    assert_eq!(served.status, 200, "{}", text(&served));
    assert_eq!(served.body, LINUX_ZIP, "the archive is served as it came");
    assert_eq!(
        served.header("cache-control").as_deref(),
        Some("private, no-store")
    );
    assert!(
        served.header("x-hodei-upstream").is_none(),
        "the provenance of the bytes is not for a holder of a capability"
    );
    // The capability lives until it expires: the same URL reads again.
    let again = anonymous(port, "GET", &the_path_of("tf-mirror", &linux));
    assert_eq!(again.status, 200);
    assert_eq!(again.body, LINUX_ZIP);
    let head = anonymous(port, "HEAD", &the_path_of("tf-mirror", &linux));
    assert_eq!(head.status, 200, "{}", head.headers);
    assert_eq!(
        head.header("content-length").as_deref(),
        Some(LINUX_ZIP.len().to_string().as_str())
    );
    assert_eq!(upstream.asked_for(&format!("/{PROVIDER}/{LINUX}")), 1);

    // The other archive is its own capability.
    let other = anonymous(port, "GET", &the_path_of("tf-mirror", &darwin));
    assert_eq!(other.status, 200);
    assert_eq!(other.body, DARWIN_ZIP);

    // The authenticated read of the archive keeps working.
    let bearer = authenticated(port, "GET", &the_path_of("tf-mirror", LINUX), &alice());
    assert_eq!(bearer.status, 200);
    assert_eq!(bearer.body, LINUX_ZIP);

    // A write is not a read: the URL is for a GET.
    let post = anonymous(port, "POST", &the_path_of("tf-mirror", &linux));
    assert_ne!(post.status, 200, "{}", text(&post));
    support::stop(&mut service);
}

/// One altered request: what it changes and what it asks for.
struct Tamper {
    name: &'static str,
    path: String,
}

#[test]
fn a_signed_url_is_bound_to_everything_it_was_issued_for() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut service = start(&deployment, &upstream, &support::the_signing_ring());
    let port = service.port;
    let document = the_version_document(port, "tf-mirror");
    let linux = the_archive_url(&document, "linux_amd64");
    let (_, query) = linux.split_once('?').expect("a query");
    let path = the_path_of("tf-mirror", LINUX);

    let replace = |from: &str, to: &str| query.replace(from, to);
    let signature = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("hodei_sig="))
        .expect("a signature");
    let flipped = {
        let mut bytes = signature.as_bytes().to_vec();
        bytes[0] = if bytes[0] == b'A' { b'B' } else { b'A' };
        String::from_utf8(bytes).expect("ASCII")
    };
    let expiry: i64 = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("hodei_exp="))
        .and_then(|value| value.parse().ok())
        .expect("an expiry");

    let tampers = vec![
        Tamper {
            name: "a flipped signature",
            path: format!("{path}?{}", replace(signature, &flipped)),
        },
        Tamper {
            name: "an extended expiry",
            path: format!(
                "{path}?{}",
                replace(
                    &format!("hodei_exp={expiry}"),
                    &format!("hodei_exp={}", expiry + 600)
                )
            ),
        },
        Tamper {
            name: "another key id",
            path: format!("{path}?{}", replace("hodei_kid=k1", "hodei_kid=k9")),
        },
        Tamper {
            name: "another account",
            path: format!(
                "{path}?{}",
                replace(
                    &format!("hodei_acct={FIRST_ACCOUNT}"),
                    &format!("hodei_acct={SECOND_ACCOUNT}")
                )
            ),
        },
        Tamper {
            name: "another principal",
            path: format!("{path}?{}", replace("alice", "mallory")),
        },
        Tamper {
            name: "an extra parameter",
            path: format!("{path}?{query}&extra=1"),
        },
        Tamper {
            name: "a repeated parameter",
            path: format!("{path}?{query}&hodei_exp={expiry}"),
        },
        Tamper {
            name: "a missing parameter",
            path: format!("{path}?{}", query.replace("hodei_kid=k1&", "")),
        },
        Tamper {
            name: "an empty signature",
            path: format!("{path}?{}", replace(signature, "")),
        },
        Tamper {
            name: "another archive of the same provider",
            path: format!("{}?{query}", the_path_of("tf-mirror", DARWIN)),
        },
        Tamper {
            name: "another repository of the same account",
            path: format!("{}?{query}", the_path_of("tf-mirror-2", LINUX)),
        },
        Tamper {
            name: "another provider",
            path: format!(
                "/terraform/tf-mirror/registry.terraform.io/hashicorp/other/{LINUX}?{query}"
            ),
        },
        Tamper {
            name: "the version document",
            path: format!("/terraform/tf-mirror/{PROVIDER}/3.2.1.json?{query}"),
        },
        Tamper {
            name: "the index",
            path: format!("/terraform/tf-mirror/{PROVIDER}/index.json?{query}"),
        },
    ];

    let mut bodies = std::collections::BTreeSet::new();
    for tamper in &tampers {
        let answer = anonymous(port, "GET", &tamper.path);
        assert_eq!(
            answer.status,
            403,
            "{} must be refused with a 403: {}",
            tamper.name,
            text(&answer)
        );
        bodies.insert(text(&answer));
        for leaked in [signature, "hodei_sig", "k1"] {
            assert!(
                !text(&answer).contains(leaked),
                "{}: the refusal reflects `{leaked}`",
                tamper.name
            );
        }
    }
    assert_eq!(bodies.len(), 1, "every refusal reads the same: {bodies:?}");
    // Refused before the store: the upstream was never asked for an archive.
    assert_eq!(upstream.asked_for(&format!("/{PROVIDER}/{LINUX}")), 0);
    assert_eq!(upstream.asked_for(&format!("/{PROVIDER}/{DARWIN}")), 0);

    // The untouched URL, after all of that, still works.
    let served = anonymous(port, "GET", &format!("{path}?{query}"));
    assert_eq!(served.status, 200, "{}", text(&served));
    assert_eq!(served.body, LINUX_ZIP);
    support::stop(&mut service);
}

#[test]
fn an_expired_url_is_refused_and_costs_the_upstream_nothing() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut environment = support::the_signing_ring();
    environment.push(("HODEI_SIGNING__TTL_SECS".to_string(), "1".to_string()));
    let mut service = start(&deployment, &upstream, &environment);
    let port = service.port;

    let document = the_version_document(port, "tf-mirror");
    let linux = the_archive_url(&document, "linux_amd64");
    let darwin = the_archive_url(&document, "darwin_arm64");
    // Within its life (plus the tolerated skew) it is served.
    let live = anonymous(port, "GET", &the_path_of("tf-mirror", &darwin));
    assert_eq!(live.status, 200, "{}", text(&live));

    // 1 s of life and 5 s of tolerated clock skew: after 8 s it is dead.
    std::thread::sleep(Duration::from_secs(8));
    let dead = anonymous(port, "GET", &the_path_of("tf-mirror", &linux));
    assert_eq!(dead.status, 403, "{}", text(&dead));
    assert_eq!(
        upstream.asked_for(&format!("/{PROVIDER}/{LINUX}")),
        0,
        "an expired URL is refused before the store"
    );
    // And the cached archive is no exception: expiry is the capability's.
    let dead_cached = anonymous(port, "GET", &the_path_of("tf-mirror", &darwin));
    assert_eq!(dead_cached.status, 403, "{}", text(&dead_cached));
    support::stop(&mut service);
}

#[test]
fn without_a_key_the_mirror_fails_closed_and_says_so() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut service = start(&deployment, &upstream, &[]);
    let port = service.port;

    let document = the_version_document(port, "tf-mirror");
    assert_eq!(document.status, 503, "{}", text(&document));
    assert!(
        text(&document).contains("no signing key is configured"),
        "the refusal says why: {}",
        text(&document)
    );
    assert!(
        !text(&document).contains("hodei_sig") && !text(&document).contains(LINUX),
        "no URL is issued: {}",
        text(&document)
    );
    // The rest of the mirror is untouched: the index and an authenticated
    // archive read do not need a signature.
    let index = authenticated(
        port,
        "GET",
        &format!("/terraform/tf-mirror/{PROVIDER}/index.json"),
        &alice(),
    );
    assert_eq!(index.status, 200);
    let bearer = authenticated(port, "GET", &the_path_of("tf-mirror", LINUX), &alice());
    assert_eq!(bearer.status, 200);

    // A forged signed-looking URL is no capability without a ring.
    let forged = format!(
        "{}?hodei_exp=4000000000&hodei_kid=k1&hodei_acct={FIRST_ACCOUNT}&hodei_sub=alice&hodei_sig=AAAA",
        the_path_of("tf-mirror", DARWIN)
    );
    let refused = anonymous(port, "GET", &forged);
    assert_eq!(refused.status, 403, "{}", text(&refused));
    assert_eq!(upstream.asked_for(&format!("/{PROVIDER}/{DARWIN}")), 0);

    let log = support::without_the_colors(&deployment.the_log());
    assert!(
        log.contains("no signing key is configured"),
        "the start announces it:\n{log}"
    );
    support::stop(&mut service);
}

#[test]
fn a_key_can_be_rotated_without_cutting_the_urls_in_flight() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let k1 = (
        "k1",
        "HODEI_TEST_SIGNING_SECRET_K1",
        support::SIGNING_SECRET,
    );
    let k2 = (
        "k2",
        "HODEI_TEST_SIGNING_SECRET_K2",
        support::SIGNING_SECRET_NEXT,
    );

    // 1. Only K1.
    let mut service = start(&deployment, &upstream, &support::signing_environment(&[k1]));
    let first = the_archive_url(
        &the_version_document(service.port, "tf-mirror"),
        "linux_amd64",
    );
    assert!(first.contains("hodei_kid=k1"), "{first}");
    support::stop(&mut service);

    // 2. K2 signs, K1 still verifies: the URL in flight is served.
    let mut service = start(
        &deployment,
        &upstream,
        &support::signing_environment(&[k2, k1]),
    );
    let in_flight = anonymous(service.port, "GET", &the_path_of("tf-mirror", &first));
    assert_eq!(in_flight.status, 200, "{}", text(&in_flight));
    let second = the_archive_url(
        &the_version_document(service.port, "tf-mirror"),
        "linux_amd64",
    );
    assert!(
        second.contains("hodei_kid=k2"),
        "the first key signs: {second}"
    );
    assert_eq!(
        anonymous(service.port, "GET", &the_path_of("tf-mirror", &second)).status,
        200
    );
    support::stop(&mut service);

    // 3. K1 retires: what it signed dies, what K2 signed lives.
    let mut service = start(&deployment, &upstream, &support::signing_environment(&[k2]));
    let retired = anonymous(service.port, "GET", &the_path_of("tf-mirror", &first));
    assert_eq!(retired.status, 403, "{}", text(&retired));
    assert_eq!(
        anonymous(service.port, "GET", &the_path_of("tf-mirror", &second)).status,
        200
    );
    support::stop(&mut service);
}

#[test]
fn cedar_and_the_trust_gate_decide_at_issuance() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut service = start(&deployment, &upstream, &support::the_signing_ring());
    let port = service.port;
    let document_path = |repository: &str| format!("/terraform/{repository}/{PROVIDER}/3.2.1.json");

    // A verified principal that no policy permits gets no URL.
    let carol = authenticated(
        port,
        "GET",
        &document_path("tf-mirror"),
        &support::token(FIRST_ACCOUNT, "carol"),
    );
    assert_eq!(carol.status, 403, "{}", text(&carol));
    assert!(!text(&carol).contains("hodei_sig"));

    // Another account does not see the mirror at all.
    let bob = authenticated(
        port,
        "GET",
        &document_path("tf-mirror"),
        &support::token(SECOND_ACCOUNT, "bob"),
    );
    assert_eq!(bob.status, 404, "{}", text(&bob));

    // Nobody: no credential, no URL.
    assert_eq!(
        anonymous(port, "GET", &document_path("tf-mirror")).status,
        401
    );

    // A trusted channel that admitted nothing does not issue URLs for what
    // it does not serve.
    let trusted = authenticated(port, "GET", &document_path("tf-trusted"), &alice());
    assert_eq!(trusted.status, 404, "{}", text(&trusted));
    assert!(!text(&trusted).contains("hodei_sig"));
    support::stop(&mut service);
}

#[test]
fn neither_the_secret_nor_the_signature_reaches_the_log_or_a_response() {
    let upstream = the_upstream();
    let deployment = the_deployment(&upstream);
    let mut service = start(&deployment, &upstream, &support::the_signing_ring());
    let port = service.port;

    let document = the_version_document(port, "tf-mirror");
    let linux = the_archive_url(&document, "linux_amd64");
    let (_, query) = linux.split_once('?').expect("a query");
    let signature = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("hodei_sig="))
        .expect("a signature")
        .to_string();
    let served = anonymous(port, "GET", &the_path_of("tf-mirror", &linux));
    assert_eq!(served.status, 200);
    let refused = anonymous(
        port,
        "GET",
        &the_path_of("tf-mirror", &linux.replace("hodei_kid=k1", "hodei_kid=k9")),
    );
    assert_eq!(refused.status, 403);

    // The trace is written as the response goes out: wait for the line.
    let log = the_log_after_the_traffic(
        &deployment,
        &format!("uri: /terraform/tf-mirror/{PROVIDER}/{LINUX}"),
    );
    assert!(
        log.contains(&format!("uri: /terraform/tf-mirror/{PROVIDER}/{LINUX}")),
        "the trace does not name the request, so the test proves nothing:\n{log}"
    );
    for leaked in [
        support::SIGNING_SECRET,
        signature.as_str(),
        "hodei_sig",
        "hodei_exp",
        "hodei_kid",
    ] {
        assert!(!log.contains(leaked), "the log holds `{leaked}`:\n{log}");
    }
    for response in [&document, &served, &refused] {
        let everything = format!("{} {}", response.headers, text(response));
        assert!(
            !everything.contains(support::SIGNING_SECRET),
            "a response holds the secret"
        );
    }
    assert!(
        !refused.headers.contains(&signature) && !text(&refused).contains(&signature),
        "a refusal does not echo the signature"
    );
    support::stop(&mut service);
}
