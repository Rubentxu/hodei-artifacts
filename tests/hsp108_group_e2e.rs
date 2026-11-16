//! Los repositorios virtuales (grupos) de HSP-108 / HAR-14, contra el wire.
//!
//! El servicio es un proceso de verdad con catalogo, almacen de bytes y
//! Cedar reales; el upstream es un servidor local que este fichero
//! controla. Lo que se mide es lo que una persona ve: de que miembro sale
//! cada respuesta (`x-hodei-resolved-from`), en que version de la
//! configuracion del grupo (`x-hodei-group-version`) y que pasa cuando un
//! miembro falla, no existe o no se puede leer.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use support::{Curl, Deployment, FIRST_ACCOUNT};

const HOSTED: &[u8] = b"content that was published in the hosted member";
const UPSTREAM: &[u8] = b"content that lives upstream";

/// Una peticion que llego al upstream.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
}

/// Un documento que sirve el upstream.
#[derive(Debug, Clone)]
struct Doc {
    body: Vec<u8>,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// El estado del upstream, que la prueba cambia mientras corre.
struct World {
    docs: Mutex<HashMap<String, Doc>>,
    failing: AtomicBool,
    seen: Mutex<Vec<Seen>>,
}

struct Upstream {
    port: u16,
    world: Arc<World>,
}

impl Upstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the address").port();
        let world = Arc::new(World {
            docs: Mutex::new(HashMap::new()),
            failing: AtomicBool::new(false),
            seen: Mutex::new(Vec::new()),
        });
        let shared = Arc::clone(&world);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let world = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &world));
            }
        });
        Self { port, world }
    }

    fn put(&self, path: &str, body: &[u8], etag: Option<&str>, last_modified: Option<&str>) {
        self.world.docs.lock().expect("docs").insert(
            path.to_string(),
            Doc {
                body: body.to_vec(),
                etag: etag.map(str::to_string),
                last_modified: last_modified.map(str::to_string),
            },
        );
    }

    fn fail(&self, failing: bool) {
        self.world.failing.store(failing, Ordering::SeqCst);
    }

    fn requests(&self) -> Vec<Seen> {
        self.world.seen.lock().expect("the log").clone()
    }

    fn hits(&self, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|one| one.path == path)
            .count()
    }
}

fn header_of(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn serve(mut stream: TcpStream, world: &World) {
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
        .trim_start_matches('/')
        .to_string();
    let if_none_match = header_of(&text, "if-none-match");
    let if_modified_since = header_of(&text, "if-modified-since");
    world
        .seen
        .lock()
        .expect("the log")
        .push(Seen { path: path.clone() });

    let doc = world.docs.lock().expect("docs").get(&path).cloned();
    let response: Vec<u8> = if world.failing.load(Ordering::SeqCst) {
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec()
    } else if let Some(doc) = doc {
        let validators = {
            let mut out = String::new();
            if let Some(etag) = &doc.etag {
                out.push_str(&format!("ETag: {etag}\r\n"));
            }
            if let Some(date) = &doc.last_modified {
                out.push_str(&format!("Last-Modified: {date}\r\n"));
            }
            out
        };
        let unchanged = match (&doc.etag, &if_none_match, &doc.last_modified) {
            (Some(etag), Some(sent), _) => etag == sent,
            (None, _, Some(date)) => if_modified_since.as_deref() == Some(date.as_str()),
            _ => false,
        };
        if unchanged {
            format!(
                "HTTP/1.1 304 Not Modified\r\n{validators}Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_bytes()
        } else {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{validators}Connection: close\r\n\r\n",
                doc.body.len()
            )
            .into_bytes();
            out.extend_from_slice(&doc.body);
            out
        }
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// La configuracion `proxy` de un repositorio: el upstream y su cache.
fn proxy_of(upstream: &Upstream) -> serde_json::Value {
    serde_json::json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{}/", upstream.port),
            "scopes": ["/"],
            "max_bytes": 1024,
        },
        "cache": { "ttl": { "metadata_secs": 3600 } },
    })
}

struct Lab {
    deployment: Deployment,
    upstream: Upstream,
}

impl Lab {
    /// `hosted`: repositorios hosted; `proxies`: repositorios proxy;
    /// `groups`: grupos con sus miembros, en el orden en que se crean (un
    /// grupo interno antes que el que lo contiene).
    fn new(
        hosted: &[&str],
        trusted: &[&str],
        proxies: &[&str],
        groups: &[(&str, &[&str])],
        policy: Option<&str>,
    ) -> Self {
        let upstream = Upstream::start();
        let deployment = Deployment::new();
        let proxies: Vec<(&str, serde_json::Value)> = proxies
            .iter()
            .map(|key| (*key, proxy_of(&upstream)))
            .collect();
        deployment.seed_proxies(hosted, trusted, &proxies);
        deployment.seed_groups(groups);
        if let Some(policy) = policy {
            deployment.write_policy(policy);
        }
        Self {
            deployment,
            upstream,
        }
    }

    fn start(&self) -> support::Service {
        let allowed = format!("127.0.0.1:{};private;http", self.upstream.port);
        self.deployment
            .start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())])
    }

    fn file(&self, name: &str, body: &[u8]) -> std::path::PathBuf {
        let path = self.deployment.directory.path().join(name);
        std::fs::write(&path, body).expect("the body is written");
        path
    }

    /// Publica en un repositorio hosted y devuelve el codigo.
    fn publish(&self, port: u16, repository: &str, path: &str, body: &[u8]) -> u16 {
        let alice = support::token(FIRST_ACCOUNT, "alice");
        let file = self.file("upload.bin", body);
        Curl::call(
            port,
            "PUT",
            &format!("/repository/{repository}/{path}"),
            &alice,
            Some(&file),
            None,
            None,
        )
        .run()
        .status
    }
}

/// Una respuesta: codigo, cuerpo y las cabeceras que dicen quien sirvio.
struct Reply {
    status: u16,
    body: Vec<u8>,
    resolved_from: Option<String>,
    group_version: Option<u64>,
    cache: Option<String>,
}

fn get(lab: &Lab, port: u16, repository: &str, path: &str) -> Reply {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("response.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        &alice,
        None,
        None,
        Some(&into),
    )
    .run();
    Reply {
        status: answer.status,
        body: std::fs::read(&into).unwrap_or_default(),
        resolved_from: answer.header("x-hodei-resolved-from"),
        group_version: answer
            .header("x-hodei-group-version")
            .and_then(|value| value.parse().ok()),
        cache: answer.header("x-hodei-cache"),
    }
}

fn verb(lab: &Lab, port: u16, method: &str, repository: &str, path: &str) -> u16 {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let file = lab.file("verb.bin", b"x");
    Curl::call(
        port,
        method,
        &format!("/repository/{repository}/{path}"),
        &alice,
        Some(&file),
        None,
        None,
    )
    .run()
    .status
}

// ---------------------------------------------------------------------------
// Resolucion determinista y shadowing.
// ---------------------------------------------------------------------------

#[test]
fn the_first_member_that_has_the_path_wins_and_the_headers_say_which() {
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &["pxy-a"],
        &[
            ("grp-hosted-first", &["hos-a", "pxy-a"]),
            ("grp-proxy-first", &["pxy-a", "hos-a"]),
        ],
        None,
    );
    lab.upstream.put("pkg/lib.bin", UPSTREAM, None, None);
    lab.upstream.put("pkg/only-up.bin", UPSTREAM, None, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    // Colision hosted/remote: gana el hosted, y el upstream ni se entera.
    let first = get(&lab, port, "grp-hosted-first", "pkg/lib.bin");
    assert_eq!(first.status, 200);
    assert_eq!(first.body, HOSTED);
    assert_eq!(first.resolved_from.as_deref(), Some("hos-a"));
    let version = first.group_version.expect("the group version is reported");
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 0);

    // El mismo contenido con el orden invertido: gana el remote.
    let second = get(&lab, port, "grp-proxy-first", "pkg/lib.bin");
    assert_eq!(second.status, 200);
    assert_eq!(second.body, UPSTREAM);
    assert_eq!(second.resolved_from.as_deref(), Some("pxy-a"));
    assert_eq!(second.cache.as_deref(), Some("miss"));

    // Es determinista: repetirlo da lo mismo.
    for _ in 0..3 {
        let again = get(&lab, port, "grp-hosted-first", "pkg/lib.bin");
        assert_eq!(again.body, HOSTED);
        assert_eq!(again.resolved_from.as_deref(), Some("hos-a"));
        assert_eq!(again.group_version, Some(version));
    }

    // Lo que solo tiene el upstream baja por el proxy y queda en su cache.
    let only = get(&lab, port, "grp-hosted-first", "pkg/only-up.bin");
    assert_eq!(only.status, 200);
    assert_eq!(only.resolved_from.as_deref(), Some("pxy-a"));
    assert_eq!(only.cache.as_deref(), Some("miss"));
    let cached = get(&lab, port, "grp-hosted-first", "pkg/only-up.bin");
    assert_eq!(cached.cache.as_deref(), Some("hit"));

    // Lo que no tiene nadie es un 404 limpio.
    assert_eq!(
        get(&lab, port, "grp-hosted-first", "pkg/none.bin").status,
        404
    );

    // Las rutas directas siguen siendo las de siempre.
    let direct = get(&lab, port, "hos-a", "pkg/lib.bin");
    assert_eq!(direct.body, HOSTED);
    assert_eq!(direct.resolved_from, None);
    support::stop(&mut service);
}

#[test]
fn a_group_inside_a_group_resolves_to_the_leaf_that_has_the_path() {
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &["pxy-a"],
        &[("inner", &["hos-a"]), ("outer", &["inner", "pxy-a"])],
        None,
    );
    lab.upstream.put("pkg/up.bin", UPSTREAM, None, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    let leaf = get(&lab, port, "outer", "pkg/lib.bin");
    assert_eq!(leaf.body, HOSTED);
    // El recibo nombra la hoja, no el grupo intermedio.
    assert_eq!(leaf.resolved_from.as_deref(), Some("hos-a"));
    let up = get(&lab, port, "outer", "pkg/up.bin");
    assert_eq!(up.body, UPSTREAM);
    assert_eq!(up.resolved_from.as_deref(), Some("pxy-a"));
    support::stop(&mut service);
}

#[test]
fn a_group_never_stores_content_of_its_own() {
    let lab = Lab::new(&["hos-a"], &[], &[], &[("grp", &["hos-a"])], None);
    let mut service = lab.start();
    let port = service.port;
    // El router solo enruta PUT/GET/HEAD: un DELETE da 405 en cualquier
    // repositorio, asi que solo el PUT dice algo del grupo.
    assert_eq!(verb(&lab, port, "PUT", "grp", "pkg/lib.bin"), 405);
    // Y no se ha colado nada en el miembro.
    assert_eq!(get(&lab, port, "hos-a", "pkg/lib.bin").status, 404);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// MISS no es ERROR.
// ---------------------------------------------------------------------------

#[test]
fn a_miss_in_a_member_continues_but_an_error_stops_without_jumping() {
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &["pxy-a"],
        &[("grp", &["pxy-a", "hos-a"])],
        None,
    );
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    // El upstream contesta 404: es un MISS y el grupo sigue con el hosted.
    let miss = get(&lab, port, "grp", "pkg/lib.bin");
    assert_eq!(miss.status, 200);
    assert_eq!(miss.body, HOSTED);
    assert_eq!(miss.resolved_from.as_deref(), Some("hos-a"));
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 1);

    // El upstream cae: es un ERROR, el grupo se detiene y NO sirve el
    // hosted aunque lo tenga -- seria contenido ambiguo.
    lab.upstream.fail(true);
    let error = get(&lab, port, "grp", "pkg/lib.bin");
    assert!(
        [502, 503, 504].contains(&error.status),
        "an upstream failure is an error, not a miss: {}",
        error.status
    );
    assert_ne!(error.body, HOSTED);
    assert_eq!(error.resolved_from, None);

    // La ruta directa del miembro sano sigue sirviendo lo suyo.
    let direct = get(&lab, port, "hos-a", "pkg/lib.bin");
    assert_eq!(direct.body, HOSTED);

    // Cuando el upstream vuelve, el grupo vuelve a resolver.
    lab.upstream.fail(false);
    let back = get(&lab, port, "grp", "pkg/lib.bin");
    assert_eq!(back.resolved_from.as_deref(), Some("hos-a"));
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Permisos: por miembro y por grupo, sin herencia.
// ---------------------------------------------------------------------------

fn policy(downloads: &[&str]) -> String {
    let mut text = String::new();
    text.push_str(&format!(
        "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"UploadAsset\", resource);\n"
    ));
    for key in downloads {
        text.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"DownloadAsset\", resource == Artifacts::Repository::\"hrn:hodei:artifacts:eu:{FIRST_ACCOUNT}:repository/{key}\");\n"
        ));
    }
    text
}

#[test]
fn a_member_the_principal_cannot_read_is_skipped_and_its_direct_route_is_closed() {
    // alice lee el grupo y el proxy, pero NO el hosted.
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &["pxy-a"],
        &[("grp", &["hos-a", "pxy-a"])],
        Some(&policy(&["grp", "pxy-a"])),
    );
    lab.upstream.put("pkg/lib.bin", UPSTREAM, None, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    // El hosted tiene la ruta pero alice no puede leerlo: se omite y
    // resuelve el proxy. Nunca se le sirve lo que su permiso no cubre.
    let through = get(&lab, port, "grp", "pkg/lib.bin");
    assert_eq!(through.status, 200);
    assert_eq!(through.body, UPSTREAM);
    assert_eq!(through.resolved_from.as_deref(), Some("pxy-a"));

    // Y que pueda leer el grupo no le abre la ruta directa del miembro.
    let direct = get(&lab, port, "hos-a", "pkg/lib.bin");
    assert!([403, 404].contains(&direct.status), "{}", direct.status);
    assert_ne!(direct.body, HOSTED);
    support::stop(&mut service);
}

#[test]
fn reading_a_member_does_not_open_the_group_endpoint() {
    // alice lee los miembros pero no el endpoint virtual.
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &[],
        &[("grp", &["hos-a"])],
        Some(&policy(&["hos-a"])),
    );
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    let direct = get(&lab, port, "hos-a", "pkg/lib.bin");
    assert_eq!(direct.status, 200);
    assert_eq!(direct.body, HOSTED);
    let virtual_one = get(&lab, port, "grp", "pkg/lib.bin");
    assert!(
        [403, 404].contains(&virtual_one.status),
        "{}",
        virtual_one.status
    );
    assert_ne!(virtual_one.body, HOSTED);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Confianza y reconfiguracion.
// ---------------------------------------------------------------------------

#[test]
fn a_trust_gate_that_hides_the_winner_does_not_make_the_group_jump_to_another_member() {
    let lab = Lab::new(
        &["hos-a"],
        &["hos-a"],
        &["pxy-a"],
        &[("grp", &["hos-a", "pxy-a"])],
        None,
    );
    lab.upstream.put("pkg/lib.bin", UPSTREAM, None, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));

    // El hosted tiene la ruta y su compuerta de confianza la oculta: el
    // grupo contesta 404 y NO baja la del upstream, que es otro contenido.
    let hidden = get(&lab, port, "grp", "pkg/lib.bin");
    assert_eq!(hidden.status, 404);
    assert_ne!(hidden.body, UPSTREAM);
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 0);
    support::stop(&mut service);
}

#[test]
fn reconfiguring_a_group_changes_who_wins_and_the_version_says_so() {
    let lab = Lab::new(
        &["hos-a"],
        &[],
        &["pxy-a"],
        &[("grp", &["hos-a", "pxy-a"])],
        None,
    );
    lab.upstream.put("pkg/lib.bin", UPSTREAM, None, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos-a", "pkg/lib.bin", HOSTED)));
    let before = get(&lab, port, "grp", "pkg/lib.bin");
    assert_eq!(before.resolved_from.as_deref(), Some("hos-a"));
    let version = before.group_version.expect("the version");
    support::stop(&mut service);

    lab.deployment.reconfigure_group("grp", &["pxy-a", "hos-a"]);
    let mut service = lab.start();
    let after = get(&lab, service.port, "grp", "pkg/lib.bin");
    assert_eq!(after.body, UPSTREAM);
    assert_eq!(after.resolved_from.as_deref(), Some("pxy-a"));
    assert!(
        after.group_version.expect("the version") > version,
        "the resolution version grows when the order changes"
    );
    support::stop(&mut service);
}
