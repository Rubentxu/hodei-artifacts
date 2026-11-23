//! HSP-241 / HCR-19: the community quickstart is a path that works.
//!
//! The tutorial is not a document about a path: it is the path. This suite
//! runs the script that the tutorial tells the community to run, against the
//! real binary, and checks what the tutorial promises:
//!
//! 1. The account is declared and its two repositories (Raw and Maven) are
//!    created over HTTP.
//! 2. A Raw blob and a Maven package are published and pulled back, and the
//!    digest of what comes back is the digest of what went in.
//! 3. The service is restarted, and the digest, the catalog and the
//!    credential survive it.
//! 4. The cleanup removes the laboratory and nothing else, and it refuses a
//!    directory that does not declare itself a laboratory.
//! 5. The production profile without a secret starts closed, and with a
//!    secret and without a policy it refuses to start with an explicit
//!    reason.
//!
//! The service is a real process and the client is a real `curl`: the
//! assertions are about what the wire said.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository, which is where the script and the templates live.
fn the_repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The script the tutorial tells the community to run.
fn the_script() -> PathBuf {
    the_repo().join("scripts/community-quickstart.sh")
}

/// A distribution that is not packaged: the binary the suite was built
/// with, and the configuration the service reads.
///
/// The tarball is what the community downloads, but a suite that has to
/// build a release to prove a tutorial works is a suite that is slow and
/// that nobody runs. The layout is the one the tarball has, so the script
/// cannot tell the difference.
fn a_distribution(root: &Path) -> PathBuf {
    let distribution = root.join("distribution");
    fs::create_dir_all(distribution.join("bin")).expect("the bin directory");
    fs::create_dir_all(distribution.join("config/community")).expect("the config directory");
    for file in [
        "config/default.toml",
        "config/release.toml",
        "config/community/schema.json",
        "config/community/policies.cedar",
    ] {
        fs::copy(the_repo().join(file), distribution.join(file))
            .unwrap_or_else(|error| panic!("the copy of {file}: {error}"));
    }
    let binary = distribution.join("bin/hodei-artifacts-api");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_hodei-artifacts-api"), &binary)
        .unwrap_or_else(|error| panic!("the link to the binary: {error}"));
    distribution
}

/// The script, run, and everything it said.
fn run_the_script(arguments: &[&str]) -> (bool, String) {
    let output = Command::new("bash")
        .arg(the_script())
        .args(arguments)
        .output()
        .expect("the quickstart script runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn the_community_quickstart_publishes_pulls_and_survives_a_restart() {
    let root = tempfile::tempdir().expect("a temp dir");
    let distribution = a_distribution(root.path());
    let lab = root.path().join("lab");
    // A file next to the laboratory: the cleanup must not touch it.
    let neighbour = root.path().join("not-the-lab");
    fs::write(&neighbour, b"this file is not the laboratory").expect("the neighbour file");

    let lab_argument = lab.to_string_lossy().into_owned();
    let (succeeded, text) = run_the_script(&[
        "run",
        "--distribution",
        &distribution.to_string_lossy(),
        "--lab-dir",
        &lab_argument,
    ]);
    assert!(succeeded, "the quickstart did not finish:\n{text}");

    // The account, its repositories and the two publishes.
    assert!(
        text.contains("created lab-raw (raw, hosted) -> 201"),
        "the Raw repository was not created:\n{text}"
    );
    assert!(
        text.contains("created lab-maven (maven2, hosted) -> 201"),
        "the Maven repository was not created:\n{text}"
    );
    assert!(
        text.contains("published lab-raw/quickstart/hello.txt -> 201"),
        "the blob was not published:\n{text}"
    );
    assert!(
        text.contains("published lab-maven/com/acme/lib/1.0.0/lib-1.0.0.jar -> 201"),
        "the jar was not published:\n{text}"
    );

    // The pulls, with the digests that prove them.
    assert!(
        text.contains("the Raw blob came back with the same digest"),
        "the blob did not come back with its digest:\n{text}"
    );
    assert!(
        text.contains("the Maven jar came back with the same content"),
        "the jar did not come back:\n{text}"
    );
    assert!(
        text.contains("the checksum of the jar came back"),
        "the checksum of the jar did not come back:\n{text}"
    );

    // The restart.
    assert!(
        text.contains("the catalog kept both repositories"),
        "the catalog did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("the Raw blob kept its digest"),
        "the blob did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("the Maven package kept its content"),
        "the package did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("without it the answer is 401"),
        "the credential did not survive the restart:\n{text}"
    );

    // The cleanup: the laboratory and nothing else.
    let (succeeded, text) = run_the_script(&["cleanup", "--lab-dir", &lab_argument]);
    assert!(succeeded, "the cleanup did not finish:\n{text}");
    assert!(
        !lab.exists(),
        "the laboratory is still there after the cleanup"
    );
    assert!(
        neighbour.exists(),
        "the cleanup touched a file that is not its own"
    );

    // And it refuses a directory that does not declare itself a laboratory.
    let (succeeded, text) = run_the_script(&["cleanup", "--lab-dir", &neighbour.to_string_lossy()]);
    assert!(
        !succeeded,
        "the cleanup removed a directory that is not a lab:\n{text}"
    );
    assert!(
        neighbour.exists(),
        "the cleanup removed a directory that does not declare itself a lab"
    );
}

#[test]
fn the_production_profile_without_a_secret_stays_closed() {
    let root = tempfile::tempdir().expect("a temp dir");
    let distribution = a_distribution(root.path());
    let lab = root.path().join("guard-lab");

    let (succeeded, text) = run_the_script(&[
        "guard",
        "--distribution",
        &distribution.to_string_lossy(),
        "--lab-dir",
        &lab.to_string_lossy(),
    ]);
    assert!(succeeded, "the guard did not finish:\n{text}");

    // The production profile declares no secret.
    assert!(
        text.contains("no auth_jwt_secret in the production profile"),
        "the production profile was not checked:\n{text}"
    );

    // Without a secret the service starts closed.
    assert!(
        text.contains("GET /health -> 200"),
        "the health check is not public:\n{text}"
    );
    assert!(
        text.contains("PUT /repository/whatever/whatever -> 404"),
        "the content surface answered without a credential:\n{text}"
    );
    assert!(
        text.contains("POST /api/v1/catalog/repositories -> 401"),
        "the catalog answered without an authenticator:\n{text}"
    );
    assert!(
        text.contains("GET /api/v1/search -> 401"),
        "the search answered without an authenticator:\n{text}"
    );

    // With a secret and without a policy, the boot refuses explicitly.
    assert!(
        text.contains("Production security requires"),
        "the refusal did not say what was missing:\n{text}"
    );
    assert!(
        text.contains("cannot read authorization policy bundle"),
        "the refusal did not say what it could not read:\n{text}"
    );
}
