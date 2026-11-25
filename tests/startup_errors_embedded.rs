//! First-boot diagnostics of the embedded profile (HSP-314 F2): the checks
//! that need the embedded engine (`legacy`) or a live Alpha to get that far.
//!
//! The diagnostics that fire before any backend is touched run on the graph
//! profile in `tests/startup_errors.rs`, with no `legacy` feature. These two
//! are different: the unreachable object store is only reached once the
//! metadata authority is open, and the signing-key length is validated after
//! it, so they run against the embedded engine until a real Alpha is wired in
//! (HSP-314 F4 retires them with the engine).

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

fn the_clean_command(dir: &std::path::Path) -> Command {
    let mut command = Command::new(BIN);
    command.current_dir(dir).env_clear();
    command
}

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
