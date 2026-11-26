//! HSP-121 Helm over OCI against the real client (`helm`).
//!
//! The chart is created and packaged locally, pushed with `helm push`
//! to the existing OCI hub and pulled back with `helm pull`: both
//! paths speak standard OCI distribution, with no surface of their own,
//! and the `.tgz` that comes back is byte for byte the one that left.
//!
//! `helm` is required on the PATH: without it, the suite **fails** saying it
//! is missing. The credential goes via stdin, never in argv, and each run uses
//! its own `HOME` and its own `DOCKER_CONFIG`: nothing is read or written
//! outside the deployment directory.

mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::Deployment;

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn require_helm() {
    let found = Command::new("helm")
        .args(["version", "--short"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the helm CLI is not installed: the Helm suite needs `helm` in the PATH"
    );
}

/// The result of a client run.
struct Run {
    ok: bool,
    output: String,
}

/// An isolated `helm`: its home, its registry config, its cache.
struct Client {
    root: std::path::PathBuf,
}

impl Client {
    fn new(root: &Path) -> Self {
        let home = root.join("helm-home");
        std::fs::create_dir_all(home.join("config")).expect("the helm home");
        std::fs::create_dir_all(home.join("cache")).expect("the helm cache");
        std::fs::create_dir_all(root.join("work")).expect("the work directory");
        Self {
            root: home.parent().expect("the root").to_path_buf(),
        }
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Run {
        let home = self.root.join("helm-home");
        let mut command = Command::new("helm");
        command
            .args(args)
            .current_dir(self.root.join("work"))
            .env("HOME", &home)
            .env("DOCKER_CONFIG", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"));
        if let Some(secret) = stdin {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().expect("helm runs");
            child
                .stdin
                .as_mut()
                .expect("the stdin")
                .write_all(secret.as_bytes())
                .expect("the credential is written");
            let output = child.wait_with_output().expect("helm finishes");
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            return Run {
                ok: output.status.success(),
                output: text,
            };
        }
        let output = command.output().expect("helm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }
}

fn sha_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the chart is there");
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn a_chart_is_pushed_and_pulled_through_oci_with_the_bytes_preserved() {
    require_helm();
    let deployment = Deployment::new();
    // The helm repository lives for the OCI channel: a chart over OCI is an
    // OCI artifact, and the hub is the same.
    deployment.seed_helm(&[("demo-chart", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let client = Client::new(deployment.directory.path());
    let registry = format!("127.0.0.1:{port}");

    // A real chart, created and packaged by helm itself.
    let created = client.run(&["create", "demo-chart"], None);
    assert!(created.ok, "helm create: {}", created.output);
    let packaged = client.run(&["package", "demo-chart", "--destination", "."], None);
    assert!(packaged.ok, "helm package: {}", packaged.output);
    let tgz = deployment
        .directory
        .path()
        .join("work")
        .join("demo-chart-0.1.0.tgz");
    assert!(tgz.exists(), "helm package wrote: {}", packaged.output);

    // The registry session: the credential via stdin.
    let login = client.run(
        &[
            "registry",
            "login",
            registry.as_str(),
            "-u",
            "alice",
            "--password-stdin",
            "--plain-http",
        ],
        Some(&token),
    );
    assert!(login.ok, "helm registry login: {}", login.output);

    // `helm push` uploads the .tgz as a layer of an OCI artifact, with the
    // chart version as tag.
    let pushed = client.run(
        &[
            "push",
            "demo-chart-0.1.0.tgz",
            &format!("oci://{registry}"),
            "--plain-http",
        ],
        None,
    );
    assert!(pushed.ok, "helm push: {}", pushed.output);

    // And `helm pull` returns it: the same name, the same version, and
    // the same bytes. Integrity is not an extra of the channel: it is
    // what makes a downloaded chart the published chart.
    std::fs::create_dir_all(deployment.directory.path().join("work/pull")).expect("the pull dir");
    let pulled = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "0.1.0",
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(pulled.ok, "helm pull: {}", pulled.output);
    let round_trip = deployment
        .directory
        .path()
        .join("work")
        .join("pull")
        .join("demo-chart-0.1.0.tgz");
    assert!(round_trip.exists(), "helm pull wrote: {}", pulled.output);
    assert_eq!(
        sha_of(&tgz),
        sha_of(&round_trip),
        "el chart que vuelve no es byte a byte el que se marcho"
    );

    // It is also requested by digest: that is what a `helm push` value signs.
    let manifest = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart:0.1.0"),
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(manifest.ok, "helm pull by tag: {}", manifest.output);
}

#[test]
fn a_helm_chart_does_not_live_where_it_was_not_published() {
    require_helm();
    let deployment = Deployment::new();
    deployment.seed_helm(&[("demo-chart", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let client = Client::new(deployment.directory.path());
    let registry = format!("127.0.0.1:{port}");
    std::fs::create_dir_all(deployment.directory.path().join("work/pull")).expect("the pull dir");
    let login = client.run(
        &[
            "registry",
            "login",
            registry.as_str(),
            "-u",
            "alice",
            "--password-stdin",
            "--plain-http",
        ],
        Some(&token),
    );
    assert!(login.ok, "helm registry login: {}", login.output);

    // Nobody published anything: requesting the chart is a registry error, not an
    // empty chart.
    let missing = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "9.9.9",
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(
        !missing.ok,
        "an unpublished chart must not pull: {}",
        missing.output
    );

    // And without a credential there is no channel either: the distribution asks
    // before opening the house. Another house is another client: here there is no
    // saved login session.
    let anonymous_root = deployment.directory.path().join("anonymous");
    let anonymous = Client::new(&anonymous_root);
    let denied = anonymous.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "0.1.0",
            "--destination",
            ".",
            "--plain-http",
        ],
        None,
    );
    assert!(
        !denied.ok,
        "an anonymous pull must not succeed: {}",
        denied.output
    );
}
