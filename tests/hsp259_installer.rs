//! HSP-259: runs the shell suite that proves `scripts/install.sh` verifies
//! SHA256 and OpenPGP signatures before installing, rejects tampered bundles,
//! and that the Homebrew formula and binstall metadata follow the release layout.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn installer_suite_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(root.join("scripts/test-install.sh"))
        .output()
        .expect("bash must be available to run the installer suite");
    assert!(
        output.status.success(),
        "installer suite failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// HSP-272: packages a real bundle with `package-release.sh` and
/// `package-oci.sh`, checks the binstall templates against the real artifact
/// and installs the bundle with `install.sh`.
///
/// The script exits 77 when podman or syft is missing (NOT_RUN). That is never
/// a silent green: the test fails unless `HODEI_ALLOW_NOT_RUN=1` is set, and
/// even then the NOT_RUN line is printed.
#[test]
fn real_bundle_packages_and_installs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(root.join("scripts/test-release-integration.sh"))
        .output()
        .expect("bash must be available to run the integration script");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => {}
        Some(77) => {
            eprintln!("NOT_RUN: real bundle integration\n{stdout}\n{stderr}");
            assert!(
                std::env::var("HODEI_ALLOW_NOT_RUN").as_deref() == Ok("1"),
                "real bundle integration is NOT_RUN (set HODEI_ALLOW_NOT_RUN=1 to accept):\n{stderr}"
            );
        }
        code => panic!("real bundle integration failed ({code:?}):\n{stdout}\n{stderr}"),
    }
}
