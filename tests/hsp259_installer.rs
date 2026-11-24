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
