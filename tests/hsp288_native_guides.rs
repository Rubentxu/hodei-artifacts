//! HSP-288: the Maven, npm and PyPI recipes of `docs/community/ecosystems.md`
//! run with the real native clients (`mvn`, `npm`, `twine` and `pip`).
//!
//! The test extracts the block introduced by "Ejemplo con cliente nativo"
//! from each section of the guide and executes it as written (with `bash`)
//! against a real service process, in a directory prepared with the
//! artifacts the block assumes. A recipe that drifts from the real routes,
//! flags or client behavior therefore turns red.
//!
//! A missing client is never a green: the test prints `NOT_RUN: <reason>`,
//! records it in `target/uat/hsp288-native-clients.txt` and fails, unless
//! `HODEI_ALLOW_NOT_RUN=1` accepts the NOT_RUN explicitly (the line is
//! printed anyway). The versions of the clients that did run are recorded in
//! the same file.

mod guide_blocks;
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

/// The catalog seeder used by `Deployment`.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn ecosystems_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/community/ecosystems.md");
    std::fs::read_to_string(path).expect("ecosystems.md")
}

fn native_block(heading: &str) -> String {
    let doc = ecosystems_doc();
    guide_blocks::native_block(guide_blocks::section(&doc, heading))
        .unwrap_or_else(|| panic!("section '{heading}' has no native-client block"))
}

/// The first line a client prints for its version flag, or why it is absent.
fn client_version(program: &str, flag: &str) -> Result<String, String> {
    match Command::new(program).arg(flag).output() {
        Ok(output) if output.status.success() => Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()),
        Ok(output) => Err(format!("`{program} {flag}` exited with {}", output.status)),
        Err(error) => Err(format!("`{program}` is not runnable: {error}")),
    }
}

fn evidence_file() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into()))
        .join("uat");
    std::fs::create_dir_all(&dir).expect("target/uat");
    dir.join("hsp288-native-clients.txt")
}

fn record(line: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(evidence_file())
        .expect("evidence file");
    writeln!(file, "{line}").expect("evidence line");
    println!("{line}");
}

/// Resolves the clients a recipe needs. If any is missing the recipe is
/// NOT_RUN: visible, recorded and failing unless explicitly accepted.
fn require_clients(recipe: &str, clients: &[(&str, &str)]) -> Option<Vec<String>> {
    let mut versions = Vec::new();
    for (program, flag) in clients {
        match client_version(program, flag) {
            Ok(version) => versions.push(format!("{program}: {version}")),
            Err(reason) => {
                let line = format!("NOT_RUN: {recipe}: {reason}");
                record(&line);
                assert!(
                    std::env::var("HODEI_ALLOW_NOT_RUN").as_deref() == Ok("1"),
                    "{line} (set HODEI_ALLOW_NOT_RUN=1 to accept the NOT_RUN)"
                );
                return None;
            }
        }
    }
    for version in &versions {
        record(&format!("VERSION {recipe}: {version}"));
    }
    Some(versions)
}

/// Runs a documented block with `bash -euo pipefail` in `dir`.
fn run_block(block: &str, dir: &Path, port: u16, token: &str) -> (bool, String) {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let mut command = Command::new("bash");
    command
        .args(["-euo", "pipefail", "-c", block])
        .current_dir(dir)
        .env("PORT", port.to_string())
        .env("HODEI_TOKEN", token)
        // npm and pip must not read or write the real user configuration.
        .env("npm_config_cache", dir.join("npm-cache"))
        .env("PIP_CONFIG_FILE", "/dev/null")
        .env("PIP_CACHE_DIR", dir.join("pip-cache"))
        .env("XDG_CONFIG_HOME", &home);
    // twine is installed for the user; keep it importable without the real HOME.
    if let Some(site) = Command::new("python3")
        .args(["-c", "import site; print(site.getusersitepackages())"])
        .output()
        .ok()
        .filter(|o| o.status.success())
    {
        command.env(
            "PYTHONPATH",
            String::from_utf8_lossy(&site.stdout).trim(),
        );
    }
    let output = command.output().expect("bash runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

fn lab(seed: impl FnOnce(&Deployment)) -> (Deployment, support::Service, String) {
    let deployment = Deployment::new();
    seed(&deployment);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    (deployment, service, token)
}

/// The local Maven repository group of the recipe, removed before and after
/// so the artifact is really fetched from the service.
struct LocalGroup(PathBuf);

impl LocalGroup {
    fn fresh() -> Self {
        let home = std::env::var_os("HOME").expect("HOME");
        let path = PathBuf::from(home).join(".m2/repository/com/acme/demo");
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for LocalGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn maven_guide_recipe_runs_with_mvn() {
    let Some(_versions) = require_clients("maven2", &[("mvn", "--version")]) else {
        return;
    };
    let block = native_block("### Maven 2 (`maven2`)");
    let (deployment, mut service, token) = lab(|d| {
        d.seed_maven(&[(
            "doc-maven",
            serde_json::json!({ "version_policy": "release" }),
        )])
    });
    let dir = deployment.directory.path().join("maven-recipe");
    std::fs::create_dir_all(&dir).expect("dir");
    let pom = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>demo</artifactId><version>1.0.0</version></project>\n";
    std::fs::write(dir.join("demo-1.0.0.pom"), pom).expect("pom");
    std::fs::write(dir.join("demo-1.0.0.jar"), b"PK\x03\x04demo-jar-content").expect("jar");
    let local = LocalGroup::fresh();

    let (ok, output) = run_block(&block, &dir, service.port, &token);
    assert!(ok, "the Maven block failed:\n{output}");
    assert!(
        output.contains("Downloaded from hodei") || output.contains("Downloading from hodei"),
        "mvn did not fetch the jar from the service:\n{output}"
    );
    assert_eq!(
        std::fs::read(local.0.join("1.0.0/demo-1.0.0.jar")).expect("resolved jar"),
        b"PK\x03\x04demo-jar-content"
    );
    support::stop(&mut service);
}

#[test]
fn npm_guide_recipe_runs_with_npm() {
    let Some(_versions) = require_clients("npm", &[("npm", "--version")]) else {
        return;
    };
    let block = native_block("### npm (`npm`)");
    let (deployment, mut service, token) =
        lab(|d| d.seed_npm(&[("doc-npm", serde_json::Value::Null)]));
    let dir = deployment.directory.path().join("npm-recipe");
    let package = dir.join("demo-npm");
    std::fs::create_dir_all(&package).expect("package dir");
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"@acme/demo","version":"1.0.0","main":"index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(package.join("index.js"), "module.exports = 42;\n").expect("index.js");

    let (ok, output) = run_block(&block, &dir, service.port, &token);
    assert!(ok, "the npm block failed:\n{output}");
    assert_eq!(
        std::fs::read_to_string(dir.join("consumer/node_modules/@acme/demo/index.js"))
            .expect("installed module"),
        "module.exports = 42;\n"
    );
    support::stop(&mut service);
}

#[test]
fn pypi_guide_recipe_runs_with_twine_and_pip_and_compares_wheel_bytes() {
    let Some(_versions) = require_clients("pypi", &[("twine", "--version"), ("pip", "--version")])
    else {
        return;
    };
    let block = native_block("### PyPI (`pypi`)");
    let (deployment, mut service, token) =
        lab(|d| d.seed_pypi(&[("doc-pypi", serde_json::Value::Null)]));
    let dir = deployment.directory.path().join("pypi-recipe");
    std::fs::create_dir_all(&dir).expect("dir");
    let wheel_bytes = support::a_wheel("demo-pkg", "1.0.0", None);
    std::fs::write(dir.join("demo_pkg-1.0.0-py3-none-any.whl"), &wheel_bytes).expect("wheel");

    let (ok, output) = run_block(&block, &dir, service.port, &token);
    assert!(ok, "the PyPI block failed:\n{output}");
    // AC3: the wheel pip downloaded is byte-identical to the published one.
    assert_eq!(
        std::fs::read(dir.join("downloaded/demo_pkg-1.0.0-py3-none-any.whl"))
            .expect("downloaded wheel"),
        wheel_bytes
    );
    support::stop(&mut service);
}

#[test]
fn native_blocks_target_the_documented_repository_routes() {
    // Pure check, no client needed: every URL the native blocks use is a
    // loopback URL under the route prefix of the section's ecosystem.
    let doc = ecosystems_doc();
    for (heading, prefix) in [
        (
            "### Maven 2 (`maven2`)",
            "http://127.0.0.1:${PORT}/repository/doc-maven",
        ),
        ("### npm (`npm`)", "http://127.0.0.1:%s/npm/doc-npm/"),
        (
            "### PyPI (`pypi`)",
            "http://127.0.0.1:${PORT}/pypi/doc-pypi/",
        ),
    ] {
        let block = guide_blocks::native_block(guide_blocks::section(&doc, heading))
            .unwrap_or_else(|| panic!("section '{heading}' has no native block"));
        let urls: Vec<String> = guide_blocks::commands(&block)
            .iter()
            .flat_map(|c| guide_blocks::http_urls(c))
            .collect();
        assert!(!urls.is_empty(), "{heading}: no URL in the native block");
        for url in urls {
            assert!(
                url.starts_with(prefix),
                "{heading}: {url} is outside {prefix}"
            );
        }
    }
}
