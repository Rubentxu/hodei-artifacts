//! First-boot diagnostics of the binary: a missing configuration and an
//! unusable content directory must say what is expected and what to do,
//! and never print a secret.

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
