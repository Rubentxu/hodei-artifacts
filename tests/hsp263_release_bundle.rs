//! HSP-263: runs the shell suite that proves `scripts/release-bundle.sh`
//! orchestrates the full release bundle, records unavailable architectures as
//! NOT_RUN with a reason, and that `scripts/verify-release.sh` validates the
//! bundle in a clean directory and rejects manipulated artifacts.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn release_bundle_suite_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(root.join("scripts/test-release-bundle.sh"))
        .output()
        .expect("bash must be available to run the release bundle suite");
    assert!(
        output.status.success(),
        "release bundle suite failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
