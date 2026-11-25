//! First-boot diagnostics of the binary: a missing configuration and an
//! unusable content directory must say what is expected and what to do,
//! and never print a secret.
//!
//! Every message asserted here comes out of the real binary, with the real
//! loader: nothing is a hand-made error.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

fn the_clean_command(dir: &std::path::Path) -> Command {
    let mut command = Command::new(BIN);
    command.current_dir(dir).env_clear();
    command
}

#[test]
fn a_missing_config_file_exits_with_the_expected_path_and_the_action() {
    let tmp = tempfile::tempdir().unwrap();
    let output = the_clean_command(tmp.path()).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = tmp.path().join("config/default.toml");
    assert!(stderr.contains(&expected.display().to_string()), "{stderr}");
    assert!(stderr.contains("Action:"), "{stderr}");
}

#[test]
fn an_unusable_content_directory_exits_with_the_path_and_never_prints_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "x").unwrap();
    let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/default.toml");
    std::fs::create_dir(tmp.path().join("config")).unwrap();
    std::fs::copy(shipped, tmp.path().join("config/default.toml")).unwrap();
    let output = the_clean_command(tmp.path())
        .env("HODEI_ROCKSDB__PATH", tmp.path().join("db"))
        .env("HODEI_CONTENT__CAS_PATH", blocker.join("cas"))
        .env("HODEI_SECURITY__JWT_SECRET", "super-secret-value-123")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&blocker.display().to_string()), "{stderr}");
    assert!(stderr.contains("HODEI_CONTENT__CAS_PATH"), "{stderr}");
    assert!(!stderr.contains("super-secret-value-123"), "{stderr}");
}

const PEM_BODY: &str = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7VJTUt9Us8cKj";

fn the_shipped_config() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/default.toml"),
    )
    .unwrap()
}

/// A working directory whose `config/default.toml` is `contents`.
fn the_directory_with_config(contents: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("config")).unwrap();
    std::fs::write(tmp.path().join("config/default.toml"), contents).unwrap();
    tmp
}

#[test]
fn a_config_without_the_server_section_names_the_section_and_its_variables() {
    let shipped = the_shipped_config();
    let start = shipped
        .find("[server]")
        .expect("the shipped config has [server]");
    let end = shipped[start + 1..]
        .find("\n[")
        .map(|offset| start + 1 + offset)
        .expect("a section follows [server]");
    let without_server = format!("{}{}", &shipped[..start], &shipped[end..]);
    assert!(!without_server.contains("[server]"));
    let tmp = the_directory_with_config(&without_server);
    let output = the_clean_command(tmp.path()).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing field `server`"), "{stderr}");
    assert!(
        stderr.contains("Required section missing: '[server]'"),
        "{stderr}"
    );
    assert!(stderr.contains("HODEI_SERVER__PORT"), "{stderr}");
    assert!(stderr.contains("config/default.toml"), "{stderr}");
    assert!(!stderr.contains("No configuration file found"), "{stderr}");
}

#[test]
fn a_partial_production_auth_bundle_names_the_variables_and_the_action() {
    let tmp = the_directory_with_config(&the_shipped_config());
    let output = the_clean_command(tmp.path())
        .env("HODEI_ROCKSDB__PATH", tmp.path().join("db"))
        .env("HODEI_CONTENT__CAS_PATH", tmp.path().join("cas"))
        .env("HODEI_SECURITY__AUTH_JWT_SECRET", "super-secret-value-123")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("HODEI_SECURITY__AUTH_ISSUER"), "{stderr}");
    assert!(
        stderr.contains("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH"),
        "{stderr}"
    );
    assert!(!stderr.contains("super-secret-value-123"), "{stderr}");
}

#[test]
fn a_private_key_pasted_into_a_value_never_reaches_the_output() {
    let tmp = the_directory_with_config(&the_shipped_config());
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{PEM_BODY}\n-----END PRIVATE KEY-----");
    let output = the_clean_command(tmp.path())
        .env("HODEI_SERVER__PORT", &pem)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("Configuration error"), "{text}");
    for leaked in ["BEGIN", "-----", PEM_BODY] {
        assert!(!text.contains(leaked), "{leaked} leaked: {text}");
    }
}

#[test]
fn an_unreachable_s3_backend_exits_with_the_bucket_the_variable_and_no_credentials() {
    let tmp = the_directory_with_config(&the_shipped_config());
    let output = the_clean_command(tmp.path())
        .env("HODEI_ROCKSDB__PATH", tmp.path().join("db"))
        // The store of content is opened only for a durable repository
        // catalog; without it the backend would not be consulted at all.
        .env("HODEI_REPOSITORY__DURABLE", "true")
        .env("HODEI_CONTENT__BACKEND", "s3")
        .env("HODEI_CONTENT__S3__BUCKET", "missing-bucket")
        .env("HODEI_CONTENT__S3__ENDPOINT", "127.0.0.1:1")
        .env("HODEI_CONTENT__S3__REGION", "us-east-1")
        .env("HODEI_CONTENT__S3__TLS", "false")
        .env("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE")
        .env("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI-secret-from-env")
        .env("AWS_SESSION_TOKEN", "session-token-from-env")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("missing-bucket"), "{text}");
    assert!(text.contains("HODEI_CONTENT__S3__ENDPOINT"), "{text}");
    for secret in [
        "AKIAIOSFODNN7EXAMPLE",
        "wJalrXUtnFEMI-secret-from-env",
        "session-token-from-env",
    ] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
}

/// Runs the binary and gives it `seconds` to stop on its own; a service that
/// boots would never stop, and the test must not wait for it.
fn the_output_within(mut command: Command, seconds: u64) -> Option<std::process::Output> {
    use std::process::Stdio;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    loop {
        if child.try_wait().unwrap().is_some() {
            return Some(child.wait_with_output().unwrap());
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn a_malformed_signing_ring_names_the_section_and_never_prints_the_secret() {
    let tmp = the_directory_with_config(&the_shipped_config());
    let mut command = the_clean_command(tmp.path());
    command
        .env("HODEI_ROCKSDB__PATH", tmp.path().join("db"))
        .env("HODEI_CONTENT__CAS_PATH", tmp.path().join("cas"))
        .env("HODEI_SIGNING__KEYS__0__ID", "k1")
        .env("HODEI_SIGNING__KEYS__0__SECRET_ENV", "THE_KEY")
        .env("HODEI_SIGNING__KEYS__0__SECRET_FILE", "/run/secrets/k1")
        .env("THE_KEY", "a-secret-that-must-not-be-printed-0123456789");
    let output = the_output_within(command, 60).expect("the boot is refused, not served");
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("signing configuration"), "{text}");
    assert!(text.contains("exactly one"), "{text}");
    assert!(
        !text.contains("a-secret-that-must-not-be-printed"),
        "{text}"
    );
}

#[test]
fn a_signing_key_that_cannot_be_used_stops_the_boot_without_printing_the_secret() {
    let tmp = the_directory_with_config(&the_shipped_config());
    let mut command = the_clean_command(tmp.path());
    command
        .env("HODEI_ROCKSDB__PATH", tmp.path().join("db"))
        .env("HODEI_CONTENT__CAS_PATH", tmp.path().join("cas"))
        .env("HODEI_SIGNING__KEYS__0__ID", "k1")
        .env("HODEI_SIGNING__KEYS__0__SECRET_ENV", "THE_KEY")
        .env("THE_KEY", "short-secret-do-not-print");
    let output = the_output_within(command, 60).expect("the boot is refused, not served");
    assert!(
        !output.status.success(),
        "a short secret must stop the boot"
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("signing"), "{text}");
    assert!(text.contains("at least 32"), "{text}");
    assert!(!text.contains("short-secret-do-not-print"), "{text}");
}
