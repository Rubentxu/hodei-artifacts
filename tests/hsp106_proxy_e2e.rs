//! The remote proxy repository (HSP-106 / HAR-12), against the wire.
//!
//! The surface it exercises is that of the **raw** format: the upstream
//! serves opaque bytes over HTTP and the proxy fetches them as they are,
//! without rewriting them, which is what a raw proxy promises.
//!
//! The service is a real process with a real catalog, byte store and
//! Cedar. The upstream is a local server that this file
//! controls: an **authorized** one (the one the egress policy lets
//! through) and the hostile responses it simulates -- redirects towards
//! forbidden networks, truncated bodies, fake checksums, excess bytes.
//!
//! What is measured is what a person will see: what is served, what is not, and what
//! receipt remains. Nothing that fails can end up presented as
//! verified content.

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use support::{Curl, Deployment, FIRST_ACCOUNT};

/// What the authorized upstream serves.
const BODY: &[u8] = b"content that lives upstream";

/// The token the service reads from the environment by reference.
const UPSTREAM_SECRET: &str = "lab-secret-value-9f3a";

/// A request that reached the upstream.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    authorization: Option<String>,
}

/// A local upstream with hand-written responses.
struct Upstream {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Upstream {
    /// Starts an upstream. `redirects_to` is the port of another upstream (or
    /// of a closed port) to which some redirects point.
    fn start(redirects_to: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, redirects_to, &log));
            }
        });
        Self { port, seen }
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }

    fn hits(&self, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|one| one.path == path)
            .count()
    }
}

fn serve(mut stream: TcpStream, other_port: u16, log: &Mutex<Vec<Seen>>) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
        // A TLS ClientHello is not an HTTP request: a plain server
        // answers it with garbage and closes, and the handshake does not complete.
        if request.first() == Some(&0x16) {
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            return;
        }
    }
    let text = String::from_utf8_lossy(&request).into_owned();
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_string();
    let authorization = text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.trim().to_string())
    });
    log.lock().expect("the log").push(Seen {
        path: path.clone(),
        authorization,
    });

    let redirect = |location: &str| -> Vec<u8> {
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    };
    let ok = |body: &[u8], extra: &str| -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    };
    let checksum = |body: &[u8]| {
        format!(
            "X-Checksum-Sha256: {}\r\n",
            hodei_evidence::ContentDigest::of_bytes(body).value
        )
    };

    let response: Vec<u8> = match path.as_str() {
        "ok/app.bin" => ok(BODY, &format!("{}ETag: \"v1\"\r\n", checksum(BODY))),
        "ok/plain.bin" => ok(BODY, ""),
        "bad/checksum.bin" => ok(BODY, &checksum(b"something else entirely")),
        "bad/truncated.bin" => {
            // Promises a thousand bytes and delivers ten.
            let mut out =
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n".to_vec();
            out.extend_from_slice(&BODY[..10]);
            out
        }
        "big/huge.bin" => ok(&vec![b'x'; 8192], ""),
        "status/500.bin" => {
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec()
        }
        "redir/same.bin" => redirect("/ok/app.bin"),
        "redir/loop.bin" => redirect("/redir/loop.bin"),
        "redir/loopback.bin" => redirect(&format!("http://127.0.0.1:{other_port}/ok/app.bin")),
        "redir/metadata.bin" => redirect("http://169.254.169.254/latest/meta-data/"),
        "redir/private.bin" => redirect("http://10.0.0.5/ok/app.bin"),
        "redir/stranger.bin" => redirect("http://evil.example/ok/app.bin"),
        _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

/// The configuration of a repository upstream.
fn upstream_of(id: &str, url: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut config = serde_json::json!({
        "id": id,
        "url": url,
        "scopes": ["/"],
        "credential_ref": "env:LAB_TOKEN",
        "max_bytes": 1024,
    });
    for (key, value) in extra.as_object().expect("an object") {
        config[key] = value.clone();
    }
    config
}

/// The egress list that authorizes the two local upstreams.
fn allow(ports: &[u16]) -> String {
    ports
        .iter()
        .map(|port| format!("127.0.0.1:{port};private;http"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Any request: code and body.
fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let file = body.map(|bytes| {
        std::fs::write(&sent, bytes).expect("the body is written");
        sent.as_path()
    });
    let answer = Curl::call(
        port,
        method,
        path,
        token,
        file,
        body.map(|_| "application/octet-stream"),
        Some(&into),
    )
    .run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

fn get(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
) -> (u16, String) {
    call(
        deployment,
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        token,
        None,
    )
}

/// The receipts the service says it has for a path.
fn receipts(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
) -> Vec<serde_json::Value> {
    let (status, body) = call(
        deployment,
        port,
        "GET",
        &format!("/api/v1/provenance?repository={repository}&path={path}"),
        token,
        None,
    );
    assert_eq!(status, 200, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
    parsed["receipts"].as_array().expect("receipts").clone()
}

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// A deployment with an authorized upstream, another one for the redirects and the
/// proxy repositories of the test.
struct Lab {
    deployment: Deployment,
    primary: Upstream,
    secondary: Upstream,
}

impl Lab {
    fn new(trusted: &[&str]) -> Self {
        let closed = support::free_port();
        let secondary = Upstream::start(closed);
        let primary = Upstream::start(secondary.port);
        let base = |upstream: &Upstream| format!("http://127.0.0.1:{}/", upstream.port);
        let deployment = Deployment::new();
        deployment.seed_proxies(
            &[],
            trusted,
            &[
                (
                    "pxy",
                    upstream_of("lab", &base(&primary), serde_json::json!({})),
                ),
                (
                    "pxy-strict",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "require_checksum": true }),
                    ),
                ),
                (
                    "pxy-cross",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "redirects": { "max": 3, "cross_origin": true } }),
                    ),
                ),
                (
                    "pxy-scoped",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "scopes": ["ok/"] }),
                    ),
                ),
                (
                    "pxy-tls",
                    upstream_of(
                        "lab",
                        &format!("https://127.0.0.1:{}/", primary.port),
                        serde_json::json!({}),
                    ),
                ),
                (
                    "pxy-trusted",
                    upstream_of("lab", &base(&primary), serde_json::json!({})),
                ),
            ],
        );
        Self {
            deployment,
            primary,
            secondary,
        }
    }

    /// Starts the service with egress authorized towards the two upstreams.
    fn start(&self) -> support::Service {
        self.start_with(&[])
    }

    fn start_with(&self, extra: &[(&str, &str)]) -> support::Service {
        let allowed = allow(&[self.primary.port, self.secondary.port]);
        let mut environment = vec![
            ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
            ("HODEI_PROXY__ALLOWED_CREDENTIALS", "LAB_TOKEN"),
            ("LAB_TOKEN", UPSTREAM_SECRET),
        ];
        environment.extend_from_slice(extra);
        self.deployment.start(&environment)
    }
}

// ---------------------------------------------------------------------------
// PROXY-01: what is downloaded is verified, stored and leaves a receipt.
// ---------------------------------------------------------------------------

#[test]
fn the_first_resolution_is_verified_receipted_and_then_served_as_ours() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 200, "{body}");
    assert_eq!(body.as_bytes(), BODY);

    // The receipt says where it came from and what it was checked with.
    let found = receipts(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(found.len(), 1);
    let receipt = &found[0];
    assert_eq!(receipt["upstream"], "lab");
    assert_eq!(receipt["status"], 200);
    assert_eq!(receipt["redirects"], 0);
    assert_eq!(receipt["size_bytes"], BODY.len());
    assert_eq!(receipt["checksum"]["kind"], "declared");
    assert_eq!(
        receipt["digest"],
        format!(
            "sha256:{}",
            hodei_evidence::ContentDigest::of_bytes(BODY).value
        )
    );
    assert!(
        receipt["source_url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/ok/app.bin") && !url.contains('@')),
        "{receipt}"
    );

    // The credential was sent to the authorized upstream -- and does not appear anywhere
    // that can be read.
    let first = lab.primary.requests();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].authorization.as_deref(),
        Some(format!("Bearer {UPSTREAM_SECRET}").as_str())
    );
    assert!(!receipt.to_string().contains(UPSTREAM_SECRET));
    assert!(!lab.deployment.the_log().contains(UPSTREAM_SECRET));
    assert!(!lab.deployment.the_log().contains(&alice));

    // Once resolved it is own content: it is not requested again.
    let (status, again) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 200);
    assert_eq!(again.as_bytes(), BODY);
    assert_eq!(lab.primary.hits("ok/app.bin"), 1);
    assert_eq!(
        receipts(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin").len(),
        1
    );
    support::stop(&mut service);
}

#[test]
fn a_content_without_a_declared_checksum_is_marked_as_computed_only() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // Without requiring it, it is served -- but the receipt does not pretend to a verification
    // that did not happen.
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/plain.bin");
    assert_eq!(status, 200);
    let found = receipts(&lab.deployment, service.port, &alice, "pxy", "ok/plain.bin");
    assert_eq!(found[0]["checksum"]["kind"], "computed_only");

    // Requiring it, it is not served.
    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-strict",
        "ok/plain.bin",
    );
    assert_eq!(status, 502);
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-strict",
            "ok/plain.bin"
        )
        .is_empty()
    );
    support::stop(&mut service);
}

#[test]
fn a_proxy_does_not_accept_uploads() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = call(
        &lab.deployment,
        service.port,
        "PUT",
        "/repository/pxy/mine.bin",
        &alice,
        Some(b"local bytes"),
    );
    assert_eq!(status, 405);
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "mine.bin");
    assert_ne!(status, 200);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// PROXY-02: a hostile upstream leaves neither content nor receipt.
// ---------------------------------------------------------------------------

#[test]
fn a_hostile_upstream_never_leaves_verified_content_behind() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    for (path, expected) in [
        // A checksum that does not match the bytes.
        ("bad/checksum.bin", 502),
        // Promises a thousand bytes and delivers ten.
        ("bad/truncated.bin", 502),
        // More bytes than the upstream's budget.
        ("big/huge.bin", 502),
        // An upstream failure is not disguised.
        ("status/500.bin", 502),
        // What does not exist, does not exist.
        ("missing/none.bin", 404),
    ] {
        let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(status, expected, "{path}: {body}");
        // Neither the message nor the log leak the credential.
        assert!(!body.contains(UPSTREAM_SECRET));
        // And nothing is left that looks verified: neither receipt nor content.
        assert!(
            receipts(&lab.deployment, service.port, &alice, "pxy", path).is_empty(),
            "{path} left a receipt"
        );
        let (again, _) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(again, expected, "{path} is served on the second request");
    }
    assert!(!lab.deployment.the_log().contains(UPSTREAM_SECRET));
    support::stop(&mut service);
}

#[test]
fn a_path_outside_the_scopes_is_not_asked_upstream() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-scoped",
        "bad/checksum.bin",
    );
    assert_eq!(status, 404);
    assert!(lab.primary.requests().is_empty(), "the upstream was asked");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-scoped",
        "ok/app.bin",
    );
    assert_eq!(status, 200);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// SSRF: egress only goes where it has been authorized.
// ---------------------------------------------------------------------------

#[test]
fn a_credential_the_operator_did_not_allow_is_never_read_nor_sent() {
    let lab = Lab::new(&[]);
    let allowed = allow(&[lab.primary.port, lab.secondary.port]);
    // The host is allowed and the secret exists in the environment, but the
    // variable is not in `allowed_credentials`: the repository names it
    // and that is not enough.
    let mut service = lab.deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("LAB_TOKEN", UPSTREAM_SECRET),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(
        lab.primary.requests().is_empty(),
        "the upstream was asked without its credential"
    );
    support::stop(&mut service);
}

#[test]
fn nothing_leaves_the_service_without_an_allow_list() {
    let lab = Lab::new(&[]);
    // Without an egress list: the default is not to go out.
    let mut service = lab.deployment.start(&[("LAB_TOKEN", UPSTREAM_SECRET)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(lab.primary.requests().is_empty());
    support::stop(&mut service);

    // With the host in the list but without the explicit administrative permission
    // for private networks, loopback remains forbidden.
    let entry = format!("127.0.0.1:{}", lab.primary.port);
    let mut service = lab.deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", entry.as_str()),
        ("LAB_TOKEN", UPSTREAM_SECRET),
    ]);
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(lab.primary.requests().is_empty());
    support::stop(&mut service);
}

#[test]
fn a_redirect_towards_a_forbidden_place_is_refused_before_it_is_followed() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    for path in [
        // Another origin on loopback (another authorized port, but a
        // cross-origin redirect is not allowed in this repository).
        "redir/loopback.bin",
        // The cloud metadata service.
        "redir/metadata.bin",
        // A private network.
        "redir/private.bin",
        // A host nobody authorized.
        "redir/stranger.bin",
        // A loop.
        "redir/loop.bin",
    ] {
        let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(status, 502, "{path}: {body}");
        assert!(
            receipts(&lab.deployment, service.port, &alice, "pxy", path).is_empty(),
            "{path} left a receipt"
        );
    }
    // The other origin received nothing: the redirect was not followed.
    assert!(lab.secondary.requests().is_empty());

    // A redirect within the same origin is followed, and the receipt says so.
    let (status, body) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy",
        "redir/same.bin",
    );
    assert_eq!(status, 200, "{body}");
    let found = receipts(
        &lab.deployment,
        service.port,
        &alice,
        "pxy",
        "redir/same.bin",
    );
    assert_eq!(found[0]["redirects"], 1);
    assert!(
        found[0]["source_url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/ok/app.bin"))
    );
    support::stop(&mut service);
}

#[test]
fn a_cross_origin_redirect_never_carries_the_credential() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // This repository does allow the redirect towards another authorized origin.
    let (status, body) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-cross",
        "redir/loopback.bin",
    );
    assert_eq!(status, 200, "{body}");
    let received = lab.secondary.requests();
    assert_eq!(received.len(), 1, "the redirect was not followed");
    assert!(
        received[0].authorization.is_none(),
        "the credential crossed to another origin: {received:?}"
    );
    // The first origin did receive it, and the receipt records the hop.
    assert!(lab.primary.requests()[0].authorization.is_some());
    let found = receipts(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-cross",
        "redir/loopback.bin",
    );
    assert_eq!(found[0]["redirects"], 1);
    assert!(
        found[0]["source_url"]
            .as_str()
            .is_some_and(|url| url.contains(&lab.secondary.port.to_string()))
    );
    support::stop(&mut service);
}

#[test]
fn an_upstream_with_an_invalid_tls_endpoint_is_refused() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // The upstream speaks plain http on a port that was configured as
    // https: the TLS handshake cannot complete, and there is no falling
    // back to an unencrypted channel.
    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-tls",
        "ok/app.bin",
    );
    assert_eq!(status, 502);
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-tls",
            "ok/app.bin"
        )
        .is_empty()
    );
    assert!(
        lab.primary
            .requests()
            .iter()
            .all(|one| one.authorization.is_none())
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Trust: what was just downloaded is not trusted merely for having arrived.
// ---------------------------------------------------------------------------

#[test]
fn what_was_just_downloaded_does_not_reach_the_trusted_channel_by_itself() {
    let lab = Lab::new(&["pxy-trusted"]);
    let policy = lab.deployment.directory.path().join("trust-policy.json");
    let permissive = hodei_trust_policy::TrustPolicy::new("default", "1", Vec::new());
    std::fs::write(
        &policy,
        serde_json::to_vec(&permissive).expect("the policy"),
    )
    .expect("written");
    let policy = policy.to_string_lossy().into_owned();
    let mut service = lab.start_with(&[("HODEI_TRUST__POLICY_FILE", policy.as_str())]);
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-trusted",
        "ok/app.bin",
    );
    assert_eq!(status, 404, "an unpromoted download was served as trusted");
    // The 404 is not a resolution failure: the upstream was in fact consulted.
    assert!(
        !lab.primary.requests().is_empty(),
        "the content was never fetched"
    );
    // And the receipt speaks of a digest that the channel does not serve: it does not say
    // that it exists either.
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-trusted",
            "ok/app.bin"
        )
        .is_empty(),
        "the provenance told about content the channel does not serve"
    );
    support::stop(&mut service);
}
