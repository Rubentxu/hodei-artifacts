//! HSP-109 hosted Maven 2 against the real client (`mvn`).
//!
//! `mvn` publishes (`deploy-file`) and resolves (`dependency:get`) against a
//! real service. It proves that the protocol we speak is the one
//! Maven speaks: the order of requests, the checksums it uploads, the
//! `maven-metadata.xml` files it sends and the ones it requests.
//!
//! `mvn` is required on the PATH: without it, the suite **fails** saying it
//! is missing, and does not pass silently. Plugins are pinned by version and the
//! user's local repository is used (and only this test's group is
//! deleted), so it does not download anything that is already cached.
//!
//! The credential goes in a temporary `settings.xml` as an `Authorization`
//! header, never in the command arguments.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

const DEPLOY: &str = "org.apache.maven.plugins:maven-deploy-plugin:3.1.2:deploy-file";
const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";

const MIXED: &str = "mvn-mixed";
const RELEASES: &str = "mvn-release";

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The test's group in Maven's local repository, which is deleted at the
/// end so as to leave nothing in the user's cache.
struct LocalGroup(PathBuf);

impl Drop for LocalGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn the_local_repository() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home).join(".m2").join("repository")
}

struct Maven {
    dir: PathBuf,
    settings: PathBuf,
    group: String,
}

struct Run {
    ok: bool,
    output: String,
}

impl Maven {
    fn new(dir: &Path, token: &str, group: &str) -> Self {
        let settings = dir.join("settings.xml");
        std::fs::write(
            &settings,
            format!(
                "<settings><servers><server><id>hodei</id><configuration><httpHeaders>\
                 <property><name>Authorization</name><value>Bearer {token}</value></property>\
                 </httpHeaders></configuration></server></servers></settings>"
            ),
        )
        .expect("the settings are written");
        Self {
            dir: dir.to_path_buf(),
            settings,
            group: group.to_string(),
        }
    }

    fn run(&self, arguments: &[String]) -> Run {
        let output = Command::new("mvn")
            .arg("--batch-mode")
            .arg("--settings")
            .arg(&self.settings)
            .args(arguments)
            .current_dir(&self.dir)
            .output()
            .expect("mvn runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    fn deploy(&self, port: u16, repository: &str, version: &str, file: &Path) -> Run {
        self.run(&[
            DEPLOY.to_string(),
            format!("-Dfile={}", file.display()),
            format!("-DgroupId={}", self.group),
            "-DartifactId=lib".to_string(),
            format!("-Dversion={version}"),
            "-Dpackaging=jar".to_string(),
            "-DgeneratePom=true".to_string(),
            "-DrepositoryId=hodei".to_string(),
            format!("-Durl=http://127.0.0.1:{port}/repository/{repository}"),
        ])
    }

    fn resolve(&self, port: u16, repository: &str, version: &str) -> Run {
        self.run(&[
            GET.to_string(),
            format!("-Dartifact={}:lib:{version}", self.group),
            "-Dtransitive=false".to_string(),
            format!("-DremoteRepositories=hodei::default::http://127.0.0.1:{port}/repository/{repository}"),
        ])
    }

    fn local(&self, version: &str, name: &str) -> PathBuf {
        the_local_repository()
            .join(self.group.replace('.', "/"))
            .join("lib")
            .join(version)
            .join(name)
    }

    fn forget(&self) {
        let _ = std::fs::remove_dir_all(the_local_repository().join(self.group.replace('.', "/")));
    }
}

fn require_maven() {
    let found = Command::new("mvn")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the Maven CLI is not installed: the Maven suite needs `mvn` in the PATH"
    );
}

fn lab() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment.seed_maven(&[
        (MIXED, serde_json::Value::Null),
        (RELEASES, serde_json::json!({ "version_policy": "release" })),
    ]);
    deployment
}

#[test]
fn mvn_deploys_and_resolves_a_release_and_a_snapshot() {
    require_maven();
    let deployment = lab();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let group = format!("com.hodei.uat{}", std::process::id());
    let local = LocalGroup(the_local_repository().join(group.replace('.', "/")));
    let maven = Maven::new(
        deployment.directory.path(),
        &support::token(FIRST_ACCOUNT, "alice"),
        &group,
    );
    let jar = deployment.directory.path().join("lib.jar");
    std::fs::write(&jar, b"PK-release-bytes-for-the-maven-cli").expect("the jar");

    // Release: deploy, and resolve back with the same bytes.
    let deployed = maven.deploy(port, MIXED, "1.0", &jar);
    assert!(deployed.ok, "{}", deployed.output);
    maven.forget();
    let resolved = maven.resolve(port, MIXED, "1.0");
    assert!(resolved.ok, "{}", resolved.output);
    assert_eq!(
        std::fs::read(maven.local("1.0", "lib-1.0.jar")).expect("the jar was resolved"),
        b"PK-release-bytes-for-the-maven-cli"
    );
    assert!(maven.local("1.0", "lib-1.0.pom").exists());

    // The release is immutable: other content with the same version fails.
    std::fs::write(&jar, b"PK-another-content").expect("the jar");
    let again = maven.deploy(port, MIXED, "1.0", &jar);
    assert!(!again.ok, "a release must not be replaced");
    assert!(
        again.output.contains("409") || again.output.to_lowercase().contains("conflict"),
        "{}",
        again.output
    );

    // Snapshot: published with a timestamp (unique) and resolved through the
    // version's metadata.
    std::fs::write(&jar, b"PK-snapshot-one").expect("the jar");
    let snapshot = maven.deploy(port, MIXED, "2.0-SNAPSHOT", &jar);
    assert!(snapshot.ok, "{}", snapshot.output);
    std::fs::write(&jar, b"PK-snapshot-two").expect("the jar");
    let snapshot = maven.deploy(port, MIXED, "2.0-SNAPSHOT", &jar);
    assert!(snapshot.ok, "{}", snapshot.output);
    maven.forget();
    let resolved = maven.resolve(port, MIXED, "2.0-SNAPSHOT");
    assert!(resolved.ok, "{}", resolved.output);
    let served = std::fs::read_dir(maven.local("2.0-SNAPSHOT", ""))
        .expect("the snapshot directory")
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().ends_with(".jar"))
        .expect("a jar was resolved");
    assert_eq!(
        std::fs::read(served.path()).expect("the snapshot jar"),
        b"PK-snapshot-two"
    );

    // A releases repository does not admit snapshots.
    let refused = maven.deploy(port, RELEASES, "3.0-SNAPSHOT", &jar);
    assert!(!refused.ok, "{}", refused.output);

    support::stop(&mut service);
    drop(local);
}

#[test]
fn mvn_is_refused_without_a_valid_credential_and_resolves_after_a_restart() {
    require_maven();
    let deployment = lab();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let group = format!("com.hodei.uat{}b", std::process::id());
    let local = LocalGroup(the_local_repository().join(group.replace('.', "/")));
    let jar = deployment.directory.path().join("lib.jar");
    std::fs::write(&jar, b"PK-persistent").expect("the jar");

    // A credential the service does not accept: 401.
    let intruder = Maven::new(deployment.directory.path(), "not-a-token", &group);
    let refused = intruder.deploy(port, MIXED, "1.0", &jar);
    assert!(!refused.ok, "{}", refused.output);
    assert!(refused.output.contains("401"), "{}", refused.output);

    // A person without permission in the policy: 403.
    let carol = Maven::new(
        deployment.directory.path(),
        &support::token(FIRST_ACCOUNT, "carol"),
        &group,
    );
    let forbidden = carol.deploy(port, MIXED, "1.0", &jar);
    assert!(!forbidden.ok, "{}", forbidden.output);
    assert!(forbidden.output.contains("403"), "{}", forbidden.output);

    let alice = Maven::new(
        deployment.directory.path(),
        &support::token(FIRST_ACCOUNT, "alice"),
        &group,
    );
    assert!(alice.deploy(port, MIXED, "1.0", &jar).ok);

    // Restart: the content and metadata are still there.
    support::kill(&mut service);
    let mut service = deployment.start_on(port, &[]);
    alice.forget();
    let resolved = alice.resolve(port, MIXED, "1.0");
    assert!(resolved.ok, "{}", resolved.output);
    assert_eq!(
        std::fs::read(alice.local("1.0", "lib-1.0.jar")).expect("the jar"),
        b"PK-persistent"
    );
    support::stop(&mut service);
    drop(local);
}
