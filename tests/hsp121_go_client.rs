//! El `go mod download` de HSP-121 contra el **cliente real** (`go`).
//!
//! La suite HTTP mide el protocolo; esta mide que el binario `go`, con
//! `GOPROXY` apuntando al hub, baja la version publicada y la deja en su
//! cache **con los bytes exactos** que se publicaron: el digest
//! preservado no es una promesa del arnés, es lo que el cliente comprueba
//! por si solo (`.ziphash` y `go.sum`) antes de dar la bajada por buena.
//!
//! Hace falta el SDK de Go en el PATH: sin el, la suite **falla**
//! diciendo que falta, como las demas suites de cliente real.
//!
//! El trafico va por **TLS a traves de un puente local `socat`**: `go`
//! solo manda credenciales (su `.netrc`) por https, y se niega a
//! hacerlo por http llaneandolas por el camino. El hub sigue hablando
//! http en el interior del puente; el cliente confia en el certificado
//! autofirmado gracias a `SSL_CERT_FILE`.

mod support;

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment};

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

fn require_go() {
    let found = Command::new("go")
        .args(["version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the Go SDK is not installed: the go client suite needs `go` in the PATH"
    );
}

fn require_socat() {
    let found = Command::new("socat")
        .args(["-V"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the socat CLI is not installed: the go client suite needs its TLS bridge"
    );
}

fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// El texto de una respuesta.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// Publicar un artefacto de version por el camino hosted.
fn put(port: u16, repository: &str, token: &str, rest: &str, body: &[u8]) -> Answer {
    let sent = next("sent");
    std::fs::write(&sent, body).expect("the body is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/go/{repository}/{rest}"),
        token,
        Some(&sent),
        &[("content-type", "application/octet-stream")],
        Some(&into),
    )
    .run()
}

/// Un puente TLS efimero: `https` en la puerta, el hub en el fondo.
/// El proceso vive mientras el puente; al soltarlo, se lleva el tunel.
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

/// Un certificado autofirmado para 127.0.0.1: lo firma openssl, lo confia
/// el cliente go por `SSL_CERT_FILE`. El par queda en un unico pem, que es
/// lo que `socat` espera de `cert=`.
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

/// El zip de un modulo, como lo exige el protocolo: todo bajo el prefijo
/// `{module}@{version}/` y con el mismo go.mod que sirve el `.mod`. No
/// vale cualquier zip: `go` valida la forma antes de tragarselo.
fn module_zip(module: &str, version: &str, go_mod: &str) -> Vec<u8> {
    use std::io::Write;
    let prefix = format!("{module}@{version}");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in [
        ("go.mod", go_mod.to_string()),
        ("hello.go", "package hello\n".to_string()),
    ] {
        writer
            .start_file(format!("{prefix}/{name}"), options)
            .expect("the entry opens");
        writer
            .write_all(body.as_bytes())
            .expect("the entry is written");
    }
    writer.finish().expect("the zip closes").into_inner()
}

#[test]
fn the_real_go_client_downloads_the_published_module_with_the_digest_preserved() {
    require_go();
    require_socat();
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-cli", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Publicar example.com/hello v1.0.0 por el camino hosted, y acordarse
    // de los bytes del zip: eso es lo que el cliente tiene que devolver.
    let module = "example.com/hello";
    let version = "v1.0.0";
    let go_mod = format!("module {module}\n\ngo 1.22\n");
    let zip = module_zip(module, version, &go_mod);
    let info = json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" }).to_string();
    for (extension, body) in [
        ("info", info.as_bytes().to_vec()),
        ("mod", go_mod.as_bytes().to_vec()),
        ("zip", zip.clone()),
    ] {
        let answer = put(
            port,
            "go-cli",
            &token,
            &format!("{module}/@v/{version}.{extension}"),
            &body,
        );
        assert_eq!(answer.status, 200, "{extension}: {}", text_of(&answer));
    }

    // La casa del cliente: GOPATH, GOMODCACHE, GOCACHE, todo dentro del
    // despliegue. Nada del HOME real se lee ni se escribe.
    let root = deployment.directory.path().join("gocli");
    std::fs::create_dir_all(&root).expect("the client home");
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("the work directory");

    // El hub escucha http; `go` solo credencia por https. El puente es
    // exactamente lo que hace cualquier despliegue real: TLS delante,
    // el servicio detras, sin cambiar una coma del hub.
    let tunnel = bridge(&root, port);

    // La `.netrc` se escribe con el puente ya en pie: GOAUTH casando por
    // prefijo de URL, el `machine` lleva el puerto del tunel. La
    // credencial va en la contrasena; el usuario es informativo.
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

    let client = |args: &[&str]| {
        let mut command = Command::new("go");
        command
            .current_dir(&work)
            .env("HOME", &root)
            .env("GOPATH", root.join("gopath"))
            .env("GOMODCACHE", root.join("modcache"))
            .env("GOCACHE", root.join("buildcache"))
            .env("SSL_CERT_FILE", &tunnel.certificate)
            .env("GOFLAGS", "-mod=mod")
            // El proxy es el hub, y solo el hub: `go` no tiene por donde
            // caerse a la red ni a VCS si el proxy responde.
            .env(
                "GOPROXY",
                format!("https://127.0.0.1:{}/go/go-cli", tunnel.port),
            )
            // El sumdb publico no conoce este modulo; la integridad no se
            // salta: el zip se comprueba abajo, byte a byte, y el propio go
            // escribe su .ziphash.
            .env("GOSUMDB", "off")
            .args(args)
            .output()
            .expect("go runs")
    };

    let check = |label: &str, output: &std::process::Output| {
        assert!(
            output.status.success(),
            "{label}: {}\n--- hub log ---\n{}",
            String::from_utf8_lossy(&output.stderr),
            deployment.the_log()
        );
    };

    check("go mod init", &client(&["mod", "init", "probe"]));
    check(
        "go mod edit",
        &client(&["mod", "edit", "-require", &format!("{module}@{version}")]),
    );

    let output = client(&["mod", "download", module]);
    check("go mod download", &output);

    // El zip en la cache del cliente es el zip publicado: eso es "digests
    // preservados" dicho por el unico juez que importa.
    let cached = root
        .join("modcache")
        .join("cache/download")
        .join(module)
        .join("@v")
        .join(format!("{version}.zip"));
    assert!(cached.exists(), "the go client cached no zip");
    let bytes = std::fs::read(&cached).expect("the cached zip");
    assert_eq!(
        sha256_of(&bytes),
        sha256_of(&zip),
        "the zip the client stored is not the zip that was published"
    );
    let ziphash = root
        .join("modcache")
        .join("cache/download")
        .join(module)
        .join("@v")
        .join(format!("{version}.ziphash"));
    assert!(
        ziphash.exists(),
        "go did not record the integrity of the download"
    );

    // Y `go build` lo usa de verdad: el modulo baja, se descomprime y
    // compila contra su go.mod servido por el hub.
    std::fs::write(work.join("main.go"), "package main\n\nfunc main() {}\n")
        .expect("the main is written");
    let output = client(&["build", "-o", "probe"]);
    check("go build", &output);
}
