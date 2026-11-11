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

/// The integration test targets of every member of the workspace, by file
/// name without extension.
///
/// The members are read from the workspace manifest, not assumed: a crate that
/// joins the workspace brings its own test targets, and a target that belongs
/// to no suite stops being run at all. Looking only at the root package's
/// `tests` directory misses every target of every other member, which is
/// exactly how a suite of the member crates was dropped from the workflow
/// without this test noticing.
fn test_targets() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for member in workspace_members() {
        let tests = member.join("tests");
        let Ok(entries) = fs::read_dir(&tests) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "rs")
                && let Some(name) = path.file_stem()
            {
                found.push(name.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The directories of the members of the workspace.
///
/// The root package is the manifest itself; the rest are the paths the
/// workspace manifest names.
fn workspace_members() -> Vec<PathBuf> {
    let root = manifest_dir();
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap_or_else(|error| panic!("cannot read the workspace manifest: {error}"));

    let mut members = vec![root.clone()];
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with("members") {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with(']') {
                break;
            }
            let member = line.trim().trim_end_matches(',').trim();
            let member = member.trim_matches('"');
            if !member.is_empty() && !member.starts_with('#') {
                members.push(root.join(member));
            }
        }
    }
    members
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

/// The doctests are run by the workflow.
///
/// They are not a test target, so no suite names them, and a segmentation
/// that forgot them would drop them silently: the guard cannot see a target
/// that is not one.
#[test]
fn the_workflow_runs_the_doctests() {
    let workflow = fs::read_to_string(workflow_path())
        .unwrap_or_else(|error| panic!("cannot read the workflow: {error}"));

    assert!(
        workflow.contains("cargo test --workspace --doc"),
        "the workflow must run the doctests; they are not a test target, so no suite names them"
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
        // The doctests are not a target: `cargo test --workspace --doc` runs
        // them, and that is the only way to run them.
        .filter(|line| !line.contains("--doc"))
        .count();

    assert_eq!(
        whole_workspace, 0,
        "the workflow runs `cargo test --workspace` without naming the targets, which runs the \
         whole suite again and undoes the segmentation"
    );
}

/// A job that runs on every pull request must not run the whole workspace.
///
/// The nextest job is opt-in on purpose: it runs the same workspace with
/// per-test isolation, and the RocksDB suites need a single thread. That is
/// only acceptable while it stays out of the pull request path; a job that
/// runs the whole suite there would undo the segmentation behind the
/// segmentation's back.
#[test]
fn no_job_that_runs_on_pull_requests_runs_the_whole_workspace() {
    let workflow = fs::read_to_string(workflow_path())
        .unwrap_or_else(|error| panic!("cannot read the workflow: {error}"));

    // The jobs of the workflow, each with its steps joined.
    let mut job_name = String::new();
    let mut manual_only = false;
    let mut offenders: Vec<String> = Vec::new();

    for line in logical_lines(&workflow) {
        if line.starts_with("  ") && line.ends_with(':') && !line.starts_with("    ") {
            // A new job.
            job_name = line.trim_end_matches(':').trim().to_string();
            manual_only = false;
            continue;
        }
        if line.trim_start().starts_with("if:") {
            manual_only = line.contains("workflow_dispatch");
            continue;
        }
        if manual_only {
            continue;
        }
        let runs_whole_workspace = line.contains("cargo nextest run --workspace")
            || (line.contains("cargo test --workspace")
                && !line.contains("--test ")
                && !line.contains("--doc"));
        if runs_whole_workspace {
            offenders.push(format!("{job_name}: {line}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "these jobs run the whole workspace on every pull request, which undoes the \
         segmentation: {offenders:?}"
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
