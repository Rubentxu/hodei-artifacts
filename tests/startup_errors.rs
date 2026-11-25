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
