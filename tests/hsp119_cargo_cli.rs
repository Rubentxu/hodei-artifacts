//! Cargo hosted of HSP-119 against the real client (`cargo`).
//!
//! `cargo` publishes a crate against a real service, adds it to
//! a consumer project with `cargo add` and compiles it with `cargo build`:
//! the sparse index, the download and the `cksum` check are done by
//! the client itself, not this test. `cargo` is required in the PATH: without
//! it, the suite **fails** saying it is missing. The credential goes in the
//! environment (`CARGO_REGISTRIES_HODEI_TOKEN`), never in argv, and each
//! run uses its own `CARGO_HOME`: nothing is read or written outside
//! the deployment directory.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Requires the command-line `cargo`.
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

/// The result of a client run.
struct Run {
    /// Whether the client finished successfully.
    ok: bool,
    /// What it said, on stdout and stderr.
    output: String,
}

/// An isolated `cargo`: its `CARGO_HOME`, its registry and its credential.
struct Client {
    /// The deployment directory, where the `CARGO_HOME` lives.
    root: PathBuf,
    /// The client's `CARGO_HOME`, with the registry config.
    cargo_home: PathBuf,
    /// The credential.
    token: String,
}

impl Client {
    /// A client whose `hodei` registry points to the service's sparse index.
    ///
    /// Each client has its own `CARGO_HOME`: two clients with different
    /// credentials cannot step on each other, and nothing touches the real user's registry.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let cargo_home = root.join(format!("cargo-home-{name}"));
        std::fs::create_dir_all(&cargo_home).expect("the cargo home");
        std::fs::write(
            cargo_home.join("config.toml"),
            format!(
                "[registries.hodei]\nindex = \"sparse+http://127.0.0.1:{port}/cargo/{repository}/\"\n\n[registry]\nglobal-credential-providers = [\"cargo:token\"]\n"
            ),
        )
        .expect("the registry config is written");
        // The credential lives where `cargo login` puts it: the sparse index
        // is also requested with it, and the `cargo:token` provider is what
        // reads it (authenticated alternative registries do not look for it without a
        // configured provider).
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

    /// Runs `cargo` with its home and its credential in the environment.
    fn run(&self, directory: &Path, args: &[&str]) -> Run {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(directory)
            .env("CARGO_HOME", &self.cargo_home)
            .env("HOME", &self.root)
            // The `cargo:token` provider accepts the credential through the environment:
            // no tokens in argv.
            .env("CARGO_REGISTRIES_HODEI_TOKEN", &self.token)
            .env("CARGO_NET_OFFLINE", "false")
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

/// The project of a publishable crate, written to disk.
fn a_crate(root: &Path, name: &str, body: &str) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(directory.join("src")).expect("the crate skeleton");
    std::fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\ndescription = \"a crate published through the real cargo client\"\nlicense = \"MIT\"\n"
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
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .expect("the manifest is written");
    std::fs::write(
        directory.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", hodei_cli_pkg::the_answer());\n}\n",
    )
    .expect("the main is written");
    directory
}

#[test]
fn cargo_publishes_adds_and_builds_against_the_registry() {
    require_cargo();
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", serde_json::Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let root = deployment.directory.path();
    let client = Client::new(root, "client", service.port, "cargo-hosted", &token);

    // `cargo` publishes: packages, uploads and the service accepts the version.
    let producer = a_crate(
        root,
        "hodei-cli-pkg",
        "pub fn the_answer() -> i32 {\n    42\n}\n",
    );
    let published = client.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(published.ok, "cargo publish failed:\n{}", published.output);

    // `cargo add` finds it by name in the sparse index.
    let consumer = a_consumer(root);
    let added = client.run(&consumer, &["add", "hodei-cli-pkg", "--registry", "hodei"]);
    assert!(added.ok, "cargo add failed:\n{}", added.output);
    let manifest = std::fs::read_to_string(consumer.join("Cargo.toml")).expect("the manifest");
    assert!(
        manifest.contains("hodei-cli-pkg"),
        "cargo add did not write the dependency:\n{manifest}"
    );

    // `cargo build` downloads the .crate and the client **verifies the
    // cksum against the index**: if the digest did not match, the download
    // is rejected before building. The marker in the output is the real download.
    let built = client.run(&consumer, &["build"]);
    assert!(built.ok, "cargo build failed:\n{}", built.output);
    assert!(
        built.output.contains("hodei-cli-pkg"),
        "the consumer did not fetch the published crate:\n{}",
        built.output
    );

    // And the code that compiles is the published one: `cargo run` executes the 42.
    let ran = client.run(&consumer, &["run", "--quiet"]);
    assert!(ran.ok, "cargo run failed:\n{}", ran.output);
    assert!(
        ran.output.contains("42"),
        "the built program is not the published crate:\n{}",
        ran.output
    );

    // A second version: the index adds it without removing the first.
    std::fs::write(
        producer.join("Cargo.toml"),
        "[package]\nname = \"hodei-cli-pkg\"\nversion = \"1.1.0\"\nedition = \"2021\"\ndescription = \"a crate published through the real cargo client\"\nlicense = \"MIT\"\n",
    )
    .expect("the second manifest is written");
    let republished = client.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(
        republished.ok,
        "the second publish failed:\n{}",
        republished.output
    );
    let latest = client.run(&consumer, &["update", "-p", "hodei-cli-pkg"]);
    assert!(latest.ok, "cargo update failed:\n{}", latest.output);
    let lock = std::fs::read_to_string(consumer.join("Cargo.lock")).expect("the lock");
    assert!(
        lock.contains("version = \"1.1.0\""),
        "the index does not offer the new version:\n{lock}"
    );

    // A credential that is not valid does not publish: the service answers 401 and
    // the client says so.
    let wrong = Client::new(
        root,
        "wrong",
        service.port,
        "cargo-hosted",
        "not-a-real-credential",
    );
    let refused = wrong.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(!refused.ok, "cargo published with a wrong credential");
    assert!(
        refused.output.contains("401"),
        "the refusal is not a 401:\n{}",
        refused.output
    );
}
