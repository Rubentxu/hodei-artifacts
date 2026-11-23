//! La reserva de namespaces de HSP-143 / HAE-10 contra el cliente real
//! (`npm`).
//!
//! El `npm` publica `@lab/widget` en el hosted de la casa y resuelve contra
//! un group que tiene ese hosted y un proxy del registro de laboratorio.
//! El upstream es el atacante de la dependency confusion: tiene
//! `@lab/widget` con version mayor y un nombre reservado que el hosted no
//! tiene. Lo que se mide con el cliente de verdad:
//!
//! - lo interno permitido resuelve (el group sirve el hosted);
//! - lo publico no reservado resuelve (el proxy trae del upstream);
//! - lo reservado **nunca** sale del upstream, ni por el group ni por el
//!   proxy directo, aunque la version publica sea mayor;
//! - el rechazo llega al cliente como el 403 que es, no como un 404.
//!
//! Hace falta `npm` en el PATH: sin el, la suite **falla** diciendo que
//! falta. La credencial va en un `.npmrc` temporal y cada ejecucion usa su
//! propia cache y su propio `HOME`.

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use sha2::{Digest, Sha512};
use support::{Deployment, FIRST_ACCOUNT};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn require_npm() {
    let found = Command::new("npm")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the npm CLI is not installed: the npm suite needs `npm` in the PATH"
    );
}

/// El resultado de una ejecucion de `npm`.
struct Run {
    ok: bool,
    output: String,
}

/// Un cliente `npm` aislado: su `HOME`, su cache y su `.npmrc`.
struct Client {
    root: PathBuf,
    rc: PathBuf,
}

impl Client {
    /// Un cliente que habla con `/npm/{repositorio}/` del servicio.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let root = root.join(name);
        std::fs::create_dir_all(root.join("home")).expect("the home");
        let rc = root.join("npmrc");
        std::fs::write(
            &rc,
            format!(
                "registry=http://127.0.0.1:{port}/npm/{repository}/\n\
                 //127.0.0.1:{port}/npm/{repository}/:_authToken={token}\n\
                 fetch-retries=0\n\
                 update-notifier=false\n"
            ),
        )
        .expect("the npmrc is written");
        Self { root, rc }
    }

    /// Ejecuta `npm` en `dir` con la cache de este cliente.
    fn run(&self, dir: &Path, args: &[&str]) -> Run {
        let output = Command::new("npm")
            .arg("--userconfig")
            .arg(&self.rc)
            .arg("--globalconfig")
            .arg(self.root.join("no-global-config"))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(["--no-audit", "--no-fund", "--loglevel=warn"])
            .args(args)
            .env("HOME", self.root.join("home"))
            .current_dir(dir)
            .output()
            .expect("npm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// Una cache vacia: lo siguiente que se instale sale del servicio.
    fn forget_the_cache(&self) {
        let _ = std::fs::remove_dir_all(self.root.join("cache"));
    }
}

/// Un paquete en `dir` listo para publicar.
fn package(dir: &Path, name: &str, version: &str, body: &str) {
    std::fs::create_dir_all(dir).expect("the package directory");
    std::fs::write(
        dir.join("package.json"),
        format!(
            "{{\"name\":\"{name}\",\"version\":\"{version}\",\"main\":\"index.js\",\
             \"description\":\"a package for the namespace suite\"}}"
        ),
    )
    .expect("package.json");
    std::fs::write(dir.join("index.js"), body).expect("index.js");
}

/// Un proyecto consumidor vacio.
fn consumer(dir: &Path) {
    std::fs::create_dir_all(dir).expect("the consumer directory");
    std::fs::write(
        dir.join("package.json"),
        "{\"name\":\"consumer\",\"version\":\"1.0.0\",\"private\":true}",
    )
    .expect("package.json");
}

fn integrity_of(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// Un registro de laboratorio: sirve lo que se le da y anota lo que le
/// pidieron, ruta a ruta.
struct Registry {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Registry {
    fn total(&self) -> usize {
        self.seen.lock().expect("the log").len()
    }

    /// Si alguna peticion toco una ruta que empieza por el prefijo dado.
    fn touched(&self, prefix: &str) -> bool {
        self.seen
            .lock()
            .expect("the log")
            .iter()
            .any(|path| path.starts_with(prefix))
    }
}

fn serve(mut stream: TcpStream, files: Arc<Vec<(String, Vec<u8>)>>, seen: Arc<Mutex<Vec<String>>>) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8_lossy(&request).into_owned();
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    seen.lock().expect("the log").push(path.clone());
    let response = match files.iter().find(|(name, _)| *name == path) {
        Some((_, body)) => {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            out.extend_from_slice(body);
            out
        }
        None => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

fn start_registry(files: Vec<(String, Vec<u8>)>) -> Registry {
    let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let files = Arc::new(files);
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files = Arc::clone(&files);
            let log = Arc::clone(&log);
            std::thread::spawn(move || serve(stream, files, log));
        }
    });
    Registry { port, seen }
}

#[test]
fn npm_resolves_the_internal_and_the_public_but_never_the_reserved_scope() {
    require_npm();
    let deployment = Deployment::new();
    let root = deployment.directory.path();

    // El tarball que el upstream tiene del nombre reservado: la version del
    // atacante, mayor que la interna.
    let impostor_tarball = b"impostor bytes served by the public registry".to_vec();

    // Se reserva el puerto del upstream para poder escribir su URL en los
    // packuments antes de arrancarlo.
    let probe = TcpListener::bind("127.0.0.1:0").expect("a port");
    let upstream_port = probe.local_addr().expect("the address").port();
    drop(probe);

    let impostor = serde_json::json!({
        "name": "@lab/widget",
        "dist-tags": { "latest": "9.9.9" },
        "versions": { "9.9.9": {
            "name": "@lab/widget",
            "version": "9.9.9",
            "dist": {
                "tarball": format!(
                    "http://127.0.0.1:{upstream_port}/@lab/widget/-/widget-9.9.9.tgz"
                ),
                "integrity": integrity_of(&impostor_tarball),
            },
        } },
    });
    let ghost = serde_json::json!({
        "name": "@lab/ghost",
        "dist-tags": { "latest": "2.0.0" },
        "versions": { "2.0.0": {
            "name": "@lab/ghost",
            "version": "2.0.0",
            "dist": {
                "tarball": format!(
                    "http://127.0.0.1:{upstream_port}/@lab/ghost/-/ghost-2.0.0.tgz"
                ),
                "integrity": integrity_of(&impostor_tarball),
            },
        } },
    });

    // El paquete publico que no esta reservado: se empaqueta de verdad para
    // que `npm install` lo construya.
    let source = root.join("left-pad-src");
    package(
        &source,
        "left-pad",
        "1.3.0",
        "module.exports = 'left-pad from upstream';\n",
    );
    let packed = Command::new("npm")
        .args(["pack", "--pack-destination"])
        .arg(root)
        .arg("--loglevel=error")
        .env("HOME", root)
        .current_dir(&source)
        .output()
        .expect("npm pack runs");
    assert!(packed.status.success(), "npm pack failed");
    let public_tarball =
        std::fs::read(root.join("left-pad-1.3.0.tgz")).expect("the packed tarball");
    let public_packument = serde_json::json!({
        "name": "left-pad",
        "dist-tags": { "latest": "1.3.0" },
        "versions": { "1.3.0": {
            "name": "left-pad",
            "version": "1.3.0",
            "dist": {
                "tarball": format!(
                    "http://127.0.0.1:{upstream_port}/left-pad/-/left-pad-1.3.0.tgz"
                ),
                "integrity": integrity_of(&public_tarball),
            },
        } },
    });

    let registry = start_registry(vec![
        (
            "/@lab/widget".to_string(),
            serde_json::to_vec(&impostor).unwrap(),
        ),
        (
            "/@lab/ghost".to_string(),
            serde_json::to_vec(&ghost).unwrap(),
        ),
        (
            "/@lab/widget/-/widget-9.9.9.tgz".to_string(),
            impostor_tarball.clone(),
        ),
        (
            "/left-pad".to_string(),
            serde_json::to_vec(&public_packument).unwrap(),
        ),
        ("/left-pad/-/left-pad-1.3.0.tgz".to_string(), public_tarball),
    ]);

    // La casa: hosted interno, proxy del registro publico y el group que
    // reserva el alcance `@lab`. La reserva se declara en el repositorio que
    // la aplica: en el group (para las lecturas por el group) y en el proxy
    // (para las lecturas directas al proxy).
    deployment.seed_repositories(&[
        serde_json::json!({ "key": "npm-hosted", "format": "npm" }),
        serde_json::json!({
            "key": "npm-central",
            "format": "npm",
            "proxy": {
                "upstream": {
                    "id": "central",
                    "url": format!("http://127.0.0.1:{}/", registry.port),
                    "scopes": ["/"],
                    "max_bytes": 1048576,
                },
            },
            "namespaces": { "reserved": ["@lab"] },
        }),
    ]);
    deployment.seed_groups_raw(&[serde_json::json!({
        "key": "npm-all",
        "members": ["npm-hosted", "npm-central"],
        "format": "npm",
        "namespaces": { "reserved": ["@lab"] },
    })]);

    let allowed = format!("127.0.0.1:{};private;http", registry.port);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);

    // El cliente interno publica la version 1.0.0 de verdad.
    let publisher = Client::new(root, "publisher", service.port, "npm-hosted", &token);
    let widget = root.join("widget-src");
    package(
        &widget,
        "@lab/widget",
        "1.0.0",
        "module.exports = 'widget from the house';\n",
    );
    let published = publisher.run(&widget, &["publish", "--access", "public"]);
    assert!(published.ok, "npm publish failed:\n{}", published.output);

    // AC#4: lo interno permitido resuelve a traves del group, y el upstream
    // (que tiene una version mayor del mismo nombre) no se consulta.
    let group_client = Client::new(root, "group", service.port, "npm-all", &token);
    let project = root.join("consumer-group");
    consumer(&project);
    group_client.forget_the_cache();
    let installed = group_client.run(&project, &["install", "@lab/widget"]);
    assert!(
        installed.ok,
        "npm install of the internal package failed:\n{}",
        installed.output
    );
    assert_eq!(
        std::fs::read_to_string(project.join("node_modules/@lab/widget/index.js"))
            .expect("the internal widget is installed"),
        "module.exports = 'widget from the house';\n"
    );

    // AC#1: un nombre reservado que solo tiene el upstream no aparece, ni
    // por el group ni por el proxy directo. El cliente ve el 403 que es.
    let denied = group_client.run(&project, &["view", "@lab/ghost", "version"]);
    assert!(!denied.ok, "npm resolved a reserved name from the public");
    assert!(
        denied.output.contains("403") || denied.output.contains("E403"),
        "a reserved name is not refused with a 403:\n{}",
        denied.output
    );
    let direct_client = Client::new(root, "direct", service.port, "npm-central", &token);
    let denied_direct = direct_client.run(&project, &["view", "@lab/widget", "version"]);
    assert!(
        !denied_direct.ok,
        "the proxy direct served a reserved name: {}",
        denied_direct.output
    );
    assert!(
        denied_direct.output.contains("403") || denied_direct.output.contains("E403"),
        "the direct refusal is not a 403:\n{}",
        denied_direct.output
    );

    // AC#4: lo publico no reservado resuelve, por el group y por el proxy
    // directo.
    let free_group = group_client.run(&project, &["view", "left-pad", "version"]);
    assert!(
        free_group.ok && free_group.output.trim() == "1.3.0",
        "the public name does not resolve through the group:\n{}",
        free_group.output
    );
    let free_direct = direct_client.run(&project, &["view", "left-pad", "version"]);
    assert!(
        free_direct.ok && free_direct.output.trim() == "1.3.0",
        "the public name does not resolve through the proxy:\n{}",
        free_direct.output
    );
    let installed_free = direct_client.run(&project, &["install", "left-pad"]);
    assert!(
        installed_free.ok,
        "npm install of left-pad failed:\n{}",
        installed_free.output
    );
    assert_eq!(
        std::fs::read_to_string(project.join("node_modules/left-pad/index.js"))
            .expect("left-pad is installed"),
        "module.exports = 'left-pad from upstream';\n"
    );

    // El upstream sirvio lo publico, pero del reservado no tiene ni una
    // peticion: el packument del impostor nunca fue pedido.
    assert!(registry.total() > 0, "the upstream was never asked at all");
    assert!(
        !registry.touched("/@lab/"),
        "the upstream was asked for the reserved scope: {:?}",
        registry.seen.lock().expect("the log").clone()
    );
    support::stop(&mut service);
}
