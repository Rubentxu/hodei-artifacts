//! Maven 2 hosted de HSP-109 contra el cliente real (`mvn`).
//!
//! El `mvn` publica (`deploy-file`) y resuelve (`dependency:get`) contra un
//! servicio de verdad. Es la prueba de que el protocolo que hablamos es el
//! que habla Maven: el orden de las peticiones, los checksums que sube, los
//! `maven-metadata.xml` que manda y los que pide.
//!
//! Hace falta `mvn` en el PATH: sin el, la suite **falla** diciendo que
//! falta, y no pasa en silencio. Los plugins van fijados por version y el
//! repositorio local del usuario se usa (y solo se borra el grupo de esta
//! prueba), asi que no descarga nada que ya este en cache.
//!
//! La credencial va en un `settings.xml` temporal como cabecera
//! `Authorization`, nunca en los argumentos del comando.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

const DEPLOY: &str = "org.apache.maven.plugins:maven-deploy-plugin:3.1.2:deploy-file";
const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";

const MIXED: &str = "mvn-mixed";
const RELEASES: &str = "mvn-release";

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El grupo de la prueba en el repositorio local de Maven, que se borra al
/// acabar para no dejar nada en la cache del usuario.
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

    // Release: deploy, y se resuelve de vuelta con los mismos bytes.
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

    // La release es inmutable: otro contenido con la misma version falla.
    std::fs::write(&jar, b"PK-another-content").expect("the jar");
    let again = maven.deploy(port, MIXED, "1.0", &jar);
    assert!(!again.ok, "a release must not be replaced");
    assert!(
        again.output.contains("409") || again.output.to_lowercase().contains("conflict"),
        "{}",
        again.output
    );

    // Snapshot: se publica con timestamp (unique) y se resuelve por la
    // metadata de la version.
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

    // Un repositorio de releases no admite snapshots.
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

    // Una credencial que el servicio no acepta: 401.
    let intruder = Maven::new(deployment.directory.path(), "not-a-token", &group);
    let refused = intruder.deploy(port, MIXED, "1.0", &jar);
    assert!(!refused.ok, "{}", refused.output);
    assert!(refused.output.contains("401"), "{}", refused.output);

    // Una persona sin permiso en la politica: 403.
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

    // Reinicio: el contenido y los metadatos siguen ahi.
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
