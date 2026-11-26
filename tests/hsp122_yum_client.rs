//! The `dnf install` of HSP-122 against the **real client**: a
//! `quay.io/fedora/fedora:latest` in podman whose `.repo` points to the hub.
//!
//! The distro is **pinned** to a specific image, which is what the
//! acceptance criterion asks for: Fedora's `dnf` and `rpm`, with
//! `repo_gpgcheck=1`. The flow is that of any sysadmin:
//!
//! 1. inside the container a real `.rpm` is built with
//!    `rpmbuild` (ours, synthetic, would not install);
//! 2. the hub publishes it through `PUT /yum/{repo}/pool/...`, whose path
//!    is dictated by the binary's own header;
//! 3. `dnf -y install hello-hodei` has to download the `repomd.xml`,
//!    **verify its signature** against the hub's `key.asc`, download the
//!    `primary.xml.gz`, check the package's SHA256 against the
//!    index and install it;
//! 4. with a third party's key, the installation has to refuse.
//!
//! Without podman or without the image, the suite fails saying it is missing, like
//! the other real-client suites.

mod support;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

const IMAGE: &str = "quay.io/fedora/fedora:latest";

fn require_podman() {
    let found = Command::new("podman")
        .args(["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "podman is not installed: the yum client suite needs a fedora container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the yum client suite"
    );
}

/// Runs a script inside the container with `work` mounted at `/work`
/// and host networking to reach the hub at 127.0.0.1.
fn container_run(work: &Path, script: &str) -> std::process::Output {
    let output = Command::new("podman")
        .arg("run")
        .arg("--rm")
        .arg("--network=host")
        .arg("-v")
        .arg(format!("{}:/work:z", work.display()))
        .arg(IMAGE)
        .arg("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("the container runs");
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("the container failed ({script}):\n{stdout}\n{stderr}");
    }
    output
}

fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let headers: &[(&str, &str)] = if sent.is_some() {
        &[("content-type", "application/octet-stream")]
    } else {
        &[]
    };
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

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The spec of the real package: `rpmbuild` requires it well formed. The path
/// of the hub's `.repo` is built here with credentials in the
/// userinfo, which is how dnf expects basic authentication.
const RPM_SPEC: &str = concat!(
    "Name: hello-hodei\n",
    "Version: 1.0.0\n",
    "Release: 1\n",
    "Summary: A tiny hello package\n",
    "License: MIT\n",
    "BuildRoot: %{_tmppath}/hello-hodei-root\n",
    "%description\n",
    "Installed through the real dnf client against Hodei.\n",
    "%install\n",
    "mkdir -p %{buildroot}/usr/share/hello-hodei\n",
    "cp /work/greeting.txt %{buildroot}/usr/share/hello-hodei/greeting.txt\n",
    "%files\n",
    "/usr/share/hello-hodei/greeting.txt\n",
);

const POOL_PATH: &str = "pool/hello-hodei-1.0.0-1.x86_64.rpm";

fn repo_file(port: u16, token: &str, key_url: &str) -> String {
    format!(
        concat!(
            "[hodei]\n",
            "name=Hodei fedora hosted\n",
            "baseurl=http://alice:{token}@127.0.0.1:{port}/yum/fedora-hosted/\n",
            "enabled=1\n",
            "gpgcheck=0\n",
            "repo_gpgcheck=1\n",
            "gpgkey={key_url}\n",
        ),
        token = token,
        port = port,
        key_url = key_url,
    )
}

#[test]
fn the_real_dnf_client_installs_and_verifies_the_signed_metadata() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_yum(&[("fedora-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("hello.spec"), RPM_SPEC).expect("the spec");
    std::fs::write(work.join("greeting.txt"), "hola desde hodei\n").expect("the payload");

    // 1. A real `.rpm`, built by the rpm of the pinned distro.
    container_run(
        &work,
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install rpm-build gnupg2 >/dev/null\n",
            "mkdir -p /work/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}\n",
            "rpmbuild --define '_topdir /work/rpmbuild' -bb /work/hello.spec >/dev/null\n",
            "chmod 644 /work/rpmbuild/RPMS/x86_64/*.rpm\n",
        ),
    );
    let rpm = std::fs::read(work.join("rpmbuild/RPMS/x86_64/hello-hodei-1.0.0-1.x86_64.rpm"))
        .expect("the built rpm");
    assert!(!rpm.is_empty(), "rpmbuild produced a package");

    // 2. It is published through the hub's yum surface: the pool path is
    //    the one dictated by the header of the binary built outside of here.
    let published = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{POOL_PATH}"),
        &token,
        Some(&rpm),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // 3. The hub's public key, armored, for the repo's gpgkey.
    let key = call(port, "GET", "/yum/fedora-hosted/key.asc", &token, None);
    assert_eq!(key.status, 200, "{}", text_of(&key));
    std::fs::write(work.join("key.asc"), key.body.as_slice()).expect("the key");

    // 4. The real manager of the pinned distro. `dnf` (dnf5) installs the
    //    package checking the index's sha256, and `microdnf`, the other
    //    official Fedora client, is the one that **enforces** the signature of the
    //    repomd: dnf5 5.4.2 warns "repomd.xml GPG signature verification
    //    error" and continues, microdnf refuses. Both flows are tested.
    let script = format!(
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install gnupg2 microdnf >/dev/null\n",
            "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
            "{repo}\n",
            "EOF\n",
            "dnf -y -q --disablerepo='*' --enablerepo=hodei install hello-hodei\n",
            "rpm -q hello-hodei\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
            "rpm -e hello-hodei\n",
            "microdnf --disablerepo='*' --enablerepo=hodei --refresh -y install hello-hodei\n",
            "rpm -q hello-hodei\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
        ),
        repo = repo_file(port, &token, "file:///work/key.asc"),
    );
    let installed = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&installed.stdout);
    let occurrences = stdout.matches("hello-hodei-1.0.0-1.x86_64").count();
    assert!(
        occurrences >= 2,
        "dnf and microdnf must each report the package installed: {stdout}"
    );
    assert!(
        stdout.matches("hola desde hodei").count() >= 2,
        "both clients must land the published payload: {stdout}"
    );
}

#[test]
fn dnf_refuses_the_metadata_signed_by_a_stranger_key() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_yum(&[("fedora-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage-stranger");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("hello.spec"), RPM_SPEC).expect("the spec");
    std::fs::write(work.join("greeting.txt"), "hola desde hodei\n").expect("the payload");
    container_run(
        &work,
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install rpm-build >/dev/null\n",
            "mkdir -p /work/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}\n",
            "rpmbuild --define '_topdir /work/rpmbuild' -bb /work/hello.spec >/dev/null\n",
            "chmod 644 /work/rpmbuild/RPMS/x86_64/*.rpm\n",
        ),
    );
    let rpm = std::fs::read(work.join("rpmbuild/RPMS/x86_64/hello-hodei-1.0.0-1.x86_64.rpm"))
        .expect("the built rpm");
    let published = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{POOL_PATH}"),
        &token,
        Some(&rpm),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // A third party that is not the hub's signer. With its key pinned in
    // the repo, the hub's metadata does not open: microdnf -- the client that
    // enforces `repo_gpgcheck` -- has to refuse, not install blindly.
    let stranger =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Stranger <x@example.local>")
            .expect("a second key");
    std::fs::write(
        work.join("stranger.asc"),
        stranger.key_asc().expect("an armored key"),
    )
    .expect("the stranger key");

    let script = format!(
        concat!(
            "dnf -y -q install gnupg2 microdnf >/dev/null\n",
            "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
            "{repo}\n",
            "EOF\n",
            "if microdnf --disablerepo='*' --enablerepo=hodei --refresh -y install hello-hodei ",
            ">/tmp/dnf.log 2>&1; then echo ACCEPTED; exit 1; fi\n",
            "cat /tmp/dnf.log; echo refused\n",
            "if rpm -q hello-hodei >/dev/null 2>&1; then echo INSTANYWAY; exit 1; fi\n",
        ),
        repo = repo_file(port, &token, "file:///work/stranger.asc"),
    );
    let refused = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(
        stdout.contains("refused"),
        "microdnf must reject a repomd whose signature does not open with the pinned key"
    );
    assert!(
        stdout
            .to_lowercase()
            .contains("signature verification error"),
        "the refusal must name the metadata signature, not a missing package: {stdout}"
    );
}
