//! The Terraform provider network mirror (HSP-157), at HTTP level.
//!
//! The mirror is the **proxy** modality of the terraform format: every read
//! is fetched from the upstream once, cached, and served byte for byte. The
//! real `terraform` client is exercised in `hsp158_terraform_modules_client`
//! for the hosted registry; here the mirror protocol is spoken the way the
//! client speaks it, against a fake upstream that counts what it is asked.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use support::{Answer, Curl, Deployment};

const PROVIDER: &str = "registry.terraform.io/hashicorp/null";

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tf-mirror-{name}-{n}"))
}

fn call(port: u16, method: &str, path: &str, token: &str) -> Answer {
    let into = next("answer");
    Curl::call_with_headers(port, method, path, token, None, &[], Some(&into)).run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
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

#[test]
fn the_terraform_mirror_proxy_caches_the_provider_and_serves_it_byte_for_byte() {
    let zip = b"PK\x03\x04 fake provider zip bytes".to_vec();
    let index = br#"{"versions":{"3.2.1":{}}}"#.to_vec();
    let version =
        br#"{"archives":{"linux_amd64":{"url":"terraform-provider-null_3.2.1_linux_amd64.zip"}}}"#
            .to_vec();
    let mut pages = HashMap::new();
    pages.insert(format!("/{PROVIDER}/index.json"), index.clone());
    pages.insert(format!("/{PROVIDER}/3.2.1.json"), version.clone());
    pages.insert(
        format!("/{PROVIDER}/terraform-provider-null_3.2.1_linux_amd64.zip"),
        zip.clone(),
    );
    let upstream = start_upstream(pages);

    let deployment = Deployment::new();
    deployment.seed_terraform_proxies(&[(
        "tf-mirror",
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

    let zip_path =
        format!("/terraform/tf-mirror/{PROVIDER}/terraform-provider-null_3.2.1_linux_amd64.zip");
    let first = call(port, "GET", &zip_path, &token);
    assert_eq!(first.status, 200, "{}", text_of(&first));
    assert_eq!(
        first.body, zip,
        "the mirror serves the zip as the upstream gave it"
    );
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));

    // The second read does not go out: the cache serves the same bytes.
    let second = call(port, "GET", &zip_path, &token);
    assert_eq!(second.status, 200, "{}", text_of(&second));
    assert_eq!(second.body, zip);
    assert_eq!(second.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(
        upstream.asked_for(&format!(
            "/{PROVIDER}/terraform-provider-null_3.2.1_linux_amd64.zip"
        )),
        1
    );

    // The protocol documents ride the same path.
    let provider_index = call(
        port,
        "GET",
        &format!("/terraform/tf-mirror/{PROVIDER}/index.json"),
        &token,
    );
    assert_eq!(provider_index.status, 200, "{}", text_of(&provider_index));
    assert_eq!(provider_index.body, index);
    let version_doc = call(
        port,
        "GET",
        &format!("/terraform/tf-mirror/{PROVIDER}/3.2.1.json"),
        &token,
    );
    assert_eq!(version_doc.status, 200, "{}", text_of(&version_doc));
    assert_eq!(version_doc.body, version);

    // A mirror is read-only: the protocol has no writes.
    let put = call(port, "PUT", &zip_path, &token);
    assert_eq!(put.status, 405, "{}", text_of(&put));

    // What the upstream does not have is not found, and a traversal is
    // not a path of the protocol.
    let missing = call(
        port,
        "GET",
        &format!("/terraform/tf-mirror/{PROVIDER}/9.9.9.json"),
        &token,
    );
    assert_eq!(missing.status, 404, "{}", text_of(&missing));
    let escaped = call(
        port,
        "GET",
        "/terraform/tf-mirror/registry.terraform.io/hashicorp/%2e%2e/index.json",
        &token,
    );
    assert_eq!(escaped.status, 404, "{}", text_of(&escaped));

    support::stop(&mut service);
}
