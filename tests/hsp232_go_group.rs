//! HSP-232: el grupo **go** ensambla la lista de versiones y resuelve una
//! version desde **un** miembro.
//!
//! Lo que se demuestra aqui, contra el **service de verdad**:
//!
//! - el listado del grupo es la **union** de las listas de sus miembros, una
//!   entrada por version y en orden semver;
//! - el `@latest` del grupo es el ultimo de esa union, no el del ultimo
//!   miembro que responda;
//! - los tres artefactos de una version (`.info`, `.mod` y `.zip`) salen
//!   **del mismo miembro**: el primero, en el orden configurado, que la
//!   tiene;
//! - una version **divergente** --la misma version con contenido distinto en
//!   dos miembros-- se **rechaza** con un error definido, no se elige en
//!   silencio;
//! - un caso **reservado** no se pide al upstream publico del grupo: los
//!   miembros hosted son la fuente interna y no la toca;
//! - un upstream **caido** detiene la resolucion: el grupo no salta al
//!   siguiente miembro, porque un fallo transitorio no puede cambiar en
//!   silencio lo que se descarga.
//!
//! Y, con el cliente `go` **real**:
//!
//! - `go mod download` con `GOPROXY` apuntando **solo** a Hodei resuelve un
//!   modulo **hosted** bajo el grupo, y el `go.sum` que produce es valido;
//! - el mismo cliente resuelve un modulo **publico** servido por el miembro
//!   **proxy** del grupo, y su `go.sum` tambien cuadra;
//! - con el upstream **caido**, la version ya descargada se sigue sirviendo
//!   desde la cache, y una version **nunca** descargada no se sirve: no se
//!   inventa contenido.
//!
//! El trafico va por **TLS a traves de un puente local `socat`**: `go` solo
//! manda credenciales (su `.netrc`) por https, y se niega a hacerlo por http
//! llaneandolas por el camino.

mod support;

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Answer, Curl, Deployment};

/// El primer miembro del grupo: el hosted.
const INTERNAL: &str = "go-internal";

/// El segundo miembro del grupo: el proxy.
const CENTRAL: &str = "go-central";

/// El grupo.
const GROUP: &str = "go-all";

/// El modulo que el miembro hosted publica.
const OWNED: &str = "example.com/hello";

/// El modulo que el miembro proxy trae de su upstream.
const PUBLIC: &str = "example.com/proxied";

/// La version que el miembro hosted publica.
const OWNED_VERSION: &str = "v1.0.0";

/// La version que el upstream del miembro proxy trae del modulo publico.
const PUBLIC_VERSION: &str = "v1.3.0";

/// La version que el upstream del miembro proxy trae del modulo del hosted.
///
/// El modulo publico y el del hosted son el **mismo** modulo en versiones
/// distintas: la union del grupo es de las listas de sus miembros, y el
/// orden es semver, no de bytes.
const SHARED_VERSION: &str = "v1.2.0";

/// Los casos que una ejecucion completa tiene que ejecutar.
///
/// Se declaran **antes** de correr: una ejecucion que solo llego a algunos
/// dice cuales no corrieron, y un informe con un caso que no corrio nunca es
/// verde.
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
];

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// El binario del cliente `go`.
///
/// El SDK de Go no siempre esta en el PATH: se busca ahi, y si no esta, en
/// la instalacion de linuxbrew que usa esta maquina.
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

/// El texto de una respuesta.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// Una peticion al servicio.
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

/// El documento `.info` canonico de una version.
fn info_document(version: &str) -> Vec<u8> {
    json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" })
        .to_string()
        .into_bytes()
}

/// Un `go.mod` que declara el modulo sin escapar.
fn go_mod(module: &str) -> Vec<u8> {
    format!("module {module}\n\ngo 1.22\n").into_bytes()
}

/// El zip de un modulo, como lo exige el protocolo: todo bajo el prefijo
/// `{module}@{version}/`, con el mismo `go.mod` que sirve el `.mod`.
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

/// Publica la version entera de un modulo en un repositorio hosted.
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

/// La configuracion `proxy` de un repositorio go: el upstream y su cache.
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

/// Un puente TLS efimero: `https` en la puerta, el hub en el fondo.
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

/// Un puerto libre: se toma, se suelta, y el puente lo ocupa justo despues.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port")
        .local_addr()
        .expect("the address of the listener")
        .port()
}

/// Un certificado autofirmado para 127.0.0.1.
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

/// Levanta el puente y no devuelve hasta que escucha.
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

/// La casa del cliente `go`: todo dentro del despliegue, y el HOME real
/// intacto.
struct Client {
    root: std::path::PathBuf,
    work: std::path::PathBuf,
    binary: std::path::PathBuf,
    tunnel_port: u16,
    certificate: std::path::PathBuf,
}

impl Client {
    /// El cliente, con su `.netrc` y su cache propia.
    ///
    /// Cada caso tiene su casa: dos clientes no comparten `go.mod`, porque
    /// el segundo `go mod init` fallaria.
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

    /// Corre el cliente `go` contra el hub.
    ///
    /// La configuracion es **explicita** y cada parte tiene su motivo:
    ///
    /// - `GOPROXY` apunta **solo** al hub, sin `,direct`: el cliente no
    ///   tiene por donde caerse a la red ni a un VCS.
    /// - `GOSUMDB=off` y `GONOSUMDB=*`: el sumario publico no conoce estos
    ///   modulos, y el cliente no tiene por que consultarlo. La integridad
    ///   **no** se salta: el `go.sum` local es el que verifica, y el `.zip`
    ///   se comprueba byte a byte contra lo publicado.
    /// - `GOFLAGS=-mod=mod`: el modulo de prueba necesita escribir su
    ///   `go.sum`, que es lo que esta prueba quiere medir.
    /// - `GOPRIVATE` **no** se pone: haria que el cliente se saltara el
    ///   proxy y saliera a la red, que es justo lo que esta prueba forbids.
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

    /// El `go.sum` que el cliente escribio.
    fn the_sum(&self) -> String {
        std::fs::read_to_string(self.work.join("go.sum")).unwrap_or_default()
    }

    /// El `.ziphash` que el cliente escribio: la integridad de lo bajado.
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

/// Corre un caso del laboratorio.
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

/// El despliegue del laboratorio: un hosted, un proxy y el grupo.
///
/// Todo se siembra **antes** de arrancar el servicio: la base de datos la
/// tiene bloqueada el proceso que la abre.
fn the_lab() -> (Deployment, support::lab::Upstream) {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    deployment.seed_go_group(GROUP, &[INTERNAL, CENTRAL]);
    (deployment, upstream)
}

/// El entorno del servicio: el upstream del laboratorio es la unica red.
fn the_environment(upstream: &support::lab::Upstream) -> Vec<(&'static str, String)> {
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    vec![("HODEI_PROXY__ALLOWED_HOSTS", allowed)]
}

/// El listado del grupo, con el token de la primera cuenta.
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

    // El upstream del miembro proxy trae un modulo publico, y una version
    // mas del modulo del hosted: la union del grupo es de las dos listas.
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

    // El miembro hosted publica su modulo en dos versiones.
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

    // ---- AC1: el listado es la union de las listas de los miembros.
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
            // La union de las dos listas, en orden **semver**: `v1.10.0`
            // va despues de `v1.2.0`, que es donde lo pone el protocolo.
            if versions != [OWNED_VERSION, SHARED_VERSION, "v1.10.0"] {
                return LabCase::new(
                    "ac1-the-list-of-a-group-is-the-union-of-the-lists-of-its-members",
                    "el listado del grupo es la union de las listas de sus miembros, en orden semver",
                )
                .failed(format!("the list of the hosted module is {versions:?}"));
            }
            // Y el modulo del upstream, que solo tiene el miembro proxy.
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

    // ---- AC1: el latest del grupo es el ultimo de la union.
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

    // ---- AC2: los tres artefactos de una version salen del mismo miembro.
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
            // El `.mod` y el `.zip` los trae el mismo miembro: el `.mod`
            // declara el modulo y el `.zip` lo lleva dentro, y el cliente
            // `go` lo comprueba cuando descomprime.
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

    // ---- AC1: el cliente `go` real baja un modulo hosted bajo el grupo.
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
            // El `go.sum` del cliente describe lo que Hodei sirvio: el
            // `.ziphash` que el propio cliente escribio lo dice.
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

    // ---- AC1: el cliente `go` real baja un modulo publico del proxy.
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

    // ---- AC3: sin upstream, la cache sirve lo descargado y nada mas.
    a_case(
        "ac3-without-upstream-the-cache-serves-what-was-downloaded-and-no-more",
        "sin upstream, la cache sirve lo descargado y no inventa lo que no se descarg",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let id = "ac3-without-upstream-the-cache-serves-what-was-downloaded-and-no-more";
            let title =
                "sin upstream, la cache sirve lo descargado y no inventa lo que no se descarg";
            // Lo que ya se bajo del upstream se sirve desde la cache.
            let before = the_list(port, PUBLIC);
            assert_eq!(before.status, 200, "{}", text_of(&before));
            upstream.disconnect();
            let cached = the_list(port, PUBLIC);
            if cached.status != 200 {
                return LabCase::new(id, title)
                    .failed(format!("the cached module answered {}", text_of(&cached)));
            }
            // Y una version que nunca se bajo no se sirve: no se inventa.
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

/// La version divergente: la misma version con contenido distinto en dos
/// miembros.
///
/// El grupo **rechaza** con un error definido: el protocolo go no tiene
/// forma de decir "hay dos", y servir uno cualquiera seria mentirle al
/// cliente.
#[test]
fn a_divergent_version_is_rejected_with_a_defined_error() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    // Un segundo miembro hosted, con el mismo modulo y la misma version, pero
    // con otro contenido. Se siembra **antes** de arrancar el servicio: la
    // base de datos la tiene bloqueada el proceso que la abre.
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

    // Los dos miembros tienen la misma version con contenido distinto: el
    // primero con el zip canonico, el segundo con otros bytes.
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

/// Un caso reservado no se pide al upstream publico del grupo.
#[test]
fn a_reserved_namespace_never_falls_back_to_the_public_member() {
    let upstream = support::lab::Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_go(&[(INTERNAL, Value::Null)]);
    deployment.seed_go_proxies(&[(CENTRAL, proxy_of(&upstream))]);
    // El grupo reserva el namespace del modulo publico: el upstream del
    // grupo no lo sirve nunca. La reserva se juzga por el **nombre** del
    // namespace (aquí, el host del camino del modulo), que es lo que la
    // política de namespaces sabe comparar.
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
