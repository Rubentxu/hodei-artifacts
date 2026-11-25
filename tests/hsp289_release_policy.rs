//! HSP-289: runs the shell suite that proves the release policies that need no
//! cargo build, podman or syft: licence policy (UNKNOWN / outside the
//! allowlist fails, with fixtures), the real surrealdb/rocksdb state of the
//! licence matrix, the clean-tree rule and the architecture/target coherence.
//! The signer pinning and tamper tests live in `scripts/test-release-bundle.sh`
//! (see `hsp263_release_bundle.rs`).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn release_policy_suite_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("bash")
        .arg(root.join("scripts/test-release-policy.sh"))
        .current_dir(&root)
        .output()
        .expect("bash must be available to run the release policy suite");
    assert!(
        output.status.success(),
        "release policy suite failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("NOT_RUN: real-tree"),
        "the real-tree licence check must run in CI:\n{stdout}"
    );
}
