//! **Real** npmjs (HSP-225 / HCR-03), with the actual npm client.
//!
//! HSP-223's lab (`support::lab`) demonstrates that the registry
//! caches, that it works without an upstream and that it serves nothing untrusted
//! **against a local upstream**. Here the same is demonstrated against the
//! real public registry (`https://registry.npmjs.org/`), and with the
//! client people use: `npm`.
//!
//! What is measured, and not asserted:
//!
//! - **AC1**: `npm view` and `npm pack` / `npm install --ignore-scripts` with
//!   a temporary `.npmrc` and HOME resolve `is-number@7.0.0` through Hodei,
//!   and the tarball's `integrity` matches the one the origin declares.
//!   The check against the real npmjs is **opt-in** and with the version
//!   pinned: without `HODEI_LAB_REAL=1` the case does not run and the report is
//!   not green, which is correct.
//! - **AC2**: a **second** fetch of the same content does **not** query the
//!   upstream (the lab's counter says so), with the upstream
//!   **disconnected** it resolves the same with local data, and a
//!   dist-tag that moves is revalidated **according to the TTL** without changing the
//!   historical bytes.
//! - **AC3**: a **reserved scope** does not fall through to the public one, the
//!   `dist.tarball` URLs the proxy returns point to **Hodei's endpoint**,
//!   and no client token appears in the log or travels to the upstream.
//!
//! # The path without network
//!
//! By default **the network is not used**: the upstream is the lab's
//! (a local server that counts) and the package is a *fixture* with the
//! public's coordinates: a real tarball, packed with the same
//! `npm pack` the client would use, but **not** npmjs's bytes. The package
//! under test never comes from anywhere else: the client's cache is
//! temporary and the package is requested from the group.
//!
//! # The client
//!
//! The PATH's `npm` may be a wrapper that needs the machine's HOME,
//! and the lab runs the client with a HOME that is not the user's.
//! That is why the client has **its** HOME (`LabHome`), **its** cache
//! and **its** `.npmrc`: what is downloaded comes from the group or does not come, and the
//! credential lives in a temporary file that disappears with the case.

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

/// The package under test: the coordinates are the public's.
const PACKAGE: &str = "is-number";

/// The version of the package under test: **pinned**, always.
const VERSION: &str = "7.0.0";

/// The other version of the fixture: the one the dist-tag moves to.
const OTHER_VERSION: &str = "7.1.0";

/// The coordinates requested from the client.
const SPEC: &str = "is-number@7.0.0";

/// The packument path in the repository.
const PACKUMENT_PATH: &str = "is-number";

/// The tarball path in the repository.
const TARBALL_PATH: &str = "is-number/-/is-number-7.0.0.tgz";

/// The reserved scope: what the house has and the public must not serve.
const RESERVED_SCOPE: &str = "@reserved";

/// The reserved scope's package.
const RESERVED_PACKAGE: &str = "@reserved/widget";

/// The real upstream: the public npm registry.
const REAL_UPSTREAM: &str = "https://registry.npmjs.org/";

/// The real upstream's host, as the service's egress understands it.
const REAL_HOST: &str = "registry.npmjs.org";

/// The `integrity` npmjs declares for `is-number@7.0.0`.
///
/// It is the value the public states, not one computed here: what is
/// checked is that the tarball downloaded through Hodei is **that one** and no other.
const REAL_INTEGRITY: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";

/// The group's *hosted* repository: the house's own content.
const HOSTED: &str = "npm-hosted";

/// The group's *proxy* repository: the public registry.
const PROXY: &str = "npm-public";

/// The group under test.
const GROUP: &str = "npm-all";

/// The TTLs the proxy is seeded with, in seconds.
///
/// The content (the tarballs) never expires: it is immutable by
/// definition. The metadata (the packuments) expires, and that is where it
/// is decided when a dist-tag that has moved is revalidated.
const METADATA_TTL: u64 = 3_600;

/// The short TTL used to demonstrate revalidation by TTL.
const SHORT_METADATA_TTL: u64 = 5;

/// The cases a complete run has to execute.
///
/// The report declares them, so a run that only reached some of them
/// says which was missing: those not executed count as `NOT_RUN`, and the
/// report is never green.
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

/// The title and the disclaimer of this suite's report.
///
/// The HSP-223 lab's disclaimer says `npm` was not talked to,
/// and here it was: the disclaimer belongs to the suite, not to the module.
const THE_TITLE: &str = "Laboratorio de npmjs — informe";

const THE_DISCLAIMER: &[&str] = &[
    "- **Ni la red por defecto.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local y el paquete es un fixture. El modo real es opt-in (`HODEI_LAB_REAL=1`) y su presupuesto es de bytes y de segundos.",
    "- **Ni el paquete publico.** En el camino sin red el tarball es un fixture con las coordenadas de `is-number@7.0.0`, no los bytes de npmjs: lo que se demuestra es el camino, no la integridad del publico. El camino real si compara el `integrity` con el que declara npmjs.",
    "- **Ni la distribucion de npm.** Se mide `npm view`, `npm pack` e `npm install --ignore-scripts` contra un registro de Hodei: no se mide la resolucion de conflictos del cliente, ni sus scripts, ni su auditoria.",
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

/// The npmjs lab: a deployment, an upstream that counts and the
/// group under test.
struct Lab {
    deployment: Deployment,
    /// The local upstream: it only exists in the no-network mode.
    upstream: Upstream,
    /// The proxy's upstream URL.
    upstream_url: String,
    /// The seconds of freshness of the proxy's metadata.
    metadata_ttl: u64,
}

impl Lab {
    /// A lab with a **local** upstream that counts.
    ///
    /// The upstream is the lab's (`support::lab::Upstream`): the
    /// counter and the upstream are the same thing, because a counter that
    /// lives in two places measures one of the two.
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

    /// A lab with the **real** upstream: the public npm registry.
    ///
    /// There is no local counter: npmjs cannot be counted. What is
    /// measured in the real mode is the `integrity` and the fact that
    /// resolution works with egress to npmjs **closed**.
    fn real() -> Self {
        Self {
            deployment: Deployment::new(),
            upstream: Upstream::start(),
            upstream_url: REAL_UPSTREAM.to_string(),
            metadata_ttl: METADATA_TTL,
        }
    }

    /// Seeds the hosted, the proxy and the group with the reservation policy
    /// passed in.
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
                // The content does not expire: a tarball is immutable. The
                // metadata expires, and that is where a dist-tag that has moved
                // is revalidated.
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

    /// Starts the service with the egress the case needs.
    fn start(&self, extra: &[(&str, &str)]) -> support::Service {
        self.deployment.start(extra)
    }

    /// The egress that lets the lab's local upstream through.
    fn the_local_egress(&self) -> String {
        format!("127.0.0.1:{};private;http", self.upstream.port())
    }
}

// ---------------------------------------------------------------------------
// The npm client.
// ---------------------------------------------------------------------------

/// A client run.
struct Run {
    ok: bool,
    output: String,
    command: String,
}

/// An isolated `npm` client: its HOME, its cache and its `.npmrc`.
///
/// The `.npmrc` points the registry at Hodei's **group**, so the
/// client has no path of its own to npmjs: what it downloads comes from the
/// group or does not come. The credential lives in the `.npmrc`, never in argv.
struct Npm {
    /// The client's HOME: it is not that of whoever runs the test.
    home: LabHome,
    /// The client's directory: the `.npmrc` and the cache.
    root: PathBuf,
    /// The client's `.npmrc`.
    rc: PathBuf,
    /// The registry the client talks to.
    registry: String,
}

impl Npm {
    /// A client that talks to `/npm/{repository}/` of the service.
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

    /// Runs `npm` in `dir` with this client's cache.
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

    /// An empty cache: whatever is installed next comes from the service.
    ///
    /// The client's cache is the client's: without emptying it, `npm`
    /// answers on its own and what is measured is its cache, not the
    /// registry's.
    fn forget_the_cache(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("cache"));
    }
}

// ---------------------------------------------------------------------------
// The fixtures.
// ---------------------------------------------------------------------------

/// The file name of a tarball, as npm writes it.
///
/// A scoped package is written `scope-name-version.tgz`: the
/// at sign cannot go in a file name.
fn the_tarball_file(name: &str, version: &str) -> String {
    let base = name.trim_start_matches('@').replace('/', "-");
    format!("{base}-{version}.tgz")
}

/// A real tarball of a package with the public's coordinates.
///
/// It is packed by the same `npm` the client would use, with its isolated
/// HOME: what the proxy serves is a tarball `npm` knows how to
/// open, not a file with a tarball's name.
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

/// The packument the lab's upstream serves.
///
/// The `dist.tarball` URLs point to the **upstream**: if the proxy does not
/// rewrite them, the client will go there and the test will say so.
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

/// The `integrity` of some bytes: `sha512` in base64, as npm writes it.
fn integrity_of(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// The sha256 of some bytes.
fn sha256_of_bytes(bytes: &[u8]) -> String {
    use sha2::Sha256;
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// An empty consumer project.
fn a_consumer(dir: &Path) {
    std::fs::create_dir_all(dir).expect("the consumer directory");
    std::fs::write(
        dir.join("package.json"),
        "{\"name\":\"consumer\",\"version\":\"1.0.0\",\"private\":true}",
    )
    .expect("the package.json of the consumer");
}

/// The service's response to a request.
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

/// Requests a path from the service through the npm surface.
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
// Utilities.
// ---------------------------------------------------------------------------

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
// AC1: the client resolves through the group, and the integrity is the origin's.
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
            // And what the client has came from the upstream, not from anywhere else.
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

            // `npm pack` downloads the tarball through the group. `npm` itself
            // checks the `integrity` the packument declares: if the
            // bytes were not the right ones, the client would fail before the
            // test got to look.
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

            // And the install: the lock says the `integrity` the client
            // verified, and it has to be the same.
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
                // Without the opt-in the case does not run, and a case that is
                // not executed is not a green case.
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
            // What the public declares, read **through Hodei**.
            let view = client.run(&work, &["view", SPEC, "dist.integrity"]);
            assert!(view.ok, "npm view failed:\n{}", view.output);
            let seen = view.output.trim().to_string();
            assert_eq!(
                seen, REAL_INTEGRITY,
                "the integrity that came through Hodei is not the one npmjs declares"
            );

            // And the real tarball, downloaded through Hodei: its bytes are the
            // public's, and `npm` verifies them against what is declared.
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

            // And the install, which is what the client would really do.
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

            // And the run respects the lab's budget.
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
// AC2: the cache, the upstream down, and the dist-tag that moves.
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

            // The first time: the upstream answers (a MISS).
            let first = client.run(&first_project, &["install", SPEC, "--ignore-scripts"]);
            assert!(first.ok, "the first install failed:\n{}", first.output);
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // The second time: the client arrives without its cache, so what
            // is measured is where what it brings comes from.
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

            // The upstream goes away: it stops accepting connections. A counter
            // that does not grow with the port closed proves nothing, so
            // what is checked is that the upstream is really dead.
            lab.upstream.disconnect();
            assert!(
                !lab.upstream.accepts_connections(),
                "the upstream is still listening: the evidence of the offline hit is not what it claims to be"
            );

            // The group (a hosted and a proxy) keeps serving the pinned
            // package: what it has, it has with provenance.
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
            // The other version of the fixture: the one the
            // dist-tag moves to. Its tarball exists too, because a packument
            // whose `latest` points to a version that is not in `versions`
            // is not a packument npm can resolve.
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

            // The MISS: the packument comes from the upstream.
            let first = client.run(&work, &["view", PACKAGE, "dist-tags.latest"]);
            assert!(first.ok, "the first view failed:\n{}", first.output);
            assert_eq!(first.output.trim(), VERSION);
            let asked = lab.upstream.total();
            assert!(asked > 0, "the upstream was never asked");

            // The upstream moves the dist-tag: the `latest` tag moves to another
            // version.
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&after).with_etag("\"v2\""));

            // Within the TTL, the cached packument is what is served: the
            // upstream is not asked again and the tag has not moved
            // yet.
            let cached = get(
                port,
                &format!("/{PACKUMENT_PATH}"),
                &token,
                &root.join("cached.json"),
            );
            assert_eq!(cached.status, 200, "{}", cached.text());
            // The response says **who** resolved it: the group member
            // that wins, not the one the client asked for.
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

            // And once the TTL has passed, revalidation is conditional: the upstream
            // answers with new bytes and the moved tag reaches the client.
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
            // The other version of the fixture: the one the
            // dist-tag moves to. Its tarball exists too, because a packument
            // whose `latest` points to a version that is not in `versions`
            // is not a packument npm can resolve.
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

            // The historical bytes, before the move.
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

            // The upstream moves the dist-tag and, with it, the version that
            // `latest` points to.
            lab.upstream
                .put_doc(PACKUMENT_PATH, Doc::new(&after).with_etag("\"v2\""));
            std::thread::sleep(Duration::from_secs(SHORT_METADATA_TTL + 2));

            // The pinned version remains the same, byte for byte: a
            // tarball is immutable content and is not rewritten.
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

            // And the tarball the group serves is the same as the
            // upstream's: there are not two versions of the same content.
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
// AC3: what is not served, and the trail that is not dirtied.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_reserved_scope_is_not_served_from_the_public_upstream() {
    a_case(
        "ac3-a-reserved-scope-is-not-served-from-the-public-upstream",
        "un scope reservado no se sirve desde el upstream publico",
        Scope::Product,
        Expectation::Refused,
        || {
            // The group reserves its scope: what is under it does not come from the
            // public upstream, even if the upstream has it.
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
            // And the public upstream did not receive a single request for that
            // scope: the reservation is decided before going out.
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
            // And the client's lock says the same: what it installs comes
            // from the group, not from the upstream.
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

            // The packument that is served carries the credential nowhere:
            // neither in the URLs nor in the body.
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
// The lab's tally.
// ---------------------------------------------------------------------------

/// The tally does not lie: a case that was not executed is not green.
///
/// It is the same rule as the HSP-223 lab, checked here with the
/// real case of this suite: without the opt-in, the real npmjs case is not
/// executed, and the report cannot be green.
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
