//! The day-to-day gate must not lie about what it ran.
//!
//! `scripts/test-affected.sh` is the gate people open every ten minutes. It
//! looks at what changed against the base branch, works out which packages
//! are affected --and which ones depend on them-- and runs only those tests.
//!
//! A gate that cannot resolve the diff, or that picks the wrong suites, is
//! worse than no gate at all: it says "green" without having run anything.
//! And a test that did not run is not a test that passed.
//!
//! So the selection is checked against **git fixtures**: a temporary
//! repository --never the real one-- where each kind of change is made in
//! turn, and the plan the script computes is compared with the matrix
//! cause -> gates.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A change and the gates it has to select.
struct Case {
    /// The name of the case, used for the branch and for the report.
    name: &'static str,
    /// The cause the change belongs to.
    cause: &'static str,
    /// The file that changes, relative to the fixture repository.
    file: &'static str,
    /// The content the file gets.
    content: &'static str,
    /// The gates the change has to select.
    gates: &'static [&'static str],
    /// The packages the change has to reach.
    crates: &'static [&'static str],
}

/// The matrix: cause -> gates.
///
/// The fixture is a small workspace with a kernel, a policy crate and an API
/// on top. The API depends on the kernel; the policy crate depends on the
/// policy engine. So the matrix can tell apart "the kernel changed" from
/// "a policy changed" from "the configuration changed".
const CASES: &[Case] = &[
    Case {
        name: "lockfile",
        cause: "lockfile",
        file: "Cargo.lock",
        content: "# the lockfile changed\nversion = 4\n",
        gates: &[
            "clippy:fixture-api",
            "clippy:hodei-policies",
            "clippy:kernel",
            "test:fixture-api",
            "test:hodei-policies",
            "test:kernel",
        ],
        crates: &["fixture-api", "hodei-policies", "kernel"],
    },
    Case {
        name: "code",
        cause: "code",
        file: "crates/kernel/src/lib.rs",
        content: "pub fn kernel() -> u32 {\n    2\n}\n",
        // The kernel changed, so it is formatted, linted and tested. The API
        // depends on it, so its tests can change too. The policy crate does
        // not depend on the kernel, so it is left alone.
        gates: &[
            "clippy:kernel",
            "fmt:kernel",
            "test:fixture-api",
            "test:kernel",
        ],
        crates: &["fixture-api", "kernel"],
    },
    Case {
        name: "config",
        cause: "config",
        file: "config/default.toml",
        content: "[server]\nport = 3001\n",
        // The configuration is loaded by the API when it starts: only its
        // tests can tell that the configuration is broken.
        gates: &["test:fixture-api"],
        crates: &["fixture-api"],
    },
    Case {
        name: "policy",
        cause: "policy",
        file: "config/community/policies.cedar",
        content: "permit(principal == \"a\", action == \"b\", resource == \"c\");\n",
        // The policy is evaluated by the packages that speak Cedar: the
        // policy crate and the API.
        gates: &["test:fixture-api", "test:hodei-policies"],
        crates: &["fixture-api", "hodei-policies"],
    },
    Case {
        name: "pipeline",
        cause: "pipeline",
        file: "pipeline.kts",
        content: "pipeline {\n    stages {\n    }\n}\n",
        // The segmentation of the suites changed, and its guardian is the
        // one who says whether it is still correct.
        gates: &["ci_suites"],
        crates: &[],
    },
    Case {
        name: "openapi",
        cause: "openapi",
        file: "docs/openapi/openapi.yaml",
        content: "openapi: 3.1.0\ninfo:\n  title: fixture\n  version: 1.0.0\n",
        // The contract has its own gate, and the API serves it.
        gates: &["openapi", "test:fixture-api"],
        crates: &["fixture-api"],
    },
    Case {
        name: "frontend",
        cause: "frontend",
        file: "frontend/src/App.tsx",
        content: "export const App = () => null;\n",
        // The frontend is not Rust: its own gate, and no Rust package.
        gates: &["frontend"],
        crates: &[],
    },
];

// ───────────────────────── el fixture ─────────────────────────

/// A temporary repository with the shape of the real one, but small.
struct Fixture {
    /// The temporary directory: the repository and the policy engine live in
    /// it, and it goes away with the test.
    directory: tempfile::TempDir,
    /// The repository itself.
    repository: PathBuf,
    /// The commit every case starts from.
    base: String,
    /// Where the builds of the fixture go: never the target directory of the
    /// real repository, which is locked by the test that is running.
    target: PathBuf,
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("the path has a parent"))
        .expect("the directories of the fixture are created");
    fs::write(path, content).expect("the file of the fixture is written");
}

fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repository)
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        // The governance of this machine --the conventional commits, the
        // reference transaction hook-- lives in the global configuration.
        // A fixture is a drawing of a repository, not a repository of the
        // workspace, and its history is written with plumbing below.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}

fn git(repository: &Path, arguments: &[&str]) -> String {
    let output = git_command(repository)
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A commit of the fixture, written with plumbing.
///
/// The machine governs the commits of the repositories of the workspace: a
/// commit has to say what it does in a conventional message, and a
/// repository without a remote is not classified. A fixture is neither: it
/// is a throwaway repository in a temporary directory whose history is a
/// drawing of a history. So the fixture writes its commits the way git
/// writes them internally --a tree, a commit and a ref-- and the
/// governance of the workspace does not apply to it.
fn commit(repository: &Path, parent: Option<&str>, message: &str) -> String {
    git(repository, &["add", "-A"]);
    let tree = git(repository, &["write-tree"]).trim().to_string();

    let mut command = git_command(repository);
    command.arg("commit-tree").arg(&tree);
    if let Some(parent) = parent {
        command.arg("-p").arg(parent);
    }
    command.arg("-m").arg(message);

    let output = command.output().expect("git runs");
    assert!(
        output.status.success(),
        "the commit of the fixture cannot be written: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A branch of the fixture that points at a commit.
fn branch(repository: &Path, name: &str, commit: &str) {
    git(
        repository,
        &["update-ref", &format!("refs/heads/{name}"), commit],
    );
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The fixture: a git repository with a kernel, a policy crate and an API.
fn fixture() -> Fixture {
    fixture_with("", "kernel = { path = \"crates/kernel\" }\n")
}

/// The same fixture, with extra root manifest text (appended at the end) and
/// the line that makes the API depend on the kernel.
fn fixture_with(root_extra: &str, api_dependency: &str) -> Fixture {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let repository = directory.path().join("fixture");
    let target = directory.path().join("target");

    // The policy engine of the fixture is a package OUTSIDE of the
    // workspace: the workspace only has to know that a package depends on
    // it, and a package of the registry would need the network.
    let engine = directory.path().join("cedar-policy");
    write(
        &engine.join("Cargo.toml"),
        "[package]\nname = \"cedar-policy\"\nversion = \"4.5.1\"\nedition = \"2024\"\n\n[workspace]\n",
    );
    write(
        &engine.join("src/lib.rs"),
        "pub fn engine() -> u32 {\n    1\n}\n",
    );

    write(
        &repository.join("Cargo.toml"),
        &format!(
            "[workspace]\nmembers = [\"crates/kernel\", \"crates/hodei-policies\"]\nresolver = \"2\"\n\n\
         [package]\nname = \"fixture-api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\n{api_dependency}{root_extra}"
        ),
    );
    write(
        &repository.join("crates/kernel/Cargo.toml"),
        "[package]\nname = \"kernel\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        &repository.join("crates/kernel/src/lib.rs"),
        "pub fn kernel() -> u32 {\n    1\n}\n",
    );
    write(
        &repository.join("crates/kernel/tests/kernel_contract.rs"),
        "#[test]\nfn the_kernel_answers() {\n    assert_eq!(kernel::kernel(), 1);\n}\n",
    );
    write(
        &repository.join("crates/hodei-policies/Cargo.toml"),
        "[package]\nname = \"hodei-policies\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\ncedar-policy = { path = \"../../../cedar-policy\" }\n",
    );
    write(
        &repository.join("crates/hodei-policies/src/lib.rs"),
        "pub fn policies() -> u32 {\n    1\n}\n",
    );
    write(
        &repository.join("crates/hodei-policies/tests/policy_contract.rs"),
        "#[test]\nfn the_policies_answer() {\n    assert_eq!(hodei_policies::policies(), 1);\n}\n",
    );
    write(
        &repository.join("src/lib.rs"),
        "pub fn api() -> u32 {\n    kernel::kernel()\n}\n",
    );
    write(
        &repository.join("tests/smoke.rs"),
        "#[test]\nfn the_api_answers() {\n    assert_eq!(fixture_api::api(), 1);\n}\n",
    );
    // The guardian of the segmentation: without it the pipeline gate has no
    // target to run.
    write(
        &repository.join("tests/ci_suites.rs"),
        "#[test]\nfn the_suites_are_the_ones_the_pipeline_declares() {\n    assert!(true);\n}\n",
    );
    write(
        &repository.join("config/default.toml"),
        "[server]\nport = 3000\n",
    );
    write(
        &repository.join("config/community/policies.cedar"),
        "permit(principal == \"base\", action == \"b\", resource == \"c\");\n",
    );
    write(
        &repository.join("docs/openapi/openapi.yaml"),
        "openapi: 3.1.0\ninfo:\n  title: the base of the fixture\n  version: 1.0.0\n",
    );
    write(
        &repository.join("frontend/package.json"),
        "{\n  \"name\": \"fixture-frontend\",\n  \"version\": \"0.0.0\"\n}\n",
    );
    write(
        &repository.join("frontend/src/App.tsx"),
        "export const App = () => <div />;\n",
    );
    write(
        &repository.join("pipeline.kts"),
        "pipeline {\n    stages {\n        stage(\"the base of the fixture\") {\n        }\n    }\n}\n",
    );
    write(&repository.join(".gitignore"), "target/\n");

    // The script under test: the real one, copied into the fixture.
    for script in ["test-affected.sh", "test-affected.py"] {
        let from = repository_root().join("scripts").join(script);
        let to = repository.join("scripts").join(script);
        fs::create_dir_all(to.parent().expect("the scripts directory"))
            .expect("the scripts directory of the fixture is created");
        fs::copy(&from, &to).expect("the script is copied into the fixture");
    }

    // The lockfile of the fixture: it has to exist, committed, so that a
    // change on it can be tested.
    let generated = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&repository)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("cargo runs");
    assert!(
        generated.status.success(),
        "the lockfile of the fixture cannot be generated: {}",
        String::from_utf8_lossy(&generated.stderr)
    );

    git(&repository, &["init", "-q"]);
    let base = commit(&repository, None, "chore: the base of the fixture");
    branch(&repository, "main", &base);
    git(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    Fixture {
        directory,
        repository,
        base,
        target,
    }
}

/// The outcome of running the script.
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn succeeded(&self) -> bool {
        self.code == Some(0)
    }
}

fn run_script(fixture: &Fixture, arguments: &[&str]) -> Outcome {
    let output = Command::new("bash")
        .arg("scripts/test-affected.sh")
        .args(arguments)
        .current_dir(&fixture.repository)
        .env("CARGO_TARGET_DIR", &fixture.target)
        .output()
        .expect("the script runs");
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The report the script wrote.
fn read_report(path: &Path) -> Value {
    let text = fs::read_to_string(path).expect("the report is written");
    serde_json::from_str(&text).expect("the report is JSON")
}

fn names(report: &Value, field: &str) -> BTreeSet<String> {
    report[field]
        .as_array()
        .expect("the report has the field")
        .iter()
        .map(|entry| entry["name"].as_str().expect("it has a name").to_string())
        .collect()
}

fn gates(report: &Value) -> BTreeSet<String> {
    report["gates"]
        .as_array()
        .expect("the report has the gates")
        .iter()
        .map(|entry| entry["gate"].as_str().expect("it has a name").to_string())
        .collect()
}

/// The plan of the fixture after one change.
fn plan_of(fixture: &Fixture, case: &Case) -> Value {
    let name = format!("case/{}", case.name);
    git(
        &fixture.repository,
        &["checkout", "-q", "-b", &name, &fixture.base],
    );
    write(&fixture.repository.join(case.file), case.content);
    let changed = commit(
        &fixture.repository,
        Some(&fixture.base),
        &format!("chore: the change of the case {}", case.name),
    );
    branch(&fixture.repository, &name, &changed);

    let report = fixture
        .directory
        .path()
        .join(format!("report-{}.json", case.name));
    let outcome = run_script(
        fixture,
        &[
            "--plan",
            "--report",
            report.to_str().expect("the path is valid"),
            &fixture.base,
        ],
    );
    assert!(
        outcome.succeeded(),
        "the plan of `{}` fails: {}",
        case.name,
        outcome.stderr
    );
    read_report(&report)
}

// ───────────────────────── la matriz ─────────────────────────

#[test]
fn every_cause_selects_the_gates_it_should() {
    let fixture = fixture();

    for case in CASES {
        let report = plan_of(&fixture, case);

        let selected = gates(&report);
        let expected: BTreeSet<String> = case.gates.iter().map(|gate| gate.to_string()).collect();
        assert_eq!(
            selected, expected,
            "the change `{}` selected the wrong gates",
            case.name
        );

        let causes: Vec<&str> = report["causes"]
            .as_array()
            .expect("the report has the causes")
            .iter()
            .map(|cause| cause["cause"].as_str().expect("it has a name"))
            .collect();
        assert!(
            causes.contains(&case.cause),
            "the change `{}` was not classified as `{}`: {causes:?}",
            case.name,
            case.cause
        );

        let reached = names(&report, "affected_crates");
        let expected_crates: BTreeSet<String> =
            case.crates.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            reached, expected_crates,
            "the change `{}` reached the wrong packages",
            case.name
        );
    }
}

#[test]
fn a_test_that_did_not_run_is_never_a_test_that_passed() {
    let fixture = fixture();
    let report = plan_of(&fixture, &CASES[1]);

    assert_eq!(
        report["result"], "NOT_RUN",
        "a plan that has run nothing cannot be a PASS"
    );
    for gate in report["gates"]
        .as_array()
        .expect("the report has the gates")
    {
        assert_eq!(
            gate["status"], "NOT_RUN",
            "the gate `{}` cannot be a PASS without having run",
            gate["gate"]
        );
        assert!(
            !gate["reason"].as_str().expect("it has a reason").is_empty(),
            "every gate has to say why it was chosen"
        );
    }
}

// ───────────────────────── el fallo honesto ─────────────────────────

#[test]
fn a_base_that_does_not_exist_fails_instead_of_pretending() {
    let fixture = fixture();

    let outcome = run_script(&fixture, &["--plan", "no-existe-esta-rama"]);

    assert!(
        !outcome.succeeded(),
        "a base that does not exist cannot be green: {}",
        outcome.stdout
    );
    assert!(
        outcome.stderr.contains("no existe"),
        "the reason has to be said: {}",
        outcome.stderr
    );
}

// ───────────────────────── la ejecucion ─────────────────────────

#[test]
fn the_gate_runs_what_it_said_it_would_run() {
    let fixture = fixture();

    git(
        &fixture.repository,
        &["checkout", "-q", "-b", "case/run", &fixture.base],
    );
    write(
        &fixture.repository.join("config/default.toml"),
        "[server]\nport = 3001\n",
    );
    let changed = commit(
        &fixture.repository,
        Some(&fixture.base),
        "chore: the configuration changed",
    );
    branch(&fixture.repository, "case/run", &changed);

    let report = fixture.directory.path().join("report-run.json");
    let outcome = run_script(
        &fixture,
        &[
            "--no-bounded",
            "--report",
            report.to_str().expect("the path is valid"),
            &fixture.base,
        ],
    );
    assert!(
        outcome.succeeded(),
        "the gate fails: {}\n{}",
        outcome.stdout,
        outcome.stderr
    );

    let report = read_report(&report);
    assert_eq!(report["result"], "PASS");
    assert_eq!(
        report["limits"], "none (--no-bounded)",
        "the gate has to say that it ran without the resource limits"
    );

    let gates = report["gates"]
        .as_array()
        .expect("the report has the gates");
    assert_eq!(gates.len(), 1, "only the API is affected: {gates:?}");
    assert_eq!(gates[0]["gate"], "test:fixture-api");
    assert_eq!(gates[0]["status"], "PASS");
    assert!(
        gates[0]["duration_ms"].as_u64().expect("it has a duration") > 0,
        "a gate that ran has a duration"
    );
    assert!(
        !gates[0]["reason"]
            .as_str()
            .expect("it has a reason")
            .is_empty(),
        "the gate has to say why it was chosen"
    );

    // The output has to say what was chosen and why: a gate that runs
    // something without saying what is a gate nobody can review.
    assert!(
        outcome.stdout.contains("test:fixture-api"),
        "the output does not say what it runs: {}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("porque"),
        "the output does not say why: {}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains("bounded no se usa"),
        "the output does not say that it ran without the limits: {}",
        outcome.stdout
    );
}

/// A PATH where `bounded` is not installed, whatever the host has.
///
/// Every directory of the current PATH that contains a `bounded` is replaced
/// by a shadow directory with symlinks to everything else in it. If the host
/// has no `bounded` the PATH is unchanged, so the test never depends on the
/// tool being installed.
fn path_without_bounded(fixture: &Fixture) -> String {
    let original = std::env::var("PATH").unwrap_or_default();
    let mut parts = Vec::new();
    // One shadow per distinct directory: PATH may repeat an entry, and a
    // wrapper that skips "its own directory" loops if the copies differ.
    let mut shadows: std::collections::HashMap<String, String> = Default::default();
    for (index, entry) in original.split(':').enumerate() {
        let directory = Path::new(entry);
        if entry.is_empty() || !directory.join("bounded").exists() {
            parts.push(entry.to_string());
            continue;
        }
        if let Some(shadow) = shadows.get(entry) {
            parts.push(shadow.clone());
            continue;
        }
        let shadow = fixture
            .directory
            .path()
            .join(format!("path-without-bounded-{index}"));
        fs::create_dir_all(&shadow).expect("the directory is created");
        for item in fs::read_dir(directory).expect("the directory is readable") {
            let item = item.expect("the entry is readable");
            if item.file_name() == "bounded" {
                continue;
            }
            let _ = std::os::unix::fs::symlink(item.path(), shadow.join(item.file_name()));
        }
        shadows.insert(entry.to_string(), shadow.to_string_lossy().into_owned());
        parts.push(shadow.to_string_lossy().into_owned());
    }
    parts.join(":")
}

#[test]
fn the_gate_runs_where_bounded_is_not_installed() {
    let fixture = fixture();

    git(
        &fixture.repository,
        &["checkout", "-q", "-b", "case/run", &fixture.base],
    );
    write(
        &fixture.repository.join("config/default.toml"),
        "[server]\nport = 3001\n",
    );
    let changed = commit(
        &fixture.repository,
        Some(&fixture.base),
        "chore: the configuration changed",
    );
    branch(&fixture.repository, "case/run", &changed);

    let path = path_without_bounded(&fixture);
    let report = fixture.directory.path().join("report-without-bounded.json");
    let output = Command::new("bash")
        .arg("scripts/test-affected.sh")
        .arg("--report")
        .arg(report.to_str().expect("the path is valid"))
        .arg(&fixture.base)
        .current_dir(&fixture.repository)
        .env("CARGO_TARGET_DIR", &fixture.target)
        .env("PATH", &path)
        .output()
        .expect("the script runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "the gate has to run where bounded is not installed: {stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("bounded no esta instalado"),
        "the gate has to say that the resource limits are not in use: {stdout}"
    );

    let report = read_report(&report);
    assert_eq!(
        report["limits"], "none (bounded no esta instalado)",
        "the report has to say that it ran without the resource limits"
    );
    let gates = report["gates"]
        .as_array()
        .expect("the report has the gates");
    assert_eq!(gates.len(), 1, "only the API is affected: {gates:?}");
    assert_eq!(
        gates[0]["status"], "PASS",
        "the gate has to run and pass without the limits: {gates:?}"
    );
}

#[test]
fn the_report_records_the_commit_the_base_and_the_reason() {
    let fixture = fixture();
    let report = plan_of(&fixture, &CASES[1]);

    let repository = &report["repository"];
    assert_eq!(
        repository["base"]["commit"]
            .as_str()
            .expect("it has a commit"),
        fixture.base,
        "the report has to record the base it compared against"
    );
    assert!(
        !repository["commit"]
            .as_str()
            .expect("it has a commit")
            .is_empty(),
        "the report has to record the commit it ran"
    );
    assert_eq!(report["mode"], "plan");
    assert!(
        report["duration_ms"].as_u64().is_some(),
        "the report has to record how long it took"
    );
}

// ───────────────────────── HSP-293: the exit code ─────────────────────────

/// Commits `files` on a new branch off the base and makes it HEAD.
fn change(fixture: &Fixture, name: &str, files: &[(&str, &str)]) -> String {
    git(
        &fixture.repository,
        &["checkout", "-q", "-b", name, &fixture.base],
    );
    for (file, content) in files {
        write(&fixture.repository.join(file), content);
    }
    let changed = commit(
        &fixture.repository,
        Some(&fixture.base),
        &format!("chore: the change {name}"),
    );
    branch(&fixture.repository, name, &changed);
    changed
}

fn report_path(fixture: &Fixture, name: &str) -> PathBuf {
    fixture.directory.path().join(format!("report-{name}.json"))
}

fn run_with_report(fixture: &Fixture, name: &str, extra: &[&str]) -> (Outcome, PathBuf) {
    let report = report_path(fixture, name);
    let mut arguments: Vec<&str> = vec!["--report", report.to_str().expect("a valid path")];
    arguments.extend_from_slice(extra);
    arguments.push(&fixture.base);
    (run_script(fixture, &arguments), report)
}

fn assert_no_pass_report(report: &Path) {
    if report.exists() {
        assert_ne!(
            read_report(report)["result"],
            "PASS",
            "no report may say PASS"
        );
    }
}

const BROKEN_POLICIES: &str = "pub fn policies() -> u32 {\n    2\n}\n";

#[test]
fn a_failing_test_makes_the_script_exit_non_zero() {
    let fixture = fixture();
    change(
        &fixture,
        "case/fail",
        &[("crates/hodei-policies/src/lib.rs", BROKEN_POLICIES)],
    );
    let (outcome, report) = run_with_report(&fixture, "fail", &["--no-bounded", "--no-clippy"]);

    assert!(
        !outcome.succeeded(),
        "a FAIL gate cannot exit 0: {}",
        outcome.stdout
    );
    assert_eq!(outcome.code, Some(1));
    assert_eq!(read_report(&report)["result"], "FAIL");
}

#[test]
fn a_gate_that_is_not_run_makes_the_script_exit_non_zero() {
    let fixture = fixture();
    change(
        &fixture,
        "case/not-run",
        &[("frontend/src/App.tsx", "export const App = () => null;\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "not-run", &["--no-bounded"]);

    assert!(
        !outcome.succeeded(),
        "a NOT_RUN gate cannot exit 0: {}",
        outcome.stdout
    );
    assert_eq!(read_report(&report)["result"], "NOT_RUN");
}

#[test]
fn the_plan_mode_keeps_exiting_zero_with_not_run_gates() {
    // Deliberate contract: a plan runs nothing, so NOT_RUN is expected.
    let fixture = fixture();
    change(
        &fixture,
        "case/plan",
        &[("frontend/src/App.tsx", "export const App = () => null;\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "plan", &["--plan"]);

    assert!(
        outcome.succeeded(),
        "{}\n{}",
        outcome.stdout,
        outcome.stderr
    );
    assert_eq!(read_report(&report)["result"], "NOT_RUN");
}

// ───────────────────────── HSP-293: ownership ─────────────────────────

#[test]
fn a_build_script_belongs_to_its_package() {
    let fixture = fixture();
    change(
        &fixture,
        "case/build-rs",
        &[("crates/kernel/build.rs", "fn main() {}\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "build-rs", &["--plan"]);
    assert!(outcome.succeeded(), "{}", outcome.stderr);
    let report = read_report(&report);

    assert!(
        gates(&report).contains("test:kernel"),
        "{:?}",
        gates(&report)
    );
    assert!(gates(&report).contains("test:fixture-api"));
    assert!(names(&report, "affected_crates").contains("kernel"));
}

#[test]
fn an_included_asset_belongs_to_the_member_that_contains_it() {
    let fixture = fixture();
    change(
        &fixture,
        "case/asset",
        &[("crates/kernel/data/table.json", "{}\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "asset", &["--plan"]);
    assert!(outcome.succeeded(), "{}", outcome.stderr);
    assert!(gates(&read_report(&report)).contains("test:kernel"));
}

#[test]
fn a_member_readme_selects_nothing() {
    let fixture = fixture();
    change(
        &fixture,
        "case/readme",
        &[("crates/kernel/README.md", "# kernel\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "readme", &["--plan"]);
    assert!(outcome.succeeded(), "{}", outcome.stderr);
    assert!(gates(&read_report(&report)).is_empty());
}

#[test]
fn a_rust_file_outside_every_package_fails_explicitly() {
    let fixture = fixture();
    change(
        &fixture,
        "case/orphan",
        &[("stray/helper.rs", "pub fn helper() {}\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "orphan", &["--plan"]);

    assert!(!outcome.succeeded(), "an unclassified .rs cannot be silent");
    assert!(
        outcome.stderr.contains("stray/helper.rs"),
        "the file has to be named: {}",
        outcome.stderr
    );
    assert_no_pass_report(&report);
}

// ───────────────────────── HSP-293: honest failures ─────────────────────────

#[test]
fn broken_cargo_metadata_fails_without_a_pass_report() {
    let fixture = fixture();
    change(
        &fixture,
        "case/broken",
        &[("Cargo.toml", "this is not toml [[[\n")],
    );
    let (outcome, report) = run_with_report(&fixture, "broken", &["--no-bounded"]);

    assert!(!outcome.succeeded(), "broken metadata cannot be green");
    assert_no_pass_report(&report);
}

#[test]
fn an_unresolvable_diff_fails_without_a_pass_report() {
    let fixture = fixture();
    change(
        &fixture,
        "case/no-diff",
        &[("config/default.toml", "[server]\nport = 3001\n")],
    );
    // Remove the root tree of the base: its commit still resolves and the
    // merge base is found, but the diff cannot be computed.
    let tree = git(
        &fixture.repository,
        &["rev-parse", &format!("{}^{{tree}}", fixture.base)],
    )
    .trim()
    .to_string();
    let object = fixture
        .repository
        .join(".git/objects")
        .join(&tree[..2])
        .join(&tree[2..]);
    let mut permissions = fs::metadata(&object).expect("loose object").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&object, permissions).expect("writable");
    fs::remove_file(&object).expect("the tree is removed");

    let (outcome, report) = run_with_report(&fixture, "no-diff", &["--no-bounded"]);

    assert!(
        !outcome.succeeded(),
        "an unresolvable diff cannot be green: {}",
        outcome.stdout
    );
    assert_no_pass_report(&report);
}

#[test]
fn no_common_ancestor_fails_without_a_pass_report() {
    let fixture = fixture();
    git(
        &fixture.repository,
        &["checkout", "-q", "-b", "case/orphan-history", &fixture.base],
    );
    write(
        &fixture.repository.join("config/default.toml"),
        "[server]\nport = 9\n",
    );
    let unrelated = commit(&fixture.repository, None, "chore: an unrelated history");
    branch(&fixture.repository, "case/orphan-history", &unrelated);
    git(
        &fixture.repository,
        &["checkout", "-q", "case/orphan-history"],
    );

    let (outcome, report) = run_with_report(&fixture, "orphan-history", &["--no-bounded"]);

    assert!(!outcome.succeeded(), "no common ancestor cannot be green");
    assert!(
        outcome.stderr.contains("ancestro comun"),
        "{}",
        outcome.stderr
    );
    assert_no_pass_report(&report);
}

// ───────────────────────── HSP-293: graph, bounded, mixed ─────────────────────────

#[test]
fn workspace_and_versioned_dependencies_build_the_graph() {
    let styles: [(&str, &str, &str); 2] = [
        (
            "workspace",
            "kernel.workspace = true\n",
            "\n[workspace.dependencies]\nkernel = { path = \"crates/kernel\" }\n",
        ),
        (
            "version",
            "kernel = { path = \"crates/kernel\", version = \"0.1.0\" }\n",
            "",
        ),
    ];
    for (name, dependency, extra) in styles {
        let fixture = fixture_with(extra, dependency);
        change(
            &fixture,
            &format!("case/graph-{name}"),
            &[(
                "crates/kernel/src/lib.rs",
                "pub fn kernel() -> u32 {\n    1\n}\n// x\n",
            )],
        );
        let (outcome, report) = run_with_report(&fixture, name, &["--plan"]);
        assert!(outcome.succeeded(), "{name}: {}", outcome.stderr);
        assert!(
            names(&read_report(&report), "affected_crates").contains("fixture-api"),
            "{name}: the API depends on the kernel and has to be reached"
        );
    }
}

/// A `bounded` that records how it was invoked and then runs the command.
fn fake_bounded(fixture: &Fixture) -> (String, PathBuf) {
    let directory = fixture.directory.path().join("fake-bin");
    let log = fixture.directory.path().join("bounded.log");
    fs::create_dir_all(&directory).expect("directory");
    let script = directory.join("bounded");
    fs::write(
        &script,
        format!(
            "#!/usr/bin/env bash\necho \"$*\" >> '{}'\nexec \"$@\"\n",
            log.display()
        ),
    )
    .expect("script");
    let mut permissions = fs::metadata(&script).expect("metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(&script, permissions).expect("executable");
    let path = format!(
        "{}:{}",
        directory.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (path, log)
}

fn run_with_path(fixture: &Fixture, path: &str, report: &Path, extra: &[&str]) -> Outcome {
    let output = Command::new("bash")
        .arg("scripts/test-affected.sh")
        .args(extra)
        .arg("--report")
        .arg(report)
        .arg(&fixture.base)
        .current_dir(&fixture.repository)
        .env("CARGO_TARGET_DIR", &fixture.target)
        .env("PATH", path)
        .output()
        .expect("the script runs");
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn bounded_wraps_the_gates_when_it_is_installed() {
    let fixture = fixture();
    change(
        &fixture,
        "case/bounded",
        &[("config/default.toml", "[server]\nport = 3001\n")],
    );
    let (path, log) = fake_bounded(&fixture);
    let report = report_path(&fixture, "bounded");
    let outcome = run_with_path(&fixture, &path, &report, &[]);

    assert!(
        outcome.succeeded(),
        "{}\n{}",
        outcome.stdout,
        outcome.stderr
    );
    assert!(
        outcome.stdout.contains("bounded: los gates"),
        "{}",
        outcome.stdout
    );
    let invoked = fs::read_to_string(&log).expect("bounded was invoked");
    assert!(invoked.contains("cargo test -p fixture-api"), "{invoked}");
    assert_eq!(read_report(&report)["limits"], "bounded");
}

#[test]
fn a_mixed_run_reports_pass_fail_and_not_run() {
    let fixture = fixture();
    let head = change(
        &fixture,
        "case/mixed",
        &[
            ("crates/hodei-policies/src/lib.rs", BROKEN_POLICIES),
            ("config/default.toml", "[server]\nport = 3001\n"),
            ("frontend/src/App.tsx", "export const App = () => null;\n"),
        ],
    );
    let (path, _log) = fake_bounded(&fixture);
    let report = report_path(&fixture, "mixed");
    let outcome = run_with_path(&fixture, &path, &report, &["--no-clippy"]);

    assert!(!outcome.succeeded(), "{}", outcome.stdout);
    let report = read_report(&report);
    assert_eq!(report["result"], "FAIL");
    assert_eq!(report["limits"], "bounded");
    assert_eq!(report["repository"]["commit"], head.as_str());

    let status_of = |gate: &str| {
        report["gates"]
            .as_array()
            .expect("gates")
            .iter()
            .find(|entry| entry["gate"] == gate)
            .map(|entry| entry["status"].as_str().expect("status").to_string())
    };
    assert_eq!(status_of("test:fixture-api").as_deref(), Some("PASS"));
    assert_eq!(status_of("test:hodei-policies").as_deref(), Some("FAIL"));
    assert_eq!(status_of("frontend").as_deref(), Some("NOT_RUN"));
}
