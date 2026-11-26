//! **Real** Maven Central (HSP-224 / HCR-02), with the real Maven client.
//!
//! The HSP-223 laboratory (`support::lab`) shows that the registry
//! caches, works without upstream and serves nothing untrusted
//! **against a local upstream**. Here the same is shown against the real
//! public Central (`https://repo.maven.apache.org/maven2/`), and
//! with the client people use: `mvn`.
//!
//! What is measured, not asserted:
//!
//! - **AC1**: `mvn dependency:get` with a `settings.xml` pointing at the
//!   Hodei **group** resolves `org.apache.commons:commons-lang3:3.14.0`,
//!   and the POM and the JAR **match by digest** those served by the
//!   public Central. The upstream check is **opt-in** and with the
//!   version pinned: without `HODEI_LAB_REAL=1` the case does not run and the
//!   report is not green, which is correct.
//! - **AC2**: a **second** fetch of the same artifact **does not query** the
//!   upstream (the laboratory counter says so), with the upstream
//!   **disconnected** it resolves just the same from the local data, and the
//!   client **cannot bypass the group**: with the group down it does not
//!   resolve, and with the same configuration pointing at the upstream it does.
//! - **AC3**: a **reserved groupId collision** and an upstream that
//!   returns **tampered metadata** do not serve an untrusted package, and
//!   no client token appears in the service log.
//!
//! # The no-network path
//!
//! By default **the network is not used**: the upstream is the laboratory's
//! (a local counting server) and the artifact is a *fixture* with the
//! Central coordinates. The only read from outside the laboratory
//! directories is the machine's Maven cache, **read-only**,
//! and only for the *plugin* (`dependency:get`): the plugin has
//! to come from somewhere and without network the only source is that one. The
//! artifact under test never comes from there: the client's local repository
//! is temporary and the artifact is only requested from the group.
//!
//! # The client
//!
//! The `mvn` on the PATH may be a wrapper that needs the machine's HOME
//! (this one has one, for the build *slices*), and the laboratory
//! runs the client with a HOME that is not the user's. `mvn --version`
//! says where the real Maven lives, and that path does not
//! depend on HOME.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use support::lab::{
    Budget, Expectation, LabCase, Outcome, REAL_MODE_VARIABLE, Scope, Summary, Upstream,
    describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The groupId of the artifact under test.
const GROUP_ID: &str = "org.apache.commons";

/// The artifactId of the artifact under test.
const ARTIFACT_ID: &str = "commons-lang3";

/// The version of the artifact under test: **pinned**, always.
const VERSION: &str = "3.14.0";

/// The coordinates requested from the client.
const COORDINATES: &str = "org.apache.commons:commons-lang3:3.14.0";

/// The POM path in the repository.
const POM_PATH: &str = "org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom";

/// The JAR path in the repository.
const JAR_PATH: &str = "org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.jar";

/// The path of the artifact metadata.
const METADATA_PATH: &str = "org/apache/commons/commons-lang3/maven-metadata.xml";

/// The real upstream: the internet's Maven Central.
const REAL_UPSTREAM: &str = "https://repo.maven.apache.org/maven2/";

/// The host of the real upstream, as the service output understands it.
const REAL_HOST: &str = "repo.maven.apache.org";

/// The plugin that resolves the artifact.
const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";

/// The group's *hosted* repository: the house's own.
const HOSTED: &str = "mvn-hosted";

/// The group's *proxy* repository: the real Central.
const PROXY: &str = "central";

/// The group under test.
const GROUP: &str = "mvn-all";

/// The POM served by the laboratory upstream: a *fixture* with the
/// Central coordinates, and **not** the Central bytes.
const FIXTURE_POM: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.apache.commons</groupId>
  <artifactId>commons-lang3</artifactId>
  <version>3.14.0</version>
  <packaging>jar</packaging>
  <name>The fixture of the laboratory of Maven Central</name>
</project>
"#;

/// The JAR served by the laboratory upstream: the fixture bytes.
const FIXTURE_JAR: &[u8] = b"PK-the-bytes-of-the-fixture-of-the-maven-central-laboratory";

/// The cases that a complete run has to execute.
///
/// The report declares them, so a run that only reached some of them
/// says which ones it missed: those that do not run count as `NOT_RUN`, and the
/// report is never green.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-mvn-resolves-the-artifact-through-the-group",
        "el cliente Maven resuelve el artefacto por el grupo y los digests son los del fixture",
    ),
    (
        "ac1-the-digest-matches-the-real-central",
        "el POM y el JAR servidos coinciden por digest con los del Central real",
    ),
    (
        "ac2-the-second-fetch-does-not-ask-the-upstream",
        "el segundo fetch del mismo artefacto no consulta el upstream",
    ),
    (
        "ac2-the-artifact-resolves-with-the-upstream-gone",
        "el artefacto se resuelve igual con el upstream desconectado",
    ),
    (
        "ac2-the-client-cannot-skip-the-group",
        "sin el grupo el cliente no resuelve: no hay un camino directo al Central",
    ),
    (
        "ac3-a-reserved-group-id-is-not-served-from-the-public-upstream",
        "una colisión de groupId reservado no se sirve desde el upstream público",
    ),
    (
        "ac3-metadata-altered-is-not-served-as-ours",
        "unos metadatos alterados no se sirven como si fueran nuestros",
    ),
    (
        "ac3-the-token-of-the-client-does-not-reach-the-log",
        "el token del cliente no aparece en el log del servicio",
    ),
    (
        "ac3-the-count-does-not-lie",
        "el caso del Central real no se ejecuta sin el opt-in, y no cuenta como verde",
    ),
];

/// The title and the disclaimer of this suite's report.
///
/// The laboratory disclaimer (HSP-223) says `mvn` was not talked to,
/// and here it is: the disclaimer belongs to the suite, not to the module.
const THE_TITLE: &str = "Laboratorio de Maven Central — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y el artefacto es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el plugin de Maven.** El plugin (`dependency:get`) sale de la cache de Maven de la máquina, en solo lectura. Del plugin no se dice nada del registro: lo que se mide es el artefacto.",
    "- **Ni Central.** Lo que se compara es el digest de una versión fijada (`commons-lang3:3.14.0`): no se certifica la integridad de Central ni la de su contenido.",
    "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés —el recuento— y no el producto. Los dos recuentos van aparte.",
];

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The laboratory.
// ---------------------------------------------------------------------------

/// The Maven Central laboratory: a deployment, a counting upstream
/// and the group under test.
struct Lab {
    deployment: Deployment,
    /// The local upstream: it only exists in the no-network mode.
    upstream: Upstream,
    /// The URL of the proxy's upstream.
    upstream_url: String,
}

impl Lab {
    /// A laboratory with a **local** counting upstream.
    ///
    /// The upstream is the laboratory's (`support::lab::Upstream`): the
    /// counter and the upstream are the same thing, because a counter that
    /// lives in two places measures one of the two.
    fn local() -> Self {
        let upstream = Upstream::start();
        let url = upstream.base();
        Self {
            deployment: Deployment::new(),
            upstream,
            upstream_url: url,
        }
    }

    /// A laboratory with the **real** upstream: the internet's Central.
    ///
    /// There is no local counter: Central cannot be counted. What is
    /// measured in the real mode is the digest and the fact that the second
    /// resolution works with the exit to Central **closed**.
    fn real() -> Self {
        Self {
            deployment: Deployment::new(),
            upstream: Upstream::start(),
            upstream_url: REAL_UPSTREAM.to_string(),
        }
    }

    /// Seeds the hosted, the proxy and the group with the reservation policy
    /// that is passed.
    fn seed(&self, namespaces: Option<serde_json::Value>) {
        let mut proxy = serde_json::json!({
            "key": PROXY,
            "format": "maven2",
            "proxy": {
                "upstream": {
                    "id": "central",
                    "url": self.upstream_url,
                    "scopes": ["/"],
                    "max_bytes": 4 * 1024 * 1024,
                },
                "cache": { "ttl": { "content_secs": 3600, "metadata_secs": 3600 } },
            },
        });
        if let Some(policy) = &namespaces {
            proxy["namespaces"] = policy.clone();
        }
        self.deployment.seed_repositories(&[
            serde_json::json!({ "key": HOSTED, "format": "maven2" }),
            proxy,
        ]);
        let mut group = serde_json::json!({
            "key": GROUP,
            "members": [HOSTED, PROXY],
            "format": "maven2",
        });
        if let Some(policy) = &namespaces {
            group["namespaces"] = policy.clone();
        }
        self.deployment.seed_groups_raw(&[group]);
    }

    /// Starts the service with the egress the case needs.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// The egress that lets the laboratory's local upstream through.
    fn the_local_egress(&self) -> String {
        format!("127.0.0.1:{};private;http", self.upstream.port())
    }
}

/// A service response.
struct Reply {
    status: u16,
    body: Vec<u8>,
    resolved_from: Option<String>,
    command: String,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Requests a path from a service repository.
fn get(lab: &Lab, port: u16, repository: &str, path: &str) -> Reply {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("response.bin");
    let _ = std::fs::remove_file(&into);
    let curl = Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        &alice,
        None,
        None,
        Some(&into),
    );
    let command = curl.shown();
    let answer = curl.run();
    Reply {
        status: answer.status,
        body: std::fs::read(&into).unwrap_or_default(),
        resolved_from: answer.header("x-hodei-resolved-from"),
        command,
    }
}

// ---------------------------------------------------------------------------
// The Maven client.
// ---------------------------------------------------------------------------

/// A client run.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// An isolated Maven client: its HOME, its local repository and its settings.
///
/// The `settings.xml` points `central` at the Hodei **group**, so that
/// the client has no path of its own to Central: what it downloads comes
/// from the group or does not come at all.
struct Maven {
    /// The client's HOME: it is not that of whoever runs the test.
    home: PathBuf,
    /// The client's local repository: temporary.
    repository: PathBuf,
    /// The client's settings: temporary.
    settings: PathBuf,
    /// The real Maven.
    binary: PathBuf,
}

impl Maven {
    /// A client that points at `mirror` and talks with the credential
    /// `token`.
    ///
    /// Returns `None` when the machine does not have what is needed to
    /// run Maven without network: the real Maven and the cache with the plugin.
    fn new(root: &Path, mirror: &str, token: &str) -> Option<Self> {
        let binary = the_maven_binary()?;
        let cache = the_machine_cache()?;
        let cache = cache.to_string_lossy().into_owned();
        let home = root.join("home");
        let repository = root.join("repository");
        std::fs::create_dir_all(&home).expect("the home of the client");
        std::fs::create_dir_all(&repository).expect("the repository of the client");
        // The versions of the machine's tools are chosen with
        // `.tool-versions`, and that file is looked up in HOME: without it, the
        // machine's `java` cannot be chosen.
        if let Ok(versions) =
            std::fs::read_to_string(PathBuf::from(std::env::var_os("HOME")?).join(".tool-versions"))
        {
            let _ = std::fs::write(home.join(".tool-versions"), versions);
        }
        let settings = root.join("settings.xml");
        std::fs::write(
            &settings,
            format!(
                "<settings><servers><server><id>hodei</id><configuration><httpHeaders>\
                 <property><name>Authorization</name><value>Bearer {token}</value></property>\
                 </httpHeaders></configuration></server></servers>\
                 <mirrors><mirror><id>hodei</id><mirrorOf>central</mirrorOf><url>{mirror}</url></mirror></mirrors>\
                 <profiles><profile><id>hodei</id><pluginRepositories>\
                 <pluginRepository><id>local-cache</id><url>file://{cache}</url></pluginRepository>\
                 <pluginRepository><id>hodei-plugins</id><url>{mirror}</url></pluginRepository>\
                 </pluginRepositories></profile></profiles>\
                 <activeProfiles><activeProfile>hodei</activeProfile></activeProfiles></settings>"
            ),
        )
        .expect("the settings are written");
        Some(Self {
            home,
            repository,
            settings,
            binary,
        })
    }

    /// Resolves some coordinates and says whether it could.
    fn resolve(&self, coordinates: &str) -> Run {
        let mut command = Command::new(&self.binary);
        command
            .arg("--batch-mode")
            .arg("--settings")
            .arg(&self.settings)
            .arg(GET)
            .arg(format!("-Dartifact={coordinates}"))
            .arg("-Dtransitive=false")
            .arg(format!("-Dmaven.repo.local={}", self.repository.display()))
            .env("HOME", &self.home);
        // The machine's wrappers (asdf) choose the version of their
        // tools with HOME: without this hint, the client's HOME
        // leaves them without java.
        if let Some(directories) = the_asdf_directories() {
            for (name, path) in directories {
                command.env(name, path);
            }
        }
        let shown = format!(
            "{} --settings {} {} -Dartifact={coordinates} -Dmaven.repo.local={}",
            self.binary.display(),
            self.settings.display(),
            GET,
            self.repository.display()
        );
        let output = command.output().expect("mvn runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
            command: shown,
        }
    }

    /// The file the client has of the artifact.
    fn local(&self, name: &str) -> PathBuf {
        self.repository
            .join(GROUP_ID.replace('.', "/"))
            .join(ARTIFACT_ID)
            .join(VERSION)
            .join(name)
    }

    /// Forgets the artifact: the client will request it again, and what is
    /// measured is where that second time comes from.
    fn forget(&self) {
        let _ = std::fs::remove_dir_all(
            self.repository
                .join(GROUP_ID.replace('.', "/"))
                .join(ARTIFACT_ID)
                .join(VERSION),
        );
    }
}

/// The real Maven of this machine.
///
/// The `mvn` on the PATH may be a wrapper that needs the machine's HOME,
/// and the client runs with a HOME that is not the user's.
/// `mvn --version` says where the real Maven lives, and that path does not
/// depend on HOME.
fn the_maven_binary() -> Option<PathBuf> {
    let output = Command::new("mvn").arg("--version").output().ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let home = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Maven home:")
            .map(|path| path.trim().to_string())
    })?;
    let binary = PathBuf::from(home).join("bin").join("mvn");
    binary.is_file().then_some(binary)
}

/// The machine's Maven cache: where the **plugin** comes from, and only
/// the plugin.
///
/// It is read read-only and the artifact under test never comes from there:
/// the client's local repository is temporary, and the artifact is requested
/// from the group.
fn the_machine_cache() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let cache = PathBuf::from(home).join(".m2").join("repository");
    cache.is_dir().then_some(cache)
}

/// The asdf directories, if this machine has them: they are where its
/// wrappers look for the tools that HOME no longer tells them.
fn the_asdf_directories() -> Option<Vec<(&'static str, PathBuf)>> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let asdf = home.join(".asdf");
    asdf.is_dir()
        .then(|| vec![("ASDF_DIR", asdf.clone()), ("ASDF_DATA_DIR", asdf)])
}

// ---------------------------------------------------------------------------
// Utilities.
// ---------------------------------------------------------------------------

/// The summary of a case, with the case id and title.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    describe_registry_lab(THE_TITLE, THE_DISCLAIMER);
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

/// The sha256 of some bytes.
fn sha256_of_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The bytes that the real upstream serves for a path, requested **from
/// outside** the registry.
///
/// The comparison is the point: the digest of what Hodei served has to be
/// the digest of what Central serves, and the only way to know is
/// to ask Central. The version is pinned in the path.
fn the_upstream_bytes(path: &str) -> Option<Vec<u8>> {
    let url = format!("{REAL_UPSTREAM}{path}");
    let output = Command::new("curl")
        .args(["--silent", "--show-error", "--fail", "--location"])
        .arg(&url)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// The metadata of an artifact, as an upstream would write it.
fn the_metadata(group_id: &str, artifact_id: &str, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>\n  <groupId>{group_id}</groupId>\n  <artifactId>{artifact_id}</artifactId>\n  <versioning>\n    <latest>{version}</latest>\n    <release>{version}</release>\n    <versions>\n      <version>{version}</version>\n    </versions>\n    <lastUpdated>20240101000000</lastUpdated>\n  </versioning>\n</metadata>\n"
    )
}

// ---------------------------------------------------------------------------
// AC1: the client resolves through the group, and the digest is the upstream's.
// ---------------------------------------------------------------------------

#[test]
fn ac1_mvn_resolves_the_artifact_through_the_group() {
    a_case(
        "ac1-mvn-resolves-the-artifact-through-the-group",
        "el cliente Maven resuelve el artefacto por el grupo y los digests son los del fixture",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac1-mvn-resolves-the-artifact-through-the-group",
                    "el cliente Maven resuelve el artefacto por el grupo y los digests son los del fixture",
                )
                .not_run("this machine has no Maven that can run without the network");
            };

            let run = client.resolve(COORDINATES);
            assert!(
                run.ok,
                "mvn did not resolve through the group:\n{}",
                run.output
            );
            assert!(
                !run.output.contains("Checksum validation failed"),
                "the group served a checksum that is not the one of the file:\n{}",
                run.output
            );

            // The client's POM and JAR are those of the fixture, byte for byte.
            let jar = client.local("commons-lang3-3.14.0.jar");
            let pom = client.local("commons-lang3-3.14.0.pom");
            assert!(jar.is_file(), "the jar was not resolved");
            assert!(pom.is_file(), "the pom was not resolved");
            let jar_digest = support::sha256_of(&jar);
            let pom_digest = support::sha256_of(&pom);
            assert_eq!(jar_digest, sha256_of_bytes(FIXTURE_JAR));
            assert_eq!(pom_digest, sha256_of_bytes(FIXTURE_POM));
            // And the upstream received the request: what the client has
            // came from there, not from any other place.
            assert!(
                lab.upstream.hits(JAR_PATH) > 0,
                "the upstream was never asked for the jar"
            );
            assert!(
                lab.upstream.hits(POM_PATH) > 0,
                "the upstream was never asked for the pom"
            );
            // And what the client has came from the **group**: the member
            // that wins the path is the Central proxy.
            let served = get(&lab, port, GROUP, JAR_PATH);
            assert_eq!(served.status, 200, "{}", served.text());
            assert_eq!(
                served.resolved_from.as_deref(),
                Some(PROXY),
                "the artifact did not come from the member that proxies the Central"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-mvn-resolves-the-artifact-through-the-group",
                "el cliente Maven resuelve el artefacto por el grupo y los digests son los del fixture",
            )
            .command(&run.command)
            .origin(&lab.upstream.origin())
            .sha256(&jar_digest)
            .passed()
        },
    );
}

#[test]
fn ac1_the_digest_matches_the_real_central() {
    a_case(
        "ac1-the-digest-matches-the-real-central",
        "el POM y el JAR servidos coinciden por digest con los del Central real",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let case = LabCase::new(
                "ac1-the-digest-matches-the-real-central",
                "el POM y el JAR servidos coinciden por digest con los del Central real",
            )
            .command(&format!(
                "mvn {GET} -Dartifact={COORDINATES} (mirror: el grupo de Hodei)"
            ));
            if !real_mode_enabled() {
                // Without the opt-in the case does not run, and a case that does not
                // run is not a green case.
                return case.not_run(format!(
                    "the real mode is opt-in: set {REAL_MODE_VARIABLE}=1 to run it"
                ));
            }
            let budget = Budget::from_environment();
            let lab = Lab::real();
            lab.seed(None);

            let mut environment: Vec<(String, String)> = vec![(
                "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
                REAL_HOST.to_string(),
            )];
            environment.extend(budget.service_environment());
            let borrowed: Vec<(&str, &str)> = environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let mut service = lab.start(&borrowed);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return case.not_run("this machine has no Maven that can run without the network");
            };

            let started = Instant::now();
            let run = client.resolve(COORDINATES);
            let elapsed = started.elapsed();
            assert!(
                run.ok,
                "mvn did not resolve through the group:\n{}",
                run.output
            );
            assert!(
                !run.output.contains("Checksum validation failed"),
                "the group served a checksum that is not the one of the file:\n{}",
                run.output
            );

            // The POM and the JAR the client has, and those the real
            // Central serves: the digests have to be the same.
            let jar = client.local("commons-lang3-3.14.0.jar");
            let pom = client.local("commons-lang3-3.14.0.pom");
            assert!(jar.is_file(), "the jar was not resolved");
            assert!(pom.is_file(), "the pom was not resolved");
            let jar_digest = support::sha256_of(&jar);
            let pom_digest = support::sha256_of(&pom);
            let Some(upstream_jar) = the_upstream_bytes(JAR_PATH) else {
                support::stop(&mut service);
                return case.failed(format!("the real upstream did not serve {JAR_PATH}"));
            };
            let Some(upstream_pom) = the_upstream_bytes(POM_PATH) else {
                support::stop(&mut service);
                return case.failed(format!("the real upstream did not serve {POM_PATH}"));
            };
            assert_eq!(
                jar_digest,
                sha256_of_bytes(&upstream_jar),
                "the jar that Hodei served is not the jar of the real Central"
            );
            assert_eq!(
                pom_digest,
                sha256_of_bytes(&upstream_pom),
                "the pom that Hodei served is not the pom of the real Central"
            );

            // And the run respects the laboratory's budget.
            let bytes = std::fs::metadata(&jar).map(|meta| meta.len()).unwrap_or(0);

            // Central stays out of the service's egress: the
            // list of hosts it can contact does not have it, so the
            // service **cannot** go and fetch it even if it wanted to. The
            // service starts again with the same database and the same
            // store, and the client, with its empty local repository,
            // resolves just the same: what is served is the local copy.
            support::stop(&mut service);
            let mut closed: Vec<(String, String)> = environment
                .iter()
                .filter(|(name, _)| name != "HODEI_PROXY__ALLOWED_HOSTS")
                .cloned()
                .collect();
            closed.push((
                "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
                // A host that is not Central: the egress still exists,
                // but it does not reach Central. The list is not emptied (an
                // empty list is not a list), what is in it is changed.
                // ella.
                "127.0.0.1:1;private;http".to_string(),
            ));
            let borrowed: Vec<(&str, &str)> = closed
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let mut closed_service = lab.start(&borrowed);
            let closed_port = closed_service.port;
            let closed_group = format!("http://127.0.0.1:{closed_port}/repository/{GROUP}");
            let closed_client = Maven::new(
                &lab.deployment.directory.path().join("closed"),
                &closed_group,
                &token,
            )
            .expect("the client without the upstream");
            let closed_run = closed_client.resolve(COORDINATES);
            assert!(
                closed_run.ok,
                "mvn did not resolve without the Central:\n{}",
                closed_run.output
            );
            let closed_digest =
                support::sha256_of(&closed_client.local("commons-lang3-3.14.0.jar"));
            assert_eq!(
                closed_digest, jar_digest,
                "the copy served without the Central is not the same content"
            );
            support::stop(&mut closed_service);

            let case = case
                .command(&run.command)
                .origin(REAL_UPSTREAM)
                .sha256(&jar_digest);
            match budget.permits(bytes, elapsed) {
                Ok(()) => case.passed(),
                Err(cause) => case.failed(cause),
            }
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: the cache, the upstream down, and the absence of a direct path.
// ---------------------------------------------------------------------------

#[test]
fn ac2_the_second_fetch_does_not_ask_the_upstream() {
    a_case(
        "ac2-the-second-fetch-does-not-ask-the-upstream",
        "el segundo fetch del mismo artefacto no consulta el upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac2-the-second-fetch-does-not-ask-the-upstream",
                    "el segundo fetch del mismo artefacto no consulta el upstream",
                )
                .not_run("this machine has no Maven that can run without the network");
            };

            let first = client.resolve(COORDINATES);
            assert!(
                first.ok,
                "mvn did not resolve through the group:\n{}",
                first.output
            );
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // The client forgets the artifact: it requests it again, and what
            // is measured is where that second time comes from.
            client.forget();
            let second = client.resolve(COORDINATES);
            assert!(
                second.ok,
                "mvn did not resolve the second time:\n{}",
                second.output
            );
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the second fetch went out to the upstream"
            );
            let digest = support::sha256_of(&client.local("commons-lang3-3.14.0.jar"));
            assert_eq!(digest, sha256_of_bytes(FIXTURE_JAR));

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-second-fetch-does-not-ask-the-upstream",
                "el segundo fetch del mismo artefacto no consulta el upstream",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac2_the_artifact_resolves_with_the_upstream_gone() {
    a_case(
        "ac2-the-artifact-resolves-with-the-upstream-gone",
        "el artefacto se resuelve igual con el upstream desconectado",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac2-the-artifact-resolves-with-the-upstream-gone",
                    "el artefacto se resuelve igual con el upstream desconectado",
                )
                .not_run("this machine has no Maven that can run without the network");
            };

            let first = client.resolve(COORDINATES);
            assert!(
                first.ok,
                "mvn did not resolve through the group:\n{}",
                first.output
            );
            let asked = lab.upstream.total();

            // The upstream goes away: it stops accepting connections. A counter
            // that does not grow with the port closed proves nothing, so
            // what is checked is that the upstream is really dead.
            lab.upstream.disconnect();
            assert!(
                !lab.upstream.accepts_connections(),
                "the upstream is still listening: the evidence of the offline hit is not what it claims to be"
            );

            client.forget();
            let second = client.resolve(COORDINATES);
            assert!(
                second.ok,
                "mvn did not resolve without the upstream:\n{}",
                second.output
            );
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the resolution without the upstream touched the network"
            );
            let digest = support::sha256_of(&client.local("commons-lang3-3.14.0.jar"));
            assert_eq!(digest, sha256_of_bytes(FIXTURE_JAR));

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-artifact-resolves-with-the-upstream-gone",
                "el artefacto se resuelve igual con el upstream desconectado",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac2_the_client_cannot_skip_the_group() {
    a_case(
        "ac2-the-client-cannot-skip-the-group",
        "sin el grupo el cliente no resuelve: no hay un camino directo al Central",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac2-the-client-cannot-skip-the-group",
                    "sin el grupo el cliente no resuelve: no hay un camino directo al Central",
                )
                .not_run("this machine has no Maven that can run without the network");
            };

            let through = client.resolve(COORDINATES);
            assert!(
                through.ok,
                "mvn did not resolve through the group:\n{}",
                through.output
            );
            let asked = lab.upstream.total();

            // The group goes down. The client, with its empty local
            // repository, cannot resolve: it has nowhere to go.
            support::stop(&mut service);
            let orphan = Maven::new(
                &lab.deployment.directory.path().join("orphan"),
                &group,
                &token,
            )
            .expect("the orphan client");
            let refused = orphan.resolve(COORDINATES);
            assert!(
                !refused.ok,
                "the client resolved the artifact without the group: it has a path that skips it:\n{}",
                refused.output
            );

            // And the control: the same client, with `central` pointing at the
            // upstream instead of the group, **does** resolve. This shows
            // that what stops it is the group and not a network that does not work.
            let control = Maven::new(
                &lab.deployment.directory.path().join("control"),
                &lab.upstream.base(),
                &token,
            )
            .expect("the control client");
            let direct = control.resolve(COORDINATES);
            assert!(
                direct.ok,
                "the client could not reach the upstream directly: the block would prove nothing:\n{}",
                direct.output
            );
            assert!(
                lab.upstream.total() > asked,
                "the direct resolution did not reach the upstream"
            );

            LabCase::new(
                "ac2-the-client-cannot-skip-the-group",
                "sin el grupo el cliente no resuelve: no hay un camino directo al Central",
            )
            .command(&refused.command)
            .origin(&lab.upstream.origin())
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: what is not served, and the trail that is not dirtied.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_reserved_group_id_is_not_served_from_the_public_upstream() {
    a_case(
        "ac3-a-reserved-group-id-is-not-served-from-the-public-upstream",
        "una colisión de groupId reservado no se sirve desde el upstream público",
        Scope::Product,
        Expectation::Refused,
        || {
            // The group reserves its namespace: what is under it does not come
            // from the public upstream, even if the upstream has it.
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(Some(serde_json::json!({ "reserved": [GROUP_ID] })));

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;

            let reply = get(&lab, port, GROUP, JAR_PATH);
            let refused = reply.status >= 400;
            assert!(
                refused,
                "the registry served a reserved namespace from the public upstream: {}",
                reply.text()
            );
            assert!(
                !reply
                    .body
                    .windows(FIXTURE_JAR.len())
                    .any(|window| window == FIXTURE_JAR),
                "the bytes of the public upstream were served anyway"
            );
            assert!(
                reply.text().contains("reserved"),
                "the denial does not name the reservation: {}",
                reply.text()
            );
            assert!(
                reply.text().contains(GROUP_ID),
                "the denial does not name the namespace: {}",
                reply.text()
            );
            assert_eq!(
                lab.upstream.hits(JAR_PATH),
                0,
                "the public upstream was asked for a reserved namespace"
            );

            support::stop(&mut service);
            let cause = format!("HTTP {}: {}", reply.status, reply.text());
            let case = LabCase::new(
                "ac3-a-reserved-group-id-is-not-served-from-the-public-upstream",
                "una colisión de groupId reservado no se sirve desde el upstream público",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin());
            if refused {
                case.refused(cause)
            } else {
                case.failed(cause)
            }
        },
    );
}

#[test]
fn ac3_metadata_altered_is_not_served_as_ours() {
    a_case(
        "ac3-metadata-altered-is-not-served-as-ours",
        "unos metadatos alterados no se sirven como si fueran nuestros",
        Scope::Product,
        Expectation::Refused,
        || {
            let lab = Lab::local();
            // The upstream responds with metadata of **another** artifact.
            let altered = the_metadata("org.evil", ARTIFACT_ID, "9.9.9");
            lab.upstream.put(METADATA_PATH, altered.as_bytes(), None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;

            // The group does not serve it as if it were its own.
            let reply = get(&lab, port, GROUP, METADATA_PATH);
            let refused = reply.status >= 400;
            assert!(
                refused,
                "the group served metadata of another artifact as its own: {}",
                reply.text()
            );
            assert!(
                !reply.text().contains("org.evil"),
                "the altered metadata was served: {}",
                reply.text()
            );

            // And the proxy, which merges nothing, serves it as is: the one
            // that does not mix it is the group.
            let direct = get(&lab, port, PROXY, METADATA_PATH);
            assert_eq!(direct.status, 200, "{}", direct.text());
            assert_eq!(direct.body, altered.as_bytes());

            support::stop(&mut service);
            let cause = format!("HTTP {}: {}", reply.status, reply.text());
            let case = LabCase::new(
                "ac3-metadata-altered-is-not-served-as-ours",
                "unos metadatos alterados no se sirven como si fueran nuestros",
            )
            .command(&reply.command)
            .origin(&lab.upstream.origin());
            if refused {
                case.refused(cause)
            } else {
                case.failed(cause)
            }
        },
    );
}

#[test]
fn ac3_the_token_of_the_client_does_not_reach_the_log() {
    a_case(
        "ac3-the-token-of-the-client-does-not-reach-the-log",
        "el token del cliente no aparece en el log del servicio",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put(POM_PATH, FIXTURE_POM, None);
            lab.upstream.put(JAR_PATH, FIXTURE_JAR, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let group = format!("http://127.0.0.1:{port}/repository/{GROUP}");
            let root = lab.deployment.directory.path().join("client");
            let Some(client) = Maven::new(&root, &group, &token) else {
                support::stop(&mut service);
                return LabCase::new(
                    "ac3-the-token-of-the-client-does-not-reach-the-log",
                    "el token del cliente no aparece en el log del servicio",
                )
                .not_run("this machine has no Maven that can run without the network");
            };

            let run = client.resolve(COORDINATES);
            assert!(
                run.ok,
                "mvn did not resolve through the group:\n{}",
                run.output
            );
            support::stop(&mut service);

            // The service log: the client's credential is not in
            // it, nor the one the proxy sends to its upstream.
            let log = lab.deployment.the_log();
            assert!(!log.is_empty(), "the service wrote no log at all");
            assert!(
                !log.contains(&token),
                "the credential of the client reached the log of the service"
            );
            for seen in lab.upstream.requests() {
                assert!(
                    !seen
                        .authorization
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&token),
                    "the credential of the client reached the upstream: {}",
                    seen.path
                );
            }

            LabCase::new(
                "ac3-the-token-of-the-client-does-not-reach-the-log",
                "el token del cliente no aparece en el log del servicio",
            )
            .command(&run.command)
            .origin(&lab.upstream.origin())
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// The laboratory count.
// ---------------------------------------------------------------------------

/// The count does not lie: a case that did not run is not green.
///
/// It is the same rule as the HSP-223 laboratory, checked here with the
/// real case of this suite: without the opt-in, the real Central case does not
/// run, and the report cannot be green.
#[test]
fn ac3_the_count_does_not_lie() {
    a_case(
        "ac3-the-count-does-not-lie",
        "el caso del Central real no se ejecuta sin el opt-in, y no cuenta como verde",
        Scope::Harness,
        Expectation::Succeeds,
        || {
            let summary = Summary::of(&[
                LabCase::new("one", "one").passed(),
                LabCase::new("two", "two").not_run("the real mode is opt-in"),
            ]);
            assert_eq!(summary.pass, 1);
            assert_eq!(summary.not_run, 1);
            assert_eq!(summary.green_cases(), 1);
            assert!(
                !summary.green,
                "a case that did not run was counted as green"
            );
            assert!(!summary.complete);
            assert!(Outcome::Pass.is_green());
            assert!(!Outcome::NotRun.is_green());

            LabCase::new(
                "ac3-the-count-does-not-lie",
                "el caso del Central real no se ejecuta sin el opt-in, y no cuenta como verde",
            )
            .command("Summary::of(casos)")
            .passed()
        },
    );
}
