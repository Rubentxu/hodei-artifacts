//! The CI segmentation has to describe the tests that actually exist.
//!
//! The workflow splits the suite into a fast one, an integration one and an
//! infrastructure one. A list that drifts from the test suite is worse than
//! no segmentation: it looks like a guarantee and is not one. A test target
//! missing from every suite stops being run at all, and a suite naming a
//! target that does not exist fails the job for no reason.
//!
//! So the segmentation is checked against the files on disk, in both
//! directions.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workflow_path() -> PathBuf {
    manifest_dir().join(".github/workflows/rust-ci.yml")
}

/// The integration test targets of the crate, by file name without extension.
fn test_targets() -> Vec<String> {
    let tests = manifest_dir().join("tests");
    let mut found: Vec<String> = fs::read_dir(&tests)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", tests.display()))
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                Some(
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            } else {
                None
            }
        })
        .collect();
    found.sort();
    found
}

/// The test targets each suite of the workflow names.
///
/// The suite is recognised by the step that runs it, and the targets are the
/// `--test <name>` flags of that step.
fn suites_in_workflow() -> BTreeMap<String, Vec<String>> {
    let workflow = fs::read_to_string(workflow_path())
        .unwrap_or_else(|error| panic!("cannot read the workflow: {error}"));

    let mut suites: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in logical_lines(&workflow) {
        if let Some(rest) = line.strip_prefix("- name: Suite ") {
            let name = rest.trim().to_string();
            current = Some(name.clone());
            suites.entry(name).or_default();
            continue;
        }
        // A new step or job ends the current suite.
        if line.starts_with("- name:") || line.starts_with("- uses:") {
            current = None;
            continue;
        }
        collect_targets(&mut suites, &current, &line);
    }
    suites
}

/// The lines of the workflow, with the backslash continuations joined.
///
/// A step that names several test targets spreads them over several lines
/// joined by a backslash, and a command is only complete once they are read
/// together.
fn logical_lines(workflow: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for raw in workflow.lines() {
        let line = raw.trim();
        if !pending.is_empty() {
            pending.push(' ');
            pending.push_str(line.trim_end_matches('\\').trim());
            if line.ends_with('\\') {
                continue;
            }
            lines.push(std::mem::take(&mut pending));
            continue;
        }
        if line.ends_with('\\') {
            pending = line.trim_end_matches('\\').trim().to_string();
            continue;
        }
        lines.push(line.to_string());
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

/// The `--test <name>` flags of one logical line.
fn collect_targets(
    suites: &mut BTreeMap<String, Vec<String>>,
    current: &Option<String>,
    line: &str,
) {
    let Some(name) = current.as_ref() else {
        return;
    };
    let Some(targets) = suites.get_mut(name) else {
        return;
    };
    let mut rest = line;
    while let Some(position) = rest.find("--test ") {
        rest = &rest[position + "--test ".len()..];
        let target = rest
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('\\');
        if !target.is_empty() {
            targets.push(target.to_string());
        }
    }
}

/// The names the suites are known by, as the workflow spells them.
const FAST: &str = "fast (unit and contract)";
const INTEGRATION: &str = "integration (database)";
const INFRASTRUCTURE: &str = "infrastructure (boot and processes)";

#[test]
fn every_test_target_belongs_to_exactly_one_suite() {
    let suites = suites_in_workflow();
    let mut assigned: BTreeMap<String, Vec<&str>> = BTreeMap::new();

    for (suite, targets) in &suites {
        for target in targets {
            assigned
                .entry(target.clone())
                .or_default()
                .push(suite.as_str());
        }
    }

    let targets = test_targets();
    for target in &targets {
        let owners = assigned.get(target).map(Vec::as_slice).unwrap_or(&[]);
        assert!(
            owners.len() == 1,
            "the test target `{target}` belongs to {} suites ({owners:?}); it must belong to \
             exactly one, or the segmentation does not describe the suite",
            owners.len()
        );
    }
}

#[test]
fn every_suite_names_a_target_that_exists() {
    let suites = suites_in_workflow();
    let targets = test_targets();
    for (suite, named) in &suites {
        for target in named {
            assert!(
                targets.contains(target),
                "the suite `{suite}` names the test target `{target}`, which does not exist; \
                 the segmentation would fail the job for no reason"
            );
        }
    }
}

#[test]
fn the_three_suites_are_the_ones_the_workflow_declares() {
    let suites = suites_in_workflow();
    for expected in [FAST, INTEGRATION, INFRASTRUCTURE] {
        assert!(
            suites.contains_key(expected),
            "the workflow must declare the suite `{expected}`; it declares {:?}",
            suites.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn no_test_target_is_left_out_of_the_suites() {
    let suites = suites_in_workflow();
    let named: Vec<&String> = suites.values().flatten().collect();
    let targets = test_targets();
    let missing: Vec<&String> = targets
        .iter()
        .filter(|target| !named.contains(target))
        .collect();
    assert!(
        missing.is_empty(),
        "these test targets are in no suite, so the CI would not run them: {missing:?}"
    );
}

/// The workflow must not run the whole suite twice: the segmentation exists to
/// stop a suite from being duplicated, and a second `--workspace` without a
/// target filter would run everything again.
#[test]
fn the_workflow_does_not_run_the_whole_workspace_twice() {
    let workflow = fs::read_to_string(workflow_path())
        .unwrap_or_else(|error| panic!("cannot read the workflow: {error}"));

    let whole_workspace = logical_lines(&workflow)
        .iter()
        .filter(|line| line.contains("cargo test --workspace"))
        .filter(|line| !line.contains("--test "))
        .filter(|line| !line.contains("nextest"))
        .count();

    assert_eq!(
        whole_workspace, 0,
        "the workflow runs `cargo test --workspace` without naming the targets, which runs the \
         whole suite again and undoes the segmentation"
    );
}

/// The suite lists are kept in one place, so a reviewer can see the whole
/// segmentation without reading three jobs.
#[test]
fn the_suites_are_listed_in_the_workflow() {
    let suites = suites_in_workflow();
    let fast = suites.get(FAST).expect("the fast suite");
    let integration = suites.get(INTEGRATION).expect("the integration suite");
    let infrastructure = suites
        .get(INFRASTRUCTURE)
        .expect("the infrastructure suite");

    assert!(
        !fast.is_empty() && !integration.is_empty() && !infrastructure.is_empty(),
        "the three suites must not be empty: {fast:?} {integration:?} {infrastructure:?}"
    );
}
