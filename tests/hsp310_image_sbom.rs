//! HSP-310: the SBOM of the built OCI image lists no embedded engine.
//!
//! `hsp314_no_embedded_engine_sbom` pins `Cargo.lock` and an SBOM generated
//! from it; it never looks at the image. This suite builds the real image with
//! `scripts/package-oci.sh` (isolated podman store and rootless runroot, so the
//! shared `:latest` tag is never retagged), reads the SBOMs the script already
//! emits and asserts, with pure checking functions that are also exercised on
//! fabricated documents (the mutation tests):
//!
//! - the image SBOMs (CycloneDX and SPDX) are not empty (positive control: the
//!   Debian layer's packages, `ca-certificates`, `libc6`, an OS component) and
//!   list no `surrealdb`, `surrealdb-core`, `rocksdb` or `librocksdb-sys`;
//! - the Rust crates of the shipped binary are NOT visible to syft in the image
//!   (the binary is not built with `cargo-auditable`), so the same release also
//!   gets the cargo SBOM (lock based: `tokio` and `axum` present, > 100
//!   packages) and a crate-path scan of the staged binary (`tokio-1.x` present,
//!   no `surrealdb-x.y` / `rocksdb-x.y` / `librocksdb-sys-x.y` crate paths).
//!
//! Without podman or syft/mise the build test prints `NOT_RUN` and returns,
//! unless `HODEI_REQUIRE_IMAGE_SBOM=1`, which turns that into a failure.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");
const FORBIDDEN: &[&str] = &["surrealdb", "rocksdb"];
const MIN_IMAGE_LIBRARIES: usize = 50;
const MIN_CARGO_PACKAGES: usize = 100;
const BIN_NAME: &str = "hodei-artifacts-api";

fn forbidden_in(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    FORBIDDEN.iter().any(|needle| lowered.contains(needle))
}

/// Names of every component of a CycloneDX document, plus how many are real
/// libraries/OS entries (not the per-file entries syft also emits).
fn cdx_names(doc: &Value) -> (Vec<String>, usize) {
    let comps = doc["components"].as_array().cloned().unwrap_or_default();
    let names = comps
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    let libs = comps
        .iter()
        .filter(|c| matches!(c["type"].as_str(), Some("library" | "operating-system")))
        .count();
    (names, libs)
}

fn spdx_names(doc: &Value) -> Vec<String> {
    doc["packages"]
        .as_array()
        .map(|p| {
            p.iter()
                .filter_map(|p| p["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn offenders(names: &[String]) -> Vec<&str> {
    names
        .iter()
        .map(String::as_str)
        .filter(|n| forbidden_in(n))
        .collect()
}

/// Pure check of an image SBOM given as component names.
fn check_names(
    label: &str,
    names: &[String],
    real_packages: usize,
    min: usize,
    required: &[&str],
) -> Result<(), String> {
    if real_packages < min {
        return Err(format!(
            "{label}: only {real_packages} packages (< {min}); an empty SBOM proves nothing"
        ));
    }
    for want in required {
        if !names.iter().any(|n| n == want) {
            return Err(format!("{label}: control component `{want}` is missing"));
        }
    }
    let bad = offenders(names);
    if !bad.is_empty() {
        return Err(format!("{label}: lists the retired engine: {bad:?}"));
    }
    Ok(())
}

fn check_image_cdx(doc: &Value) -> Result<(), String> {
    let (names, libs) = cdx_names(doc);
    check_names(
        "image CycloneDX",
        &names,
        libs,
        MIN_IMAGE_LIBRARIES,
        &["ca-certificates", "libc6"],
    )
}

fn check_image_spdx(doc: &Value) -> Result<(), String> {
    let names = spdx_names(doc);
    check_names(
        "image SPDX",
        &names,
        names.len(),
        MIN_IMAGE_LIBRARIES,
        &["ca-certificates", "libc6"],
    )
}

fn check_cargo_spdx(doc: &Value) -> Result<(), String> {
    let names = spdx_names(doc);
    check_names(
        "cargo SPDX",
        &names,
        names.len(),
        MIN_CARGO_PACKAGES,
        &["tokio", "axum"],
    )
}

/// Crate paths look like `name-1.2.3` in a cargo-built binary (panic locations,
/// registry paths); plain prose such as "SurrealDB over RocksDB" must not match.
fn crate_paths(bytes: &[u8], name: &str) -> bool {
    let needle = format!("{name}-").into_bytes();
    bytes.windows(needle.len() + 2).any(|w| {
        w.starts_with(&needle) && w[needle.len()].is_ascii_digit() && w[needle.len() + 1] == b'.'
    })
}

fn check_binary(bytes: &[u8]) -> Result<(), String> {
    if !crate_paths(bytes, "tokio") {
        return Err("binary: control crate path `tokio-N.` not found".into());
    }
    let bad: Vec<&str> = ["surrealdb", "surrealdb-core", "rocksdb", "librocksdb-sys"]
        .into_iter()
        .filter(|n| crate_paths(bytes, n))
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("binary: embeds crates of the retired engine: {bad:?}"))
    }
}

// ---------------------------------------------------------------- pure tests

fn fake_cdx(extra: Option<&str>) -> Value {
    let mut comps: Vec<Value> = (0..60)
        .map(|i| serde_json::json!({"type": "library", "name": format!("pkg{i}")}))
        .collect();
    comps.push(serde_json::json!({"type": "library", "name": "ca-certificates"}));
    comps.push(serde_json::json!({"type": "library", "name": "libc6"}));
    if let Some(name) = extra {
        comps.push(serde_json::json!({"type": "library", "name": name}));
    }
    serde_json::json!({ "components": comps })
}

#[test]
fn a_clean_fabricated_sbom_passes() {
    check_image_cdx(&fake_cdx(None)).expect("clean SBOM");
}

#[test]
fn mutation_a_surrealdb_component_fails_the_check() {
    for bad in ["surrealdb", "surrealdb-core", "rocksdb", "librocksdb-sys"] {
        let err = check_image_cdx(&fake_cdx(Some(bad))).expect_err(bad);
        assert!(err.contains("retired engine"), "{bad}: {err}");
    }
}

#[test]
fn mutation_an_empty_or_thin_sbom_cannot_pass() {
    let empty = serde_json::json!({"components": []});
    assert!(check_image_cdx(&empty).unwrap_err().contains("proves nothing"));
    let no_components_key = serde_json::json!({});
    assert!(check_image_cdx(&no_components_key).is_err());
    // Files do not count as packages.
    let files: Vec<Value> = (0..500)
        .map(|i| serde_json::json!({"type": "file", "name": format!("/f{i}")}))
        .collect();
    assert!(check_image_cdx(&serde_json::json!({ "components": files })).is_err());
}

#[test]
fn mutation_a_missing_control_component_fails() {
    let doc = serde_json::json!({"components":
        (0..80).map(|i| serde_json::json!({"type":"library","name":format!("p{i}")})).collect::<Vec<_>>()});
    assert!(check_image_cdx(&doc).unwrap_err().contains("control component"));
}

#[test]
fn spdx_and_cargo_checks_reject_the_engine_too() {
    let mut pkgs: Vec<Value> = (0..120)
        .map(|i| serde_json::json!({"name": format!("p{i}")}))
        .collect();
    for n in ["tokio", "axum", "ca-certificates", "libc6"] {
        pkgs.push(serde_json::json!({ "name": n }));
    }
    let clean = serde_json::json!({ "packages": pkgs.clone() });
    check_cargo_spdx(&clean).expect("clean cargo");
    check_image_spdx(&clean).expect("clean spdx");
    pkgs.push(serde_json::json!({"name": "surrealdb-core"}));
    let dirty = serde_json::json!({ "packages": pkgs });
    assert!(check_cargo_spdx(&dirty).is_err());
    assert!(check_image_spdx(&dirty).is_err());
}

#[test]
fn binary_scan_matches_crate_paths_not_prose() {
    let prose = b"tokio-1.54.1 the embedded engine (SurrealDB over RocksDB) is retired";
    check_binary(prose).expect("prose is not a crate path");
    assert!(check_binary(b"x tokio-1.54.1 /surrealdb-core-2.3.0/src").is_err());
    assert!(check_binary(b"/librocksdb-sys-0.16.0/ tokio-1.2.3").is_err());
    assert!(check_binary(b"no control here").is_err());
}

// ------------------------------------------------------------- real image

fn tool_available(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tree_is_clean() -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(REPO_ROOT)
        .output()
        .map(|o| o.status.success() && o.stdout.is_empty())
        .unwrap_or(false)
}

fn not_run(reason: &str) {
    assert!(
        std::env::var("HODEI_REQUIRE_IMAGE_SBOM").as_deref() != Ok("1"),
        "image SBOM check is NOT_RUN ({reason}) but HODEI_REQUIRE_IMAGE_SBOM=1"
    );
    eprintln!("NOT_RUN: {reason}; the image SBOM was not checked");
}

fn read_json(dir: &Path, suffix: &str) -> Value {
    let path: PathBuf = std::fs::read_dir(dir)
        .expect("read dist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(suffix))
        .unwrap_or_else(|| panic!("no *{suffix} in {}", dir.display()));
    serde_json::from_str(&std::fs::read_to_string(path).expect("read sbom")).expect("parse sbom")
}

#[test]
fn the_sbom_of_the_built_image_lists_no_embedded_engine() {
    if !tool_available("podman", "--version") {
        return not_run("podman is not installed");
    }
    if !tool_available("syft", "version") && !tool_available("mise", "--version") {
        return not_run("neither syft nor mise (syft@latest) is available");
    }

    // Isolated store: package-oci.sh tags `:latest` in whatever store it uses.
    let work = TempDir::new().expect("temp work dir");
    let dist = work.path().join("dist");
    let root = work.path().join("podman-root");
    let run = work.path().join("podman-runroot");
    let out = Command::new(Path::new(REPO_ROOT).join("scripts/package-oci.sh"))
        .arg(&dist)
        .env("PODMAN_ROOT", &root)
        .env("PODMAN_RUNROOT", &run)
        .env(
            "HODEI_ALLOW_DIRTY_TREE",
            if tree_is_clean() { "0" } else { "1" },
        )
        .current_dir(REPO_ROOT)
        .output()
        .expect("run scripts/package-oci.sh");
    let podman_cleanup = || {
        let _ = Command::new("podman")
            .args(["--storage-driver=vfs", "--root"])
            .arg(&root)
            .arg("--runroot")
            .arg(&run)
            .args(["system", "reset", "--force"])
            .output();
    };
    if !out.status.success() {
        podman_cleanup();
        panic!(
            "scripts/package-oci.sh failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let cdx = read_json(&dist, "-oci.cdx.json");
    let spdx = read_json(&dist, "-oci.spdx.json");
    let cargo = read_json(&dist, "-cargo.spdx.json");
    let binary = std::fs::read(dist.join("oci-stage/bin").join(BIN_NAME)).expect("staged binary");
    podman_cleanup();

    let results = [
        check_image_cdx(&cdx),
        check_image_spdx(&spdx),
        check_cargo_spdx(&cargo),
        check_binary(&binary),
    ];
    let failures: Vec<String> = results.into_iter().filter_map(Result::err).collect();
    assert!(failures.is_empty(), "{failures:#?}");
}
