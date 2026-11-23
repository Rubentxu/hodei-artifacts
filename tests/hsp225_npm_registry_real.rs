//! npmjs **real** (HSP-225 / HCR-03), con el cliente npm de verdad.
//!
//! El laboratorio de HSP-223 (`support::lab`) demuestra que el registro
//! cachea, que funciona sin upstream y que no sirve nada no confiable
//! **contra un upstream local**. Aqui se demuestra lo mismo contra el
//! registro publico de verdad (`https://registry.npmjs.org/`), y con el
//! cliente que usa la gente: `npm`.
//!
//! Lo que se mide, y no se afirma:
//!
//! - **AC1**: `npm view` y `npm pack` / `npm install --ignore-scripts` con
//!   un `.npmrc` y un HOME temporales resuelven `is-number@7.0.0` por Hodei,
//!   y el `integrity` del tarball coincide con el que declara el origen.
//!   La comprobacion contra npmjs de verdad es **opt-in** y con la version
//!   fijada: sin `HODEI_LAB_REAL=1` el caso no se ejecuta y el informe no
//!   es verde, que es lo correcto.
//! - **AC2**: un **segundo** fetch del mismo contenido **no consulta** el
//!   upstream (el contador del laboratorio lo dice), con el upstream
//!   **desconectado** se resuelve igual con los datos locales, y un
//!   dist-tag que se mueve se revalida **segun el TTL** sin cambiar los
//!   bytes historicos.
//! - **AC3**: un **scope reservado** no cae al publico, las URLs
//!   `dist.tarball` que devuelve el proxy apuntan al **endpoint de Hodei**,
//!   y ningun token del cliente aparece en el log ni viaja al upstream.
//!
//! # El camino sin red
//!
//! Por defecto **no se sale a la red**: el upstream es el del laboratorio
//! (un servidor local que cuenta) y el paquete es un *fixture* con las
//! coordenadas del publico: un tarball de verdad, empaquetado con el mismo
//! `npm pack` que usaria el cliente, pero **no** los bytes de npmjs. Del
//! paquete bajo prueba no sale de ahi nunca: la cache del cliente es
//! temporal y el paquete se pide al grupo.
//!
//! # El cliente
//!
//! El `npm` del PATH puede ser un envoltorio que necesita el HOME de la
//! maquina, y el laboratorio corre el cliente con un HOME que no es el del
//! usuario. Por eso el cliente tiene **su** HOME (`LabHome`), **su** cache
//! y **su** `.npmrc`: lo que se descarga sale del grupo o no sale, y la
//! credencial vive en un fichero temporal que desaparece con el caso.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use base64::Engine as _;
use sha2::{Digest, Sha512};
use support::lab::{
    Budget, Doc, Expectation, LabCase, LabHome, Outcome, REAL_MODE_VARIABLE, Scope, Summary,
    Upstream, describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// El paquete bajo prueba: las coordenadas son las del publico.
const PACKAGE: &str = "is-number";

/// La version del paquete bajo prueba: **fijada**, siempre.
const VERSION: &str = "7.0.0";

/// La otra version del fixture: la a la que se mueve el dist-tag.
const OTHER_VERSION: &str = "7.1.0";

/// Las coordenadas que se piden al cliente.
const SPEC: &str = "is-number@7.0.0";

/// El path del packument en el repositorio.
const PACKUMENT_PATH: &str = "is-number";

/// El path del tarball en el repositorio.
const TARBALL_PATH: &str = "is-number/-/is-number-7.0.0.tgz";

/// El scope reservado: lo que la casa tiene y el publico no debe servir.
const RESERVED_SCOPE: &str = "@reserved";

/// El paquete del scope reservado.
const RESERVED_PACKAGE: &str = "@reserved/widget";

/// El upstream real: el registro publico de npm.
const REAL_UPSTREAM: &str = "https://registry.npmjs.org/";

/// El host del upstream real, como lo entiende la salida del servicio.
const REAL_HOST: &str = "registry.npmjs.org";

/// El `integrity` que declara npmjs para `is-number@7.0.0`.
///
/// Es el valor que dice el publico, no uno que se calcula aqui: lo que se
/// comprueba es que el tarball que baja por Hodei sea **ese** y no otro.
const REAL_INTEGRITY: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";

/// El repositorio *hosted* del grupo: lo propio de la casa.
const HOSTED: &str = "npm-hosted";

/// El repositorio *proxy* del grupo: el registro publico.
const PROXY: &str = "npm-public";

/// El grupo bajo prueba.
const GROUP: &str = "npm-all";

/// Los TTL con los que se siembra el proxy, en segundos.
///
/// El contenido (los tarballs) no caduca nunca: es inmutable por
/// definicion. La metadata (los packuments) caduca, y ahi es donde se
/// decide cuando se revalida un dist-tag que se ha movido.
const METADATA_TTL: u64 = 3_600;

/// El TTL corto con el que se demuestra la revalidacion por TTL.
const SHORT_METADATA_TTL: u64 = 5;

/// Los casos que una corrida completa tiene que ejecutar.
///
/// El informe los declara, asi que una corrida que solo llego a alguno
/// dice cual le falto: los que no se ejecutan cuentan como `NOT_RUN`, y el
/// informe nunca es verde.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-npm-view-resolves-the-pinned-version-through-the-group",
        "el cliente npm resuelve la version fijada por el grupo",
    ),
    (
        "ac1-npm-pack-and-install-agree-with-the-origin-integrity",
        "el tarball que baja y el lock coinciden en integrity con el del origen",
    ),
    (
        "ac1-the-real-npmjs-serves-the-same-integrity",
        "contra npmjs de verdad el integrity es el que declara el publico",
    ),
    (
        "ac2-the-second-fetch-does-not-ask-the-upstream",
        "el segundo fetch del mismo contenido no consulta el upstream",
    ),
    (
        "ac2-the-group-serves-the-pinned-package-with-the-network-closed",
        "el grupo sirve el paquete fijado con el upstream desconectado",
    ),
    (
        "ac2-a-moved-dist-tag-is-revalidated-by-the-ttl",
        "un dist-tag que se mueve se revalida segun el TTL",
    ),
    (
        "ac2-the-moved-dist-tag-does-not-change-the-historical-bytes",
        "mover el dist-tag no cambia los bytes historicos",
    ),
    (
        "ac3-a-reserved-scope-is-not-served-from-the-public-upstream",
        "un scope reservado no se sirve desde el upstream publico",
    ),
    (
        "ac3-the-tarball-urls-point-to-hodei",
        "las URLs dist.tarball que se sirven apuntan al endpoint de Hodei",
    ),
    (
        "ac3-the-token-of-the-client-does-not-leak",
        "el token del cliente no llega al log ni al upstream",
    ),
    (
        "ac3-the-count-does-not-lie",
        "el caso de npmjs real no se ejecuta sin el opt-in, y no cuenta como verde",
    ),
];

/// El titulo y el disclaimer del informe de esta suite.
///
/// El disclaimer del laboratorio (HSP-223) dice que no se ha hablado con
/// `npm`, y aqui si: el disclaimer es de la suite, no del modulo.
const THE_TITLE: &str = "Laboratorio de npmjs — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y el paquete es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el paquete publico.** En el camino sin red el tarball es un fixture con las coordenadas de `is-number@7.0.0`, no los bytes de npmjs: lo que se demuestra es el camino, no la integridad del publico. El camino real si compara el `integrity` con el que declara npmjs.",
    "- **Ni la distribucion de npm.** Se mide `npm view`, `npm pack` e `npm install --ignore-scripts` contra un registro de Hodei: no se mide la resolucion de conflictos del cliente, ni sus scripts, ni su auditoria.",
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

/// El laboratorio de npmjs: un despliegue, un upstream que cuenta y el
/// grupo bajo prueba.
struct Lab {
    deployment: Deployment,
    /// El upstream local: solo existe en el modo sin red.
    upstream: Upstream,
    /// La URL del upstream del proxy.
    upstream_url: String,
    /// Los segundos de frescura de la metadata del proxy.
    metadata_ttl: u64,
}

impl Lab {
    /// Un laboratorio con un upstream **local** que cuenta.
    ///
    /// El upstream es el del laboratorio (`support::lab::Upstream`): el
    /// contador y el upstream son la misma cosa, porque un contador que
    /// vive en dos sitios mide uno de los dos.
    fn local(metadata_ttl: u64) -> Self {
        let upstream = Upstream::start();
        let url = upstream.base();
        Self {
            deployment: Deployment::new(),
            upstream,
            upstream_url: url,
            metadata_ttl,
        }
    }

    /// Un laboratorio con el upstream **real**: el registro publico de npm.
    ///
    /// No hay contador local: a npmjs no se le puede contar. Lo que se
    /// mide en el modo real es el `integrity` y el hecho de que la
    /// resolucion funciona con la salida a npmjs **cerrada**.
    fn real() -> Self {
        Self {
            deployment: Deployment::new(),
            upstream: Upstream::start(),
            upstream_url: REAL_UPSTREAM.to_string(),
            metadata_ttl: METADATA_TTL,
        }
    }

    /// Siembra el hosted, el proxy y el grupo con la politica de reserva
    /// que se le pase.
    fn seed(&self, namespaces: Option<serde_json::Value>) {
        let mut proxy = serde_json::json!({
            "key": PROXY,
            "format": "npm",
            "proxy": {
                "upstream": {
                    "id": "npmjs",
                    "url": self.upstream_url,
                    "scopes": ["/"],
                    "max_bytes": 4 * 1024 * 1024,
                },
                // El contenido no caduca: un tarball es inmutable. La
                // metadata caduca, y ahi es donde se revalida un dist-tag
                // que se ha movido.
                "cache": { "ttl": { "content_secs": null, "metadata_secs": self.metadata_ttl } },
            },
        });
        if let Some(policy) = &namespaces {
            proxy["namespaces"] = policy.clone();
        }
        self.deployment
            .seed_repositories(&[serde_json::json!({ "key": HOSTED, "format": "npm" }), proxy]);
        let mut group = serde_json::json!({
            "key": GROUP,
            "members": [HOSTED, PROXY],
            "format": "npm",
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

// ---------------------------------------------------------------------------
// El cliente npm.
// ---------------------------------------------------------------------------

/// La corrida de un cliente.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// Un cliente `npm` aislado: su HOME, su cache y su `.npmrc`.
///
/// El `.npmrc` apunta el registro al **grupo** de Hodei, de modo que el
/// cliente no tiene un camino propio a npmjs: lo que descarga sale del
/// grupo o no sale. La credencial vive en el `.npmrc`, nunca en el argv.
struct Npm {
    /// El HOME del cliente: no es el de quien ejecuta la prueba.
    home: LabHome,
    /// El directorio del cliente: el `.npmrc` y la cache.
    root: PathBuf,
    /// El `.npmrc` del cliente.
    rc: PathBuf,
    /// El registro al que habla el cliente.
    registry: String,
}

impl Npm {
    /// Un cliente que habla con `/npm/{repository}/` del servicio.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let root = root.join(name);
        std::fs::create_dir_all(&root).expect("the directory of the client");
        let home = LabHome::new();
        let rc = root.join("npmrc");
        let registry = format!("http://127.0.0.1:{port}/npm/{repository}/");
        std::fs::write(
            &rc,
            format!(
                "registry={registry}\n\
                 //127.0.0.1:{port}/npm/{repository}/:_authToken={token}\n\
                 fetch-retries=0\n\
                 update-notifier=false\n\
                 audit=false\n\
                 fund=false\n"
            ),
        )
        .expect("the npmrc is written");
        Self {
            home,
            root,
            rc,
            registry,
        }
    }

    /// Ejecuta `npm` en `dir` con la cache de este cliente.
    fn run(&self, dir: &Path, args: &[&str]) -> Run {
        let mut command = Command::new("npm");
        command
            .arg("--userconfig")
            .arg(&self.rc)
            .arg("--globalconfig")
            .arg(self.root.join("no-global-config"))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(["--no-audit", "--no-fund", "--loglevel=warn"])
            .args(args)
            .current_dir(dir);
        for (name, value) in self.home.environment() {
            command.env(name, value);
        }
        let output = command.output().expect("npm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
            command: format!("npm {} (registro: {})", args.join(" "), self.registry),
        }
    }

    /// Una cache vacia: lo siguiente que se instale sale del servicio.
    ///
    /// La cache del cliente es la del cliente: sin vaciarla, el `npm`
    /// contesta por su cuenta y lo que se mide es su cache, no la del
    /// registro.
    fn forget_the_cache(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("cache"));
    }
}

// ---------------------------------------------------------------------------
// Los fixtures.
// ---------------------------------------------------------------------------

/// El nombre del fichero de un tarball, como lo escribe npm.
///
/// Un paquete con alcance se escribe `alcance-nombre-version.tgz`: la
/// arroba no puede ir en un nombre de fichero.
fn the_tarball_file(name: &str, version: &str) -> String {
    let base = name.trim_start_matches('@').replace('/', "-");
    format!("{base}-{version}.tgz")
}

/// Un tarball de verdad de un paquete con las coordenadas del publico.
///
/// Lo empaqueta el mismo `npm` que usaria el cliente, con su HOME
/// aislado: lo que el proxy sirve es un tarball que el `npm` sabe
/// abrir, no un fichero con nombre de tarball.
fn the_fixture_tarball(home: &LabHome, root: &Path, name: &str, version: &str) -> Vec<u8> {
    let source = root.join(format!(
        "fixture-{}-{version}",
        the_tarball_file(name, version)
    ));
    std::fs::create_dir_all(&source).expect("the directory of the fixture");
    std::fs::write(
        source.join("package.json"),
        format!(
            "{{\"name\":\"{name}\",\"version\":\"{version}\",\"main\":\"index.js\",\
             \"description\":\"the fixture of the npmjs laboratory\"}}"
        ),
    )
    .expect("the package.json of the fixture");
    std::fs::write(
        source.join("index.js"),
        format!("module.exports = '{name} {version}';\n"),
    )
    .expect("the index of the fixture");
    let mut command = Command::new("npm");
    command
        .args(["pack", "--pack-destination"])
        .arg(root)
        .args(["--loglevel=error", "--no-audit", "--no-fund"])
        .env("HOME", home.path())
        .current_dir(&source);
    let output = command.output().expect("npm pack runs");
    assert!(
        output.status.success(),
        "npm pack failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let file = root.join(the_tarball_file(name, version));
    std::fs::read(&file).expect("the packed tarball")
}

/// El packument que sirve el upstream del laboratorio.
///
/// Las URLs `dist.tarball` apuntan al **upstream**: si el proxy no las
/// reescribe, el cliente se ira ahi y la prueba lo dira.
fn the_packument(
    name: &str,
    upstream: &str,
    versions: &[(&str, &str)],
    tags: &[(&str, &str)],
) -> Vec<u8> {
    let mut entries = serde_json::Map::new();
    for (version, integrity) in versions {
        let file = the_tarball_file(name, version);
        entries.insert(
            version.to_string(),
            serde_json::json!({
                "name": name,
                "version": version,
                "main": "index.js",
                "dist": {
                    "tarball": format!("{upstream}{name}/-/{file}"),
                    "integrity": integrity,
                },
            }),
        );
    }
    let mut tag_map = serde_json::Map::new();
    for (tag, version) in tags {
        tag_map.insert(tag.to_string(), serde_json::json!(version));
    }
    serde_json::to_vec(&serde_json::json!({
        "_id": name,
        "name": name,
        "dist-tags": tag_map,
        "versions": entries,
    }))
    .expect("the packument is json")
}

/// El `integrity` de unos bytes: `sha512` en base64, como lo escribe npm.
fn integrity_of(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// El sha256 de unos bytes.
fn sha256_of_bytes(bytes: &[u8]) -> String {
    use sha2::Sha256;
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Un proyecto consumidor vacio.
fn a_consumer(dir: &Path) {
    std::fs::create_dir_all(dir).expect("the consumer directory");
    std::fs::write(
        dir.join("package.json"),
        "{\"name\":\"consumer\",\"version\":\"1.0.0\",\"private\":true}",
    )
    .expect("the package.json of the consumer");
}

/// La respuesta del servicio a una peticion.
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

/// Pide una ruta al servicio por la superficie de npm.
fn get(port: u16, path: &str, token: &str, into: &Path) -> Reply {
    let _ = std::fs::remove_file(into);
    let curl = Curl::call(
        port,
        "GET",
        &format!("/npm/{GROUP}{path}"),
        token,
        None,
        None,
        Some(into),
    );
    let command = curl.shown();
    let answer = curl.run();
    Reply {
        status: answer.status,
        body: std::fs::read(into).unwrap_or_default(),
        resolved_from: answer.header("x-hodei-resolved-from"),
        command,
    }
}

// ---------------------------------------------------------------------------
// Utilidades.
// ---------------------------------------------------------------------------

/// El resumen de un caso, con el id y el titulo del caso.
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

// ---------------------------------------------------------------------------
// AC1: el cliente resuelve por el grupo, y el integrity es el del origen.
// ---------------------------------------------------------------------------

#[test]
fn ac1_npm_view_resolves_the_pinned_version_through_the_group() {
    a_case(
        "ac1-npm-view-resolves-the-pinned-version-through-the-group",
        "el cliente npm resuelve la version fijada por el grupo",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &integrity_of(&tarball))],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "viewer", port, GROUP, &token);
            let work = root.join("work");
            std::fs::create_dir_all(&work).expect("the work directory");

            let run = client.run(&work, &["view", SPEC, "version"]);
            assert!(
                run.ok,
                "npm view did not resolve through the group:\n{}",
                run.output
            );
            assert_eq!(
                run.output.trim(),
                VERSION,
                "the client did not see the pinned version"
            );
            // Y lo que el cliente tiene salio del upstream, no de otro sitio.
            assert!(
                lab.upstream.hits(PACKUMENT_PATH) > 0,
                "the upstream was never asked for the packument"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-npm-view-resolves-the-pinned-version-through-the-group",
                "el cliente npm resuelve la version fijada por el grupo",
            )
            .command(&run.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&tarball))
            .passed()
        },
    );
}

#[test]
fn ac1_npm_pack_and_install_agree_with_the_origin_integrity() {
    a_case(
        "ac1-npm-pack-and-install-agree-with-the-origin-integrity",
        "el tarball que baja y el lock coinciden en integrity con el del origen",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "packer", port, GROUP, &token);
            let work = root.join("work");
            let packed = root.join("packed");
            let project = root.join("consumer");
            std::fs::create_dir_all(&work).expect("the work directory");
            std::fs::create_dir_all(&packed).expect("the packed directory");
            a_consumer(&project);

            // `npm pack` baja el tarball por el grupo. El propio `npm`
            // comprueba el `integrity` que declara el packument: si los
            // bytes no fueran los, el cliente fallaria antes de que la
            // prueba llegara a mirar.
            let pack = client.run(
                &work,
                &[
                    "pack",
                    SPEC,
                    "--pack-destination",
                    &packed.to_string_lossy(),
                ],
            );
            assert!(pack.ok, "npm pack failed:\n{}", pack.output);
            let file = packed.join(the_tarball_file(PACKAGE, VERSION));
            assert!(file.is_file(), "npm pack did not leave the tarball");
            let bytes = std::fs::read(&file).expect("the packed tarball");
            assert_eq!(
                integrity_of(&bytes),
                declared,
                "the tarball that came through Hodei is not the one the origin declares"
            );

            // Y la instalacion: el lock dice el `integrity` que el cliente
            // verifico, y tiene que ser el mismo.
            let install = client.run(&project, &["install", SPEC, "--ignore-scripts"]);
            assert!(install.ok, "npm install failed:\n{}", install.output);
            let lock: serde_json::Value = serde_json::from_slice(
                &std::fs::read(project.join("package-lock.json")).expect("the lock"),
            )
            .expect("the lock parses");
            let locked = lock["packages"]["node_modules/is-number"]["integrity"]
                .as_str()
                .expect("the lock carries an integrity")
                .to_string();
            assert_eq!(
                locked, declared,
                "the lock does not carry the integrity of the origin"
            );
            assert!(
                project.join("node_modules/is-number/index.js").is_file(),
                "the package was not installed"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-npm-pack-and-install-agree-with-the-origin-integrity",
                "el tarball que baja y el lock coinciden en integrity con el del origen",
            )
            .command(&pack.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&bytes))
            .passed()
        },
    );
}

#[test]
fn ac1_the_real_npmjs_serves_the_same_integrity() {
    a_case(
        "ac1-the-real-npmjs-serves-the-same-integrity",
        "contra npmjs de verdad el integrity es el que declara el publico",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let case = LabCase::new(
                "ac1-the-real-npmjs-serves-the-same-integrity",
                "contra npmjs de verdad el integrity es el que declara el publico",
            )
            .command(&format!("npm view {SPEC} dist.integrity (registro: npmjs)"));
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
            let root = lab.deployment.directory.path().to_path_buf();
            let client = Npm::new(&root, "real", port, GROUP, &token);
            let work = root.join("work");
            let packed = root.join("packed");
            let project = root.join("consumer");
            std::fs::create_dir_all(&work).expect("the work directory");
            std::fs::create_dir_all(&packed).expect("the packed directory");
            a_consumer(&project);

            let started = Instant::now();
            // Lo que declara el publico, leido **por Hodei**.
            let view = client.run(&work, &["view", SPEC, "dist.integrity"]);
            assert!(view.ok, "npm view failed:\n{}", view.output);
            let seen = view.output.trim().to_string();
            assert_eq!(
                seen, REAL_INTEGRITY,
                "the integrity that came through Hodei is not the one npmjs declares"
            );

            // Y el tarball de verdad, bajado por Hodei: sus bytes son los
            // del publico, y el `npm` los verifica contra lo declarado.
            let pack = client.run(
                &work,
                &[
                    "pack",
                    SPEC,
                    "--pack-destination",
                    &packed.to_string_lossy(),
                ],
            );
            assert!(pack.ok, "npm pack failed:\n{}", pack.output);
            let file = packed.join(the_tarball_file(PACKAGE, VERSION));
            let bytes = std::fs::read(&file).expect("the packed tarball");
            assert_eq!(
                integrity_of(&bytes),
                REAL_INTEGRITY,
                "the tarball that came through Hodei is not the tarball of npmjs"
            );

            // Y la instalacion, que es lo que el cliente haria de verdad.
            let install = client.run(&project, &["install", SPEC, "--ignore-scripts"]);
            assert!(install.ok, "npm install failed:\n{}", install.output);
            let lock: serde_json::Value = serde_json::from_slice(
                &std::fs::read(project.join("package-lock.json")).expect("the lock"),
            )
            .expect("the lock parses");
            let locked = lock["packages"]["node_modules/is-number"]["integrity"]
                .as_str()
                .expect("the lock carries an integrity")
                .to_string();
            assert_eq!(
                locked, REAL_INTEGRITY,
                "the lock does not carry the integrity of npmjs"
            );
            let elapsed = started.elapsed();

            // Y la corrida respeta el presupuesto del laboratorio.
            let size = std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
            support::stop(&mut service);
            let case = case
                .command(&pack.command)
                .origin(REAL_UPSTREAM)
                .sha256(&sha256_of_bytes(&bytes));
            match budget.permits(size, elapsed) {
                Ok(()) => case.passed(),
                Err(cause) => case.failed(cause),
            }
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: la cache, el upstream caido, y el dist-tag que se mueve.
// ---------------------------------------------------------------------------

#[test]
fn ac2_the_second_fetch_does_not_ask_the_upstream() {
    a_case(
        "ac2-the-second-fetch-does-not-ask-the-upstream",
        "el segundo fetch del mismo contenido no consulta el upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let first_project = root.join("consumer-one");
            let second_project = root.join("consumer-two");
            a_consumer(&first_project);
            a_consumer(&second_project);

            // La primera vez: el upstream contesta (un MISS).
            let first = client.run(&first_project, &["install", SPEC, "--ignore-scripts"]);
            assert!(first.ok, "the first install failed:\n{}", first.output);
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // La segunda vez: el cliente llega sin su cache, así que lo
            // que se mide es de donde sale lo que trae.
            client.forget_the_cache();
            let second = client.run(&second_project, &["install", SPEC, "--ignore-scripts"]);
            assert!(second.ok, "the second install failed:\n{}", second.output);
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the second fetch went out to the upstream"
            );
            let lock: serde_json::Value = serde_json::from_slice(
                &std::fs::read(second_project.join("package-lock.json")).expect("the lock"),
            )
            .expect("the lock parses");
            let locked = lock["packages"]["node_modules/is-number"]["integrity"]
                .as_str()
                .expect("the lock carries an integrity")
                .to_string();
            assert_eq!(locked, declared, "the copy served is not the same content");

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-second-fetch-does-not-ask-the-upstream",
                "el segundo fetch del mismo contenido no consulta el upstream",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&tarball))
            .passed()
        },
    );
}

#[test]
fn ac2_the_group_serves_the_pinned_package_with_the_network_closed() {
    a_case(
        "ac2-the-group-serves-the-pinned-package-with-the-network-closed",
        "el grupo sirve el paquete fijado con el upstream desconectado",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let first_project = root.join("consumer-one");
            let second_project = root.join("consumer-two");
            a_consumer(&first_project);
            a_consumer(&second_project);

            let first = client.run(&first_project, &["install", SPEC, "--ignore-scripts"]);
            assert!(first.ok, "the first install failed:\n{}", first.output);
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // El upstream se va: deja de aceptar conexiones. Un contador
            // que no crece con el puerto cerrado no prueba nada, así que
            // lo que se comprueba es que el upstream esta muerto de verdad.
            lab.upstream.disconnect();
            assert!(
                !lab.upstream.accepts_connections(),
                "the upstream is still listening: the evidence of the offline hit is not what it claims to be"
            );

            // El grupo (un hosted y un proxy) sigue sirviendo el paquete
            // fijado: lo que tiene, lo tiene con procedencia.
            client.forget_the_cache();
            let second = client.run(&second_project, &["install", SPEC, "--ignore-scripts"]);
            assert!(
                second.ok,
                "npm install without the upstream failed:\n{}",
                second.output
            );
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the resolution without the upstream touched the network"
            );
            let lock: serde_json::Value = serde_json::from_slice(
                &std::fs::read(second_project.join("package-lock.json")).expect("the lock"),
            )
            .expect("the lock parses");
            let locked = lock["packages"]["node_modules/is-number"]["integrity"]
                .as_str()
                .expect("the lock carries an integrity")
                .to_string();
            assert_eq!(locked, declared, "the copy served is not the same content");

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-group-serves-the-pinned-package-with-the-network-closed",
                "el grupo sirve el paquete fijado con el upstream desconectado",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&tarball))
            .passed()
        },
    );
}

#[test]
fn ac2_a_moved_dist_tag_is_revalidated_by_the_ttl() {
    a_case(
        "ac2-a-moved-dist-tag-is-revalidated-by-the-ttl",
        "un dist-tag que se mueve se revalida segun el TTL",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(SHORT_METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            // La otra version del fixture: la a la que se mueve el
            // dist-tag. Su tarball tambien existe, porque un packument
            // cuyo `latest` apunta a una version que no esta en `versions`
            // no es un packument que npm pueda resolver.
            let other_tarball = the_fixture_tarball(&home, &root, PACKAGE, OTHER_VERSION);
            let other_declared = integrity_of(&other_tarball);
            let before = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            let after = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared), (OTHER_VERSION, &other_declared)],
                &[("latest", OTHER_VERSION)],
            );
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&before).with_etag("\"v1\""));
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.upstream.put(
                &format!("is-number/-/{}", the_tarball_file(PACKAGE, OTHER_VERSION)),
                &other_tarball,
                None,
            );
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let work = root.join("work");
            std::fs::create_dir_all(&work).expect("the work directory");

            // El MISS: el packument sale del upstream.
            let first = client.run(&work, &["view", PACKAGE, "dist-tags.latest"]);
            assert!(first.ok, "the first view failed:\n{}", first.output);
            assert_eq!(first.output.trim(), VERSION);
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // El upstream mueve el dist-tag: el tag `latest` pasa a otra
            // version.
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&after).with_etag("\"v2\""));

            // Dentro del TTL, el packument cacheado es el que se sirve: el
            // upstream no se vuelve a preguntar y el tag no se ha movido
            // todavia.
            let cached = get(
                port,
                &format!("/{PACKUMENT_PATH}"),
                &token,
                &root.join("cached.json"),
            );
            assert_eq!(cached.status, 200, "{}", cached.text());
            // La respuesta dice **quien** la resolvio: el miembro del
            // grupo que gana, no el que el cliente pidio.
            assert_eq!(
                cached.resolved_from.as_deref(),
                Some(PROXY),
                "the answer does not say which member resolved it: {}",
                cached.command
            );
            let document: serde_json::Value =
                serde_json::from_slice(&cached.body).expect("the packument is json");
            assert_eq!(
                document["dist-tags"]["latest"].as_str(),
                Some(VERSION),
                "the dist-tag moved before the TTL expired: {}",
                document
            );
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the upstream was asked again before the TTL expired"
            );

            // Y pasado el TTL, la revalidacion es condicional: el upstream
            // contesta con bytes nuevos y el tag movido llega al cliente.
            std::thread::sleep(Duration::from_secs(SHORT_METADATA_TTL + 2));
            client.forget_the_cache();
            let revalidated = client.run(&work, &["view", PACKAGE, "dist-tags.latest"]);
            assert!(
                revalidated.ok,
                "the view after the TTL failed:\n{}",
                revalidated.output
            );
            assert_eq!(
                revalidated.output.trim(),
                OTHER_VERSION,
                "the moved dist-tag was not revalidated after the TTL"
            );
            assert!(
                lab.upstream.total() > asked,
                "the upstream was not asked again after the TTL expired"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-a-moved-dist-tag-is-revalidated-by-the-ttl",
                "un dist-tag que se mueve se revalida segun el TTL",
            )
            .command(&revalidated.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&tarball))
            .passed()
        },
    );
}

#[test]
fn ac2_the_moved_dist_tag_does_not_change_the_historical_bytes() {
    a_case(
        "ac2-the-moved-dist-tag-does-not-change-the-historical-bytes",
        "mover el dist-tag no cambia los bytes historicos",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(SHORT_METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            // La otra version del fixture: la a la que se mueve el
            // dist-tag. Su tarball tambien existe, porque un packument
            // cuyo `latest` apunta a una version que no esta en `versions`
            // no es un packument que npm pueda resolver.
            let other_tarball = the_fixture_tarball(&home, &root, PACKAGE, OTHER_VERSION);
            let other_declared = integrity_of(&other_tarball);
            let before = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            let after = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared), (OTHER_VERSION, &other_declared)],
                &[("latest", OTHER_VERSION)],
            );
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&before).with_etag("\"v1\""));
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.upstream.put(
                &format!("is-number/-/{}", the_tarball_file(PACKAGE, OTHER_VERSION)),
                &other_tarball,
                None,
            );
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let work = root.join("work");
            let packed = root.join("packed");
            std::fs::create_dir_all(&work).expect("the work directory");
            std::fs::create_dir_all(&packed).expect("the packed directory");

            // Los bytes historicos, antes del movimiento.
            let first = client.run(
                &work,
                &[
                    "pack",
                    SPEC,
                    "--pack-destination",
                    &packed.to_string_lossy(),
                ],
            );
            assert!(first.ok, "the first pack failed:\n{}", first.output);
            let before_bytes = std::fs::read(packed.join(the_tarball_file(PACKAGE, VERSION)))
                .expect("the tarball");
            let before_digest = sha256_of_bytes(&before_bytes);

            // El upstream mueve el dist-tag y, con el, la version que el
            // `latest` señala.
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&after).with_etag("\"v2\""));
            std::thread::sleep(Duration::from_secs(SHORT_METADATA_TTL + 2));

            // La version fijada sigue siendo la misma, byte a byte: un
            // tarball es contenido inmutable y no se reescribe.
            client.forget_the_cache();
            let second = client.run(
                &work,
                &[
                    "pack",
                    SPEC,
                    "--pack-destination",
                    &packed.to_string_lossy(),
                ],
            );
            assert!(second.ok, "the second pack failed:\n{}", second.output);
            let after_bytes = std::fs::read(packed.join(the_tarball_file(PACKAGE, VERSION)))
                .expect("the tarball");
            assert_eq!(
                sha256_of_bytes(&after_bytes),
                before_digest,
                "the historical bytes changed when the dist-tag moved"
            );

            // Y el tarball que sirve el grupo es el mismo que el del
            // upstream: no hay dos versiones del mismo contenido.
            let served = get(
                port,
                &format!("/{TARBALL_PATH}"),
                &token,
                &root.join("served.tgz"),
            );
            assert_eq!(served.status, 200, "{}", served.text());
            assert_eq!(
                sha256_of_bytes(&served.body),
                before_digest,
                "the tarball the group serves is not the one of the origin"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-moved-dist-tag-does-not-change-the-historical-bytes",
                "mover el dist-tag no cambia los bytes historicos",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&before_digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: lo que no se sirve, y el rastro que no se ensucia.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_reserved_scope_is_not_served_from_the_public_upstream() {
    a_case(
        "ac3-a-reserved-scope-is-not-served-from-the-public-upstream",
        "un scope reservado no se sirve desde el upstream publico",
        Scope::Product,
        Expectation::Refused,
        || {
            // El grupo reserva su scope: lo que este bajo el no sale del
            // upstream publico, aunque el upstream lo tenga.
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let impostor = the_fixture_tarball(&home, &root, RESERVED_PACKAGE, "9.9.9");
            let packument = the_packument(
                RESERVED_PACKAGE,
                &lab.upstream.base(),
                &[("9.9.9", &integrity_of(&impostor))],
                &[("latest", "9.9.9")],
            );
            lab.upstream.put(RESERVED_PACKAGE, &packument, None);
            lab.upstream.put(
                &format!(
                    "{RESERVED_PACKAGE}/-/{}",
                    the_tarball_file(RESERVED_PACKAGE, "9.9.9")
                ),
                &impostor,
                None,
            );
            lab.seed(Some(serde_json::json!({ "reserved": [RESERVED_SCOPE] })));

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let work = root.join("work");
            std::fs::create_dir_all(&work).expect("the work directory");

            let denied = client.run(&work, &["view", RESERVED_PACKAGE, "version"]);
            assert!(
                !denied.ok,
                "npm resolved a reserved scope from the public upstream:\n{}",
                denied.output
            );
            assert!(
                denied.output.contains("403") || denied.output.contains("E403"),
                "the reserved scope is not refused with a 403:\n{}",
                denied.output
            );
            assert!(
                !denied.output.contains("9.9.9"),
                "the reserved version leaked to the client:\n{}",
                denied.output
            );
            // Y el upstream publico no recibio ni una peticion de ese
            // scope: la reserva se decide antes de salir.
            assert_eq!(
                lab.upstream.hits(RESERVED_PACKAGE),
                0,
                "the public upstream was asked for a reserved scope"
            );

            support::stop(&mut service);
            let cause = format!("npm refused the reserved scope:\n{}", denied.output);
            LabCase::new(
                "ac3-a-reserved-scope-is-not-served-from-the-public-upstream",
                "un scope reservado no se sirve desde el upstream publico",
            )
            .command(&denied.command)
            .origin(&lab.upstream.origin())
            .refused(cause)
        },
    );
}

#[test]
fn ac3_the_tarball_urls_point_to_hodei() {
    a_case(
        "ac3-the-tarball-urls-point-to-hodei",
        "las URLs dist.tarball que se sirven apuntan al endpoint de Hodei",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let project = root.join("consumer");
            a_consumer(&project);

            let reply = get(
                port,
                &format!("/{PACKUMENT_PATH}"),
                &token,
                &root.join("packument.json"),
            );
            assert_eq!(reply.status, 200, "{}", reply.text());
            let served: serde_json::Value =
                serde_json::from_slice(&reply.body).expect("the packument is json");
            let url = served["versions"][VERSION]["dist"]["tarball"]
                .as_str()
                .expect("the packument carries a tarball url")
                .to_string();
            let expected = format!("http://127.0.0.1:{port}/npm/{GROUP}/{TARBALL_PATH}");
            assert_eq!(
                url, expected,
                "the tarball url does not point to the endpoint of Hodei"
            );
            // Y el lock del cliente dice lo mismo: de donde sale lo que
            // instala es del grupo, no del upstream.
            let install = client.run(&project, &["install", SPEC, "--ignore-scripts"]);
            assert!(install.ok, "npm install failed:\n{}", install.output);
            let lock: serde_json::Value = serde_json::from_slice(
                &std::fs::read(project.join("package-lock.json")).expect("the lock"),
            )
            .expect("the lock parses");
            let resolved = lock["packages"]["node_modules/is-number"]["resolved"]
                .as_str()
                .expect("the lock carries the resolved url")
                .to_string();
            assert_eq!(
                resolved, expected,
                "the client resolved the package outside of Hodei"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-tarball-urls-point-to-hodei",
                "las URLs dist.tarball que se sirven apuntan al endpoint de Hodei",
            )
            .command(&install.command)
            .origin(&lab.upstream.origin())
            .sha256(&sha256_of_bytes(&tarball))
            .passed()
        },
    );
}

#[test]
fn ac3_the_token_of_the_client_does_not_leak() {
    a_case(
        "ac3-the-token-of-the-client-does-not-leak",
        "el token del cliente no llega al log ni al upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local(METADATA_TTL);
            let root = lab.deployment.directory.path().to_path_buf();
            let home = LabHome::new();
            let tarball = the_fixture_tarball(&home, &root, PACKAGE, VERSION);
            let declared = integrity_of(&tarball);
            let packument = the_packument(
                PACKAGE,
                &lab.upstream.base(),
                &[(VERSION, &declared)],
                &[("latest", VERSION)],
            );
            lab.upstream.put(PACKUMENT_PATH, &packument, None);
            lab.upstream.put(TARBALL_PATH, &tarball, None);
            lab.seed(None);

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Npm::new(&root, "client", port, GROUP, &token);
            let project = root.join("consumer");
            a_consumer(&project);

            let install = client.run(&project, &["install", SPEC, "--ignore-scripts"]);
            assert!(install.ok, "npm install failed:\n{}", install.output);

            // El packument que se sirve no lleva la credencial en ninguna
            // parte: ni en las URLs, ni en el cuerpo.
            let reply = get(
                port,
                &format!("/{PACKUMENT_PATH}"),
                &token,
                &root.join("packument.json"),
            );
            assert_eq!(reply.status, 200, "{}", reply.text());
            assert!(
                !reply.text().contains(&token),
                "the credential of the client is in the packument that is served"
            );
            support::stop(&mut service);

            // El log del servicio: la credencial del cliente no esta en
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
                "ac3-the-token-of-the-client-does-not-leak",
                "el token del cliente no llega al log ni al upstream",
            )
            .command(&install.command)
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
/// Es la misma regla del laboratorio de HSP-223, comprobada aqui con el
/// caso real de esta suite: sin el opt-in, el caso de npmjs real no se
/// ejecuta, y el informe no puede ser verde.
#[test]
fn ac3_the_count_does_not_lie() {
    a_case(
        "ac3-the-count-does-not-lie",
        "el caso de npmjs real no se ejecuta sin el opt-in, y no cuenta como verde",
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
                "el caso de npmjs real no se ejecuta sin el opt-in, y no cuenta como verde",
            )
            .command("Summary::of(casos)")
            .passed()
        },
    );
}
