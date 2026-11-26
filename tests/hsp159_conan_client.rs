//! Hosted Conan 2 of HSP-159 against the real client (`conan`).
//!
//! The venv's `conan` publishes a recipe with two profiles against the
//! real service, uploads it with `conan upload`, lists it from the
//! remote and resolves it with `conan install` in clean caches: the
//! revisions (rrev/prev), the `package_id` and the checksums are produced by the
//! client itself, not by this test. It requires `conan` 2.x: in `CONAN_BIN`
//! (the UAT venv) or on the `PATH`; without it, the suite **fails**
//! saying it is missing. Each run uses its own `CONAN_HOME`: nothing is
//! read or written outside the deployment directory.
//!
//! The spike's reference remote was `conan_server` 1.66 behind a
//! registry proxy (`target/uat/hsp159/spike-ref/`); the dialect
//! observed there is what this test demands of the service.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Requires `conan` 2.x from the venv or the PATH, and returns its path.
fn the_conan_binary() -> String {
    if let Ok(path) = std::env::var("CONAN_BIN") {
        return path;
    }
    "conan".to_string()
}

fn require_conan() -> String {
    let binary = the_conan_binary();
    let found = Command::new(&binary)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the conan CLI is not installed: the Conan suite needs `conan` in CONAN_BIN (the UAT venv) or in the PATH"
    );
    let version = String::from_utf8_lossy(
        &Command::new(&binary)
            .arg("--version")
            .output()
            .expect("the version")
            .stdout,
    )
    .trim()
    .to_string();
    assert!(
        version.starts_with("Conan version 2."),
        "the Conan suite pins the client major version 2.x, found: {version}"
    );
    binary
}

/// The result of a client run.
struct Run {
    /// Whether the client finished successfully.
    ok: bool,
    /// What it said, on stdout and stderr.
    output: String,
}

impl Run {
    /// The text if it finished successfully, and a failure with everything else if not.
    fn expect(&self, what: &str) -> String {
        assert!(self.ok, "{what} failed:\n{}", self.output);
        self.output.clone()
    }
}

/// An isolated `conan`: its `CONAN_HOME`, its remote and its credential.
struct Client {
    /// The path of the executable.
    binary: String,
    /// The deployment directory.
    root: PathBuf,
    /// The `CONAN_HOME` of this cache.
    home: PathBuf,
}

impl Client {
    /// A clean cache with Hodei's remote and its credential.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let home = root.join(format!("conan-home-{name}"));
        std::fs::create_dir_all(&home).expect("the conan home");
        let client = Self {
            binary: the_conan_binary(),
            root: root.to_path_buf(),
            home,
        };
        client.run(&["profile", "detect", "-f"]);
        let url = format!("http://127.0.0.1:{port}/conan/{repository}");
        client.run(&["remote", "add", "hodei", &url]);
        // The remote's credential is the hub's bearer credential: the
        // client exchanges it for the token in the dialect's authenticate.
        client
            .run(&["remote", "login", "hodei", "alice", "-p", token])
            .expect("the login");
        client
    }

    /// Runs `conan` with its isolated home.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("CONAN_HOME", &self.home)
            .env("HOME", &self.root)
            .env("CONAN_NON_INTERACTIVE", "1")
            // `conan create` writes its generator scripts (`conanbuild.sh`,
            // `conanrun.sh`, ...) into the working directory: the deployment
            // directory, never the checkout the suite runs from.
            .current_dir(&self.root)
            .output()
            .expect("conan runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// The package files in the cache, with their flavor to identify
    /// the `package_id` they serve.
    ///
    /// The cache stores `conan_package.tgz` in `{package}/d/` and the
    /// served files in `{package}/p/`, with `{package}` under `p/b/`
    /// when the binary was created by this client and under `p/` when it downloaded it.
    /// The relationship between the two is that of sibling directories, and it does not
    /// depend on the name: one client does not impose its own on the other.
    fn package_files(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let packages = self.home.join("p");
        the_walk_for_packages(&packages, &mut found);
        found.sort();
        found
    }
}

/// The search for the packages of a conan cache: each directory that
/// has `d/conan_package.tgz` and `p/flavor.txt` is a binary with its
/// content.
fn the_walk_for_packages(directory: &Path, found: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("an entry");
        let path = entry.path();
        if path.is_dir() {
            the_walk_for_packages(&path, found);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("conan_package.tgz") {
            let parent = path
                .parent()
                .expect("the d directory")
                .parent()
                .expect("the package");
            let flavor =
                std::fs::read_to_string(parent.join("p").join("flavor.txt")).unwrap_or_default();
            found.push((flavor, path.display().to_string()));
        }
    }
}

/// The sha256 of a file, as both caches verify it.
fn sha256_of(path: &str) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// The versioned identifiers that `conan list` prints: the
/// hexadecimal tokens of the listing (rrev and prev).
fn the_revisions_of_output(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        for token in line.split_whitespace() {
            let token = token.trim_end_matches('#');
            let token = token
                .split('(')
                .next()
                .expect("a token has no parenthesis")
                .trim();
            let hex = token.chars().all(|c| c.is_ascii_hexdigit());
            if hex && token.len() >= 16 && token.len() <= 64 {
                found.push(token.to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The recipe with one `package_id` per flavor: the same recipe (same rrev)
/// produces two binaries that do not get mixed.
fn the_recipe(root: &Path) -> PathBuf {
    let directory = root.join("recipes/hello");
    std::fs::create_dir_all(&directory).expect("the recipe directory");
    let recipe = directory.join("conanfile.py");
    std::fs::write(
        &recipe,
        r#"
from conan import ConanFile
import os, shutil

class HelloConan(ConanFile):
    name = "hello"
    version = "1.0.0"
    settings = "os"
    options = {"flavor": [1, 2]}
    default_options = {"flavor": 1}

    def package(self):
        shutil.copy(os.path.join(self.recipe_folder, "conanfile.py"),
                    os.path.join(self.package_folder, "conanfile.py"))
        with open(os.path.join(self.package_folder, "flavor.txt"), "w") as f:
            f.write("flavor=%s" % self.options.flavor)
"#,
    )
    .expect("the recipe");
    directory
}

fn the_profile(root: &Path, name: &str, flavor: &str) -> PathBuf {
    let directory = root.join("profiles");
    std::fs::create_dir_all(&directory).expect("the profiles directory");
    let profile = directory.join(name);
    std::fs::write(
        &profile,
        format!("[settings]\nos=Linux\narch=x86_64\n\n[options]\nhello*:flavor={flavor}\n"),
    )
    .expect("the profile");
    profile
}

/// The catalog seeder: each suite brings it because the harness restarts
/// the suite binary to seed before starting the service.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_conan_client_publishes_and_resolves_by_package_id() {
    require_conan();
    let deployment = Deployment::new();
    deployment.seed_conan(&[("conan-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author", port, "conan-hosted", &token);
    let recipe = the_recipe(&root);
    let flavor_one = the_profile(&root, "flavor1", "1");
    let flavor_two = the_profile(&root, "flavor2", "2");

    // AC1: create with TWO profiles. The same recipe produces two package_ids
    // with their prevs; the recipe revision does not change.
    author
        .run(&[
            "create",
            recipe.to_str().unwrap(),
            "-pr:a",
            flavor_one.to_str().unwrap(),
            "--user=hodei",
            "--channel=stable",
        ])
        .expect("the create of flavor 1");
    author
        .run(&[
            "create",
            recipe.to_str().unwrap(),
            "-pr:a",
            flavor_two.to_str().unwrap(),
            "--user=hodei",
            "--channel=stable",
        ])
        .expect("the create of flavor 2");

    // AC2: the real upload against the remote. On upload, the client
    // packages each binary in the author's cache: that is where the
    // digests are left BEFORE anyone downloads them, which is what the other cache
    // has to receive byte for byte.
    author
        .run(&[
            "upload",
            "hello/1.0.0@hodei/stable",
            "-r",
            "hodei",
            "--confirm",
        ])
        .expect("the upload");
    let packages = author.package_files();
    assert_eq!(
        packages.len(),
        2,
        "two profiles produce two packages, found: {:?}",
        packages
    );
    let flavors: Vec<&str> = packages.iter().map(|(flavor, _)| flavor.as_str()).collect();
    assert!(
        flavors.contains(&"flavor=1") && flavors.contains(&"flavor=2"),
        "the two packages carry their own flavor: {:?}",
        flavors
    );
    let uploaded_digests: Vec<(String, String)> = packages
        .iter()
        .map(|(flavor, path)| (flavor.clone(), sha256_of(path)))
        .collect();

    // AC2: the remote's listing is consistent and the revisions are
    // preserved: the remote reports the same revisions as the local cache.
    // The client only lists revisions when the pattern asks for them (`#*`).
    let local = author
        .run(&["list", "hello/1.0.0@hodei/stable#*"])
        .expect("the local list");
    let remote = author
        .run(&["list", "hello/1.0.0@hodei/stable#*", "-r", "hodei"])
        .expect("the remote list");
    assert!(
        remote.contains("hello/1.0.0@hodei/stable"),
        "the remote lists the recipe: {}",
        remote
    );
    let remote_revisions = the_revisions_of_output(&remote);
    let local_revisions = the_revisions_of_output(&local);
    assert!(
        !remote_revisions.is_empty(),
        "the remote lists revisions (rrev and prev): {}",
        remote
    );
    assert_eq!(
        remote_revisions, local_revisions,
        "the remote says the same revisions the local cache says:\nremote:\n{}\nlocal:\n{}",
        remote, local
    );

    // AC2/AC4: the digest is verifiable and scoping is per account. Another
    // principal does not see what is not theirs: their listing of the same remote
    // finds nothing, because the repository lives in another account.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger", port, "conan-hosted", &other);
    let stranger_list = stranger.run(&["list", "hello/*", "-r", "hodei"]);
    assert!(
        stranger_list.ok,
        "the listing of another principal answers, not an error: {}",
        stranger_list.output
    );
    assert!(
        !stranger_list.output.contains("hello/1.0.0"),
        "another principal does not see the recipe of the first account: {}",
        stranger_list.output
    );

    // AC1/AC3: resolve in a clean cache, BOTH profiles. Each
    // package_id receives ITS bytes: the flavor that is downloaded is the one the profile
    // asks for, and the digest matches the one that was uploaded.
    let consumer = Client::new(&root, "consumer", port, "conan-hosted", &token);
    consumer
        .run(&[
            "install",
            "--requires=hello/1.0.0@hodei/stable",
            "-pr:a",
            flavor_one.to_str().unwrap(),
            "-r",
            "hodei",
        ])
        .expect("the install of flavor 1");
    consumer
        .run(&[
            "install",
            "--requires=hello/1.0.0@hodei/stable",
            "-pr:a",
            flavor_two.to_str().unwrap(),
            "-r",
            "hodei",
        ])
        .expect("the install of flavor 2");
    let downloaded = consumer.package_files();
    assert_eq!(downloaded.len(), 2, "both packages land in the cache");
    for (flavor, path) in &downloaded {
        let digest = sha256_of(path);
        let expected = uploaded_digests
            .iter()
            .find(|(uploaded, _)| uploaded == flavor)
            .map(|(_, digest)| digest)
            .unwrap_or_else(|| panic!("the flavor {flavor} was uploaded"));
        assert_eq!(
            &digest, expected,
            "the bytes served for {flavor} are the bytes uploaded"
        );
    }

    // AC3: offline. The remote dies: the warm cache resolves without network and
    // the missing binary fails, without falling back to anything.
    support::stop(&mut service);
    let warm = consumer.run(&[
        "install",
        "--requires=hello/1.0.0@hodei/stable",
        "-pr:a",
        flavor_one.to_str().unwrap(),
        "-r",
        "hodei",
    ]);
    assert!(
        warm.ok,
        "the offline install of a warm cache works with the remote dead: {}",
        warm.output
    );
    // An asset is missing: the cache does not have the flavor 2 binary after
    // removing it, and the dead remote gives no alternative: the install fails.
    consumer
        .run(&["remove", "hello/1.0.0@hodei/stable:*", "-c"])
        .expect("the removal of the cached binaries");
    let missing = consumer.run(&[
        "install",
        "--requires=hello/1.0.0@hodei/stable",
        "-pr:a",
        flavor_two.to_str().unwrap(),
        "-r",
        "hodei",
    ]);
    assert!(
        !missing.ok,
        "the offline install without the binary fails instead of falling to anything: {}",
        missing.output
    );
    assert!(
        missing.output.contains("missing binary") || missing.output.contains("ERROR"),
        "the failure says why: {}",
        missing.output
    );
}
