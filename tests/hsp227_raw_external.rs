//! HSP-227: External raw: cached proxy and group precedence.
//!
//! What is demonstrated here, against the **real service**:
//!
//! - a group of raw repositories serves the bytes the upstream
//!   declared, with its digest and its headers;
//! - `HEAD` and `Range` answer the same as `GET`;
//! - when two members have the same path, the first wins, and the
//!   **trail** says which it was and in which version of the group's
//!   configuration, without mixing one's headers with the other's;
//! - traversal is blocked, a forbidden redirect is not followed, and a
//!   read without credentials receives no content.
//!
//! By default the path **does not use the network**: the upstream is the
//! lab's and the content is a fixture. The case against a public origin
//! is opt-in (`HODEI_LAB_REAL=1`), and without it it stays `NOT_RUN`, so
//! the report is not green: a case that is not executed is not a case that
//! passes.

mod support;

use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use support::lab::{
    Budget, Doc, Expectation, LabCase, Outcome, REAL_MODE_VARIABLE, Scope, Summary, Upstream,
    describe_registry_lab, real_mode_enabled, run_registry_lab_case,
};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The path of the content under test.
const ASSET_PATH: &str = "docs/readme.txt";

/// The group's **hosted** repository.
const HOSTED: &str = "raw-hosted";

/// The group's **proxy** repository: the one that talks to the upstream.
const PROXY: &str = "raw-mirror";

/// The group under test.
const GROUP: &str = "raw-group";

/// The public origin of the real case: a **pinned** file (a tag, not
/// a branch), with a permissive license.
const REAL_BASE: &str = "https://raw.githubusercontent.com/rust-lang/rust/1.75.0/";

/// The real file's path inside its origin.
const REAL_PATH: &str = "README.md";

/// The cases a complete run has to execute.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-the-group-serves-the-bytes-the-upstream-declared",
        "el grupo sirve los bytes, el digest y las cabeceras que declaro el upstream",
    ),
    (
        "ac1-head-and-range-answer-the-same",
        "HEAD y Range contestan lo mismo que el GET",
    ),
    (
        "ac1-the-public-origin-serves-the-same-bytes",
        "contra un origen publico sale el mismo contenido que por la descarga directa",
    ),
    (
        "ac2-the-first-member-that-has-the-path-wins",
        "gana el primer miembro que tiene la ruta, y el rastro lo dice",
    ),
    (
        "ac2-the-traversal-is-blocked",
        "el traversal se bloquea y no se sirve nada",
    ),
    (
        "ac3-a-redirect-that-is-not-allowed-is-not-followed",
        "una redireccion prohibida no se sigue",
    ),
    (
        "ac3-a-read-without-a-credential-receives-no-content",
        "una lectura sin credencial no recibe contenido",
    ),
    (
        "ac3-the-count-does-not-lie",
        "el caso real no se ejecuta sin el opt-in, y no cuenta como verde",
    ),
];

/// The title and the disclaimer of this suite's report.
const THE_TITLE: &str = "Laboratorio de Raw — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y el contenido es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el contenido publico.** En el camino sin red el contenido es un fixture, no los bytes del origen publico: lo que se demuestra es el camino, no la integridad de ese fichero. El camino real compara con la descarga directa.",
    "- **Ni la superficie de paquetes.** Se mide la superficie cruda (`/repository/...`) con `curl`: no se mide ninguna API de paquetes, ni la resolucion de dependencias, ni la instalacion.",
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

/// The raw lab: a deployment, an upstream that counts and the group
/// under test.
struct Lab {
    deployment: Deployment,
    /// The proxy's local upstream.
    upstream: Upstream,
    /// The proxy's upstream URL.
    upstream_url: String,
    /// The content the upstream serves.
    payload: Vec<u8>,
    /// The content's digest.
    digest: String,
}

impl Lab {
    /// A lab with a **local** upstream that counts.
    fn local() -> Self {
        let upstream = Upstream::start();
        let url = upstream.base();
        let payload = b"# el contenido del upstream\n\ncon un digest declarado.\n".to_vec();
        let digest = the_sha256(&payload);
        Self {
            deployment: Deployment::new(),
            upstream,
            upstream_url: url,
            payload,
            digest,
        }
    }

    /// Seeds the hosted, the proxy and the group.
    fn seed(&self) {
        self.deployment.seed_repositories(&[
            json!({ "key": HOSTED, "format": "raw" }),
            json!({
                "key": PROXY,
                "format": "raw",
                "proxy": {
                    "upstream": {
                        "id": "origin",
                        "url": format!("{}/files/", self.upstream_url),
                        "scopes": ["/"],
                        "max_bytes": 1024 * 1024,
                    },
                    "cache": { "ttl": { "content_secs": null, "metadata_secs": 300 } },
                },
            }),
        ]);
        self.deployment.seed_groups_raw(&[json!({
            "key": GROUP,
            "members": [HOSTED, PROXY],
            "format": "raw",
        })]);
    }

    /// Starts the service with the egress the case needs.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// The egress that lets the lab's local upstream through.
    fn the_local_egress(&self) -> String {
        format!("127.0.0.1:{};private;http", self.upstream.port())
    }

    /// The path the upstream receives: its base plus the path.
    fn the_upstream_path(&self) -> String {
        format!("files/{ASSET_PATH}")
    }
}

/// The SHA256 of some bytes, in hexadecimal.
fn the_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A request to the service, with its response.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    headers: &[(&str, &str)],
) -> support::Answer {
    let into = dir.join("answer.bin");
    let _ = std::fs::remove_file(&into);
    Curl::call_with_headers(port, method, path, token, None, headers, Some(&into)).run()
}

/// The text of a response.
fn text_of(answer: &support::Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// A case's summary, with the case's id and title.
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
// AC1: the bytes, the digest and the headers.
// ---------------------------------------------------------------------------

#[test]
fn ac1_the_group_serves_the_bytes_the_upstream_declared() {
    a_case(
        "ac1-the-group-serves-the-bytes-the-upstream-declared",
        "el grupo sirve los bytes, el digest y las cabeceras que declaro el upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload)
                    .with_etag("\"v1\"")
                    .with_declared_sha256(&lab.digest),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                &token,
                &[],
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            assert_eq!(
                the_sha256(&answer.body),
                lab.digest,
                "the bytes that came down are not the ones the upstream declared"
            );
            // The ETag that is served identifies **the served content**, not
            // the version the upstream has: it is the digest of what it
            // downloaded, and that is why it cannot be another member's.
            assert_eq!(
                answer.header("etag").as_deref(),
                Some(format!("\"sha256:{}\"", lab.digest).as_str()),
                "the ETag does not identify the content that was served"
            );
            assert!(
                lab.upstream.hits(&lab.the_upstream_path()) > 0,
                "the upstream was never asked"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-the-group-serves-the-bytes-the-upstream-declared",
                "el grupo sirve los bytes, el digest y las cabeceras que declaro el upstream",
            )
            .command(&format!("GET /repository/{GROUP}/{ASSET_PATH}"))
            .origin(&lab.upstream.origin())
            .sha256(&lab.digest)
            .passed()
        },
    );
}

#[test]
fn ac1_head_and_range_answer_the_same() {
    a_case(
        "ac1-head-and-range-answer-the-same",
        "HEAD y Range contestan lo mismo que el GET",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload)
                    .with_etag("\"v1\"")
                    .with_declared_sha256(&lab.digest),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            // HEAD: the same headers, no body.
            let head = call(
                dir,
                port,
                "HEAD",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                &token,
                &[],
            );
            assert_eq!(head.status, 200, "{}", text_of(&head));
            assert!(
                head.body.is_empty(),
                "the HEAD answered with a body: {}",
                text_of(&head)
            );
            assert_eq!(
                head.header("etag").as_deref(),
                Some(format!("\"sha256:{}\"", lab.digest).as_str()),
                "the HEAD does not carry the ETag of the content"
            );
            assert_eq!(
                head.header("content-length").as_deref(),
                Some(lab.payload.len().to_string().as_str()),
                "the HEAD does not carry the length of the content"
            );

            // Range: a chunk, and the code that says so.
            let ranged = call(
                dir,
                port,
                "GET",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                &token,
                &[("range", "bytes=0-3")],
            );
            assert_eq!(ranged.status, 206, "{}", text_of(&ranged));
            assert_eq!(
                &ranged.body,
                &lab.payload[..4],
                "the range did not answer with the bytes that were asked for"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-head-and-range-answer-the-same",
                "HEAD y Range contestan lo mismo que el GET",
            )
            .command(&format!(
                "HEAD y GET Range /repository/{GROUP}/{ASSET_PATH}"
            ))
            .origin(&lab.upstream.origin())
            .sha256(&lab.digest)
            .passed()
        },
    );
}

#[test]
fn ac1_the_public_origin_serves_the_same_bytes() {
    a_case(
        "ac1-the-public-origin-serves-the-same-bytes",
        "contra un origen publico sale el mismo contenido que por la descarga directa",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let case = LabCase::new(
                "ac1-the-public-origin-serves-the-same-bytes",
                "contra un origen publico sale el mismo contenido que por la descarga directa",
            )
            .command(&format!(
                "GET /repository/{PROXY}/{REAL_PATH} (origen: {REAL_BASE})"
            ));
            if !real_mode_enabled() {
                return case.not_run(format!(
                    "the real mode is opt-in: set {REAL_MODE_VARIABLE}=1 to run it"
                ));
            }

            let budget = Budget::from_environment();
            let deployment = Deployment::new();
            deployment.seed_repositories(&[json!({
                "key": PROXY,
                "format": "raw",
                "proxy": {
                    "upstream": {
                        "id": "public",
                        "url": REAL_BASE,
                        "scopes": ["/"],
                        "max_bytes": 4 * 1024 * 1024,
                    },
                    "cache": { "ttl": { "content_secs": null, "metadata_secs": 300 } },
                },
            })]);
            let mut service =
                deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", "raw.githubusercontent.com")]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = deployment.directory.path();

            // The direct download: the reference of what has to be served.
            let direct = std::process::Command::new("curl")
                .args(["--silent", "--show-error", "--fail", "--location"])
                .arg(format!("{REAL_BASE}{REAL_PATH}"))
                .output()
                .expect("curl runs");
            assert!(
                direct.status.success(),
                "the direct download failed:\n{}",
                String::from_utf8_lossy(&direct.stderr)
            );
            let expected = the_sha256(&direct.stdout);

            let started = std::time::Instant::now();
            let answer = call(
                dir,
                port,
                "GET",
                &format!("/repository/{PROXY}/{REAL_PATH}"),
                &token,
                &[],
            );
            let elapsed = started.elapsed();
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            assert_eq!(
                the_sha256(&answer.body),
                expected,
                "what the proxy served is not what the origin serves"
            );

            let size = answer.body.len() as u64;
            support::stop(&mut service);
            let case = case
                .command(&format!("GET /repository/{PROXY}/{REAL_PATH}"))
                .origin(REAL_BASE)
                .sha256(&expected);
            match budget.permits(size, elapsed) {
                Ok(()) => case.passed(),
                Err(cause) => case.failed(cause),
            }
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: precedence and the trail.
// ---------------------------------------------------------------------------

#[test]
fn ac2_the_first_member_that_has_the_path_wins() {
    a_case(
        "ac2-the-first-member-that-has-the-path-wins",
        "gana el primer miembro que tiene la ruta, y el rastro lo dice",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload)
                    .with_etag("\"upstream\"")
                    .with_declared_sha256(&lab.digest),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            // The hosted member publishes **the same path** with **other
            // bytes**: if the group mixed one's index with
            // the other's file, what was served would not match its digest.
            let hosted = b"# el contenido del hosted\n\ncon otros bytes.\n".to_vec();
            let hosted_digest = the_sha256(&hosted);
            let published = upload(dir, port, HOSTED, &token, &hosted);
            assert!(
                matches!(published.status, 200 | 201),
                "the hosted member did not publish: {}",
                text_of(&published)
            );

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                &token,
                &[],
            );
            assert_eq!(answer.status, 200, "{}", text_of(&answer));
            assert_eq!(
                the_sha256(&answer.body),
                hosted_digest,
                "the group did not serve the bytes of the member that won"
            );
            // And the headers are those of the winner, not a mix: the
            // ETag identifies the content that was served, and it is the
            // winning member's.
            assert_eq!(
                answer.header("etag").as_deref(),
                Some(format!("\"sha256:{hosted_digest}\"").as_str()),
                "the ETag does not belong to the member that won"
            );
            assert_ne!(
                answer.header("etag").as_deref(),
                Some("\"upstream\""),
                "the ETag of the loser traveled with the content of the winner"
            );
            // The trail says who won and in which version of the
            // group's configuration.
            assert_eq!(
                answer.header("x-hodei-resolved-from").as_deref(),
                Some(HOSTED),
                "the answer does not say which member won: {}",
                text_of(&answer)
            );
            assert!(
                answer.header("x-hodei-group-version").is_some(),
                "the answer does not say in which version of the group it was resolved"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-first-member-that-has-the-path-wins",
                "gana el primer miembro que tiene la ruta, y el rastro lo dice",
            )
            .command(&format!("GET /repository/{GROUP}/{ASSET_PATH}"))
            .origin(&lab.upstream.origin())
            .sha256(&hosted_digest)
            .passed()
        },
    );
}

#[test]
fn ac2_the_traversal_is_blocked() {
    a_case(
        "ac2-the-traversal-is-blocked",
        "el traversal se bloquea y no se sirve nada",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload)
                    .with_etag("\"v1\"")
                    .with_declared_sha256(&lab.digest),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            for hostile in [
                "/repository/{group}/../../etc/passwd",
                "/repository/{group}/..%2f..%2fetc%2fpasswd",
                "/repository/{group}/%2e%2e/%2e%2e/etc/passwd",
            ] {
                let path = hostile.replace("{group}", GROUP);
                let answer = call(dir, port, "GET", &path, &token, &[]);
                assert!(
                    answer.status >= 400,
                    "the traversal {path} was answered with {}",
                    answer.status
                );
                assert!(
                    !text_of(&answer).contains("root:"),
                    "the traversal {path} served a file of the machine"
                );
            }

            support::stop(&mut service);
            LabCase::new(
                "ac2-the-traversal-is-blocked",
                "el traversal se bloquea y no se sirve nada",
            )
            .command(&format!("GET /repository/{GROUP}/../etc/passwd"))
            .origin(&lab.upstream.origin())
            .sha256(&lab.digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: what cannot be served.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_redirect_that_is_not_allowed_is_not_followed() {
    a_case(
        "ac3-a-redirect-that-is-not-allowed-is-not-followed",
        "una redireccion prohibida no se sigue",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            // The upstream sends to another origin: the destination is not in the
            // egress list, so the redirect is not followed.
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload).redirect("https://otro.example/secreto"),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let token = support::token(FIRST_ACCOUNT, "alice");
            let dir = lab.deployment.directory.path();

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                &token,
                &[],
            );
            assert!(
                answer.status >= 400,
                "the redirect was followed: {}",
                text_of(&answer)
            );
            assert!(
                !text_of(&answer).contains("secreto"),
                "the content of the redirect came through: {}",
                text_of(&answer)
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-redirect-that-is-not-allowed-is-not-followed",
                "una redireccion prohibida no se sigue",
            )
            .command(&format!(
                "GET /repository/{GROUP}/{ASSET_PATH} (upstream: 302)"
            ))
            .origin(&lab.upstream.origin())
            .sha256(&lab.digest)
            .passed()
        },
    );
}

#[test]
fn ac3_a_read_without_a_credential_receives_no_content() {
    a_case(
        "ac3-a-read-without-a-credential-receives-no-content",
        "una lectura sin credencial no recibe contenido",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = Lab::local();
            lab.upstream.put_doc(
                &lab.the_upstream_path(),
                Doc::new(&lab.payload)
                    .with_etag("\"v1\"")
                    .with_declared_sha256(&lab.digest),
            );
            lab.seed();

            let mut service = lab.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &lab.the_local_egress())]);
            let port = service.port;
            let dir = lab.deployment.directory.path();

            let answer = call(
                dir,
                port,
                "GET",
                &format!("/repository/{GROUP}/{ASSET_PATH}"),
                "",
                &[],
            );
            assert_eq!(answer.status, 401, "{}", text_of(&answer));
            assert!(
                !text_of(&answer).contains("el contenido del upstream"),
                "the unauthorized read received the content: {}",
                text_of(&answer)
            );
            assert!(
                lab.upstream.total() == 0,
                "the upstream was asked before the credential was checked"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-read-without-a-credential-receives-no-content",
                "una lectura sin credencial no recibe contenido",
            )
            .command(&format!(
                "GET /repository/{GROUP}/{ASSET_PATH} (sin credencial)"
            ))
            .origin(&lab.upstream.origin())
            .sha256(&lab.digest)
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// The tally.
// ---------------------------------------------------------------------------

#[test]
fn ac3_the_count_does_not_lie() {
    a_case(
        "ac3-the-count-does-not-lie",
        "el caso real no se ejecuta sin el opt-in, y no cuenta como verde",
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
                "el caso real no se ejecuta sin el opt-in, y no cuenta como verde",
            )
            .command("Summary::of(casos)")
            .passed()
        },
    );
}

/// An upload to the hosted: the raw content.
fn upload(dir: &Path, port: u16, repository: &str, token: &str, payload: &[u8]) -> support::Answer {
    let sent = dir.join("upload.bin");
    let into = dir.join("upload-answer.bin");
    std::fs::write(&sent, payload).expect("the upload body is written");
    Curl::call(
        port,
        "PUT",
        &format!("/repository/{repository}/{ASSET_PATH}"),
        token,
        Some(&sent),
        Some("application/octet-stream"),
        Some(&into),
    )
    .run()
}
