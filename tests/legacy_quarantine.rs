//! Guard for the quarantined legacy suites.
//!
//! The suites under `tests/legacy/` are not compiled and not executed (the
//! directory has no `main.rs`, so Cargo never turns it into a test target).
//! Quarantine is only safe while the *record* of what was parked stays in
//! sync with what is actually parked: a file that is added, removed or
//! extended without updating the drift report would silently drop a
//! guarantee nobody can see any more.
//!
//! This test enforces that synchronisation. It is deliberately cheap — it
//! only reads the working tree, spawns nothing and binds no port.
//!
//! What it asserts:
//!
//! - Q1 Every quarantined source file is named in `tests/legacy/README.md`.
//! - Q2 The README names no file that is not quarantined (no stale entries).
//! - Q3 The number of test functions parked in the quarantine is the number
//!   the README declares, so a test added inside a parked file cannot
//!   disappear unnoticed.
//! - Q4 `tests/legacy/` has no `main.rs`, so it can never silently become a
//!   Cargo test target again.
//! - Q5 The permanent replacement contract exists and is a Cargo target.

use std::fs;
use std::path::{Path, PathBuf};

/// Root of the package under test.
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The quarantine directory.
fn quarantine_dir() -> PathBuf {
    manifest_dir().join("tests").join("legacy")
}

/// The drift report.
fn readme_path() -> PathBuf {
    quarantine_dir().join("README.md")
}

/// Every `.rs` file under `tests/legacy/`, relative to that directory, sorted.
fn quarantined_sources() -> Vec<String> {
    let root = quarantine_dir();
    let mut found = Vec::new();
    collect_rs_files(&root, &root, &mut found);
    found.sort();
    found
}

fn collect_rs_files(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("cannot read entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(root, &path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            found.push(relative);
        }
    }
}

/// Counts the test functions declared in a Rust source file.
///
/// Both the plain and the `tokio` flavour are counted; a function that is
/// commented out does not count.
fn count_test_functions(source: &str) -> usize {
    source
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.starts_with("#[test]") || line.starts_with("#[tokio::test")
        })
        .count()
}

/// Total number of test functions parked in the quarantine.
fn total_quarantined_tests() -> usize {
    let root = quarantine_dir();
    let mut total = 0;
    for relative in quarantined_sources() {
        let source = fs::read_to_string(root.join(&relative))
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.join(&relative).display()));
        total += count_test_functions(&source);
    }
    total
}

#[test]
fn q1_every_quarantined_file_is_listed_in_the_drift_report() {
    let readme = fs::read_to_string(readme_path()).expect("the drift report must exist");

    let sources = quarantined_sources();
    let unlisted: Vec<&String> = sources
        .iter()
        .filter(|relative| !readme.contains(relative.as_str()))
        .collect();

    assert!(
        unlisted.is_empty(),
        "these quarantined files are not listed in tests/legacy/README.md: {unlisted:?}. \
         Every parked file must be documented, otherwise its guarantee is lost silently."
    );
}

#[test]
fn q2_the_drift_report_lists_no_file_that_is_not_quarantined() {
    let readme = fs::read_to_string(readme_path()).expect("the drift report must exist");
    let present = quarantined_sources();

    // Every quarantine-relative `.rs` path mentioned in the report must exist
    // on disk. Paths that point outside the quarantine (the permanent
    // contract, the guard itself) are not part of this check.
    let mentioned: Vec<&str> = readme
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix('`')
                .and_then(|rest| rest.split('`').next())
                .filter(|candidate| candidate.ends_with(".rs"))
                .filter(|candidate| !candidate.starts_with("tests/"))
        })
        .collect();

    let stale: Vec<&&str> = mentioned
        .iter()
        .filter(|candidate| {
            // The report refers to files by their path inside the quarantine.
            let normalized = candidate.trim_start_matches("legacy/");
            !present
                .iter()
                .any(|p| p == normalized || p.ends_with(normalized))
        })
        .collect();

    assert!(
        stale.is_empty(),
        "tests/legacy/README.md mentions files that are not quarantined: {stale:?}. \
         Remove the stale entries or restore the file."
    );
}

#[test]
fn q3_the_number_of_parked_tests_matches_the_drift_report() {
    let readme = fs::read_to_string(readme_path()).expect("the drift report must exist");

    let actual = total_quarantined_tests();

    // The report states the parked total per suite; the sum must match the
    // number of test functions actually present in the tree.
    let declared: usize = readme
        .lines()
        .filter(|line| line.starts_with("## `legacy/"))
        .filter_map(|line| {
            // The separator is an em dash, which is three bytes long in UTF-8.
            // `find` returns a byte index, so slicing at `index + 1` would cut
            // *inside* the character and panic. `split_once` returns slices
            // that are already on character boundaries.
            let (_, rest) = line.split_once('—')?;
            rest.split_whitespace()
                .next()
                .and_then(|n| n.parse::<usize>().ok())
        })
        .sum();

    assert_eq!(
        actual, declared,
        "the drift report declares {declared} parked tests but the tree contains {actual}. \
         A test added or removed inside tests/legacy/ must be reflected in the report."
    );

    assert!(
        actual > 0,
        "the quarantine must not be empty: if the legacy suites were restored, \
         delete the quarantine instead of leaving an empty directory behind."
    );
}

#[test]
fn q4_the_quarantine_can_never_become_a_cargo_test_target_again() {
    let manifest = fs::read_to_string(quarantine_dir().join("main.rs"));
    assert!(
        manifest.is_err(),
        "tests/legacy/main.rs must not exist: Cargo would turn the directory into a \
         test target and compile suites that are known to be broken."
    );
}

#[test]
fn q5_the_permanent_replacement_contract_exists() {
    let contract = manifest_dir().join("tests").join("http_contract.rs");
    assert!(
        contract.is_file(),
        "tests/http_contract.rs is the permanent replacement for the quarantined suites \
         and must exist."
    );

    let source = fs::read_to_string(&contract).expect("the contract must be readable");
    assert!(
        count_test_functions(&source) >= 8,
        "the permanent contract must keep its guarantees; it currently declares fewer \
         than the 8 documented in tests/legacy/README.md."
    );
}
