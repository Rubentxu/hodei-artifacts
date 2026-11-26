//! HSP-232: the **go** group assembles the version list and resolves a
//! version from **one** member.
//!
//! What is demonstrated here, against the **real service**:
//!
//! - the group's listing is the **union** of its members' lists, one
//!   entry per version and in semver order;
//! - the group's `@latest` is the last of that union, not that of the last
//!   member to respond;
//! - the three artifacts of a version (`.info`, `.mod` and `.zip`) come
//!   **from the same member**: the first, in the configured order, that
//!   has it;
//! - a **divergent** version --the same version with different content in
//!   two members-- is **rejected** with a defined error, not chosen
//!   silently;
//! - a **reserved** case is not requested from the group's public upstream: the
//!   hosted members are the internal source and it does not touch it;
//! - a member with a **partitioned scope** is not served: the group answers with
//!   the scope diagnostic, not with bytes and not with a 404, which would be
//!   lying;
//! - a **down** member does not stop resolution: the group skips to the
//!   next member, and the error only comes out if **nothing** answered;
//! - the trust gate is also applied on the **proxy** path:
//!   a proxy on the **trusted** channel does not serve what has no grant;
//! - the **HEAD** of a proxy path declares the real size of the asset.
//!
//! And, with the **real** `go` client:
//!
//! - `go mod download` with `GOPROXY` pointing **only** to Hodei resolves a
//!   **hosted** module under the group, and the `go.sum` it produces is valid;
//! - the same client resolves a **public** module served by the group's
//!   **proxy** member, and its `go.sum` also adds up;
//! - with the upstream **down**, the already downloaded version keeps being served
//!   from the cache, and a version **never** downloaded is not served: no
//!   content is invented.
//!
//! The traffic goes over **TLS through a local `socat` bridge**: `go` only
//! sends credentials (its `.netrc`) over https, and refuses to do so over http,
//! exposing them in plain along the way.

mod support;

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Answer, Curl, Deployment};

/// The first member of the group: the hosted one.
const INTERNAL: &str = "go-internal";

/// The second member of the group: the proxy.
const CENTRAL: &str = "go-central";

/// The group.
const GROUP: &str = "go-all";

/// The group with the **proxy member first**: precedence rules, and it is the
/// down proxy that is consulted before the hosted one.
const GROUP_FALLEN: &str = "go-fallen";

/// A go proxy on the **trusted** channel.
const TRUSTED_PROXY: &str = "go-trusted";

/// The group member with the **partitioned content scope**.
const PARTITIONED: &str = "go-partitioned";

/// The group with the partitioned-scope member.
const GROUP_SCOPE: &str = "go-scoped";

/// The scope diagnostic: the same one that the raw surface states.
const SCOPE_DIAGNOSTIC: &str =
    "this service serves the content of a repository only in the global scope";

/// The module that the hosted member publishes.
const OWNED: &str = "example.com/hello";

/// The module that the proxy member brings from its upstream.
const PUBLIC: &str = "example.com/proxied";

/// The version that the hosted member publishes.
const OWNED_VERSION: &str = "v1.0.0";

/// The version that the proxy member's upstream brings of the public module.
const PUBLIC_VERSION: &str = "v1.3.0";

/// The version that the proxy member's upstream brings of the hosted's module.
///
/// The public module and the hosted's are the **same** module in different
/// versions: the group's union is of its members' lists, and the
/// order is semver, not by bytes.
const SHARED_VERSION: &str = "v1.2.0";

/// The cases that a complete run has to execute.
///
/// They are declared **before** running: a run that only reached some
/// says which did not run, and a report with a case that did not run is never
/// green.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
        "el listado del grupo es la union de las listas de sus miembros, en orden semver",
    ),
    (
        "ac1-the-latest-of-a-group-is-the-latest-of-the-union",
        "el latest del grupo es el ultimo de la union, no el del ultimo miembro",
    ),
    (
        "ac1-the-real-go-client-downloads-a-hosted-module-under-the-group",
        "el cliente go real baja un modulo hosted bajo el grupo y su go.sum es valido",
    ),
    (
        "ac1-the-real-go-client-downloads-a-public-module-under-the-group",
        "el cliente go real baja un modulo publico servido por el miembro proxy del grupo",
    ),
    (
        "ac2-the-three-artifacts-of-a-version-come-from-one-member",
        "el info, el mod y el zip de una version salen del mismo miembro",
    ),
    (
        "ac2-a-divergent-version-is-rejected",
        "una version divergente entre dos miembros se rechaza con un error definido",
    ),
    (
        "ac2-a-reserved-namespace-never-falls-back",
        "un caso reservado no se pide al upstream publico del grupo",
    ),
    (
        "ac3-without-upstream-the-cache-serves-what-was-downloaded-and-no-more",
        "sin upstream, la cache sirve lo descargado y no inventa lo que no se descarg",
    ),
    (
        "ac3-a-fallen-member-does-not-stop-the-group",
        "un miembro caido del grupo no detiene la resolucion",
    ),
    (
        "ac2-a-trusted-proxy-does-not-serve-what-was-not-granted",
        "un proxy en el canal trusted no sirve lo que no tiene concesion",
    ),
    (
        "ac2-a-member-with-a-partitioned-scope-is-not-served",
        "un miembro de ambito particionado no se sirve: el grupo lo dice con el diagnostico del ambito",
    ),
    (
        "ac1-the-head-of-a-proxy-path-carries-the-content-length",
        "el HEAD de una ruta proxy declara el tamano real del asset",
    ),
];

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// The `go` client binary.
///
/// The Go SDK is not always on the PATH: it is looked for there, and if it is not, in
/// the linuxbrew installation that this machine uses.
fn the_go_binary() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("HODEI_GO") {
        let path = std::path::PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    for candidate in ["go", "/home/linuxbrew/.linuxbrew/bin/go"] {
        let found = Command::new(candidate).args(["version"]).output();
        if found.is_ok_and(|output| output.status.success()) {
            return Some(std::path::PathBuf::from(candidate));
        }
    }
    None
}

/// The text of a response.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// A request to the service.
fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if sent.is_some() {
        headers.push(("content-type", "application/octet-stream"));
    }
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &headers,
        Some(&into),
    )
    .run()
}

/// The canonical `.info` document of a version.
fn info_document(version: &str) -> Vec<u8> {
    json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" })
        .to_string()
        .into_bytes()
}

/// A `go.mod` that declares the module without escaping.
fn go_mod(module: &str) -> Vec<u8> {
    format!("module {module}\n\ngo 1.22\n").into_bytes()
}

/// The zip of a module, as the protocol requires: everything under the prefix
/// `{module}@{version}/`, with the same `go.mod` that the `.mod` serves.
fn module_zip(module: &str, version: &str, go_mod: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let prefix = format!("{module}@{version}");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer
        .start_file(format!("{prefix}/go.mod"), options)
        .expect("the go.mod opens");
    writer.write_all(go_mod).expect("the go.mod is written");
    writer
        .start_file(format!("{prefix}/hello.go"), options)
        .expect("the source opens");
    writer
        .write_all(b"package hello\n")
        .expect("the source is written");
    writer.finish().expect("the zip closes").into_inner()
}

/// Publishes the whole version of a module to a hosted repository.
fn publish_version(
    port: u16,
    repository: &str,
    token: &str,
    module: &str,
    version: &str,
    zip: &[u8],
) {
    for (extension, body) in [
        ("info", info_document(version)),
        ("mod", go_mod(module)),
        ("zip", zip.to_vec()),
    ] {
        let answer = call(
            port,
            "PUT",
            &format!("/go/{repository}/{module}/@v/{version}.{extension}"),
            token,
            Some(&body),
        );
        assert_eq!(
            answer.status,
            200,
            "publish {version}.{extension}: {}",
            text_of(&answer)
        );
    }
}

/// The `proxy` configuration of a go repository: the upstream and its cache.
fn proxy_of(upstream: &support::lab::Upstream) -> Value {
    json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{}/", upstream.port()),
            "scopes": ["/"],
            "max_bytes": 65536,
        },
        "cache": { "ttl": { "metadata_secs": 3600 } },
    })
}

/// An ephemeral TLS bridge: `https` at the door, the hub in the back.
struct Bridge {
    child: std::process::Child,
    port: u16,
    certificate: std::path::PathBuf,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A free port: it is taken, released, and the bridge occupies it right after.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port")
        .local_addr()
        .expect("the address of the listener")
        .port()
}

/// A self-signed certificate for 127.0.0.1.
fn self_signed(root: &Path) -> std::path::PathBuf {
    let pem = root.join("bridge.pem");
    let output = Command::new("openssl")
        .args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout"])
        .arg(&pem)
        .arg("-out")
        .arg(&pem)
        .args(["-days", "1", "-subj", "/CN=127.0.0.1"])
        .args(["-addext", "subjectAltName=IP:127.0.0.1"])
        .stderr(Stdio::piped())
        .output()
        .expect("openssl runs");
    assert!(
        output.status.success(),
        "openssl req: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    pem
}

/// Brings up the bridge and does not return until it listens.
fn bridge(root: &Path, hub: u16) -> Bridge {
    std::fs::create_dir_all(root).expect("the bridge directory");
    let certificate = self_signed(root);
    let port = free_port();
    let child = Command::new("socat")
        .arg(format!(
            "OPENSSL-LISTEN:{port},cert={},key={},verify=0,reuseaddr,fork",
            certificate.display(),
            certificate.display()
        ))
        .arg(format!("TCP:127.0.0.1:{hub}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge starts");
    let bridge = Bridge {
        child,
        port,
        certificate,
    };
    bridge.wait_ready();
    bridge
}

impl Bridge {
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the TLS bridge never listened on {}", self.port);
    }
}

/// The `go` client's home: everything inside the deployment, and the real HOME
/// untouched.
struct Client {
    root: std::path::PathBuf,
    work: std::path::PathBuf,
    binary: std::path::PathBuf,
    tunnel_port: u16,
    certificate: std::path::PathBuf,
}

impl Client {
    /// The client, with its `.netrc` and its own cache.
    ///
    /// Each case has its own home: two clients do not share `go.mod`, because
    /// the second `go mod init` would fail.
    fn new(deployment: &Deployment, tunnel: &Bridge, token: &str, name: &str) -> Self {
        let root = deployment.directory.path().join(name);
        std::fs::create_dir_all(&root).expect("the client home");
        let work = root.join("work");
        std::fs::create_dir_all(&work).expect("the work directory");
        let netrc = root.join(".netrc");
        std::fs::write(
            &netrc,
            format!(
                "machine 127.0.0.1:{} login alice password {token}\n",
                tunnel.port
            ),
        )
        .expect("the netrc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&netrc, std::fs::Permissions::from_mode(0o600))
                .expect("the netrc is private");
        }
        Self {
            root,
            work,
            binary: the_go_binary().expect("the Go SDK is installed"),
            tunnel_port: tunnel.port,
            certificate: tunnel.certificate.clone(),
        }
    }

    /// Runs the `go` client against the hub.
    ///
    /// The configuration is **explicit** and each part has its reason:
    ///
    /// - `GOPROXY` points **only** to the hub, without `,direct`: the client
    ///   has no way to fall back to the network or to a VCS.
    /// - `GOSUMDB=off` and `GONOSUMDB=*`: the public sum database does not know these
    ///   modules, and the client has no reason to consult it. Integrity
    ///   is **not** skipped: the local `go.sum` is what verifies, and the `.zip`
    ///   is checked byte for byte against what was published.
    /// - `GOFLAGS=-mod=mod`: the test module needs to write its
    ///   `go.sum`, which is what this test wants to measure.
    /// - `GOPRIVATE` is **not** set: it would make the client skip the
    ///   proxy and go out to the network, which is exactly what this test forbids.
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(&self.binary)
            .current_dir(&self.work)
            .env("HOME", &self.root)
            .env("GOPATH", self.root.join("gopath"))
            .env("GOMODCACHE", self.root.join("modcache"))
            .env("GOCACHE", self.root.join("buildcache"))
            .env("SSL_CERT_FILE", &self.certificate)
            .env("GOFLAGS", "-mod=mod")
            .env("GOSUMDB", "off")
            .env("GONOSUMDB", "*")
            .env(
                "GOPROXY",
                format!("https://127.0.0.1:{}/go/{GROUP}", self.tunnel_port),
            )
            .args(args)
            .output()
            .expect("go runs")
    }

    /// The `go.sum` that the client wrote.
    fn the_sum(&self) -> String {
        std::fs::read_to_string(self.work.join("go.sum")).unwrap_or_default()
    }

    /// The `.ziphash` that the client wrote: the integrity of what was downloaded.
    fn the_ziphash(&self, module: &str, version: &str) -> Option<String> {
        let path = self
            .root
            .join("modcache")
            .join("cache/download")
            .join(module)
            .join("@v")
            .join(format!("{version}.ziphash"));
        std::fs::read_to_string(path).ok()
    }
}

/// Runs a lab case.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    describe_registry_lab(
        "Laboratorio del grupo go — informe",
        &[
            "- **Un upstream, no una red.** El upstream de distribucion es el registro del laboratorio, en local: lo que se mide es el camino del grupo con un miembro proxy, no la red de una distribucion real.",
            "- **El sumario publico no participa.** `GOSUMDB=off` y `GONOSUMDB=*` evitan que el cliente consulte `sum.golang.org`; la integridad la verifica el `go.sum` local, que es lo que esta suite mide.",
            "- **El cliente real es `go`.** Si el entorno no lo tiene, el caso se declara `NOT_RUN` con el motivo: nunca `PASS` por una salida vacia.",
        ],
    );
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

/// The lab deployment: a hosted, a proxy and the group.
///
/// Everything is seeded **before** starting the service: the database is
/// locked by the process that opens it.
fn the_lab() -> (Deployment, support::lab::Upstream) {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    deployment.seed_go_group(GROUP, &[INTERNAL, CENTRAL]);
    (deployment, upstream)
}

/// The service's environment: the lab's upstream is the only network.
fn the_environment(upstream: &support::lab::Upstream) -> Vec<(&'static str, String)> {
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    vec![("HODEI_PROXY__ALLOWED_HOSTS", allowed)]
}

/// The group's listing, with the first account's token.
fn the_list(port: u16, module: &str) -> Answer {
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    call(
        port,
        "GET",
        &format!("/go/{GROUP}/{module}/@v/list"),
        &token,
        None,
    )
}

#[test]
fn the_group_of_go_assembles_the_versions_and_resolves_them_from_one_member() {
    let (deployment, upstream) = the_lab();
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    // The proxy member's upstream brings a public module, and one more
    // version of the hosted's module: the group's union is of both lists.
    upstream.put(
        &format!("{PUBLIC}/@v/list"),
        format!("{PUBLIC_VERSION}\n").as_bytes(),
        None,
    );
    upstream.put(
        &format!("{OWNED}/@v/list"),
        format!("{SHARED_VERSION}\n").as_bytes(),
        None,
    );
    upstream.put(
        &format!("{OWNED}/@v/{SHARED_VERSION}.info"),
        &info_document(SHARED_VERSION),
        None,
    );
    upstream.put(
        &format!("{OWNED}/@v/{SHARED_VERSION}.mod"),
        &go_mod(OWNED),
        None,
    );
    upstream.put(
        &format!("{OWNED}/@v/{SHARED_VERSION}.zip"),
        &module_zip(OWNED, SHARED_VERSION, &go_mod(OWNED)),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.info"),
        &info_document(PUBLIC_VERSION),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.mod"),
        &go_mod(PUBLIC),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
        &module_zip(PUBLIC, PUBLIC_VERSION, &go_mod(PUBLIC)),
        None,
    );

    let environment = the_environment(&upstream);
    let service = deployment.start(
        &environment
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>(),
    );
    let port = service.port;

    // The hosted member publishes its module in two versions.
    publish_version(
        port,
        INTERNAL,
        &token,
        OWNED,
        OWNED_VERSION,
        &module_zip(OWNED, OWNED_VERSION, &go_mod(OWNED)),
    );
    publish_version(
        port,
        INTERNAL,
        &token,
        OWNED,
        "v1.10.0",
        &module_zip(OWNED, "v1.10.0", &go_mod(OWNED)),
    );

    // ---- AC1: the listing is the union of the members' lists.
    a_case(
        "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
        "el listado del grupo es la union de las listas de sus miembros, en orden semver",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let answer = the_list(port, OWNED);
            if answer.status != 200 {
                return LabCase::new(
                    "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                    "el listado del grupo es la union de las listas de sus miembros, en orden semver",
                )
                .failed(format!("the list answered {}", text_of(&answer)));
            }
            let body = text_of(&answer);
            let versions = body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            // The union of the two lists, in **semver** order: `v1.10.0`
            // goes after `v1.2.0`, which is where the protocol puts it.
            if versions != [OWNED_VERSION, SHARED_VERSION, "v1.10.0"] {
                return LabCase::new(
                    "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                    "el listado del grupo es la union de las listas de sus miembros, en orden semver",
                )
                .failed(format!("the list of the hosted module is {versions:?}"));
            }
            // And the upstream's module, which only the proxy member has.
            let answer = the_list(port, PUBLIC);
            if answer.status != 200 {
                return LabCase::new(
                    "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                    "el listado del grupo es la union de las listas de sus miembros, en orden semver",
                )
                .failed(format!("the list of the public module answered {}", text_of(&answer)));
            }
            let body = text_of(&answer);
            if !body.contains(PUBLIC_VERSION) {
                return LabCase::new(
                    "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                    "el listado del grupo es la union de las listas de sus miembros, en orden semver",
                )
                .failed(format!("the list of the public module is {body:?}"));
            }
            LabCase::new(
                "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                "el listado del grupo es la union de las listas de sus miembros, en orden semver",
            )
            .passed()
        },
    );

    // ---- AC1: the group's latest is the last of the union.
    a_case(
        "ac1-the-latest-of-a-group-is-the-latest-of-the-union",
        "el latest del grupo es el ultimo de la union, no el del ultimo miembro",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let answer = call(
                port,
                "GET",
                &format!("/go/{GROUP}/{PUBLIC}/@latest"),
                &token,
                None,
            );
            if answer.status != 200 {
                return LabCase::new(
                    "ac1-the-latest-of-a-group-is-the-latest-of-the-union",
                    "el latest del grupo es el ultimo de la union, no el del ultimo miembro",
                )
                .failed(format!("the latest answered {}", text_of(&answer)));
            }
            let body: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
            if body["Version"].as_str() != Some(PUBLIC_VERSION) {
                return LabCase::new(
                    "ac1-the-latest-of-a-group-is-the-latest-of-the-union",
                    "el latest del grupo es el ultimo de la union, no el del ultimo miembro",
                )
                .failed(format!("the latest document is {body}"));
            }
            LabCase::new(
                "ac1-the-latest-of-a-group-is-the-latest-of-the-union",
                "el latest del grupo es el ultimo de la union, no el del ultimo miembro",
            )
            .passed()
        },
    );

    // ---- AC2: the three artifacts of a version come from the same member.
    a_case(
        "ac2-the-three-artifacts-of-a-version-come-from-one-member",
        "el info, el mod y el zip de una version salen del mismo miembro",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac2-the-three-artifacts-of-a-version-come-from-one-member";
            let title = "el info, el mod y el zip de una version salen del mismo miembro";
            let mut digests = Vec::new();
            for extension in ["info", "mod", "zip"] {
                let answer = call(
                    port,
                    "GET",
                    &format!("/go/{GROUP}/{PUBLIC}/@v/{PUBLIC_VERSION}.{extension}"),
                    &token,
                    None,
                );
                if answer.status != 200 {
                    return LabCase::new(id, title)
                        .failed(format!("the {extension} answered {}", text_of(&answer)));
                }
                digests.push((extension, answer.body.clone()));
            }
            // The `.mod` and the `.zip` come from the same member: the `.mod`
            // declares the module and the `.zip` carries it inside, and the `go`
            // client checks it when it unzips.
            let declared = digests
                .iter()
                .find(|(extension, _)| *extension == "mod")
                .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
                .unwrap_or_default();
            if !declared.contains(&format!("module {PUBLIC}")) {
                return LabCase::new(id, title).failed(format!(
                    "the mod served does not declare the module: {declared:?}"
                ));
            }
            LabCase::new(id, title).passed()
        },
    );

    // ---- AC1: the real `go` client downloads a hosted module under the group.
    a_case(
        "ac1-the-real-go-client-downloads-a-hosted-module-under-the-group",
        "el cliente go real baja un modulo hosted bajo el grupo y su go.sum es valido",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac1-the-real-go-client-downloads-a-hosted-module-under-the-group";
            let title =
                "el cliente go real baja un modulo hosted bajo el grupo y su go.sum es valido";
            let Some(binary) = the_go_binary() else {
                return LabCase::new(id, title).not_run("the Go SDK is not installed");
            };
            let _ = binary;
            let tunnel = bridge(&deployment.directory.path().join("tls"), port);
            let client = Client::new(&deployment, &tunnel, &token, "goclient-hosted");
            let check = |label: &str, output: &std::process::Output| {
                assert!(
                    output.status.success(),
                    "{label}: {}\n--- hub log ---\n{}",
                    String::from_utf8_lossy(&output.stderr),
                    deployment.the_log()
                );
            };
            check("go mod init", &client.run(&["mod", "init", "probe"]));
            check(
                "go mod edit",
                &client.run(&[
                    "mod",
                    "edit",
                    "-require",
                    &format!("{OWNED}@{OWNED_VERSION}"),
                ]),
            );
            let output = client.run(&["mod", "download", OWNED]);
            check("go mod download", &output);
            // The client's `go.sum` describes what Hodei served: the
            // `.ziphash` that the client itself wrote says so.
            let sum = client.the_sum();
            if !sum.contains(OWNED) || !sum.contains("h1:") {
                return LabCase::new(id, title).failed(format!("the go.sum is {sum:?}"));
            }
            if client.the_ziphash(OWNED, OWNED_VERSION).is_none() {
                return LabCase::new(id, title)
                    .failed("the go client did not record the integrity of the download");
            }
            LabCase::new(id, title).passed()
        },
    );

    // ---- AC1: the real `go` client downloads a public module from the proxy.
    a_case(
        "ac1-the-real-go-client-downloads-a-public-module-under-the-group",
        "el cliente go real baja un modulo publico servido por el miembro proxy del grupo",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac1-the-real-go-client-downloads-a-public-module-under-the-group";
            let title =
                "el cliente go real baja un modulo publico servido por el miembro proxy del grupo";
            if the_go_binary().is_none() {
                return LabCase::new(id, title).not_run("the Go SDK is not installed");
            }
            let tunnel = bridge(&deployment.directory.path().join("tls2"), port);
            let client = Client::new(&deployment, &tunnel, &token, "goclient-public");
            let check = |label: &str, output: &std::process::Output| {
                assert!(
                    output.status.success(),
                    "{label}: {}\n--- hub log ---\n{}",
                    String::from_utf8_lossy(&output.stderr),
                    deployment.the_log()
                );
            };
            check("go mod init", &client.run(&["mod", "init", "probe"]));
            check(
                "go mod edit",
                &client.run(&[
                    "mod",
                    "edit",
                    "-require",
                    &format!("{PUBLIC}@{PUBLIC_VERSION}"),
                ]),
            );
            let output = client.run(&["mod", "download", PUBLIC]);
            check("go mod download", &output);
            let sum = client.the_sum();
            if !sum.contains(PUBLIC) || !sum.contains("h1:") {
                return LabCase::new(id, title).failed(format!("the go.sum is {sum:?}"));
            }
            LabCase::new(id, title).passed()
        },
    );

    // ---- AC3: without upstream, the cache serves what was downloaded and nothing else.
    a_case(
        "ac3-without-upstream-the-cache-serves-what-was-downloaded-and-no-more",
        "sin upstream, la cache sirve lo descargado y no inventa lo que no se descarg",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac3-without-upstream-the-cache-serves-what-was-downloaded-and-no-more";
            let title =
                "sin upstream, la cache sirve lo descargado y no inventa lo que no se descarg";
            // What was already downloaded from the upstream is served from the cache.
            let before = the_list(port, PUBLIC);
            assert_eq!(before.status, 200, "{}", text_of(&before));
            upstream.disconnect();
            let cached = the_list(port, PUBLIC);
            if cached.status != 200 {
                return LabCase::new(id, title)
                    .failed(format!("the cached module answered {}", text_of(&cached)));
            }
            // And a version that was never downloaded is not served: it is not invented.
            let never = call(
                port,
                "GET",
                &format!("/go/{GROUP}/{PUBLIC}/@v/v9.9.9.zip"),
                &token,
                None,
            );
            if never.status == 200 {
                return LabCase::new(id, title)
                    .failed("a version that was never downloaded was served");
            }
            LabCase::new(id, title).passed()
        },
    );
}

/// The divergent version: the same version with different content in two
/// members.
///
/// The group **rejects** with a defined error: the go protocol has no
/// way to say "there are two", and serving any one of them would be lying to the
/// client.
#[test]
fn a_divergent_version_is_rejected_with_a_defined_error() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    // A second hosted member, with the same module and the same version, but
    // with other content. It is seeded **before** starting the service: the
    // database is locked by the process that opens it.
    deployment.seed_go(&[(INTERNAL, Value::Null), ("go-other", Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    deployment.seed_go_group(GROUP, &[INTERNAL, "go-other", CENTRAL]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let environment = the_environment(&upstream);
    let service = deployment.start(
        &environment
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>(),
    );
    let port = service.port;

    // Both members have the same version with different content: the
    // first with the canonical zip, the second with other bytes.
    publish_version(
        port,
        INTERNAL,
        &token,
        OWNED,
        OWNED_VERSION,
        &module_zip(OWNED, OWNED_VERSION, &go_mod(OWNED)),
    );
    publish_version(
        port,
        "go-other",
        &token,
        OWNED,
        OWNED_VERSION,
        b"the other content of the same version",
    );

    a_case(
        "ac2-a-divergent-version-is-rejected",
        "una version divergente entre dos miembros se rechaza con un error definido",
        Scope::Product,
        Expectation::Refused,
        || {
            let id = "ac2-a-divergent-version-is-rejected";
            let title =
                "una version divergente entre dos miembros se rechaza con un error definido";
            let answer = call(
                port,
                "GET",
                &format!("/go/{GROUP}/{OWNED}/@v/{OWNED_VERSION}.zip"),
                &token,
                None,
            );
            if answer.status != 409 {
                return LabCase::new(id, title).failed(format!(
                    "the divergent version answered {}: {}",
                    answer.status,
                    text_of(&answer)
                ));
            }
            let body = text_of(&answer);
            if !body.contains("different content") {
                return LabCase::new(id, title)
                    .failed(format!("the refusal does not explain the conflict: {body}"));
            }
            LabCase::new(id, title).refused(body)
        },
    );
}

/// A reserved case is not requested from the group's public upstream.
#[test]
fn a_reserved_namespace_never_falls_back_to_the_public_member() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    // The group reserves the namespace of the public module: the group's
    // upstream never serves it. The reservation is judged by the **name** of the
    // namespace (here, the host of the module path), which is what the
    // namespace policy knows how to compare.
    deployment.seed_groups_raw(&[serde_json::json!({
        "key": GROUP,
        "members": [INTERNAL, CENTRAL],
        "format": "go",
        "namespaces": { "reserved": ["example.com"] },
    })]);
    let environment = the_environment(&upstream);
    let service = deployment.start(
        &environment
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>(),
    );
    let port = service.port;

    upstream.put(
        &format!("{PUBLIC}/@v/list"),
        format!("{PUBLIC_VERSION}\n").as_bytes(),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.info"),
        &info_document(PUBLIC_VERSION),
        None,
    );

    a_case(
        "ac2-a-reserved-namespace-never-falls-back",
        "un caso reservado no se pide al upstream publico del grupo",
        Scope::Product,
        Expectation::Refused,
        || {
            let id = "ac2-a-reserved-namespace-never-falls-back";
            let title = "un caso reservado no se pide al upstream publico del grupo";
            let answer = the_list(port, PUBLIC);
            if answer.status == 200 {
                return LabCase::new(id, title)
                    .failed("the reserved module was served from the public member");
            }
            // And not by the path of a version either: the reserved case does
            // not fall back either through the listing or through the artifact.
            let version = call(
                port,
                "GET",
                &format!("/go/{GROUP}/{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
                &support::token(support::FIRST_ACCOUNT, "alice"),
                None,
            );
            if version.status == 200 {
                return LabCase::new(id, title)
                    .failed("the reserved version was served from the public member");
            }
            let asked = upstream.hits(&format!("{PUBLIC}/@v/list"));
            if asked != 0 {
                return LabCase::new(id, title).failed(format!(
                    "the reserved module reached the upstream {asked} times"
                ));
            }
            LabCase::new(id, title).refused(text_of(&answer))
        },
    );
}

/// A down member of the group does not stop resolution.
///
/// The group has the **proxy member first**, and the service starts **without
/// network**: the proxy does not go out to the network and has no cached copy of the module
/// that the hosted publishes. A down member is a MISS: the group continues with the
/// next member, and only if **nothing** answered does the error come out.
#[test]
fn a_fallen_member_of_the_group_does_not_stop_the_resolution() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    // The proxy goes **first**: precedence decides, and it is the
    // down member that is consulted before the hosted one.
    deployment.seed_go_group(GROUP_FALLEN, &[CENTRAL, INTERNAL]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    // The service starts without network: the proxy does not go out to the network and has no
    // cached copy of the hosted's module.
    let environment = the_environment(&upstream);
    let mut environment: Vec<(&str, &str)> = environment
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    environment.push(("HODEI_PROXY__OFFLINE", "true"));
    let service = deployment.start(&environment);
    let port = service.port;

    // The hosted member publishes its module **after** starting the
    // service: this way the module is not in the proxy's cache, and the case
    // measures that the group continues with the next member.
    publish_version(
        port,
        INTERNAL,
        &token,
        OWNED,
        OWNED_VERSION,
        &module_zip(OWNED, OWNED_VERSION, &go_mod(OWNED)),
    );

    a_case(
        "ac3-a-fallen-member-does-not-stop-the-group",
        "un miembro caido del grupo no detiene la resolucion",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac3-a-fallen-member-does-not-stop-the-group";
            let title = "un miembro caido del grupo no detiene la resolucion";
            let answer = call(
                port,
                "GET",
                &format!("/go/{GROUP_FALLEN}/{OWNED}/@v/list"),
                &token,
                None,
            );
            if answer.status != 200 {
                return LabCase::new(id, title).failed(format!(
                    "the list answered {}: {}",
                    answer.status,
                    text_of(&answer)
                ));
            }
            let body = text_of(&answer);
            if !body.contains(OWNED_VERSION) {
                return LabCase::new(id, title)
                    .failed(format!("the list of the hosted module is {body:?}"));
            }
            LabCase::new(id, title).passed()
        },
    );
}

/// A proxy on the **trusted** channel does not serve what has no grant.
///
/// The case brings **two** proxies to the same upstream: one on the open channel (the
/// control) and another on the trust channel. The control is what
/// distinguishes "the gate works" from "the proxy is broken".
#[test]
fn a_trusted_proxy_does_not_serve_what_was_not_granted() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    // Both proxies point to the same upstream: what separates them is the
    // channel, and nothing else.
    deployment.seed_go_proxies_trusted(
        &[TRUSTED_PROXY],
        &[
            (CENTRAL, proxy_of(&upstream)),
            (TRUSTED_PROXY, proxy_of(&upstream)),
        ],
    );
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    upstream.put(
        &format!("{PUBLIC}/@v/list"),
        format!("{PUBLIC_VERSION}\n").as_bytes(),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.info"),
        &info_document(PUBLIC_VERSION),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.mod"),
        &go_mod(PUBLIC),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
        &module_zip(PUBLIC, PUBLIC_VERSION, &go_mod(PUBLIC)),
        None,
    );

    let environment = the_environment(&upstream);
    let service = deployment.start(
        &environment
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>(),
    );
    let port = service.port;

    a_case(
        "ac2-a-trusted-proxy-does-not-serve-what-was-not-granted",
        "un proxy en el canal trusted no sirve lo que no tiene concesion",
        Scope::Product,
        Expectation::Refused,
        || {
            let id = "ac2-a-trusted-proxy-does-not-serve-what-was-not-granted";
            let title = "un proxy en el canal trusted no sirve lo que no tiene concesion";
            let trusted = call(
                port,
                "GET",
                &format!("/go/{TRUSTED_PROXY}/{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
                &token,
                None,
            );
            if trusted.status == 200 {
                return LabCase::new(id, title)
                    .failed("the trusted proxy served content that was not granted");
            }
            // The control: the same upstream, the same content and the open
            // channel **is** served. Without this, the case would not distinguish the
            // gate from a broken proxy.
            let open = call(
                port,
                "GET",
                &format!("/go/{CENTRAL}/{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
                &token,
                None,
            );
            if open.status != 200 {
                return LabCase::new(id, title).failed(format!(
                    "the open proxy did not serve the content: {}",
                    text_of(&open)
                ));
            }
            LabCase::new(id, title)
                .refused(format!("the trusted proxy answered {}", trusted.status))
        },
    );
}

/// The **HEAD** of a proxy path declares the real size of the asset.
///
/// Without a body, a HEAD can only state the size by header: a
/// `Content-Length` of zero describes an empty asset, not a zip that does have
/// a size.
#[test]
fn the_head_of_a_proxy_path_carries_the_content_length() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    deployment.seed_go_group(GROUP, &[INTERNAL, CENTRAL]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    upstream.put(
        &format!("{PUBLIC}/@v/list"),
        format!("{PUBLIC_VERSION}\n").as_bytes(),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.info"),
        &info_document(PUBLIC_VERSION),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.mod"),
        &go_mod(PUBLIC),
        None,
    );
    upstream.put(
        &format!("{PUBLIC}/@v/{PUBLIC_VERSION}.zip"),
        &module_zip(PUBLIC, PUBLIC_VERSION, &go_mod(PUBLIC)),
        None,
    );

    let environment = the_environment(&upstream);
    let service = deployment.start(
        &environment
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect::<Vec<_>>(),
    );
    let port = service.port;

    a_case(
        "ac1-the-head-of-a-proxy-path-carries-the-content-length",
        "el HEAD de una ruta proxy declara el tamano real del asset",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac1-the-head-of-a-proxy-path-carries-the-content-length";
            let title = "el HEAD de una ruta proxy declara el tamano real del asset";
            let path = format!("/go/{GROUP}/{PUBLIC}/@v/{PUBLIC_VERSION}.zip");
            // The GET states the real size: the body that is served.
            let body = call(port, "GET", &path, &token, None);
            if body.status != 200 {
                return LabCase::new(id, title)
                    .failed(format!("the zip answered {}", text_of(&body)));
            }
            let length = body.body.len();
            let head = call(port, "HEAD", &path, &token, None);
            if head.status != 200 {
                return LabCase::new(id, title)
                    .failed(format!("the head answered {}", text_of(&head)));
            }
            let Some(declared) = head.header("content-length") else {
                return LabCase::new(id, title)
                    .failed("the head of the proxy path did not declare the content length");
            };
            if declared != length.to_string() {
                return LabCase::new(id, title).failed(format!(
                    "the head declared {declared} and the zip served {length} bytes"
                ));
            }
            LabCase::new(id, title).passed()
        },
    );
}

/// A partitioned-scope member is not served: the group says so with the
/// scope diagnostic.
///
/// This release only serves the **global scope**. A member whose content
/// is of partitioned scope is not served, and that is said plainly: a 404 would be
/// lying, because the repository exists and what is not served is its
/// content.
///
/// The case brings the **control**: the same module, published by the
/// global-scope member, is served by the group without the partitioned member. Without
/// that control, the case would not distinguish "the scope policy" from "a group
/// that has nothing to serve".
#[test]
fn a_member_with_a_partitioned_scope_is_not_served() {
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    // The partitioned-scope member: its content is not served by this
    // release, and the seeder declares it as such.
    deployment.seed_go_partitioned(&[(PARTITIONED, Value::Null)]);
    // Two groups with the same global member: one with the
    // partitioned member and another without it.
    deployment.seed_go_group(GROUP, &[INTERNAL]);
    deployment.seed_go_group(GROUP_SCOPE, &[INTERNAL, PARTITIONED]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let environment: Vec<(&str, &str)> = Vec::new();
    let service = deployment.start(&environment);
    let port = service.port;

    // The global-scope member publishes its module: it is what the group
    // serves, and what the group with the partitioned member has to
    // **declare** that it does not serve.
    publish_version(
        port,
        INTERNAL,
        &token,
        OWNED,
        OWNED_VERSION,
        &module_zip(OWNED, OWNED_VERSION, &go_mod(OWNED)),
    );

    a_case(
        "ac2-a-member-with-a-partitioned-scope-is-not-served",
        "un miembro de ambito particionado no se sirve: el grupo lo dice con el diagnostico del ambito",
        Scope::Product,
        Expectation::Refused,
        || {
            let id = "ac2-a-member-with-a-partitioned-scope-is-not-served";
            let title = "un miembro de ambito particionado no se sirve: el grupo lo dice con el diagnostico del ambito";
            // The control: the group without the partitioned member serves the
            // module, and the listing is the same one the other group would serve.
            let control = the_list(port, OWNED);
            if control.status != 200 {
                return LabCase::new(id, title).failed(format!(
                    "the group without the partitioned member answered {}: {}",
                    control.status,
                    text_of(&control)
                ));
            }
            // The listing of the group with the partitioned member: it is not
            // served, and the reason is the scope.
            let list = call(
                port,
                "GET",
                &format!("/go/{GROUP_SCOPE}/{OWNED}/@v/list"),
                &token,
                None,
            );
            if list.status != 501 {
                return LabCase::new(id, title).failed(format!(
                    "the list of the group with the partitioned member answered {}: {}",
                    list.status,
                    text_of(&list)
                ));
            }
            let body = text_of(&list);
            if !body.contains(SCOPE_DIAGNOSTIC) {
                return LabCase::new(id, title)
                    .failed(format!("the refusal does not say the scope: {body}"));
            }
            // And the resolution of a version: the group cannot choose an
            // owner that cannot be served.
            let version = call(
                port,
                "GET",
                &format!("/go/{GROUP_SCOPE}/{OWNED}/@v/{OWNED_VERSION}.zip"),
                &token,
                None,
            );
            if version.status != 501 {
                return LabCase::new(id, title).failed(format!(
                    "the version of the group with the partitioned member answered {}: {}",
                    version.status,
                    text_of(&version)
                ));
            }
            LabCase::new(id, title).refused(body)
        },
    );
}
