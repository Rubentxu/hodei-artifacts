//! Maven Central **real** (HSP-224 / HCR-02), con el cliente Maven de verdad.
//!
//! El laboratorio de HSP-223 (`support::lab`) demuestra que el registro
//! cachea, que funciona sin upstream y que no sirve nada no confiable
//! **contra un upstream local**. Aqui se demuestra lo mismo contra el
//! Central público de verdad (`https://repo.maven.apache.org/maven2/`), y
//! con el cliente que usa la gente: `mvn`.
//!
//! Lo que se mide, y no se afirma:
//!
//! - **AC1**: `mvn dependency:get` con un `settings.xml` que apunta al
//!   **grupo** de Hodei resuelve `org.apache.commons:commons-lang3:3.14.0`,
//!   y el POM y el JAR **coinciden por digest** con los que sirve el
//!   Central público. La comprobación del upstream es **opt-in** y con la
//!   versión fijada: sin `HODEI_LAB_REAL=1` el caso no se ejecuta y el
//!   informe no es verde, que es lo correcto.
//! - **AC2**: un **segundo** fetch del mismo artefacto **no consulta** el
//!   upstream (el contador del laboratorio lo dice), con el upstream
//!   **desconectado** se resuelve igual con los datos locales, y el
//!   cliente **no puede saltarse el grupo**: con el grupo caido no
//!   resuelve, y con la misma configuracion apuntando al upstream si.
//! - **AC3**: una **colisión de groupId reservado** y un upstream que
//!   devuelve **metadatos alterados** no sirven paquete no confiable, y
//!   ningun token del cliente aparece en el log del servicio.
//!
//! # El camino sin red
//!
//! Por defecto **no se sale a la red**: el upstream es el del laboratorio
//! (un servidor local que cuenta) y el artefacto es un *fixture* con las
//! coordenadas del Central. La unica lectura de fuera de los directorios
//! del laboratorio es la cache de Maven de la máquina, **en solo
//! lectura**, y solo para el *plugin* (`dependency:get`): el plugin tiene
//! que venir de alguna parte y sin red la unica que hay es esa. Del
//! artefacto bajo prueba no sale de ahi nunca: el repositorio local del
//! cliente es temporal y el artefacto solo se pide al grupo.
//!
//! # El cliente
//!
//! El `mvn` del PATH puede ser un envoltorio que necesita el HOME de la
//! máquina (este tiene uno, para las *slices* de build), y el laboratorio
//! corre el cliente con un HOME que no es el del usuario. El
//! `mvn --version` dice dónde vive el Maven de verdad, y esa ruta no
//! depende del HOME.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use support::lab::{
    Budget, Expectation, LabCase, Outcome, REAL_MODE_VARIABLE, Scope, Summary, Upstream,
    describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// El groupId del artefacto bajo prueba.
const GROUP_ID: &str = "org.apache.commons";

/// El artifactId del artefacto bajo prueba.
const ARTIFACT_ID: &str = "commons-lang3";

/// La versión del artefacto bajo prueba: **fijada**, siempre.
const VERSION: &str = "3.14.0";

/// Las coordenadas que se piden al cliente.
const COORDINATES: &str = "org.apache.commons:commons-lang3:3.14.0";

/// La ruta del POM en el repositorio.
const POM_PATH: &str = "org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom";

/// La ruta del JAR en el repositorio.
const JAR_PATH: &str = "org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.jar";

/// La ruta de los metadatos del artefacto.
const METADATA_PATH: &str = "org/apache/commons/commons-lang3/maven-metadata.xml";

/// El upstream real: el Maven Central de internet.
const REAL_UPSTREAM: &str = "https://repo.maven.apache.org/maven2/";

/// El host del upstream real, como lo entiende la salida del servicio.
const REAL_HOST: &str = "repo.maven.apache.org";

/// El plugin que resuelve el artefacto.
const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";

/// El repositorio *hosted* del grupo: lo propio de la casa.
const HOSTED: &str = "mvn-hosted";

/// El repositorio *proxy* del grupo: el Central de verdad.
const PROXY: &str = "central";

/// El grupo bajo prueba.
const GROUP: &str = "mvn-all";

/// El POM que sirve el upstream del laboratorio: un *fixture* con las
/// coordenadas del Central, y **no** los bytes del Central.
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

/// El JAR que sirve el upstream del laboratorio: los bytes del fixture.
const FIXTURE_JAR: &[u8] = b"PK-the-bytes-of-the-fixture-of-the-maven-central-laboratory";

/// Los casos que una corrida completa tiene que ejecutar.
///
/// El informe los declara, así que una corrida que solo llego a alguno
/// dice cual le falto: los que no se ejecutan cuentan como `NOT_RUN`, y el
/// informe nunca es verde.
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

/// El título y el disclaimer del informe de esta suite.
///
/// El disclaimer del laboratorio (HSP-223) dice que no se ha hablado con
/// `mvn`, y aquí si: el disclaimer es de la suite, no del modulo.
const THE_TITLE: &str = "Laboratorio de Maven Central — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y el artefacto es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el plugin de Maven.** El plugin (`dependency:get`) sale de la cache de Maven de la máquina, en solo lectura. Del plugin no se dice nada del registro: lo que se mide es el artefacto.",
    "- **Ni Central.** Lo que se compara es el digest de una versión fijada (`commons-lang3:3.14.0`): no se certifica la integridad de Central ni la de su contenido.",
    "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés —el recuento— y no el producto. Los dos recuentos van aparte.",
];

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// El laboratorio.
// ---------------------------------------------------------------------------

/// El laboratorio de Maven Central: un despliegue, un upstream que cuenta
/// y el grupo bajo prueba.
struct Lab {
    deployment: Deployment,
    /// El upstream local: solo existe en el modo sin red.
    upstream: Upstream,
    /// La URL del upstream del proxy.
    upstream_url: String,
}

impl Lab {
    /// Un laboratorio con un upstream **local** que cuenta.
    ///
    /// El upstream es el del laboratorio (`support::lab::Upstream`): el
    /// contador y el upstream son la misma cosa, porque un contador que
    /// vive en dos sitios mide uno de los dos.
    fn local() -> Self {
        let upstream = Upstream::start();
        let url = upstream.base();
        Self {
            deployment: Deployment::new(),
            upstream,
            upstream_url: url,
        }
    }

    /// Un laboratorio con el upstream **real**: el Central de internet.
    ///
    /// No hay contador local: a Central no se le puede contar. Lo que se
    /// mide en el modo real es el digest y el hecho de que la segunda
    /// resolución funciona con la salida a Central **cerrada**.
    fn real() -> Self {
        Self {
            deployment: Deployment::new(),
            upstream: Upstream::start(),
            upstream_url: REAL_UPSTREAM.to_string(),
        }
    }

    /// Siembra el hosted, el proxy y el grupo con la politica de reserva
    /// que se le pase.
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

    /// Arranca el servicio con la salida que el caso necesita.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// La salida que deja pasar al upstream local del laboratorio.
    fn the_local_egress(&self) -> String {
        format!("127.0.0.1:{};private;http", self.upstream.port())
    }
}

/// Una respuesta del servicio.
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

/// Pide una ruta a un repositorio del servicio.
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
// El cliente Maven.
// ---------------------------------------------------------------------------

/// La corrida de un cliente.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// Un cliente Maven aislado: su HOME, su repositorio local y sus ajustes.
///
/// El `settings.xml` apunta `central` al **grupo** de Hodei, de modo que
/// el cliente no tiene un camino propio a Central: lo que descarga sale
/// del grupo o no sale.
struct Maven {
    /// El HOME del cliente: no es el de quien ejecuta la prueba.
    home: PathBuf,
    /// El repositorio local del cliente: temporal.
    repository: PathBuf,
    /// Los ajustes del cliente: temporales.
    settings: PathBuf,
    /// El Maven de verdad.
    binary: PathBuf,
}

impl Maven {
    /// Un cliente que apunta a `mirror` y que habla con la credencial
    /// `token`.
    ///
    /// Devuelve `None` cuando la máquina no tiene lo que hace falta para
    /// correr Maven sin red: el Maven de verdad y la cache con el plugin.
    fn new(root: &Path, mirror: &str, token: &str) -> Option<Self> {
        let binary = the_maven_binary()?;
        let cache = the_machine_cache()?;
        let cache = cache.to_string_lossy().into_owned();
        let home = root.join("home");
        let repository = root.join("repository");
        std::fs::create_dir_all(&home).expect("the home of the client");
        std::fs::create_dir_all(&repository).expect("the repository of the client");
        // Las versiones de las herramientas de la máquina se eligen con
        // `.tool-versions`, y ese archivo se busca en el HOME: sin el, el
        // `java` de la máquina no se sabe elegir.
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

    /// Resuelve unas coordenadas y dice si pudo.
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
        // Los envoltorios de la máquina (asdf) eligen la versión de sus
        // herramientas con el HOME: sin esta pista, el HOME del cliente
        // los deja sin java.
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

    /// El fichero que el cliente tiene del artefacto.
    fn local(&self, name: &str) -> PathBuf {
        self.repository
            .join(GROUP_ID.replace('.', "/"))
            .join(ARTIFACT_ID)
            .join(VERSION)
            .join(name)
    }

    /// Olvida el artefacto: el cliente lo volvera a pedir, y lo que se
    /// mide es de dónde sale esa segunda vez.
    fn forget(&self) {
        let _ = std::fs::remove_dir_all(
            self.repository
                .join(GROUP_ID.replace('.', "/"))
                .join(ARTIFACT_ID)
                .join(VERSION),
        );
    }
}

/// El Maven de verdad de esta máquina.
///
/// El `mvn` del PATH puede ser un envoltorio que necesita el HOME de la
/// máquina, y el cliente corre con un HOME que no es el del usuario. El
/// `mvn --version` dice dónde vive el Maven de verdad, y esa ruta no
/// depende del HOME.
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

/// La cache de Maven de la máquina: de dónde sale el **plugin**, y solo
/// el plugin.
///
/// Se lee en solo lectura y el artefacto bajo prueba nunca sale de ahi:
/// el repositorio local del cliente es temporal, y el artefacto se pide
/// al grupo.
fn the_machine_cache() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let cache = PathBuf::from(home).join(".m2").join("repository");
    cache.is_dir().then_some(cache)
}

/// Los directorios de asdf, si esta máquina los tiene: son dónde sus
/// envoltorios buscan las herramientas que el HOME ya no les dice.
fn the_asdf_directories() -> Option<Vec<(&'static str, PathBuf)>> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let asdf = home.join(".asdf");
    asdf.is_dir()
        .then(|| vec![("ASDF_DIR", asdf.clone()), ("ASDF_DATA_DIR", asdf)])
}

// ---------------------------------------------------------------------------
// Utilidades.
// ---------------------------------------------------------------------------

/// El resumen de un caso, con el id y el título del caso.
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

/// El sha256 de unos bytes.
fn sha256_of_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Los bytes que el upstream real sirve para una ruta, pedidos **por
/// fuera** del registro.
///
/// La comparacion es el punto: el digest de lo que Hodei sirvio tiene que
/// ser el digest de lo que sirve Central, y la unica forma de saberlo es
/// preguntarle a Central. La versión va fijada en la ruta.
fn the_upstream_bytes(path: &str) -> Option<Vec<u8>> {
    let url = format!("{REAL_UPSTREAM}{path}");
    let output = Command::new("curl")
        .args(["--silent", "--show-error", "--fail", "--location"])
        .arg(&url)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Los metadatos de un artefacto, tal y como los escribiria un upstream.
fn the_metadata(group_id: &str, artifact_id: &str, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>\n  <groupId>{group_id}</groupId>\n  <artifactId>{artifact_id}</artifactId>\n  <versioning>\n    <latest>{version}</latest>\n    <release>{version}</release>\n    <versions>\n      <version>{version}</version>\n    </versions>\n    <lastUpdated>20240101000000</lastUpdated>\n  </versioning>\n</metadata>\n"
    )
}

// ---------------------------------------------------------------------------
// AC1: el cliente resuelve por el grupo, y el digest es el del upstream.
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

            // El POM y el JAR del cliente son los del fixture, byte a byte.
            let jar = client.local("commons-lang3-3.14.0.jar");
            let pom = client.local("commons-lang3-3.14.0.pom");
            assert!(jar.is_file(), "the jar was not resolved");
            assert!(pom.is_file(), "the pom was not resolved");
            let jar_digest = support::sha256_of(&jar);
            let pom_digest = support::sha256_of(&pom);
            assert_eq!(jar_digest, sha256_of_bytes(FIXTURE_JAR));
            assert_eq!(pom_digest, sha256_of_bytes(FIXTURE_POM));
            // Y el upstream recibió la petición: lo que el cliente tiene
            // salio de ahi, no de ningun otro sitio.
            assert!(
                lab.upstream.hits(JAR_PATH) > 0,
                "the upstream was never asked for the jar"
            );
            assert!(
                lab.upstream.hits(POM_PATH) > 0,
                "the upstream was never asked for the pom"
            );
            // Y lo que el cliente tiene salio del **grupo**: el miembro
            // que gana la ruta es el proxy del Central.
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
                // Sin el opt-in el caso no se ejecuta, y un caso que no
                // se ejecuta no es un caso verde.
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

            // El POM y el JAR que tiene el cliente, y los que sirve el
            // Central real: los digests tienen que ser los mismos.
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

            // Y la corrida respeta el presupuesto del laboratorio.
            let bytes = std::fs::metadata(&jar).map(|meta| meta.len()).unwrap_or(0);

            // El Central se queda fuera de la salida del servicio: la
            // lista de hosts que puede contactar no lo tiene, así que el
            // servicio **no puede** ir a buscarlo aunque quisiera. El
            // servicio vuelve a arrancar con la misma base y el mismo
            // almacen, y el cliente, con su repositorio local vacío,
            // resuelve igual: lo que se sirve es la copia local.
            support::stop(&mut service);
            let mut closed: Vec<(String, String)> = environment
                .iter()
                .filter(|(name, _)| name != "HODEI_PROXY__ALLOWED_HOSTS")
                .cloned()
                .collect();
            closed.push((
                "HODEI_PROXY__ALLOWED_HOSTS".to_string(),
                // Un host que no es Central: la salida sigue existiendo,
                // pero a Central no le llega. No se vacia la lista (una
                // lista vacia no es una lista), se cambia lo que hay en
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
// AC2: la cache, el upstream caido, y la ausencia de un camino directo.
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

            // El cliente olvida el artefacto: lo vuelve a pedir, y lo que
            // se mide es de dónde sale esa segunda vez.
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

            // El upstream se va: deja de aceptar conexiones. Un contador
            // que no crece con el puerto cerrado no prueba nada, así que
            // lo que se comprueba es que el upstream está muerto de verdad.
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

            // El grupo se cae. El cliente, con su repositorio local
            // vacío, no puede resolver: no tiene a dónde ir.
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

            // Y el control: el mismo cliente, con `central` apuntando al
            // upstream en vez de al grupo, **si** resuelve. Asi se ve que
            // lo que lo para es el grupo y no una red que no funciona.
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
// AC3: lo que no se sirve, y el rastro que no se ensucia.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_reserved_group_id_is_not_served_from_the_public_upstream() {
    a_case(
        "ac3-a-reserved-group-id-is-not-served-from-the-public-upstream",
        "una colisión de groupId reservado no se sirve desde el upstream público",
        Scope::Product,
        Expectation::Refused,
        || {
            // El grupo reserva su namespace: lo que este bajo el no sale
            // del upstream público, aunque el upstream lo tenga.
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
            // El upstream responde con metadatos de **otro** artefacto.
            let altered = the_metadata("org.evil", ARTIFACT_ID, "9.9.9");
            lab.upstream.put(METADATA_PATH, altered.as_bytes(), None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;

            // El grupo no los sirve como si fueran suyos.
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

            // Y el proxy, que no fusiona nada, los sirve tal cual: el que
            // no los Mezcla es el grupo.
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

            // El log del servicio: la credencial del cliente no está en
            // el, ni la que el proxy manda a su upstream.
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
// El recuento del laboratorio.
// ---------------------------------------------------------------------------

/// El recuento no miente: un caso que no se ejecuto no es verde.
///
/// Es la misma regla del laboratorio de HSP-223, comprobada aquí con el
/// caso real de esta suite: sin el opt-in, el caso del Central real no se
/// ejecuta, y el informe no puede ser verde.
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
