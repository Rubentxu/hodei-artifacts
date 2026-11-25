//! Contract of the helpers behind `scripts/compose-smoke.sh` (HSP-273).
//!
//! The smoke itself needs Podman and a built image, so it cannot be a unit
//! test. What can be checked everywhere is the logic it delegates:
//!
//! 1. the credential scanner flags a known secret literal, including lines
//!    with `${...}` defaults and short literals, and never touches the real
//!    compose files (the injections go into temporary copies);
//! 2. the failure classifier tells an unreachable backend (connectivity) from
//!    a backend the API does not support, using the real messages of the API;
//! 3. the smoke script actually uses both helpers.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const COMPOSE_FILES: &[&str] = &[
    "compose.yaml",
    "compose.external.yaml",
    "compose.env.example",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Runs the scanner on `files`; returns (exit code, stdout).
fn scan(files: &[&Path]) -> (Option<i32>, String) {
    let output = Command::new("python3")
        .arg("-I")
        .arg(root().join("scripts/compose-scan-secrets.py"))
        .args(files)
        .output()
        .expect("python3 runs the scanner");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

/// Copies the real compose file to a temporary directory and appends `extra`.
fn tampered_copy(directory: &Path, name: &str, extra: &str) -> PathBuf {
    let mut content = fs::read_to_string(root().join(name)).expect("the compose file is readable");
    content.push_str(extra);
    let path = directory.join(name);
    fs::write(&path, content).expect("the copy is written");
    path
}

#[test]
fn the_shipped_compose_files_carry_no_credential_literal() {
    let files: Vec<PathBuf> = COMPOSE_FILES.iter().map(|name| root().join(name)).collect();
    let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let (code, stdout) = scan(&refs);
    assert_eq!(code, Some(0), "unexpected findings: {stdout}");
}

#[test]
fn a_known_secret_literal_fails_the_scan_in_every_shape() {
    let injections: &[(&str, &str, &str)] = &[
        (
            "compose.yaml",
            "\n      GARAGE_RPC_SECRET: 0123456789abcdef0123\n",
            "long literal",
        ),
        (
            "compose.yaml",
            "\n      GARAGE_ADMIN_TOKEN: abc\n",
            "short literal",
        ),
        (
            "compose.external.yaml",
            "\n      AWS_SECRET_ACCESS_KEY: ${AWS_SECRET_ACCESS_KEY:-hunter2hunter2}\n",
            "literal default inside ${...}",
        ),
        (
            "compose.external.yaml",
            "\n      AWS_ACCESS_KEY_ID: ${AWS_ACCESS_KEY_ID:-xy}\n",
            "short literal default inside ${...}",
        ),
        (
            "compose.yaml",
            "\n      - AWS_SECRET_ACCESS_KEY=\"s3cr3t\"\n",
            "quoted list item",
        ),
        (
            "compose.env.example",
            "\nGARAGE_RPC_SECRET=deadbeef\n",
            "env example literal",
        ),
    ];
    for (file, extra, label) in injections {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let copy = tampered_copy(directory.path(), file, extra);
        let (code, stdout) = scan(&[copy.as_path()]);
        assert_eq!(
            code,
            Some(1),
            "{label} was not detected in {file}: {stdout}"
        );
        assert!(
            !stdout.contains("hunter2")
                && !stdout.contains("s3cr3t")
                && !stdout.contains("deadbeef"),
            "the scanner must not print the secret value: {stdout}"
        );
    }
}

#[test]
fn pure_references_and_comments_do_not_trip_the_scan() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let copy = tampered_copy(
        directory.path(),
        "compose.yaml",
        "\n      # GARAGE_RPC_SECRET: commented-out-literal\n      \
         GARAGE_ADMIN_TOKEN: ${GARAGE_ADMIN_TOKEN:?set it}\n      \
         AWS_SECRET_ACCESS_KEY: ${AWS_SECRET_ACCESS_KEY}\n      \
         GARAGE_RPC_SECRET_FILE: /run/secrets/rpc\n",
    );
    let (code, stdout) = scan(&[copy.as_path()]);
    assert_eq!(code, Some(0), "references must pass: {stdout}");
}

fn classify(log: &str) -> String {
    let mut child = Command::new("bash")
        .arg(root().join("scripts/compose-classify-failure.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the classifier starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(log.as_bytes())
        .expect("the log is written");
    let output = child.wait_with_output().expect("the classifier finishes");
    assert!(output.status.success(), "the classifier must exit 0");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// What the API printed (observed with Podman) when it rejected `db_type = dgraph`.
const UNSUPPORTED_LOG: &str = "Configuration error: Unsupported database type 'dgraph'. \
Only 'rocksdb' is supported. Please set HODEI_DATABASE__DB_TYPE to 'rocksdb'\n\
Configuration files read: config/default.toml, config/external.toml.\n";

/// What the API printed (observed with Podman) when the S3 endpoint was down.
const CONNECTIVITY_LOG: &str = "Bootstrap failed: Failed to initialize namespace/database: \
the content bucket \"b\" cannot be used: DispatchFailure(ConnectError(\"tcp connect error\", \
127.0.0.1:1, Os { code: 111, kind: ConnectionRefused, message: \"Connection refused\" }))\n";

#[test]
fn a_down_backend_and_an_unsupported_backend_are_told_apart() {
    assert_eq!(classify(UNSUPPORTED_LOG), "unsupported-backend");
    assert_eq!(classify(CONNECTIVITY_LOG), "connectivity");
    assert_eq!(classify("starting up\nsomething else broke\n"), "unknown");
    // A rejected configuration is the root cause even if a later line mentions a refused connection.
    let both = format!("{CONNECTIVITY_LOG}{UNSUPPORTED_LOG}");
    assert_eq!(classify(&both), "unsupported-backend");
}

#[test]
fn the_smoke_asserts_a_different_failure_mode_for_each_profile() {
    let script = fs::read_to_string(root().join("scripts/compose-smoke.sh"))
        .expect("the smoke script is readable");
    assert!(
        script.contains("compose-scan-secrets.py"),
        "the smoke must use the scanner"
    );
    assert!(
        script.contains("compose-classify-failure.sh"),
        "the smoke must use the classifier"
    );
    assert!(
        script.contains(r#"[ "$CLASS" = "connectivity" ]"#),
        "the unreachable S3 case must assert connectivity"
    );
    assert!(
        script.contains(r#"[ "$CLASS" = "unsupported-backend" ]"#),
        "the external profile case must assert unsupported-backend"
    );
}
