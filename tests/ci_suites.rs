//! The CI segmentation has to describe the tests that actually exist.
//!
//! The pipeline splits the suite into a fast one, an integration one and an
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

fn pipeline_path() -> PathBuf {
    manifest_dir().join("pipeline.kts")
}

/// The integration test targets of every member of the workspace, by file
/// name without extension.
///
/// The members are read from the workspace manifest, not assumed: a crate that
/// joins the workspace brings its own test targets, and a target that belongs
/// to no suite stops being run at all. Looking only at the root package's
/// `tests` directory misses every target of every other member, which is
/// exactly how a suite of the member crates was dropped from the pipeline
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

/// The body of every stage of the pipeline, by the name the stage declares.
///
/// A stage is recognised by its `stage("...")` call, and its body is the
/// text between the braces that follow it. The braces are matched, and the
/// text of a string literal is skipped, so a brace inside a command does
/// not end the stage early.
fn stages_in_pipeline() -> BTreeMap<String, String> {
    let pipeline = fs::read_to_string(pipeline_path())
        .unwrap_or_else(|error| panic!("cannot read the pipeline: {error}"));

    let mut stages: BTreeMap<String, String> = BTreeMap::new();
    let mut cursor = 0usize;
    while let Some(position) = pipeline[cursor..].find("stage(\"") {
        let start = cursor + position;
        let name_start = start + "stage(\"".len();
        let Some(name_end) = pipeline[name_start..].find('"') else {
            break;
        };
        let name = pipeline[name_start..name_start + name_end].to_string();
        let body_start = name_start + name_end + 1;
        let Some(end) = the_matching_brace(&pipeline[body_start..]) else {
            break;
        };
        stages.insert(name, pipeline[body_start..body_start + end].to_string());
        cursor = body_start + end;
    }
    stages
}

/// The position of the brace that closes the one that opens the text.
///
/// The text starts right after the name of a stage, so the first brace of
/// the stage opens it. A string literal is skipped whole: a brace inside a
/// command is a character, not a block.
fn the_matching_brace(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = 0usize;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// The test targets each suite of the pipeline names.
///
/// A suite is a stage that runs tests; the targets are the `--test <name>`
/// flags of its commands.
fn suites_in_pipeline() -> BTreeMap<String, Vec<String>> {
    stages_in_pipeline()
        .into_iter()
        .filter(|(_, body)| body.contains("cargo test"))
        .map(|(name, body)| {
            let targets = collect_targets(&body);
            (name, targets)
        })
        .collect()
}

/// The `--test <name>` flags of one body.
///
/// The name is the identifier that follows the flag: it stops at the first
/// character that cannot be part of one, so the comma or the quote that
/// closes it in the source do not travel with it.
fn collect_targets(body: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = body;
    while let Some(position) = rest.find("--test ") {
        rest = &rest[position + "--test ".len()..];
        let target: String = rest
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        if !target.is_empty() {
            targets.push(target);
        }
    }
    targets
}

/// Whether a stage runs the whole workspace, without naming the targets.
///
/// The segmentation exists to stop a suite from being duplicated, and a
/// second `cargo test --workspace` without a target filter would run
/// everything again.
fn runs_the_whole_workspace(body: &str) -> bool {
    body.contains("cargo test --workspace") && !body.contains("--test ") && !body.contains("--doc")
}

/// Whether a stage is the opt-in isolation lane.
///
/// `nextest` runs the same workspace with per-test isolation, and the
/// RocksDB and binary suites need a single test thread to avoid contention.
/// It is not in the pipeline on purpose —it would duplicate the coverage
/// on every run— but it is a legitimate way of running the whole
/// workspace, so it is the one exemption.
fn is_the_isolation_lane(body: &str) -> bool {
    body.contains("cargo nextest run --workspace")
}

/// The names the suites are known by, as the pipeline spells them.
const FAST: &str = "fast (unit and contract)";
const INTEGRATION: &str = "integration (database)";
const INFRASTRUCTURE: &str = "infrastructure (boot and processes)";
/// HSP-314 F2: `legacy` is not the default feature any more, so the suites
/// that need the embedded engine run in their own stage, with
/// `--features legacy`, until HSP-314 F4 retires the engine.
const LEGACY: &str = "legacy (embedded engine)";
/// HSP-314 F4 S2b: the suites that boot the application on the graph profile
/// against a real Dgraph run here, without `legacy`.
const DGRAPH: &str = "integration (dgraph)";

#[test]
fn every_test_target_belongs_to_exactly_one_suite() {
    let suites = suites_in_pipeline();
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
    let suites = suites_in_pipeline();
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
fn the_three_suites_are_the_ones_the_pipeline_declares() {
    let suites = suites_in_pipeline();
    for expected in [FAST, INTEGRATION, INFRASTRUCTURE, LEGACY] {
        assert!(
            suites.contains_key(expected),
            "the pipeline must declare the suite `{expected}`; it declares {:?}",
            suites.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn no_test_target_is_left_out_of_the_suites() {
    let suites = suites_in_pipeline();
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

/// The doctests are run by the pipeline.
///
/// They are not a test target, so no suite names them, and a segmentation
/// that forgot them would drop them silently: the guard cannot see a target
/// that is not one.
#[test]
fn the_pipeline_runs_the_doctests() {
    let stages = stages_in_pipeline();

    assert!(
        stages
            .values()
            .any(|body| body.contains("cargo test --workspace --doc")),
        "the pipeline must run the doctests; they are not a test target, so no suite names them"
    );
}

/// The pipeline must not run the whole suite twice: the segmentation exists
/// to stop a suite from being duplicated, and a second `--workspace` without
/// a target filter would run everything again.
#[test]
fn the_pipeline_does_not_run_the_whole_workspace_twice() {
    let stages = stages_in_pipeline();

    let whole_workspace = stages
        .iter()
        .filter(|(_, body)| runs_the_whole_workspace(body))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        whole_workspace,
        Vec::<String>::new(),
        "these stages run `cargo test --workspace` without naming the targets, which runs the \
         whole suite again and undoes the segmentation"
    );
}

/// A stage must not run the whole workspace.
///
/// The isolation lane is the one exception: it exists to run the same
/// workspace with per-test isolation, and it is not in the pipeline on
/// purpose.
#[test]
fn no_stage_runs_the_whole_workspace() {
    let stages = stages_in_pipeline();

    let offenders: Vec<String> = stages
        .iter()
        .filter(|(_, body)| runs_the_whole_workspace(body))
        .filter(|(_, body)| !is_the_isolation_lane(body))
        .map(|(name, _)| name.clone())
        .collect();

    assert!(
        offenders.is_empty(),
        "these stages run the whole workspace, which undoes the segmentation behind the \
         segmentation's back: {offenders:?}"
    );
}

/// The suite lists are kept in one place, so a reviewer can see the whole
/// segmentation without reading three stages.
/// The inventory of the tests that are discovered and the suite each one
/// runs in.
///
/// The guard proves that the segmentation does not drift. The inventory says
/// what it actually is, in the log of the run, because a suite whose
/// contents nobody has read is a suite nobody maintains.
#[test]
fn the_inventory_of_the_discovered_tests_is_reported() {
    let targets = test_targets();
    let suites = suites_in_pipeline();

    let mut lines = Vec::new();
    for target in &targets {
        let suite = suites
            .iter()
            .find(|(_, list)| list.contains(target))
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "NONE".to_string());
        lines.push(format!("  {target} -> {suite}"));
    }

    println!(
        "test inventory: {} target(s) discovered, {} suite(s) declared\n{}",
        targets.len(),
        suites.len(),
        lines.join("\n")
    );

    assert!(
        !targets.is_empty(),
        "the workspace has test targets, so the inventory cannot be empty"
    );
}

#[test]
fn the_suites_are_listed_in_the_pipeline() {
    let suites = suites_in_pipeline();
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

/// The conformity report is the evidence of the black-box conformance, and
/// it is archived by the pipeline.
///
/// A report that only exists in a file of a machine is lost as soon as the
/// machine is cleaned, and a suite that passes without writing it has not
/// done its work: the pipeline fails when the report is missing.
#[test]
fn the_pipeline_archives_the_conformity_report() {
    let stages = stages_in_pipeline();
    // The conformity report is written by `raw_conformity_uat`, which boots
    // the application on the graph profile and so runs in the dgraph stage.
    let dgraph = stages.get(DGRAPH).expect("the dgraph suite");

    assert!(
        dgraph.contains("target/uat/raw-conformity.md"),
        "the dgraph stage must mention the conformity report"
    );
    assert!(
        dgraph.contains("archiveArtifacts"),
        "the dgraph stage must archive the conformity report"
    );
}

/// The report of a previous run is not the evidence of this one.
///
/// The stage removes the report before it runs the suite, so a suite that
/// passes without writing the report leaves the stage without the file,
/// and the stage fails.
#[test]
fn the_pipeline_removes_the_previous_conformity_report() {
    let stages = stages_in_pipeline();
    let dgraph = stages.get(DGRAPH).expect("the dgraph suite");
    let legacy = stages.get(LEGACY).expect("the legacy suite");

    assert!(
        dgraph.contains("rm -f target/uat/raw-conformity.md"),
        "the dgraph stage must remove the report of the previous run before running the suite"
    );
    assert!(
        legacy.contains("--features legacy"),
        "the legacy stage must run its suites with the legacy feature"
    );
}

/// The targets Cargo only builds with the `legacy` feature
/// (`required-features = ["legacy"]` in the manifest).
fn legacy_only_targets() -> Vec<String> {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml"))
        .unwrap_or_else(|error| panic!("cannot read the manifest: {error}"));
    let mut targets = Vec::new();
    for block in manifest.split("[[test]]").skip(1) {
        let block = block.split("\n[").next().unwrap_or(block);
        if !block.contains("required-features = [\"legacy\"]") {
            continue;
        }
        if let Some(line) = block
            .lines()
            .find(|line| line.trim().starts_with("name = "))
        {
            targets.push(line.split('"').nth(1).unwrap_or_default().to_string());
        }
    }
    targets
}

/// HSP-314 F2: `legacy` is not a default feature, so a target that needs it
/// and is run by a command without `--features legacy` fails the stage with
/// "requires the features: legacy". Every such target has to be named by a
/// command that turns the feature on.
#[test]
fn every_legacy_only_target_runs_with_the_legacy_feature() {
    let targets = legacy_only_targets();
    assert!(
        !targets.is_empty(),
        "the manifest declares legacy-only targets, so the list cannot be empty"
    );
    let stages = stages_in_pipeline();
    let bodies: Vec<&String> = stages.values().collect();
    for target in &targets {
        let needle = format!("--test {target}\"");
        let needle_inline = format!("--test {target} ");
        let owners: Vec<&&String> = bodies
            .iter()
            .filter(|body| body.contains(&needle) || body.contains(&needle_inline))
            .collect();
        assert!(
            !owners.is_empty(),
            "the legacy-only target `{target}` is named by no stage"
        );
        for body in owners {
            let command_start = body
                .find(&needle)
                .or_else(|| body.find(&needle_inline))
                .expect("the target is in the body");
            let command_head = &body[..command_start];
            let command_begin = command_head.rfind("cargo test").unwrap_or(0);
            let command = &command_head[command_begin..];
            assert!(
                command.contains("--features legacy"),
                "the legacy-only target `{target}` is run by a `cargo test` command without \
                 `--features legacy`: {command}"
            );
        }
    }
}
