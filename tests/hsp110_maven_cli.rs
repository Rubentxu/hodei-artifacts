//! Maven 2 proxy y group de HSP-110 contra el cliente real (`mvn`).
//!
//! Un `mvn dependency:get` resuelve un artefacto a traves de un group que
//! tiene un hosted y un proxy de un "Central" de laboratorio: la primera vez
//! baja del upstream y la segunda, con el servicio en modo offline, sale de
//! lo cacheado sin que el upstream reciba una peticion. Maven comprueba los
//! checksums que el group le sirve, asi que un sidecar que no fuese el del
//! fichero servido haria fallar la resolucion.
//!
//! Hace falta `mvn` en el PATH: sin el, la suite **falla** diciendo que
//! falta. La credencial va en un `settings.xml` temporal, nunca en argv, y de
//! la cache local del usuario solo se borra el grupo de esta prueba.

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use support::{Deployment, FIRST_ACCOUNT};

const GET: &str = "org.apache.maven.plugins:maven-dependency-plugin:3.6.1:get";
const GROUP: &str = "hodei.hsp110";
const JAR: &[u8] = b"jar bytes that a real maven client resolves through the group";

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Un "Central" de laboratorio: sirve lo que se le da y cuenta peticiones.
struct Central {
    port: u16,
    requests: Arc<AtomicUsize>,
}

impl Central {
    fn start(files: Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the address").port();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);
        let files = Arc::new(files);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let files = Arc::clone(&files);
                let counter = Arc::clone(&counter);
                std::thread::spawn(move || serve(stream, &files, &counter));
            }
        });
        Self { port, requests }
    }

    fn total(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn serve(mut stream: TcpStream, files: &[(String, Vec<u8>)], counter: &AtomicUsize) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    counter.fetch_add(1, Ordering::SeqCst);
    let text = String::from_utf8_lossy(&request).into_owned();
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_string();
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

/// El grupo de la prueba en el repositorio local de Maven, que se borra al
/// acabar para no dejar nada en la cache del usuario.
struct LocalGroup(PathBuf);

impl LocalGroup {
    fn new() -> Self {
        let home = std::env::var_os("HOME").expect("HOME");
        let path = PathBuf::from(home)
            .join(".m2")
            .join("repository")
            .join(GROUP.replace('.', "/"));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }

    fn forget(&self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }

    fn jar(&self) -> PathBuf {
        self.0.join("lib").join("1.0").join("lib-1.0.jar")
    }
}

impl Drop for LocalGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn require_maven() {
    let found = Command::new("mvn")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the Maven CLI is not installed: the Maven suite needs `mvn` in the PATH"
    );
}

fn settings(dir: &Path, token: &str) -> PathBuf {
    let settings = dir.join("settings.xml");
    std::fs::write(
        &settings,
        format!(
            "<settings><servers><server><id>hodei</id><configuration><httpHeaders>\
             <property><name>Authorization</name><value>Bearer {token}</value></property>\
             </httpHeaders></configuration></server></servers></settings>"
        ),
    )
    .expect("the settings are written");
    settings
}

/// Resuelve `GROUP:lib:1.0` a traves de `repository`; devuelve exito y salida.
fn resolve(dir: &Path, settings: &Path, port: u16, repository: &str) -> (bool, String) {
    let output = Command::new("mvn")
        .arg("--batch-mode")
        .arg("--settings")
        .arg(settings)
        .arg(GET)
        .arg(format!("-Dartifact={GROUP}:lib:1.0"))
        .arg("-Dtransitive=false")
        .arg(format!(
            "-DremoteRepositories=hodei::default::http://127.0.0.1:{port}/repository/{repository}"
        ))
        .current_dir(dir)
        .output()
        .expect("mvn runs");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

#[test]
fn mvn_resolves_through_a_group_and_again_offline_from_the_cache() {
    require_maven();
    let prefix = GROUP.replace('.', "/");
    let pom = format!(
        "<project><modelVersion>4.0.0</modelVersion><groupId>{GROUP}</groupId>\
         <artifactId>lib</artifactId><version>1.0</version></project>"
    );
    let central = Central::start(vec![
        (format!("{prefix}/lib/1.0/lib-1.0.pom"), pom.into_bytes()),
        (format!("{prefix}/lib/1.0/lib-1.0.jar"), JAR.to_vec()),
    ]);
    let deployment = Deployment::new();
    deployment.seed_maven(&[("mvn-hosted", serde_json::Value::Null)]);
    deployment.seed_maven_proxies(&[(
        "central",
        serde_json::Value::Null,
        serde_json::json!({
            "upstream": {
                "id": "central",
                "url": format!("http://127.0.0.1:{}/", central.port),
                "scopes": ["/"],
                "max_bytes": 4096,
            },
        }),
    )]);
    deployment.seed_maven_groups(&[("mvn-all", &["mvn-hosted", "central"])]);
    let allowed = format!("127.0.0.1:{};private;http", central.port);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let settings = settings(deployment.directory.path(), &token);
    let local = LocalGroup::new();

    // Primera vez: baja de Central a traves del proxy.
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let (ok, output) = resolve(
        deployment.directory.path(),
        &settings,
        service.port,
        "mvn-all",
    );
    assert!(ok, "mvn did not resolve through the group:\n{output}");
    assert_eq!(
        std::fs::read(local.jar()).expect("the jar was resolved"),
        JAR
    );
    assert!(
        !output.contains("Checksum validation failed"),
        "the group served a checksum that is not the one of the file:\n{output}"
    );
    assert!(central.total() > 0, "nothing came from the upstream");
    support::stop(&mut service);

    // Segunda vez, sin red y con la cache local de Maven vacia: sale de lo
    // que el proxy guardo, y el upstream no se entera.
    local.forget();
    let before = central.total();
    let mut offline = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let (ok, output) = resolve(
        deployment.directory.path(),
        &settings,
        offline.port,
        "mvn-all",
    );
    assert!(ok, "mvn did not resolve offline from the group:\n{output}");
    assert_eq!(
        std::fs::read(local.jar()).expect("the jar was resolved"),
        JAR
    );
    assert!(
        !output.contains("Checksum validation failed"),
        "the offline group served a checksum that is not the one of the file:\n{output}"
    );
    assert_eq!(central.total(), before, "offline touched the network");
    support::stop(&mut offline);
}
