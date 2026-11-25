//! HSP-241 / HCR-19: the community quickstart is a path that works.
//!
//! The tutorial is not a document about a path: it is the path. This suite
//! runs the script that the tutorial tells the community to run, against the
//! real binary, and checks what the tutorial promises:
//!
//! 1. The account is declared and its two repositories (Raw and Maven) are
//!    created over HTTP.
//! 2. A Raw blob and a Maven package are published and pulled back, and the
//!    digest of what comes back is the digest of what went in.
//! 3. The service is restarted, and the digest, the catalog and the
//!    credential survive it.
//! 4. The cleanup removes the laboratory and nothing else, and it refuses a
//!    directory that does not declare itself a laboratory.
//! 5. The production profile without a secret starts closed, and with a
//!    secret and without a policy it refuses to start with an explicit
//!    reason.
//!
//! 6. (HSP-286) The same path, taken from the real tarball that
//!    `scripts/package-release.sh` produces and run without `cargo` in the
//!    `PATH`, including a valid token of another account that gets a 403.
//! 7. (HSP-270) The bash block of the README Quickstart is extracted and
//!    executed against that tarball, digest verification included.
//!
//! The service is a real process and the client is a real `curl`: the
//! assertions are about what the wire said.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The repository, which is where the script and the templates live.
fn the_repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The script the tutorial tells the community to run.
fn the_script() -> PathBuf {
    the_repo().join("scripts/community-quickstart.sh")
}

/// A distribution that is not packaged: the binary the suite was built
/// with, and the configuration the service reads.
///
/// The tarball is what the community downloads, but a suite that has to
/// build a release to prove a tutorial works is a suite that is slow and
/// that nobody runs. The layout is the one the tarball has, so the script
/// cannot tell the difference.
fn a_distribution(root: &Path) -> PathBuf {
    let distribution = root.join("distribution");
    fs::create_dir_all(distribution.join("bin")).expect("the bin directory");
    fs::create_dir_all(distribution.join("config/community")).expect("the config directory");
    for file in [
        "config/default.toml",
        "config/release.toml",
        "config/community/schema.json",
        "config/community/policies.cedar",
    ] {
        fs::copy(the_repo().join(file), distribution.join(file))
            .unwrap_or_else(|error| panic!("the copy of {file}: {error}"));
    }
    let binary = distribution.join("bin/hodei-artifacts-api");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_hodei-artifacts-api"), &binary)
        .unwrap_or_else(|error| panic!("the link to the binary: {error}"));
    distribution
}

/// The script, run, and everything it said.
fn run_the_script(arguments: &[&str]) -> (bool, String) {
    let output = Command::new("bash")
        .arg(the_script())
        .args(arguments)
        .output()
        .expect("the quickstart script runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn the_community_quickstart_publishes_pulls_and_survives_a_restart() {
    let root = tempfile::tempdir().expect("a temp dir");
    let distribution = a_distribution(root.path());
    let lab = root.path().join("lab");
    // A file next to the laboratory: the cleanup must not touch it.
    let neighbour = root.path().join("not-the-lab");
    fs::write(&neighbour, b"this file is not the laboratory").expect("the neighbour file");

    let lab_argument = lab.to_string_lossy().into_owned();
    let (succeeded, text) = run_the_script(&[
        "run",
        "--distribution",
        &distribution.to_string_lossy(),
        "--lab-dir",
        &lab_argument,
    ]);
    assert!(succeeded, "the quickstart did not finish:\n{text}");

    // The account, its repositories and the two publishes.
    assert!(
        text.contains("created lab-raw (raw, hosted) -> 201"),
        "the Raw repository was not created:\n{text}"
    );
    assert!(
        text.contains("created lab-maven (maven2, hosted) -> 201"),
        "the Maven repository was not created:\n{text}"
    );
    assert!(
        text.contains("published lab-raw/quickstart/hello.txt -> 201"),
        "the blob was not published:\n{text}"
    );
    assert!(
        text.contains("published lab-maven/com/acme/lib/1.0.0/lib-1.0.0.jar -> 201"),
        "the jar was not published:\n{text}"
    );

    // The pulls, with the digests that prove them.
    assert!(
        text.contains("the Raw blob came back with the same digest"),
        "the blob did not come back with its digest:\n{text}"
    );
    assert!(
        text.contains("the Maven jar came back with the same content"),
        "the jar did not come back:\n{text}"
    );
    assert!(
        text.contains("the checksum of the jar came back"),
        "the checksum of the jar did not come back:\n{text}"
    );

    // The restart.
    assert!(
        text.contains("the catalog kept both repositories"),
        "the catalog did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("the Raw blob kept its digest"),
        "the blob did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("the Maven package kept its content"),
        "the package did not survive the restart:\n{text}"
    );
    assert!(
        text.contains("without it the answer is 401"),
        "the credential did not survive the restart:\n{text}"
    );

    // The cleanup: the laboratory and nothing else.
    let (succeeded, text) = run_the_script(&["cleanup", "--lab-dir", &lab_argument]);
    assert!(succeeded, "the cleanup did not finish:\n{text}");
    assert!(
        !lab.exists(),
        "the laboratory is still there after the cleanup"
    );
    assert!(
        neighbour.exists(),
        "the cleanup touched a file that is not its own"
    );

    // And it refuses a directory that does not declare itself a laboratory.
    let (succeeded, text) = run_the_script(&["cleanup", "--lab-dir", &neighbour.to_string_lossy()]);
    assert!(
        !succeeded,
        "the cleanup removed a directory that is not a lab:\n{text}"
    );
    assert!(
        neighbour.exists(),
        "the cleanup removed a directory that does not declare itself a lab"
    );
}

#[test]
fn the_production_profile_without_a_secret_stays_closed() {
    let root = tempfile::tempdir().expect("a temp dir");
    let distribution = a_distribution(root.path());
    let lab = root.path().join("guard-lab");

    let (succeeded, text) = run_the_script(&[
        "guard",
        "--distribution",
        &distribution.to_string_lossy(),
        "--lab-dir",
        &lab.to_string_lossy(),
    ]);
    assert!(succeeded, "the guard did not finish:\n{text}");

    // The production profile declares no secret.
    assert!(
        text.contains("no auth_jwt_secret in the production profile"),
        "the production profile was not checked:\n{text}"
    );

    // Without a secret the service starts closed.
    assert!(
        text.contains("GET /health -> 200"),
        "the health check is not public:\n{text}"
    );
    assert!(
        text.contains("PUT /repository/whatever/whatever -> 404"),
        "the content surface answered without a credential:\n{text}"
    );
    assert!(
        text.contains("POST /api/v1/catalog/repositories -> 401"),
        "the catalog answered without an authenticator:\n{text}"
    );
    assert!(
        text.contains("GET /api/v1/search -> 401"),
        "the search answered without an authenticator:\n{text}"
    );

    // With a secret and without a policy, the boot refuses explicitly.
    assert!(
        text.contains("Production security requires"),
        "the refusal did not say what was missing:\n{text}"
    );
    assert!(
        text.contains("cannot read authorization policy bundle"),
        "the refusal did not say what it could not read:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// HSP-286 / HSP-270: the real tarball, a clean directory and no toolchain.
// ---------------------------------------------------------------------------

/// The distribution as `scripts/package-release.sh` produces it.
struct Packaged {
    /// The directory the script wrote into.
    out_dir: PathBuf,
    /// `hodei-artifacts-api-<version>-<target>`.
    package_name: String,
    /// The version and the target, as the script resolved them.
    version: String,
    target: String,
}

impl Packaged {
    fn archive(&self) -> PathBuf {
        self.out_dir.join(format!("{}.tar.gz", self.package_name))
    }

    fn checksum_file(&self) -> PathBuf {
        self.out_dir
            .join(format!("{}.tar.gz.sha256", self.package_name))
    }
}

/// The tarball is built once per test binary and shared by every test that
/// needs it: it is the one fixture of the HSP-286 and HSP-270 checks.
///
/// The script is the one the release uses. Only the build profile differs
/// (`HODEI_PACKAGE_PROFILE=debug`), because the binaries that this very
/// suite was built with are the ones the script then finds, instead of
/// paying for an optimized build that nobody would run in a test.
fn the_packaged_release() -> &'static Packaged {
    static PACKAGED: OnceLock<Packaged> = OnceLock::new();
    PACKAGED.get_or_init(|| {
        let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hsp241-package");
        let _ = fs::remove_dir_all(&out_dir);
        let output = Command::new("bash")
            .arg(the_repo().join("scripts/package-release.sh"))
            .arg(&out_dir)
            .env("HODEI_PACKAGE_PROFILE", "debug")
            .current_dir(the_repo())
            .output()
            .expect("package-release.sh runs");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "package-release.sh failed:\n{text}"
        );
        let archive_line = text
            .lines()
            .find_map(|line| line.strip_prefix("distribution: "))
            .unwrap_or_else(|| panic!("package-release.sh did not name the archive:\n{text}"));
        let package_name = Path::new(archive_line.trim())
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".tar.gz"))
            .expect("the archive name")
            .to_owned();
        let rest = package_name
            .strip_prefix("hodei-artifacts-api-")
            .expect("the package name starts with the binary name");
        let (version, target) = rest.split_once('-').expect("version and target");
        Packaged {
            out_dir,
            version: version.to_owned(),
            target: target.to_owned(),
            package_name,
        }
    })
}

/// The directories a machine without a Rust toolchain has in its `PATH`.
const CLEAN_PATH: &str = "/usr/bin:/bin";

/// Proves that the clean `PATH` really has no toolchain, so the tests that
/// use it are not vacuous on a machine that installs cargo system-wide.
fn assert_no_toolchain_in_the_clean_path() {
    let output = Command::new("bash")
        .args(["-c", "command -v cargo rustc rustup"])
        .env_clear()
        .env("PATH", CLEAN_PATH)
        .output()
        .expect("bash runs");
    assert!(
        !output.status.success() && output.stdout.is_empty(),
        "the clean PATH still finds a toolchain: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A command with nothing of the caller's environment but a clean `PATH`.
fn a_clean_command(home: &Path) -> Command {
    let mut command = Command::new("bash");
    command
        .env_clear()
        .env("PATH", CLEAN_PATH)
        .env("HOME", home)
        .env("TMPDIR", home);
    command
}

fn output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// The assertions about what the `run` said, shared by both paths.
fn assert_the_run_told_the_whole_story(text: &str) {
    for expected in [
        "created lab-raw (raw, hosted) -> 201",
        "created lab-maven (maven2, hosted) -> 201",
        "the Raw blob came back with the same digest",
        "the Maven jar came back with the same content",
        "the checksum of the jar came back",
        "the Raw blob kept its digest",
        "the Maven package kept its content",
        "without it the answer is 401",
        "cannot create a repository -> 403",
    ] {
        assert!(text.contains(expected), "missing `{expected}`:\n{text}");
    }
}

#[test]
fn the_real_tarball_runs_in_a_clean_directory_without_cargo() {
    assert_no_toolchain_in_the_clean_path();
    let packaged = the_packaged_release();
    let clean = tempfile::tempdir().expect("a clean directory");
    let unpack = clean.path().join("unpack");
    fs::create_dir_all(&unpack).expect("the unpack directory");

    let untar = Command::new("tar")
        .arg("-xzf")
        .arg(packaged.archive())
        .arg("-C")
        .arg(&unpack)
        .output()
        .expect("tar runs");
    assert!(
        untar.status.success(),
        "tar failed: {}",
        output_text(&untar)
    );

    let distribution = unpack.join(&packaged.package_name);
    assert!(
        distribution.join("community-quickstart.sh").is_file(),
        "the tarball does not carry the quickstart script"
    );
    assert!(
        distribution.join("bin/hodei-artifacts-api").is_file(),
        "the tarball does not carry the binary (a symlink to the test binary would not be a file here)"
    );

    // The script runs from inside the extracted tarball, which is the cwd a
    // person has after `cd`, with no toolchain and nothing of our environment.
    let output = a_clean_command(clean.path())
        .current_dir(&distribution)
        .args(["./community-quickstart.sh", "run"])
        .output()
        .expect("the quickstart runs");
    let text = output_text(&output);
    assert!(output.status.success(), "the quickstart failed:\n{text}");
    assert_the_run_told_the_whole_story(&text);
    assert!(
        text.contains("cannot read the Raw blob of the lab")
            || text.contains("the same token cannot read the Raw blob of the lab -> 40"),
        "the foreign token was not checked against the blob:\n{text}"
    );
}

#[test]
fn the_faq_declares_that_the_503_of_a_down_verifier_is_not_exercised() {
    // The laboratory verifies HS256 tokens with a local secret: there is no
    // remote verifier to take down, and inventing one would be fiction. The
    // FAQ has to say so instead of leaving a promise without a test.
    let faq = fs::read_to_string(the_repo().join("docs/community/quickstart.md"))
        .expect("the quickstart guide");
    assert!(
        faq.contains("Ningún test del quickstart provoca un\n> `503`"),
        "the FAQ does not declare that the 503 is not exercised"
    );
    assert!(
        faq.contains("responde `403` (no `401`)"),
        "the FAQ does not point to the 403 the run demonstrates"
    );
}

// --- HSP-270: the README block is the thing that runs --------------------------

/// The first fenced `bash` block of the Quickstart section of a README.
fn the_quickstart_block(readme: &str) -> String {
    let section = readme
        .lines()
        .skip_while(|line| !(line.starts_with("## ") && line.to_lowercase().contains("quickstart")))
        .skip(1)
        .collect::<Vec<_>>();
    let mut block = Vec::new();
    let mut inside = false;
    for line in section {
        if line.starts_with("## ") {
            break;
        }
        if inside {
            if line.starts_with("```") {
                return block.join("\n") + "\n";
            }
            block.push(line.to_owned());
        } else if line.trim() == "```bash" {
            inside = true;
        }
    }
    panic!("the Quickstart section has no closed bash block");
}

/// The block with the two documented placeholders resolved.
fn resolved(block: &str, packaged: &Packaged) -> String {
    block
        .lines()
        .map(|line| {
            if line.starts_with("VERSION=") {
                format!("VERSION={}", packaged.version)
            } else if line.starts_with("TARGET=") {
                format!("TARGET={}", packaged.target)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A directory with the archive and its checksum file, and a script that is
/// the README block, kept in a different directory from the downloads.
fn the_readme_workbench(root: &Path, packaged: &Packaged, block: &str) -> (PathBuf, PathBuf) {
    let downloads = root.join("downloads");
    let scripts = root.join("scripts");
    fs::create_dir_all(&downloads).expect("downloads");
    fs::create_dir_all(&scripts).expect("scripts");
    fs::copy(
        packaged.archive(),
        downloads.join(format!("{}.tar.gz", packaged.package_name)),
    )
    .expect("the archive copy");
    fs::copy(
        packaged.checksum_file(),
        downloads.join(format!("{}.tar.gz.sha256", packaged.package_name)),
    )
    .expect("the checksum copy");
    let script = scripts.join("readme-quickstart.sh");
    fs::write(&script, resolved(block, packaged)).expect("the block script");
    (downloads, script)
}

#[test]
fn the_readme_quickstart_block_runs_against_the_real_bundle() {
    assert_no_toolchain_in_the_clean_path();
    let packaged = the_packaged_release();
    let readme = fs::read_to_string(the_repo().join("README.md")).expect("the README");
    let block = the_quickstart_block(&readme);
    assert!(
        block.contains("sha256sum -c"),
        "the README block does not verify the digest of the archive"
    );
    assert!(
        !block.contains("<version>") && !block.contains("<target>"),
        "the README block still has unresolved placeholders"
    );

    let root = tempfile::tempdir().expect("a temp dir");
    let (downloads, script) = the_readme_workbench(root.path(), packaged, &block);
    let output = a_clean_command(root.path())
        .current_dir(&downloads)
        .args(["-e"])
        .arg(&script)
        .output()
        .expect("the block runs");
    let text = output_text(&output);
    assert!(output.status.success(), "the README block failed:\n{text}");
    assert!(
        text.contains(&format!("{}.tar.gz: OK", packaged.package_name)),
        "the digest of the archive was not verified:\n{text}"
    );
    assert_the_run_told_the_whole_story(&text);
}

#[test]
fn the_readme_quickstart_block_refuses_a_tampered_archive() {
    let packaged = the_packaged_release();
    let readme = fs::read_to_string(the_repo().join("README.md")).expect("the README");
    let block = the_quickstart_block(&readme);

    let root = tempfile::tempdir().expect("a temp dir");
    let (downloads, script) = the_readme_workbench(root.path(), packaged, &block);
    let archive = downloads.join(format!("{}.tar.gz", packaged.package_name));
    let mut bytes = fs::read(&archive).expect("the archive");
    bytes.push(0);
    fs::write(&archive, bytes).expect("the tampered archive");

    let output = a_clean_command(root.path())
        .current_dir(&downloads)
        .args(["-e"])
        .arg(&script)
        .output()
        .expect("the block runs");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "the README block accepted a tampered archive:\n{text}"
    );
    assert!(
        text.contains("FAILED"),
        "no digest failure was reported:\n{text}"
    );
    assert!(
        !text.contains("==> The lab"),
        "the quickstart ran from an archive that did not verify:\n{text}"
    );
}

#[test]
fn the_spanish_readme_has_the_same_quickstart_and_the_same_headings() {
    let english = fs::read_to_string(the_repo().join("README.md")).expect("README.md");
    let spanish = fs::read_to_string(the_repo().join("README.es.md")).expect("README.es.md");

    let commands = |block: String| -> Vec<String> {
        block
            .lines()
            .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        commands(the_quickstart_block(&english)),
        commands(the_quickstart_block(&spanish)),
        "the executable lines of the two Quickstart blocks diverged"
    );

    // The headings (outside code fences) have the same shape: same count and
    // same level at every position.
    let levels = |text: &str| -> Vec<usize> {
        let mut inside = false;
        let mut levels = Vec::new();
        for line in text.lines() {
            if line.starts_with("```") {
                inside = !inside;
            } else if !inside && line.starts_with('#') {
                levels.push(line.chars().take_while(|c| *c == '#').count());
            }
        }
        levels
    };
    assert_eq!(
        levels(&english),
        levels(&spanish),
        "README.md and README.es.md have different heading structure"
    );
}
