//! HSP-314 F4 / AC5: the embedded engine is gone from what the image ships.
//!
//! The SBOMs of the release are produced by `scripts/package-oci.sh` with
//! `syft`: the OCI SBOM scans the image archive, and the cargo SBOM scans
//! `Cargo.lock`. The lock is therefore the input both of them derive the Rust
//! components from, so a package named `surrealdb` or `rocksdb` anywhere in it
//! would surface in the SBOM. This suite pins the lock (always) and, when
//! `syft` is installed, the real SBOM that `syft` generates from it, instead
//! of inventing a second generator. The full OCI image SBOM is checked by
//! `hsp244_verifiable_release` (podman) and by `release-license-matrix.py`.

use std::path::PathBuf;
use std::process::Command;

const FORBIDDEN: &[&str] = &["surrealdb", "rocksdb"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn names_forbidden(text: &str) -> Vec<String> {
    let lowered = text.to_ascii_lowercase();
    FORBIDDEN
        .iter()
        .filter(|needle| lowered.contains(*needle))
        .map(|needle| needle.to_string())
        .collect()
}

#[test]
fn the_lock_has_no_embedded_engine_package() {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).expect("read Cargo.lock");
    let offenders: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \""))
        .filter(|name| !names_forbidden(name).is_empty())
        .collect();
    assert!(
        offenders.is_empty(),
        "Cargo.lock must not resolve the retired embedded engine, found: {offenders:?}"
    );
    // The same fact from the other side: no librocksdb-sys style native build either.
    assert!(
        !lock.contains("librocksdb"),
        "Cargo.lock must not carry librocksdb-sys"
    );
}

#[test]
fn the_manifest_declares_no_embedded_engine_nor_legacy_feature() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("read Cargo.toml");
    let declared: Vec<&str> = manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| !names_forbidden(line).is_empty() || line.starts_with("legacy"))
        .collect();
    assert!(
        declared.is_empty(),
        "Cargo.toml must not declare the embedded engine or a `legacy` feature: {declared:?}"
    );
}

#[test]
fn the_sbom_syft_generates_from_the_lock_lists_no_embedded_engine() {
    let output = match Command::new("syft")
        .arg("scan")
        .arg(format!("file:{}", root().join("Cargo.lock").display()))
        .args(["-q", "-o", "cyclonedx-json"])
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            // Visible, not green by silence: the lock assertion above still holds.
            eprintln!("NOT_RUN: syft is not installed; SBOM generation from Cargo.lock skipped");
            return;
        }
    };
    assert!(
        output.status.success(),
        "syft failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sbom = String::from_utf8(output.stdout).expect("syft emits UTF-8");
    assert!(
        sbom.contains("\"components\""),
        "the SBOM must list components, otherwise this check proves nothing"
    );
    assert!(
        names_forbidden(&sbom).is_empty(),
        "the SBOM of Cargo.lock must not list surrealdb or rocksdb"
    );
}
