//! Verifiable release pipeline tests: SBOM (SPDX/CycloneDX via syft),
//! build provenance attestation, dependency/license matrix, OpenPGP signing
//! without static CI secrets, and clean-directory verification (HSP-244 / HCR-22).
//!
//! Verifies:
//! - AC1: Release emits tarball, checksums, OCI image archive/digest, SBOMs
//!   (SPDX/CycloneDX via `syft`), provenance attestation, and OpenPGP detached
//!   signatures linked to the same commit; `scripts/verify-release.sh` verifies
//!   digests/signatures and rejects any tampered asset.
//! - AC2: Production OCI image (`Containerfile`) contains NO legacy migration
//!   binaries (`ensure-iam-index`, `ensure-graph-index`, `backfill-iam-scope`,
//!   etc.), honestly records `Cargo.toml` / `HSP-213` SurrealDB status, and
//!   emits a dependency/license matrix from `cargo metadata --locked`.
//! - AC3: External consumer/verifier (`scripts/verify-release.sh` + `gpgv`)
//!   verifies signature and provenance locally in a clean directory, and
//!   documents exact limits without claiming unmeasured SLSA levels.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;
use tempfile::TempDir;

const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");
const IMAGE_TAG: &str = "localhost/hodei-artifacts-api:latest";

fn podman_args() -> Vec<String> {
    let driver = std::env::var("PODMAN_STORAGE_DRIVER").unwrap_or_else(|_| "vfs".to_string());
    let root = std::env::var("PODMAN_ROOT").unwrap_or_else(|_| "/var/tmp/podman-root".to_string());
    let runroot =
        std::env::var("PODMAN_RUNROOT").unwrap_or_else(|_| "/var/tmp/podman-runroot".to_string());
    vec![
        format!("--storage-driver={driver}"),
        "--root".to_string(),
        root,
        "--runroot".to_string(),
        runroot,
    ]
}

fn podman(args: &[&str]) -> Output {
    let base = podman_args();
    Command::new("podman")
        .args(&base)
        .args(args)
        .output()
        .expect("podman command must execute")
}

/// Package the verifiable release once per test binary invocation so all tests
/// inspect the exact same correlated output.
fn packaged_dist() -> &'static Path {
    static DIST: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    let (_, path) = DIST.get_or_init(|| {
        let dir = TempDir::new().expect("create temp dist directory");
        let dist_path = dir.path().to_path_buf();
        let script = Path::new(REPO_ROOT).join("scripts/package-oci.sh");
        let out = Command::new(&script)
            .arg(&dist_path)
            .current_dir(REPO_ROOT)
            .output()
            .expect("run scripts/package-oci.sh");
        assert!(
            out.status.success(),
            "scripts/package-oci.sh failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (dir, dist_path)
    });
    path.as_path()
}

fn find_manifest(dist: &Path) -> (PathBuf, Value) {
    let manifest_path = fs::read_dir(dist)
        .expect("read dist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(".manifest.json"))
        .expect("manifest.json must exist");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    (manifest_path, manifest)
}

fn copy_dist_files(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).expect("read src").filter_map(Result::ok) {
        if entry.path().is_file() {
            fs::copy(entry.path(), dst.join(entry.file_name())).expect("copy release asset");
        }
    }
}

#[test]
fn verifiable_release_emits_sboms_provenance_license_matrix_and_signatures() {
    let dist = packaged_dist();
    let (manifest_path, manifest) = find_manifest(dist);

    let head_out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(REPO_ROOT)
        .output()
        .expect("git rev-parse HEAD");
    assert!(head_out.status.success(), "git rev-parse HEAD must succeed");
    let head_commit = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
    assert_eq!(
        manifest["commit"].as_str().unwrap(),
        head_commit,
        "manifest commit must match HEAD"
    );

    // Verify all correlated artifacts exist.
    let tarball_name = manifest["tarball"]["filename"].as_str().unwrap();
    let oci_archive_name = manifest["oci"]["archive"].as_str().unwrap();
    let oci_spdx_name = manifest["sbom"]["oci_spdx"]["filename"].as_str().unwrap();
    let oci_cdx_name = manifest["sbom"]["oci_cyclonedx"]["filename"]
        .as_str()
        .unwrap();
    let cargo_spdx_name = manifest["sbom"]["cargo_spdx"]["filename"].as_str().unwrap();
    let license_matrix_name = manifest["license_matrix"]["filename"].as_str().unwrap();
    let provenance_name = manifest["provenance"]["filename"].as_str().unwrap();
    let pubkey_name = manifest["signing"]["public_key"].as_str().unwrap();

    for name in [
        tarball_name,
        oci_archive_name,
        oci_spdx_name,
        oci_cdx_name,
        cargo_spdx_name,
        license_matrix_name,
        provenance_name,
        pubkey_name,
        "SHA256SUMS",
        "SHA256SUMS.asc",
    ] {
        assert!(
            dist.join(name).is_file(),
            "missing release artifact: {name}"
        );
    }
    let manifest_asc = format!("{}.asc", manifest_path.display());
    assert!(
        Path::new(&manifest_asc).is_file(),
        "missing manifest detached signature: {manifest_asc}"
    );

    // Check binary & OCI image digests.
    assert!(
        manifest["oci"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(
        manifest["oci"]["binary_sha256"].as_str().unwrap().len(),
        64,
        "OCI binary sha256 must be a 64-char hex digest"
    );

    // Validate SPDX SBOMs.
    for spdx_file in [oci_spdx_name, cargo_spdx_name] {
        let spdx: Value = serde_json::from_str(
            &fs::read_to_string(dist.join(spdx_file)).expect("read spdx sbom"),
        )
        .expect("parse spdx sbom");
        assert!(
            spdx["spdxVersion"].as_str().unwrap().starts_with("SPDX-"),
            "{spdx_file} must declare spdxVersion"
        );
        assert!(
            !spdx["packages"].as_array().unwrap().is_empty(),
            "{spdx_file} must contain packages"
        );
    }

    // Validate CycloneDX SBOM.
    let cdx: Value =
        serde_json::from_str(&fs::read_to_string(dist.join(oci_cdx_name)).expect("read cdx sbom"))
            .expect("parse cdx sbom");
    assert_eq!(cdx["bomFormat"].as_str().unwrap(), "CycloneDX");
    assert!(
        !cdx["components"].as_array().unwrap().is_empty(),
        "CycloneDX SBOM must contain components"
    );

    // Validate dependency/license matrix and honest SurrealDB / HSP-213 status.
    let matrix: Value = serde_json::from_str(
        &fs::read_to_string(dist.join(license_matrix_name)).expect("read license matrix"),
    )
    .expect("parse license matrix");
    assert!(
        matrix["total_packages"].as_u64().unwrap() > 0,
        "license matrix must list resolved packages"
    );
    assert!(
        !matrix["license_summary"].as_object().unwrap().is_empty(),
        "license_summary must be populated"
    );
    assert!(
        matrix["surrealdb_status"]["present_in_cargo_toml"]
            .as_bool()
            .unwrap(),
        "Cargo.toml still declares surrealdb pending HSP-213; matrix must report it honestly"
    );
    assert!(
        matrix["surrealdb_status"]["note"]
            .as_str()
            .unwrap()
            .contains("HSP-213"),
        "surrealdb_status note must reference HSP-213"
    );

    // Validate provenance attestation and honest SLSA limits.
    let prov: Value = serde_json::from_str(
        &fs::read_to_string(dist.join(provenance_name)).expect("read provenance"),
    )
    .expect("parse provenance");
    assert_eq!(
        prov["_type"].as_str().unwrap(),
        "https://in-toto.io/Statement/v1"
    );
    assert_eq!(
        prov["predicateType"].as_str().unwrap(),
        "https://slsa.dev/provenance/v1"
    );
    let limits = prov["predicate"]["runDetails"]["metadata"]["honest_limits"]
        .as_str()
        .unwrap();
    assert!(
        limits.contains("does not claim SLSA Build Level 2"),
        "provenance must state honest SLSA limits: {limits}"
    );
    assert_eq!(
        manifest["signing"]["mechanism"].as_str().unwrap(),
        "openpgp-detached"
    );
}

#[test]
fn production_oci_image_excludes_legacy_migration_binaries() {
    let _ = packaged_dist();

    let out = podman(&[
        "run",
        "--rm",
        "--entrypoint",
        "/bin/sh",
        IMAGE_TAG,
        "-c",
        "ls -1 /app/bin",
    ]);
    assert!(
        out.status.success(),
        "failed to list /app/bin in OCI image: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bins: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        bins,
        vec!["hodei-artifacts-api".to_string()],
        "OCI image /app/bin must contain ONLY hodei-artifacts-api, got: {bins:?}"
    );

    // Explicitly verify none of the legacy/operator migration binaries exist in /app.
    let forbidden_check = podman(&[
        "run",
        "--rm",
        "--entrypoint",
        "/bin/sh",
        IMAGE_TAG,
        "-c",
        "for b in ensure-iam-index ensure-graph-index backfill-iam-scope hodei-recovery perf_lab; do if [ -e \"/app/bin/$b\" ]; then echo FOUND:$b; exit 1; fi; done",
    ]);
    assert!(
        forbidden_check.status.success(),
        "forbidden legacy/operator binary found in OCI image: {}",
        String::from_utf8_lossy(&forbidden_check.stdout)
    );
}

#[test]
fn external_clean_dir_verification_passes_and_rejects_tampering() {
    let dist = packaged_dist();
    let verify_script = Path::new(REPO_ROOT).join("scripts/verify-release.sh");

    // 1. Verify valid bundle in clean directory via verify-release.sh.
    let ok_out = Command::new(&verify_script)
        .arg(dist)
        .output()
        .expect("run verify-release.sh");
    assert!(
        ok_out.status.success(),
        "verify-release.sh failed on valid bundle:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ok_out.stdout),
        String::from_utf8_lossy(&ok_out.stderr)
    );
    let ok_stdout = String::from_utf8_lossy(&ok_out.stdout);
    assert!(
        ok_stdout.contains("signing=openpgp-detached/"),
        "verify-release.sh output must report signing mechanism: {ok_stdout}"
    );

    // 2. External consumer check using gpgv directly in a clean directory.
    let consumer_dir = TempDir::new().expect("consumer temp dir");
    copy_dist_files(dist, consumer_dir.path());
    let trusted_gpg = consumer_dir.path().join("trusted.gpg");
    let pubkey = fs::read(consumer_dir.path().join("release-signing-key.asc"))
        .expect("read release-signing-key.asc");
    let mut dearmor = Command::new("gpg")
        .arg("--dearmor")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn gpg --dearmor");
    dearmor
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&pubkey)
        .expect("write pubkey");
    let dearmor_out = dearmor.wait_with_output().expect("gpg --dearmor output");
    assert!(dearmor_out.status.success(), "gpg --dearmor must succeed");
    fs::write(&trusted_gpg, &dearmor_out.stdout).expect("write trusted.gpg");

    let gpgv_out = Command::new("gpgv")
        .arg("--keyring")
        .arg(&trusted_gpg)
        .arg(consumer_dir.path().join("SHA256SUMS.asc"))
        .arg(consumer_dir.path().join("SHA256SUMS"))
        .output()
        .expect("run gpgv");
    assert!(
        gpgv_out.status.success(),
        "external gpgv verification failed:\nstderr:\n{}",
        String::from_utf8_lossy(&gpgv_out.stderr)
    );

    // 3. Tampering rejection: (a) OCI archive, (b) SPDX SBOM, (c) SHA256SUMS.
    let (_, manifest) = find_manifest(dist);
    let oci_archive_name = manifest["oci"]["archive"].as_str().unwrap();
    let oci_spdx_name = manifest["sbom"]["oci_spdx"]["filename"].as_str().unwrap();

    for target_file in [oci_archive_name, oci_spdx_name, "SHA256SUMS"] {
        let tampered_dir = TempDir::new().expect("temp tampered dir");
        copy_dist_files(dist, tampered_dir.path());
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(tampered_dir.path().join(target_file))
            .expect("open asset for tampering");
        f.write_all(b"\n# tamper\n").expect("append tamper bytes");
        drop(f);

        let tampered_out = Command::new(&verify_script)
            .arg(tampered_dir.path())
            .output()
            .expect("run verify-release.sh on tampered bundle");
        assert!(
            !tampered_out.status.success(),
            "verify-release.sh MUST reject tampered {target_file}"
        );
    }
}
