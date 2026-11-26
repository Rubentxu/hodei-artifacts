//! HSP-226: real PyPI: Simple API, wheel and sdist with an isolated pip.
//!
//! What is demonstrated here, against the **real service**:
//!
//! - `pip download` resolves a version pinned by Hodei's group, and
//!   the file it downloads matches by SHA256 the one the index declares;
//! - the index that is served is **ours**: it keeps the project's identity
//!   and its links point to Hodei, not to the upstream;
//! - the group does not mix one member's index with another's file;
//! - what is already in the cache resolves with the upstream disconnected;
//! - the `yanked` and `requires-python` states are declared, and hostile
//!   metadata creates neither external links nor unescaped HTML.
//!
//! By default the path **does not use the network**: the upstream is the lab's
//! and the distribution is a fixture. The case against
//! `pypi.org` is opt-in (`HODEI_LAB_REAL=1`), and without it it stays `NOT_RUN`, so
//! the report is not green: a case that does not run is not a case
//! that passes.

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

/// The project under test: the coordinates are the public ones.
const PROJECT: &str = "requests";

/// The version of the project under test: **pinned**, always.
const VERSION: &str = "2.32.3";

/// The other project in the group: the one that lives in the hosted.
const OTHER_PROJECT: &str = "labpkg";

/// The version of the other project.
const OTHER_VERSION: &str = "1.0";

/// The group's **hosted** repository.
const HOSTED: &str = "pypi-hosted";

/// The group's **proxy** repository: the one that talks to the upstream.
const PROXY: &str = "pypi-central";

/// The group's second proxy: the same project with **other** bytes.
const MIRROR: &str = "pypi-mirror";

/// The group under test.
const GROUP: &str = "pypi-all";

/// The public PyPI index.
const REAL_UPSTREAM: &str = "https://pypi.org/simple/";

/// The hosts that the real mode lets out: the index and its CDN.
const REAL_HOSTS: &str = "pypi.org,files.pythonhosted.org";

/// The boundary of the multipart body parts.
const BOUNDARY: &str = "hodeipypiboundary";

/// The cases that a complete run has to execute.
///
/// The report declares them, so a run that only reached some of them
/// says which it missed: those that are not executed count as `NOT_RUN`, and the
/// report is never green.
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

/// The title and the disclaimer of this suite's report.
///
/// The lab disclaimer (HSP-223) says that `pip` has not been spoken to,
/// and here it has: the disclaimer belongs to the suite, not to the module.
const THE_TITLE: &str = "Laboratorio de PyPI — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y la distribucion es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el paquete publico.** En el camino sin red la distribucion es un fixture con las coordenadas de `requests@2.32.3`, no los bytes de PyPI: lo que se demuestra es el camino, no la integridad del publico. El camino real si compara el SHA256 con el que declara PyPI.",
    "- **Ni la distribucion de pip.** Se mide `pip download` contra un indice de Hodei: no se mide la resolucion de dependencias de pip, ni sus scripts, ni la instalacion.",
    "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés —el recuento— y no el producto. Los dos recuentos van aparte.",
];

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The lab.
// ---------------------------------------------------------------------------

/// The PyPI lab: a deployment, an upstream that counts, a
/// second upstream with the same bytes changed and the group under test.
struct Lab {
    deployment: Deployment,
    /// The local upstream of the main proxy.
    upstream: Upstream,
    /// The local upstream of the mirror: the same project, other bytes.
    mirror: Upstream,
    /// The URL of the main proxy's upstream.
    upstream_url: String,
    /// The URL of the mirror's upstream.
    mirror_url: String,
}

impl Lab {
    /// A lab with two **local** upstreams that count.
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

    /// A lab with the **real** upstream: the public PyPI index.
    ///
    /// There is no local counter: PyPI cannot be counted. What is measured
    /// in real mode is the SHA256 and the fact that resolution
    /// works with the exit to PyPI **closed**.
    fn real() -> Self {
        let upstream = Upstream::start();
        let mirror = Upstream::start();
        Self {
            deployment: Deployment::new(),
            upstream,
            mirror,
            // The origin of the public index: the proxy adds the `simple/`.
            upstream_url: "https://pypi.org/".to_string(),
            mirror_url: "https://pypi.org/".to_string(),
        }
    }

    /// Seeds the hosted, the two proxies and the group.
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

    /// A proxy that talks to an upstream.
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

    /// Starts the service with the egress that the case needs.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// The egress that lets the lab's local upstream through.
    fn the_local_egress(&self) -> String {
        format!(
            "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
            self.upstream.port(),
            self.mirror.port()
        )
    }
}

// ---------------------------------------------------------------------------
// The fixtures.
// ---------------------------------------------------------------------------

/// The file name of a distribution, as PyPI writes it.
fn the_file_name(project: &str, version: &str) -> String {
    format!("{project}-{version}-py3-none-any.whl")
}

/// A project's page in the Simple API, exactly as the
/// upstream declares it.
///
/// The `url` URLs point to the **upstream**: if the proxy does not rewrite them, the
/// client will go there and the test will say so.
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

/// The SHA256 of some bytes, in hexadecimal.
fn the_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// An upload to the hosted, as `twine` sends it.
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

/// The body of an upload, as `twine` sends it.
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
// The pip client.
// ---------------------------------------------------------------------------

/// The run of a client.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// An isolated `pip` client: its HOME, its cache and its index.
///
/// The index points to Hodei's **group**, so the client has no
/// path of its own to PyPI: what it downloads comes out of the group or not at all. The
/// credential travels in the URL, which is how `pip` carries it.
struct Pip {
    /// The client's HOME: not that of whoever runs the test.
    home: LabHome,
    /// The client's directory.
    root: PathBuf,
    /// The index the client talks to.
    index: String,
}

impl Pip {
    /// A client that talks to the indicated group.
    fn new(root: &Path, name: &str, port: u16, group: &str, token: &str) -> Self {
        let home = LabHome::new();
        let directory = root.join(name);
        std::fs::create_dir_all(&directory).expect("the directory of the client");
        // The credential goes in the password: the PyPI surface accepts
        // basic auth, whose password **is** the platform credential.
        let index = format!("http://hodei:{token}@127.0.0.1:{port}/pypi/{group}/simple/");
        Self {
            home,
            root: directory,
            index,
        }
    }

    /// Runs `pip` with the isolated HOME and without any configuration in between.
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

/// A request to the service, with its response.
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

/// The JSON of a response.
fn json_of(answer: &support::Answer) -> Value {
    serde_json::from_slice(&answer.body).expect("the answer is json")
}

/// The text of a response.
fn text_of(answer: &support::Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

// ---------------------------------------------------------------------------
// The lab summary.
// ---------------------------------------------------------------------------

/// The summary of a case, with the case's id and title.
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
// AC1: pip resolves through the group, and the SHA256 is the origin's.
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

            // What was downloaded is the upstream's file, byte for byte.
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

            // The index in JSON: the project's identity and its links.
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

            // And the index in HTML: the same links, escaped.
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
                // Without the opt-in the case is not executed, and a case that
                // is not executed is not a green case.
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

            // The SHA256 that the public index declares, read from the public index.
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

/// The name of the file that pip left in the directory.
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

/// The SHA256 that PyPI's public index declares.
///
/// The JSON of `pypi.org` is read with `curl`: what the public declares is
/// the reference, and the proxy cannot change it.
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
// AC2: the group does not mix, and what is cached resolves without upstream.
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

            // The main proxy: the upstream with **these** bytes.
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

            // The mirror: the same project, **other** bytes. If the group
            // mixed one member's index with another's file, the
            // index's SHA256 would not match what is served.
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

            // The hosted member publishes its own project: the group is
            // hosted **and** proxy, and each member answers for its own.
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

            // The index declares a hash, and what is served is that one.
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

            // And the hosted's project is served from the hosted: the group
            // joins the two members without mixing them.
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

            // The first time: the upstream answers.
            let first = client.run(&work, &[&format!("{PROJECT}=={VERSION}")]);
            assert!(first.ok, "the first download failed:\n{}", first.output);
            let asked = lab.upstream.total();

            // And then the upstream goes away: what is already in the cache
            // resolves without it.
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
// AC3: the states are declared, and hostile metadata does not escape.
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
            // The upstream declares the version as withdrawn, and with the reason.
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

            // And in HTML, the reason is escaped.
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
            // The upstream publishes a name that is not a distribution, and
            // a `requires-python` that tries to close the HTML tag.
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
            // The hostile value is escaped, which is the only thing that can be
            // done with it in HTML.
            assert!(
                html.contains("&lt;script&gt;"),
                "the hostile value was not escaped: {html}"
            );
            assert!(
                !html.contains("alert(1)</script>"),
                "the hostile value closed the tag: {html}"
            );
            // And the name that is not a distribution is not announced: a
            // link that cannot be downloaded is a broken link.
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

            // The index in JSON declares the value as is, and only the
            // file that is a distribution.
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
