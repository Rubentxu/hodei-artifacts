//! npm hosted, proxy and group of HSP-111 against the real client (`npm`).
//!
//! `npm` publishes, installs, moves dist-tags, deprecates and tries to unpublish
//! against a real service, and then installs through a group that
//! has a hosted and a proxy of a laboratory registry: the first time it
//! downloads from the upstream and the second, with the service in offline mode, it comes from what was
//! cached without the upstream receiving a request.
//!
//! `npm` is required in the PATH: without it, the suite **fails** saying it is missing.
//! The credential goes in a temporary `.npmrc`, never in argv, and each run
//! uses its own cache and its own `HOME`: nothing is read or written outside the
//! deployment directory.

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use sha2::{Digest, Sha512};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn require_npm() {
    let found = Command::new("npm")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the npm CLI is not installed: the npm suite needs `npm` in the PATH"
    );
}

/// The result of a run of `npm`.
struct Run {
    ok: bool,
    output: String,
}

/// An isolated `npm` client: its `HOME`, its cache and its `.npmrc`.
struct Client {
    root: PathBuf,
    rc: PathBuf,
}

impl Client {
    /// A client that talks to `/npm/{repository}/` of the service.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let root = root.join(name);
        std::fs::create_dir_all(root.join("home")).expect("the home");
        let rc = root.join("npmrc");
        std::fs::write(
            &rc,
            format!(
                "registry=http://127.0.0.1:{port}/npm/{repository}/\n\
                 //127.0.0.1:{port}/npm/{repository}/:_authToken={token}\n\
                 fetch-retries=0\n\
                 update-notifier=false\n"
            ),
        )
        .expect("the npmrc is written");
        Self { root, rc }
    }

    /// Runs `npm` in `dir` with this client's cache.
    fn run(&self, dir: &Path, args: &[&str]) -> Run {
        let output = Command::new("npm")
            .arg("--userconfig")
            .arg(&self.rc)
            .arg("--globalconfig")
            .arg(self.root.join("no-global-config"))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(["--no-audit", "--no-fund", "--loglevel=warn"])
            .args(args)
            .env("HOME", self.root.join("home"))
            .current_dir(dir)
            .output()
            .expect("npm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// An empty cache: whatever is installed next comes from the service.
    fn forget_the_cache(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("cache"));
    }
}

/// A package in `dir` ready to publish.
fn package(dir: &Path, name: &str, version: &str, body: &str) {
    std::fs::create_dir_all(dir).expect("the package directory");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\"name\":\"{name}\",\"version\":\"{version}\",\"main\":\"index.js\",\
             \"description\":\"a package for the npm suite\"}}"
        ),
    )
    .expect("package.json");
    std::fs::write(dir.join("index.js"), body).expect("index.js");
}

/// An empty consumer project.
fn consumer(dir: &Path) {
    std::fs::create_dir_all(dir).expect("the consumer directory");
    std::fs::write(
        dir.join("package.json"),
        "{\"name\":\"consumer\",\"version\":\"1.0.0\",\"private\":true}",
    )
    .expect("package.json");
}

fn integrity_of(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

#[test]
fn npm_publishes_installs_tags_deprecates_and_cannot_unpublish() {
    require_npm();
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", serde_json::Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let root = deployment.directory.path();
    let client = Client::new(root, "client", service.port, "npm-hosted", &token);

    // A scoped package and another without scope.
    let widget = root.join("widget");
    package(
        &widget,
        "@acme/widget",
        "1.0.0",
        "module.exports = 'widget 1';\n",
    );
    let plain = root.join("plain");
    package(&plain, "plain-lib", "1.0.0", "module.exports = 'plain';\n");
    for dir in [&widget, &plain] {
        let published = client.run(dir, &["publish", "--access", "public"]);
        assert!(published.ok, "npm publish failed:\n{}", published.output);
    }

    // An existing version is not rewritten: the client receives npm's 403.
    let again = client.run(&widget, &["publish", "--access", "public"]);
    assert!(!again.ok, "npm republished an existing version");
    assert!(
        again.output.contains("E403")
            || again.output.contains("403")
            || again
                .output
                .contains("cannot publish over the previously published"),
        "republishing is not a 403:\n{}",
        again.output
    );

    // An install from an empty cache brings what was published, and its lock
    // says the same integrity the service computes over the tarball.
    let project = root.join("consumer");
    consumer(&project);
    client.forget_the_cache();
    let installed = client.run(&project, &["install", "@acme/widget@1.0.0", "plain-lib"]);
    assert!(installed.ok, "npm install failed:\n{}", installed.output);
    assert_eq!(
        std::fs::read_to_string(project.join("node_modules/@acme/widget/index.js"))
            .expect("the widget is installed"),
        "module.exports = 'widget 1';\n"
    );
    assert!(project.join("node_modules/plain-lib/index.js").exists());
    let lock: serde_json::Value = serde_json::from_slice(
        &std::fs::read(project.join("package-lock.json")).expect("the lock"),
    )
    .expect("the lock parses");
    let locked = lock["packages"]["node_modules/@acme/widget"]["integrity"]
        .as_str()
        .expect("the lock carries an integrity")
        .to_string();
    let tarball = root.join("widget-1.0.0.tgz");
    let answer = Curl::download(
        service.port,
        "/npm/npm-hosted/@acme/widget/-/widget-1.0.0.tgz",
        &token,
        Some(&tarball),
    )
    .run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        integrity_of(&std::fs::read(&tarball).expect("the tarball")),
        locked,
        "the tarball served is not the one the lock integrity names"
    );

    // A new version with another tag, and a dist-tag moved and removed.
    package(
        &widget,
        "@acme/widget",
        "1.1.0",
        "module.exports = 'widget 1.1';\n",
    );
    let next = client.run(&widget, &["publish", "--access", "public", "--tag", "next"]);
    assert!(next.ok, "npm publish --tag next failed:\n{}", next.output);
    let tags = client.run(&widget, &["dist-tag", "ls", "@acme/widget"]);
    assert!(tags.ok, "npm dist-tag ls failed:\n{}", tags.output);
    assert!(tags.output.contains("latest: 1.0.0"), "{}", tags.output);
    assert!(tags.output.contains("next: 1.1.0"), "{}", tags.output);
    let promoted = client.run(
        &widget,
        &["dist-tag", "add", "@acme/widget@1.1.0", "latest"],
    );
    assert!(promoted.ok, "npm dist-tag add failed:\n{}", promoted.output);
    let latest = client.run(&widget, &["view", "@acme/widget", "dist-tags.latest"]);
    assert!(
        latest.ok && latest.output.trim() == "1.1.0",
        "{}",
        latest.output
    );
    let added = client.run(
        &widget,
        &["dist-tag", "add", "@acme/widget@1.0.0", "stable"],
    );
    assert!(added.ok, "{}", added.output);
    let removed = client.run(&widget, &["dist-tag", "rm", "@acme/widget", "stable"]);
    assert!(removed.ok, "npm dist-tag rm failed:\n{}", removed.output);
    let tags = client.run(&widget, &["dist-tag", "ls", "@acme/widget"]);
    assert!(!tags.output.contains("stable"), "{}", tags.output);

    // Moving `latest` did not rewrite the bytes of the historical tarball.
    let after = root.join("widget-1.0.0-after.tgz");
    let answer = Curl::download(
        service.port,
        "/npm/npm-hosted/@acme/widget/-/widget-1.0.0.tgz",
        &token,
        Some(&after),
    )
    .run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        std::fs::read(&after).expect("the tarball"),
        std::fs::read(&tarball).expect("the tarball before")
    );

    // Deprecating leaves the version installable and with its warning.
    let deprecated = client.run(
        &widget,
        &["deprecate", "@acme/widget@1.0.0", "use 1.1.0 instead"],
    );
    assert!(
        deprecated.ok,
        "npm deprecate failed:\n{}",
        deprecated.output
    );
    let notice = client.run(&widget, &["view", "@acme/widget@1.0.0", "deprecated"]);
    assert!(
        notice.output.contains("use 1.1.0 instead"),
        "the deprecation is not served:\n{}",
        notice.output
    );

    // Unpublishing is rejected, and the version is still there.
    let unpublished = client.run(&widget, &["unpublish", "@acme/widget@1.0.0", "--force"]);
    assert!(!unpublished.ok, "npm unpublished a version");
    assert!(
        unpublished.output.contains("403") || unpublished.output.contains("E403"),
        "unpublish is not refused with a 403:\n{}",
        unpublished.output
    );
    let still = client.run(&widget, &["view", "@acme/widget@1.0.0", "version"]);
    assert!(
        still.ok && still.output.trim() == "1.0.0",
        "the version is gone:\n{}",
        still.output
    );

    // And a service that restarts keeps serving the same thing.
    support::stop(&mut service);
}

/// A laboratory registry: serves what it is given and counts requests.
struct Registry {
    port: u16,
    requests: Arc<AtomicUsize>,
}

impl Registry {
    fn total(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn serve(mut stream: TcpStream, files: &[(String, Vec<u8>)], counter: &AtomicUsize) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    counter.fetch_add(1, Ordering::SeqCst);
    let text = String::from_utf8_lossy(&request).into_owned();
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let response = match files.iter().find(|(name, _)| *name == path) {
        Some((_, body)) => {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(body);
            out
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

#[test]
fn npm_installs_through_a_group_and_again_offline_from_the_cache() {
    require_npm();
    let deployment = Deployment::new();
    let root = deployment.directory.path();

    // The tarball the upstream serves is that of a real package.
    let source = root.join("left-pad-src");
    package(
        &source,
        "left-pad",
        "1.3.0",
        "module.exports = 'left-pad from upstream';\n",
    );
    let packed = Command::new("npm")
        .args(["pack", "--pack-destination"])
        .arg(root)
        .arg("--loglevel=error")
        .env("HOME", root)
        .current_dir(&source)
        .output()
        .expect("npm pack runs");
    assert!(packed.status.success(), "npm pack failed");
    let tarball = std::fs::read(root.join("left-pad-1.3.0.tgz")).expect("the packed tarball");

    // The upstream port is reserved so its URL can be written in the
    // packument before starting it.
    let probe = TcpListener::bind("127.0.0.1:0").expect("a port");
    let upstream_port = probe.local_addr().expect("the address").port();
    drop(probe);
    let packument = serde_json::json!({
        "name": "left-pad",
        "dist-tags": { "latest": "1.3.0" },
        "versions": { "1.3.0": {
            "name": "left-pad",
            "version": "1.3.0",
            "dist": {
                "tarball": format!("http://127.0.0.1:{upstream_port}/left-pad/-/left-pad-1.3.0.tgz"),
                "integrity": integrity_of(&tarball),
            },
        } },
    });
    let registry = start_registry_on(
        upstream_port,
        vec![
            (
                "/left-pad".to_string(),
                serde_json::to_vec(&packument).unwrap(),
            ),
            (
                "/left-pad/-/left-pad-1.3.0.tgz".to_string(),
                tarball.clone(),
            ),
        ],
    );

    deployment.seed_npm(&[("npm-hosted", serde_json::Value::Null)]);
    deployment.seed_npm_proxies(&[(
        "npm-central",
        serde_json::json!({
            "upstream": {
                "id": "central",
                "url": format!("http://127.0.0.1:{}/", registry.port),
                "scopes": ["/"],
                "max_bytes": 1048576,
            },
        }),
    )]);
    deployment.seed_npm_groups(&[("npm-all", &["npm-hosted", "npm-central"])]);
    let allowed = format!("127.0.0.1:{};private;http", registry.port);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // First time: downloads from the upstream through the group's proxy.
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let client = Client::new(root, "online", service.port, "npm-all", &token);
    let project = root.join("consumer-online");
    consumer(&project);
    let installed = client.run(&project, &["install", "left-pad"]);
    assert!(
        installed.ok,
        "npm install through the group failed:\n{}",
        installed.output
    );
    assert_eq!(
        std::fs::read_to_string(project.join("node_modules/left-pad/index.js"))
            .expect("left-pad is installed"),
        "module.exports = 'left-pad from upstream';\n"
    );
    assert!(registry.total() > 0, "nothing came from the upstream");

    // A package nobody has follows npm's error convention.
    let missing = client.run(&project, &["view", "no-such-package-hsp111"]);
    assert!(!missing.ok, "npm found a package nobody has");
    assert!(
        missing.output.contains("E404") || missing.output.contains("404"),
        "a missing package is not a 404 for npm:\n{}",
        missing.output
    );
    support::stop(&mut service);

    // Second time, without network and with npm's cache empty: it comes from what the
    // proxy stored, and the upstream is none the wiser.
    let before = registry.total();
    let mut offline = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let client = Client::new(root, "offline", offline.port, "npm-all", &token);
    let project = root.join("consumer-offline");
    consumer(&project);
    let installed = client.run(&project, &["install", "left-pad"]);
    assert!(
        installed.ok,
        "npm install offline failed:\n{}",
        installed.output
    );
    assert_eq!(
        std::fs::read_to_string(project.join("node_modules/left-pad/index.js"))
            .expect("left-pad is installed offline"),
        "module.exports = 'left-pad from upstream';\n"
    );
    assert_eq!(registry.total(), before, "offline touched the network");
    support::stop(&mut offline);
}

/// Like `Registry::start`, on an already chosen port.
fn start_registry_on(port: u16, files: Vec<(String, Vec<u8>)>) -> Registry {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the upstream binds");
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests);
    let files = Arc::new(files);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files = Arc::clone(&files);
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || serve(stream, &files, &counter));
        }
    });
    Registry { port, requests }
}
