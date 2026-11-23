//! HSP-226: PyPI real: Simple API, wheel y sdist con pip aislado.
//!
//! Lo que se demuestra aqui, contra el **servicio de verdad**:
//!
//! - `pip download` resuelve una version fijada por el grupo de Hodei, y
//!   el fichero que baja coincide por SHA256 con el que el indice declara;
//! - el indice que se sirve es **nuestro**: conserva la identidad del
//!   proyecto y sus enlaces apuntan a Hodei, no al upstream;
//! - el grupo no mezcla el indice de un miembro con el fichero de otro;
//! - lo que ya esta en la cache se resuelve con el upstream desconectado;
//! - los estados `yanked` y `requires-python` se declaran, y una metadata
//!   hostil no crea enlaces externos ni HTML sin escapar.
//!
//! Por defecto el camino **no usa la red**: el upstream es el del
//! laboratorio y la distribucion es un fixture. El caso contra
//! `pypi.org` es opt-in (`HODEI_LAB_REAL=1`), y sin el queda `NOT_RUN`, con
//! lo que el informe no es verde: un caso que no se ejecuta no es un caso
//! que pasa.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::lab::{
    Budget, Expectation, LabCase, LabHome, Outcome, REAL_MODE_VARIABLE, Scope, Summary, Upstream,
    describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// El proyecto bajo prueba: las coordenadas son las del publico.
const PROJECT: &str = "requests";

/// La version del proyecto bajo prueba: **fijada**, siempre.
const VERSION: &str = "2.32.3";

/// El otro proyecto del grupo: el que vive en el hosted.
const OTHER_PROJECT: &str = "labpkg";

/// La version del otro proyecto.
const OTHER_VERSION: &str = "1.0";

/// El repositorio **hosted** del grupo.
const HOSTED: &str = "pypi-hosted";

/// El repositorio **proxy** del grupo: el que habla con el upstream.
const PROXY: &str = "pypi-central";

/// El segundo proxy del grupo: el mismo proyecto con **otros** bytes.
const MIRROR: &str = "pypi-mirror";

/// El grupo bajo prueba.
const GROUP: &str = "pypi-all";

/// El indice publico de PyPI.
const REAL_UPSTREAM: &str = "https://pypi.org/simple/";

/// Los hosts que el modo real deja salir: el indice y su CDN.
const REAL_HOSTS: &str = "pypi.org,files.pythonhosted.org";

/// La frontera de las partes del cuerpo multipart.
const BOUNDARY: &str = "hodeipypiboundary";

/// Los casos que una corrida completa tiene que ejecutar.
///
/// El informe los declara, asi que una corrida que solo llego a alguno
/// dice cual le falto: los que no se ejecutan cuentan como `NOT_RUN`, y el
/// informe nunca es verde.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-pip-download-resolves-the-pinned-version-through-the-group",
        "pip resuelve la version fijada por el grupo y el SHA256 coincide",
    ),
    (
        "ac1-the-index-keeps-the-identity-and-the-internal-links",
        "el indice conserva la identidad y sus enlaces son de Hodei",
    ),
    (
        "ac1-the-real-pypi-serves-the-same-sha256",
        "contra pypi.org de verdad el SHA256 es el que declara el publico",
    ),
    (
        "ac2-the-group-does-not-mix-the-index-of-one-member-with-the-file-of-another",
        "el grupo no mezcla el indice de un miembro con el fichero de otro",
    ),
    (
        "ac2-the-project-resolves-offline-from-the-cache",
        "lo que ya esta en la cache se resuelve con el upstream caido",
    ),
    (
        "ac3-the-yanked-and-the-requires-python-are-declared",
        "los estados yanked y requires-python se declaran en el indice",
    ),
    (
        "ac3-a-hostile-metadata-does-not-create-external-links-nor-unescaped-html",
        "una metadata hostil no crea enlaces externos ni HTML sin escapar",
    ),
    (
        "ac3-the-count-does-not-lie",
        "el caso de pypi.org real no se ejecuta sin el opt-in, y no cuenta como verde",
    ),
];

/// El titulo y el disclaimer del informe de esta suite.
///
/// El disclaimer del laboratorio (HSP-223) dice que no se ha hablado con
/// `pip`, y aqui si: el disclaimer es de la suite, no del modulo.
const THE_TITLE: &str = "Laboratorio de PyPI — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y la distribucion es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el paquete publico.** En el camino sin red la distribucion es un fixture con las coordenadas de `requests@2.32.3`, no los bytes de PyPI: lo que se demuestra es el camino, no la integridad del publico. El camino real si compara el SHA256 con el que declara PyPI.",
    "- **Ni la distribucion de pip.** Se mide `pip download` contra un indice de Hodei: no se mide la resolucion de dependencias de pip, ni sus scripts, ni la instalacion.",
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

/// El laboratorio de PyPI: un despliegue, un upstream que cuenta, un
/// segundo upstream con los mismos bytes cambiados y el grupo bajo prueba.
struct Lab {
    deployment: Deployment,
    /// El upstream local del proxy principal.
    upstream: Upstream,
    /// El upstream local del espejo: el mismo proyecto, otros bytes.
    mirror: Upstream,
    /// La URL del upstream del proxy principal.
    upstream_url: String,
    /// La URL del upstream del espejo.
    mirror_url: String,
}

impl Lab {
    /// Un laboratorio con dos upstreams **locales** que cuentan.
    fn local() -> Self {
        let upstream = Upstream::start();
        let mirror = Upstream::start();
        let upstream_url = upstream.base();
        let mirror_url = mirror.base();
        Self {
            deployment: Deployment::new(),
            upstream,
            mirror,
            upstream_url,
            mirror_url,
        }
    }

    /// Un laboratorio con el upstream **real**: el indice publico de PyPI.
    ///
    /// No hay contador local: a PyPI no se le puede contar. Lo que se mide
    /// en el modo real es el SHA256 y el hecho de que la resolucion
    /// funciona con la salida a PyPI **cerrada**.
    fn real() -> Self {
        let upstream = Upstream::start();
        let mirror = Upstream::start();
        Self {
            deployment: Deployment::new(),
            upstream,
            mirror,
            // El origen del indice publico: el proxy anade el `simple/`.
            upstream_url: "https://pypi.org/".to_string(),
            mirror_url: "https://pypi.org/".to_string(),
        }
    }

    /// Siembra el hosted, los dos proxies y el grupo.
    fn seed(&self) {
        self.deployment.seed_repositories(&[
            serde_json::json!({ "key": HOSTED, "format": "pypi" }),
            self.the_proxy(PROXY, "central", &self.upstream_url),
            self.the_proxy(MIRROR, "mirror", &self.mirror_url),
        ]);
        self.deployment.seed_groups_raw(&[serde_json::json!({
            "key": GROUP,
            "members": [HOSTED, PROXY, MIRROR],
            "format": "pypi",
        })]);
    }

    /// Un proxy que habla con un upstream.
    fn the_proxy(&self, key: &str, id: &str, url: &str) -> Value {
        serde_json::json!({
            "key": key,
            "format": "pypi",
            "proxy": {
                "upstream": {
                    "id": id,
                    "url": format!("{url}simple/"),
                    "scopes": ["/"],
                    "max_bytes": 4 * 1024 * 1024,
                },
                "cache": { "ttl": { "content_secs": null, "metadata_secs": 300 } },
            },
        })
    }

    /// Arranca el servicio con la salida que el caso necesita.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// La salida que deja pasar al upstream local del laboratorio.
    fn the_local_egress(&self) -> String {
        format!(
            "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
            self.upstream.port(),
            self.mirror.port()
        )
    }
}

// ---------------------------------------------------------------------------
// Los fixtures.
// ---------------------------------------------------------------------------

/// El nombre del fichero de una distribucion, como lo escribe PyPI.
fn the_file_name(project: &str, version: &str) -> String {
    format!("{project}-{version}-py3-none-any.whl")
}

/// La pagina de un proyecto en la Simple API, tal y como la declara el
/// upstream.
///
/// Las URLs `url` apuntan al **upstream**: si el proxy no las reescribe, el
/// cliente se ira alla y la prueba lo dira.
fn the_page(project: &str, packages: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let mut entries = Vec::new();
    for (filename, sha256) in files {
        entries.push(json!({
            "filename": filename,
            "url": format!("{packages}{filename}"),
            "hashes": { "sha256": sha256 },
            "requires-python": ">=3.8",
            "yanked": false,
        }));
    }
    serde_json::to_vec(&json!({
        "meta": { "api-version": "1.0" },
        "name": project,
        "files": entries,
    }))
    .expect("the page is json")
}

/// El SHA256 de unos bytes, en hexadecimal.
fn the_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Una subida al hosted, como la manda `twine`.
fn upload(dir: &Path, port: u16, repository: &str, token: &str, body: Vec<u8>) -> support::Answer {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sent = dir.join(format!("upload-{n}.bin"));
    let into = dir.join(format!("upload-answer-{n}.bin"));
    std::fs::write(&sent, body).expect("the upload body is written");
    Curl::call(
        port,
        "POST",
        &format!("/pypi/{repository}/"),
        token,
        Some(&sent),
        Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
        Some(&into),
    )
    .run()
}

/// El cuerpo de una subida, como lo manda `twine`.
fn the_upload(project: &str, version: &str, filename: &str, file: &[u8], sha256: &str) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", project),
        ("version", version),
        ("metadata_version", "2.1"),
        ("requires_python", ">=3.9"),
        ("sha256_digest", sha256),
    ] {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\n\
                 Content-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\n\
             Content-Disposition: form-data; name=\"content\"; filename=\"{filename}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

// ---------------------------------------------------------------------------
// El cliente pip.
// ---------------------------------------------------------------------------

/// La corrida de un cliente.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// Un cliente `pip` aislado: su HOME, su cache y su indice.
///
/// El indice apunta al **grupo** de Hodei, de modo que el cliente no tiene
/// un camino propio a PyPI: lo que descarga sale del grupo o no sale. La
/// credencial viaja en la URL, que es como la lleva `pip`.
struct Pip {
    /// El HOME del cliente: no es el de quien ejecuta la prueba.
    home: LabHome,
    /// El directorio del cliente.
    root: PathBuf,
    /// El indice al que habla el cliente.
    index: String,
}

impl Pip {
    /// Un cliente que habla con el grupo indicado.
    fn new(root: &Path, name: &str, port: u16, group: &str, token: &str) -> Self {
        let home = LabHome::new();
        let directory = root.join(name);
        std::fs::create_dir_all(&directory).expect("the directory of the client");
        // La credencial va en la password: la superficie de PyPI acepta la
        // basica, cuya contrasena **es** la credencial de la plataforma.
        let index = format!("http://hodei:{token}@127.0.0.1:{port}/pypi/{group}/simple/");
        Self {
            home,
            root: directory,
            index,
        }
    }

    /// Corre `pip` con el HOME aislado y sin configuracion de por medio.
    fn run(&self, work: &Path, arguments: &[&str]) -> Run {
        let mut command = Command::new("pip");
        command
            .args(["download", "--no-deps", "--no-cache-dir", "--isolated"])
            .args(["--index-url", &self.index])
            .arg("--dest")
            .arg(work)
            .args(arguments)
            .env("HOME", self.home.path())
            .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
            .env("PIP_NO_INPUT", "1")
            .current_dir(&self.root);
        let shown = format!("pip download {}", arguments.join(" "));
        let output = command.output().expect("pip runs");
        Run {
            ok: output.status.success(),
            output: format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            command: shown,
        }
    }
}

/// Una peticion al servicio, con su respuesta.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    headers: &[(&str, &str)],
    body: Option<&Path>,
) -> support::Answer {
    let into = dir.join("answer.bin");
    let _ = std::fs::remove_file(&into);
    Curl::call_with_headers(port, method, path, token, body, headers, Some(&into)).run()
}

/// El JSON de una respuesta.
fn json_of(answer: &support::Answer) -> Value {
    serde_json::from_slice(&answer.body).expect("the answer is json")
}

/// El texto de una respuesta.
fn text_of(answer: &support::Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

// ---------------------------------------------------------------------------
// El resumen del laboratorio.
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
// AC1: pip resuelve por el grupo, y el SHA256 es el del origen.
// ---------------------------------------------------------------------------

#[test]
fn ac1_pip_download_resolves_the_pinned_version_through_the_group() {
    a_case(
        "ac1-pip-download-resolves-the-pinned-version-through-the-group",
        "pip resuelve la version fijada por el grupo y el SHA256 coincide",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            let filename = the_file_name(PROJECT, VERSION);
            let page = the_page(
                PROJECT,
                &format!("{}/packages/", lab.upstream.base()),
                &[(filename.as_str(), digest.as_str())],
            );
            lab.upstream.put("simple/requests", &page, None);
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let root = lab.deployment.directory.path().to_path_buf();
            let client = Pip::new(&root, "downloader", port, GROUP, &token);
            let work = root.join("downloads");
            std::fs::create_dir_all(&work).expect("the download directory");

            let run = client.run(&work, &[&format!("{PROJECT}=={VERSION}")]);
            assert!(run.ok, "pip download failed:\n{}", run.output);

            // Lo que baj�� es el fichero del upstream, byte a byte.
            let downloaded = work.join(&filename);
            let bytes = std::fs::read(&downloaded).expect("the downloaded file");
            assert_eq!(
                the_sha256(&bytes),
                digest,
                "the file that came down is not the one the index declared"
            );
            assert!(
                lab.upstream.hits(&format!("packages/{filename}")) > 0,
                "the upstream was never asked for the file"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-pip-download-resolves-the-pinned-version-through-the-group",
                "pip resuelve la version fijada por el grupo y el SHA256 coincide",
            )
            .command(&run.command)
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac1_the_index_keeps_the_identity_and_the_internal_links() {
    a_case(
        "ac1-the-index-keeps-the-identity-and-the-internal-links",
        "el indice conserva la identidad y sus enlaces son de Hodei",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            let filename = the_file_name(PROJECT, VERSION);
            let page = the_page(
                PROJECT,
                &format!("{}/packages/", lab.upstream.base()),
                &[(filename.as_str(), digest.as_str())],
            );
            lab.upstream.put("simple/requests", &page, None);
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            // El indice en JSON: la identidad del proyecto y sus enlaces.
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[(
                    "accept",
                    "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
                )],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let document = json_of(&answer);
            assert_eq!(
                document["name"].as_str(),
                Some(PROJECT),
                "the index does not keep the identity of the project: {document}"
            );
            let url = document["files"][0]["url"]
                .as_str()
                .expect("the url of the file");
            assert!(
                url.contains(&format!("/pypi/{GROUP}/files/{PROJECT}/")),
                "the link does not point to Hodei: {url}"
            );
            assert!(
                !url.contains(&lab.upstream.base()),
                "the link points to the upstream: {url}"
            );
            assert_eq!(
                document["files"][0]["hashes"]["sha256"].as_str(),
                Some(digest.as_str()),
                "the index does not declare the hash of the file: {document}"
            );

            // Y el indice en HTML: los mismos enlaces, escapados.
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[("accept", "text/html")],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let html = text_of(&answer);
            assert!(
                html.contains(&format!("/pypi/{GROUP}/files/{PROJECT}/")),
                "the HTML does not point to Hodei: {html}"
            );
            assert!(
                !html.contains(&lab.upstream.base()),
                "the HTML points to the upstream: {html}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-the-index-keeps-the-identity-and-the-internal-links",
                "el indice conserva la identidad y sus enlaces son de Hodei",
            )
            .command(&format!("GET /pypi/{GROUP}/simple/{PROJECT}/"))
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac1_the_real_pypi_serves_the_same_sha256() {
    a_case(
        "ac1-the-real-pypi-serves-the-same-sha256",
        "contra pypi.org de verdad el SHA256 es el que declara el publico",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let case = LabCase::new(
                "ac1-the-real-pypi-serves-the-same-sha256",
                "contra pypi.org de verdad el SHA256 es el que declara el publico",
            )
            .command(&format!(
                "pip download {PROJECT}=={VERSION} (registro: pypi.org)"
            ));
            if !real_mode_enabled() {
                // Sin el opt-in el caso no se ejecuta, y un caso que no
                // se ejecuta no es un caso verde.
                return case.not_run(format!(
                    "the real mode is opt-in: set {REAL_MODE_VARIABLE}=1 to run it"
                ));
            }

            let lab = Lab::real();
            let root = lab.deployment.directory.path().to_path_buf();
            let budget = Budget::from_environment();
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", REAL_HOSTS)]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let client = Pip::new(&root, "real", port, GROUP, &token);
            let work = root.join("downloads");
            std::fs::create_dir_all(&work).expect("the download directory");

            let started = std::time::Instant::now();
            let run = client.run(&work, &[&format!("{PROJECT}=={VERSION}")]);
            let elapsed = started.elapsed();
            assert!(run.ok, "pip download failed:\n{}", run.output);

            // El SHA256 que declara el publico, leido del indice publico.
            let filename = filename_of_the_download(&work, PROJECT, VERSION);
            let declared = the_public_sha256(&filename);
            let downloaded = work.join(&filename);
            let bytes = std::fs::read(&downloaded).expect("the downloaded file");
            assert_eq!(
                the_sha256(&bytes),
                declared,
                "the file that came down is not the one pypi.org declares"
            );

            let size = std::fs::metadata(&downloaded)
                .map(|meta| meta.len())
                .unwrap_or(0);
            support::stop(&mut service);
            let case = case
                .command(&run.command)
                .origin(REAL_UPSTREAM)
                .sha256(&declared);
            match budget.permits(size, elapsed) {
                Ok(()) => case.passed(),
                Err(cause) => case.failed(cause),
            }
        },
    );
}

/// El nombre del fichero que pip dejo en el directorio.
fn filename_of_the_download(work: &Path, project: &str, version: &str) -> String {
    let mut found: Vec<String> = std::fs::read_dir(work)
        .expect("the download directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(project) && name.starts_with(version))
        .collect();
    found.sort();
    found
        .first()
        .cloned()
        .unwrap_or_else(|| the_file_name(project, version))
}

/// El SHA256 que declara el indice publico de PyPI.
///
/// Se lee el JSON de `pypi.org` con `curl`: lo que declara el publico es
/// la referencia, y el proxy no puede cambiarla.
fn the_public_sha256(filename: &str) -> String {
    let output = Command::new("curl")
        .args(["--silent", "--show-error", "--fail"])
        .arg(format!("https://pypi.org/pypi/{PROJECT}/{VERSION}/json"))
        .output()
        .expect("curl runs");
    assert!(
        output.status.success(),
        "the public index could not be read:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value = serde_json::from_slice(&output.stdout).expect("the public index is json");
    for entry in document["urls"]
        .as_array()
        .expect("the urls of the public index")
    {
        if entry["filename"].as_str() == Some(filename) {
            return entry["digests"]["sha256"]
                .as_str()
                .expect("the sha256 of the public index")
                .to_ascii_lowercase();
        }
    }
    panic!("the public index does not declare {filename}");
}

// ---------------------------------------------------------------------------
// AC2: el grupo no mezcla, y lo cacheado se resuelve sin upstream.
// ---------------------------------------------------------------------------

#[test]
fn ac2_the_group_does_not_mix_the_index_of_one_member_with_the_file_of_another() {
    a_case(
        "ac2-the-group-does-not-mix-the-index-of-one-member-with-the-file-of-another",
        "el grupo no mezcla el indice de un miembro con el fichero de otro",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let filename = the_file_name(PROJECT, VERSION);

            // El proxy principal: el upstream con **estos** bytes.
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            lab.upstream.put(
                "simple/requests",
                &the_page(
                    PROJECT,
                    &format!("{}/packages/", lab.upstream.base()),
                    &[(filename.as_str(), digest.as_str())],
                ),
                None,
            );
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);

            // El espejo: el mismo proyecto, **otros** bytes. Si el grupo
            // mezclara el indice de uno con el fichero de otro, el
            // SHA256 del indice no casaria con lo que se sirve.
            let other = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let other_digest = the_sha256(&other);
            lab.mirror.put(
                "simple/requests",
                &the_page(
                    PROJECT,
                    &format!("{}/packages/", lab.mirror.base()),
                    &[(filename.as_str(), other_digest.as_str())],
                ),
                None,
            );
            lab.mirror
                .put(&format!("packages/{filename}"), &other, None);

            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            // El miembro hosted publica su propio proyecto: el grupo es
            // hosted **y** proxy, y cada miembro responde de lo suyo.
            let hosted_wheel = support::a_wheel(OTHER_PROJECT, OTHER_VERSION, Some(">=3.9"));
            let hosted_digest = the_sha256(&hosted_wheel);
            let hosted_file = the_file_name(OTHER_PROJECT, OTHER_VERSION);
            let published = upload(
                dir,
                port,
                HOSTED,
                &token,
                the_upload(
                    OTHER_PROJECT,
                    OTHER_VERSION,
                    &hosted_file,
                    &hosted_wheel,
                    &hosted_digest,
                ),
            );
            assert!(
                matches!(published.status, 200 | 201),
                "the hosted member did not publish: {}",
                text_of(&published)
            );

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[(
                    "accept",
                    "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
                )],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let document = json_of(&answer);
            let files = document["files"]
                .as_array()
                .expect("the files of the index");
            assert_eq!(
                files.len(),
                1,
                "the group announced the file twice: {document}"
            );

            // El indice declara un hash, y lo que se sirve es ese.
            let declared = files[0]["hashes"]["sha256"]
                .as_str()
                .expect("the declared hash")
                .to_string();
            let url = files[0]["url"].as_str().expect("the url of the file");
            let path = url
                .split_once("127.0.0.1:")
                .and_then(|(_, rest)| rest.split_once('/').map(|(_, path)| path.to_string()))
                .expect("the path of the file");
            let served = call(dir, port, "GET", &format!("/{path}"), &token, &[], None);
            assert_eq!(served.status, 200, "{}", text_of(&served));
            assert_eq!(
                the_sha256(&served.body),
                declared,
                "the index and the file come from different members: the index says \
                 {declared} and the file is {}",
                the_sha256(&served.body)
            );

            // Y el proyecto del hosted se sirve desde el hosted: el grupo
            // une a los dos miembros sin mezclarlos.
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{OTHER_PROJECT}/"),
                &token,
                &[(
                    "accept",
                    "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
                )],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let document = json_of(&answer);
            assert_eq!(
                document["files"][0]["hashes"]["sha256"].as_str(),
                Some(hosted_digest.as_str()),
                "the hosted member does not serve its own file: {document}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-group-does-not-mix-the-index-of-one-member-with-the-file-of-another",
                "el grupo no mezcla el indice de un miembro con el fichero de otro",
            )
            .command(&format!("GET /pypi/{GROUP}/simple/{PROJECT}/"))
            .origin(&lab.upstream.origin())
            .sha256(&declared)
            .passed()
        },
    );
}

#[test]
fn ac2_the_project_resolves_offline_from_the_cache() {
    a_case(
        "ac2-the-project-resolves-offline-from-the-cache",
        "lo que ya esta en la cache se resuelve con el upstream caido",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            let filename = the_file_name(PROJECT, VERSION);
            lab.upstream.put(
                "simple/requests",
                &the_page(
                    PROJECT,
                    &format!("{}/packages/", lab.upstream.base()),
                    &[(filename.as_str(), digest.as_str())],
                ),
                None,
            );
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let root = lab.deployment.directory.path().to_path_buf();
            let client = Pip::new(&root, "offline", port, GROUP, &token);
            let work = root.join("downloads");
            std::fs::create_dir_all(&work).expect("the download directory");

            // La primera vez: el upstream contesta.
            let first = client.run(&work, &[&format!("{PROJECT}=={VERSION}")]);
            assert!(first.ok, "the first download failed:\n{}", first.output);
            let asked = lab.upstream.total();

            // Y luego el upstream se va: lo que ya esta en la cache se
            // resuelve sin el.
            lab.upstream.disconnect();
            let second = client.run(&work, &[&format!("{PROJECT}=={VERSION}")]);
            assert!(
                second.ok,
                "the download failed with the upstream gone:\n{}",
                second.output
            );
            assert_eq!(
                lab.upstream.total(),
                asked,
                "the upstream was asked again with the cache Warm"
            );
            let bytes = std::fs::read(work.join(&filename)).expect("the downloaded file");
            assert_eq!(
                the_sha256(&bytes),
                digest,
                "the file that came from the cache is not the same one"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-project-resolves-offline-from-the-cache",
                "lo que ya esta en la cache se resuelve con el upstream caido",
            )
            .command(&second.command)
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: los estados se declaran, y una metadata hostil no se escapa.
// ---------------------------------------------------------------------------

#[test]
fn ac3_the_yanked_and_the_requires_python_are_declared() {
    a_case(
        "ac3-the-yanked-and-the-requires-python-are-declared",
        "los estados yanked y requires-python se declaran en el indice",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let filename = the_file_name(PROJECT, VERSION);
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            // El upstream declara la version como retirada, y con el motivo.
            let page = json!({
                "meta": { "api-version": "1.0" },
                "name": PROJECT,
                "files": [{
                    "filename": filename,
                    "url": format!("{}/packages/{filename}", lab.upstream.base()),
                    "hashes": { "sha256": digest },
                    "requires-python": ">=3.8",
                    "yanked": "the version was withdrawn",
                }],
            });
            lab.upstream.put(
                "simple/requests",
                &serde_json::to_vec(&page).expect("json"),
                None,
            );
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[(
                    "accept",
                    "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
                )],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let document = json_of(&answer);
            assert_eq!(
                document["files"][0]["yanked"].as_str(),
                Some("the version was withdrawn"),
                "the index does not declare the yank: {document}"
            );
            assert_eq!(
                document["files"][0]["requires-python"].as_str(),
                Some(">=3.8"),
                "the index does not declare the python it needs: {document}"
            );

            // Y en HTML, el motivo va escapado.
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[("accept", "text/html")],
                None,
            );
            let html = text_of(&answer);
            assert!(
                html.contains("data-yanked="),
                "the HTML does not declare the yank: {html}"
            );
            assert!(
                html.contains("data-requires-python="),
                "the HTML does not declare the python it needs: {html}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-yanked-and-the-requires-python-are-declared",
                "los estados yanked y requires-python se declaran en el indice",
            )
            .command(&format!("GET /pypi/{GROUP}/simple/{PROJECT}/"))
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac3_a_hostile_metadata_does_not_create_external_links_nor_unescaped_html() {
    a_case(
        "ac3-a-hostile-metadata-does-not-create-external-links-nor-unescaped-html",
        "una metadata hostil no crea enlaces externos ni HTML sin escapar",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            let filename = the_file_name(PROJECT, VERSION);
            let wheel = support::a_wheel(PROJECT, VERSION, Some(">=3.8"));
            let digest = the_sha256(&wheel);
            // El upstream publica un nombre que no es una distribucion, y
            // un `requires-python` que quiere cerrar la etiqueta del HTML.
            let hostile_name = "\"><script>alert(1)</script>-1.0-py3-none-any.whl";
            let hostile_python = "\"><script>alert(1)</script>";
            let page = json!({
                "meta": { "api-version": "1.0" },
                "name": PROJECT,
                "files": [{
                    "filename": filename,
                    "url": format!("{}/packages/{filename}", lab.upstream.base()),
                    "hashes": { "sha256": digest },
                    "requires-python": hostile_python,
                    "yanked": false,
                }, {
                    "filename": hostile_name,
                    "url": format!("{}/packages/evil", lab.upstream.base()),
                    "hashes": { "sha256": digest },
                    "requires-python": ">=3.8",
                    "yanked": false,
                }],
            });
            lab.upstream.put(
                "simple/requests",
                &serde_json::to_vec(&page).expect("json"),
                None,
            );
            lab.upstream
                .put(&format!("packages/{filename}"), &wheel, None);
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[("accept", "text/html")],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let html = text_of(&answer);
            assert!(
                !html.contains("<script>"),
                "the upstream wrote HTML: {html}"
            );
            // El valor hostil va escapado, que es lo unico que se puede
            // hacer con el en un HTML.
            assert!(
                html.contains("&lt;script&gt;"),
                "the hostile value was not escaped: {html}"
            );
            assert!(
                !html.contains("alert(1)</script>"),
                "the hostile value closed the tag: {html}"
            );
            // Y el nombre que no es una distribucion no se anuncia: un
            // enlace que no se puede descargar es un enlace roto.
            assert!(
                !html.contains("alert(1)</script>-1.0"),
                "the upstream published a name that is not a distribution: {html}"
            );
            assert!(
                !html.contains("evil"),
                "the upstream published a link of its own: {html}"
            );
            assert!(
                !html.contains(&lab.upstream.base()),
                "the index points to the upstream: {html}"
            );

            // El indice en JSON declara el valor tal cual, y solo el
            // fichero que es una distribucion.
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/pypi/{GROUP}/simple/{PROJECT}/"),
                &token,
                &[(
                    "accept",
                    "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
                )],
                None,
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            let document = json_of(&answer);
            let files = document["files"]
                .as_array()
                .expect("the files of the index");
            assert_eq!(
                files.len(),
                1,
                "the index announced a file that is not a distribution: {document}"
            );
            assert_eq!(
                files[0]["filename"].as_str(),
                Some(filename.as_str()),
                "the index announced another file: {document}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-hostile-metadata-does-not-create-external-links-nor-unescaped-html",
                "una metadata hostil no crea enlaces externos ni HTML sin escapar",
            )
            .command(&format!("GET /pypi/{GROUP}/simple/{PROJECT}/"))
            .origin(&lab.upstream.origin())
            .sha256(&digest)
            .passed()
        },
    );
}

#[test]
fn ac3_the_count_does_not_lie() {
    a_case(
        "ac3-the-count-does-not-lie",
        "el caso de pypi.org real no se ejecuta sin el opt-in, y no cuenta como verde",
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
                "el caso de pypi.org real no se ejecuta sin el opt-in, y no cuenta como verde",
            )
            .command("Summary::of(casos)")
            .passed()
        },
    );
}
