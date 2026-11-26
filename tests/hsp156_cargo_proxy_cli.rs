//! The cargo proxy of HSP-156 against the real client (`cargo`).
//!
//! The `cargo` resolves a crate that ONLY exists in a local upstream: the
//! `crates-io` source is replaced with the Hodei sparse registry
//! (`[source.crates-io] replace-with`), the client reads the index,
//! downloads the .crate and verifies its cksum against the index. With
//! network and offline (the proxy cache serves without the upstream), and
//! a credential that is not valid does not get in.
//!
//! The `cargo` binary is required in the PATH: without it, the suite
//! **fails** saying it is missing. The credential travels in the
//! environment, never in argv.

mod support;

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use sha2::{Digest as _, Sha256};
use support::{Deployment, FIRST_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Requires the `cargo` command line.
fn require_cargo() {
    let found = Command::new("cargo")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the cargo CLI is not installed: the Cargo suite needs `cargo` in the PATH"
    );
}

/// The sha256 in lowercase hex: the `cksum` of the index.
fn the_cksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The result of one run of the client.
struct Run {
    ok: bool,
    output: String,
}

/// An isolated `cargo`: its `CARGO_HOME`, its replaced source and its
/// credential.
struct Client {
    root: PathBuf,
    cargo_home: PathBuf,
    token: String,
}

impl Client {
    /// A client whose `crates-io` is replaced with the Hodei proxy.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let cargo_home = root.join(format!("cargo-home-{name}"));
        std::fs::create_dir_all(&cargo_home).expect("the cargo home");
        std::fs::write(
            cargo_home.join("config.toml"),
            format!(
                "[source.crates-io]\nreplace-with = \"hodei\"\n\n\
                 [registries.hodei]\nindex = \"sparse+http://127.0.0.1:{port}/cargo/{repository}/\"\n\n\
                 [registry]\nglobal-credential-providers = [\"cargo:token\"]\n"
            ),
        )
        .expect("the source replacement is written");
        std::fs::write(
            cargo_home.join("credentials.toml"),
            format!("[registry.hodei]\ntoken = \"{token}\"\n"),
        )
        .expect("the credentials are written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(
                cargo_home.join("credentials.toml"),
                std::fs::Permissions::from_mode(0o600),
            )
            .expect("the credentials are private");
        }
        Self {
            root: root.to_path_buf(),
            cargo_home,
            token: token.to_string(),
        }
    }

    /// The download cache of the client: without it, the next resolution
    /// asks for the .crate again.
    fn forget_the_downloads(&self) {
        let cache = self.cargo_home.join("registry/cache");
        if cache.exists() {
            std::fs::remove_dir_all(&cache).expect("the download cache is removed");
        }
        let src = self.cargo_home.join("registry/src");
        if src.exists() {
            std::fs::remove_dir_all(&src).expect("the unpacked sources are removed");
        }
    }

    /// Runs the `cargo` with its home and its credential in the
    /// environment.
    fn run(&self, directory: &Path, args: &[&str]) -> Run {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(directory)
            .env("CARGO_HOME", &self.cargo_home)
            .env("HOME", &self.root)
            .env("CARGO_REGISTRIES_HODEI_TOKEN", &self.token)
            .env("CARGO_NET_OFFLINE", "false")
            // The client writes into the target of the project it runs,
            // not into the one of the harness that executes it.
            .env("CARGO_TARGET_DIR", directory.join("target"))
            .output()
            .expect("cargo runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }
}

/// The project of a crate, written to disk.
fn a_crate(root: &Path, name: &str, body: &str) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(directory.join("src")).expect("the crate skeleton");
    std::fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\ndescription = \"a crate served through the proxy\"\nlicense = \"MIT\"\n"
        ),
    )
    .expect("the manifest is written");
    std::fs::write(directory.join("src/lib.rs"), body).expect("the library is written");
    directory
}

/// The consumer project, with a main that uses the dependency.
fn a_consumer(root: &Path) -> PathBuf {
    let directory = root.join("consumer");
    std::fs::create_dir_all(directory.join("src")).expect("the consumer skeleton");
    std::fs::write(
        directory.join("Cargo.toml"),
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nhodei-proxy-pkg = \"1.0.0\"\n",
    )
    .expect("the manifest is written");
    std::fs::write(
        directory.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", hodei_proxy_pkg::the_answer());\n}\n",
    )
    .expect("the main is written");
    directory
}

/// A minimal sparse upstream: the config.json, the index document and
/// the .crate of one version.
fn start_upstream(name: &str, version: &str, lines: String, crate_bytes: Vec<u8>) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let prefix = hodei_catalog::cargo::Route::index_path(name).expect("a valid name");
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        "/config.json".to_string(),
        serde_json::json!({
            "dl": format!("http://127.0.0.1:{port}/files/{{crate}}/{{version}}.crate")
        })
        .to_string()
        .into_bytes(),
    );
    pages.insert(format!("/{prefix}"), lines.into_bytes());
    pages.insert(format!("/files/{name}/{version}.crate"), crate_bytes);
    let pages = Arc::new(pages);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let pages = Arc::clone(&pages);
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
                let Some(path) = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    return;
                };
                let answer = match pages.get(path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body.iter().copied())
                    .collect::<Vec<u8>>(),
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec(),
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    port
}

#[test]
fn cargo_fetches_and_builds_through_the_proxy_in_network_and_offline() {
    require_cargo();
    let deployment = Deployment::new();
    let root = deployment.directory.path();

    // The crate that ONLY exists in the upstream: `cargo package` packs
    // it with the real format of the registry.
    let producer = a_crate(
        root,
        "hodei-proxy-pkg",
        "pub fn the_answer() -> i32 {\n    42\n}\n",
    );
    let packed = Command::new("cargo")
        .args(["package", "--allow-dirty", "--no-verify", "--offline"])
        .current_dir(&producer)
        .env("CARGO_HOME", root.join("cargo-home-packager"))
        .env("HOME", root)
        // The packing writes into ITS own target, not the harness one.
        .env("CARGO_TARGET_DIR", producer.join("target"))
        .output()
        .expect("cargo package runs");
    assert!(
        packed.status.success(),
        "cargo package failed:\n{}{}",
        String::from_utf8_lossy(&packed.stdout),
        String::from_utf8_lossy(&packed.stderr)
    );
    let crate_bytes = std::fs::read(producer.join("target/package/hodei-proxy-pkg-1.0.0.crate"))
        .expect("the packaged crate");
    assert!(!crate_bytes.is_empty(), "the packaged crate is empty");
    let line = format!(
        "{{\"name\":\"hodei-proxy-pkg\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"{}\",\"features\":{{}},\"yanked\":false,\"v\":2}}\n",
        the_cksum(&crate_bytes)
    );
    let upstream_port = start_upstream("hodei-proxy-pkg", "1.0.0", line, crate_bytes.clone());

    deployment.seed_cargo_proxies(&[(
        "cargo-proxy",
        serde_json::json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{upstream_port}/"),
                "scopes": ["/"],
                "max_bytes": 4_194_304,
            },
        }),
    )]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[(
        "HODEI_PROXY__ALLOWED_HOSTS",
        &format!("127.0.0.1:{upstream_port};private;http"),
    )]);
    let port = service.port;
    let client = Client::new(root, "client", port, "cargo-proxy", &token);
    let consumer = a_consumer(root);

    // With network: the client reads the sparse index of the proxy,
    // downloads the .crate and verifies the cksum its line declares.
    let fetched = client.run(&consumer, &["fetch"]);
    assert!(fetched.ok, "cargo fetch failed:\n{}", fetched.output);
    assert!(
        fetched.output.contains("hodei-proxy-pkg"),
        "the fetch did not go through the proxy:\n{}",
        fetched.output
    );

    // And what compiles is the upstream crate.
    let built = client.run(&consumer, &["build"]);
    assert!(built.ok, "cargo build failed:\n{}", built.output);
    let ran = client.run(&consumer, &["run", "--quiet"]);
    assert!(ran.ok, "cargo run failed:\n{}", ran.output);
    assert!(
        ran.output.contains("42"),
        "the built program is not the upstream crate:\n{}",
        ran.output
    );

    // A credential that is not valid does not get in: the client reports
    // the 401.
    let stranger = Client::new(
        root,
        "stranger",
        port,
        "cargo-proxy",
        "not-a-real-credential",
    );
    let refused = stranger.run(&consumer, &["fetch"]);
    assert!(!refused.ok, "cargo fetched with a wrong credential");
    assert!(
        refused.output.contains("401"),
        "the refusal is not a 401:\n{}",
        refused.output
    );

    // Offline for the proxy: without the upstream, the cache serves what
    // was verified and the client resolves again without touching the
    // upstream network.
    support::stop(&mut service);
    let mut service = deployment.start_on(
        port,
        &[
            (
                "HODEI_PROXY__ALLOWED_HOSTS",
                &format!("127.0.0.1:{upstream_port};private;http"),
            ),
            ("HODEI_PROXY__OFFLINE", "true"),
        ],
    );
    let again = Client::new(root, "client", port, "cargo-proxy", &token);
    again.forget_the_downloads();
    let offline = again.run(&consumer, &["fetch"]);
    assert!(
        offline.ok,
        "cargo fetch did not work offline through the proxy cache:\n{}",
        offline.output
    );
    support::stop(&mut service);
}
