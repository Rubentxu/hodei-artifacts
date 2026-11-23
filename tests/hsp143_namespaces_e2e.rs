//! La reserva de namespaces de HSP-143 / HAE-10, contra el wire.
//!
//! El servicio es un proceso de verdad con catalogo, almacen y Cedar reales;
//! el upstream es un servidor local que este fichero controla. Lo que se
//! mide es lo que un cliente ve: una ruta reservada **nunca** sale del
//! upstream publico, aunque su version sea mayor, aunque este en la cache,
//! aunque no haya red, y aunque el grupo tenga otro espejo publico detras.
//! La negativa se explica: el cuerpo nombra la reserva y las fuentes que
//! quedaron fuera.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use support::{Curl, Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

const INTERNAL: &[u8] = b"content published in the internal hosted member";
const PUBLIC: &[u8] = b"content that lives in the public upstream";

/// Un documento que sirve el upstream.
#[derive(Clone)]
struct Doc {
    body: Vec<u8>,
    etag: Option<String>,
}

/// El estado del upstream, que la prueba cambia mientras corre.
struct World {
    docs: Mutex<HashMap<String, Doc>>,
    seen: Mutex<Vec<String>>,
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

    fn put(&self, path: &str, body: &[u8], etag: Option<&str>) {
        self.world.docs.lock().expect("docs").insert(
            path.to_string(),
            Doc {
                body: body.to_vec(),
                etag: etag.map(str::to_string),
            },
        );
    }

    fn hits(&self, path: &str) -> usize {
        self.world
            .seen
            .lock()
            .expect("the log")
            .iter()
            .filter(|one| one.as_str() == path)
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
    world.seen.lock().expect("the log").push(path.clone());

    let doc = world.docs.lock().expect("docs").get(&path).cloned();
    let response: Vec<u8> = if let Some(doc) = doc {
        let validators = doc
            .etag
            .as_ref()
            .map(|etag| format!("ETag: {etag}\r\n"))
            .unwrap_or_default();
        let unchanged = matches!(
            (&doc.etag, &if_none_match),
            (Some(etag), Some(sent)) if etag == sent
        );
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
    /// `hosted`: repositorios hosted; `proxies`: (clave, politica de
    /// namespaces o null); `groups`: (clave, miembros, politica o null).
    fn new(
        hosted: &[&str],
        proxies: &[(&str, serde_json::Value)],
        groups: &[(&str, &[&str], serde_json::Value)],
    ) -> Self {
        let upstream = Upstream::start();
        let deployment = Deployment::new();
        let entries = hosted
            .iter()
            .map(|key| serde_json::json!({ "key": key, "format": "raw" }))
            .chain(proxies.iter().map(|(key, namespaces)| {
                serde_json::json!({
                    "key": key,
                    "format": "raw",
                    "proxy": proxy_of(&upstream),
                    "namespaces": namespaces,
                })
            }))
            .collect::<Vec<_>>();
        deployment.seed_repositories(&entries);
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members, namespaces)| {
                serde_json::json!({ "key": key, "members": members, "namespaces": namespaces })
            })
            .collect();
        deployment.seed_groups_raw(&groups);
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
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
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
    }
}

fn get_as(_lab: &Lab, port: u16, account: &str, repository: &str, path: &str) -> u16 {
    let token = support::token(account, "alice");
    Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        &token,
        None,
        None,
        None,
    )
    .run()
    .status
}

fn receipts(lab: &Lab, port: u16, repository: &str, path: &str) -> Vec<serde_json::Value> {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("receipts.json");
    let answer = Curl::call(
        port,
        "GET",
        &format!("/api/v1/provenance?repository={repository}&path={path}"),
        &alice,
        None,
        None,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "{}", answer.status);
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&into).unwrap_or_default()).expect("json");
    parsed["receipts"].as_array().expect("receipts").clone()
}

fn reserved_lab() -> serde_json::Value {
    serde_json::json!({ "reserved": ["lab"] })
}

/// AC#1: la reserva gana siempre, aunque la version publica sea mayor.
///
/// El upstream tiene `lab/lib.bin` con mas contenido que el hosted: sin
/// reserva, un grupo con el proxy delante lo serviria. Con la reserva, el
/// proxy no se consulta y el hosted gana por ser interno.
#[test]
fn a_reserved_path_is_never_served_from_the_public_upstream_even_when_newer() {
    let lab = Lab::new(
        &["hos"],
        &[("pxy", serde_json::Value::Null)],
        &[("grp", &["pxy", "hos"], serde_json::Value::Null)],
    );
    lab.upstream.put("lab/lib.bin", PUBLIC, None);
    lab.upstream.put("lab/only-upstream.bin", PUBLIC, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos", "lab/lib.bin", INTERNAL)));

    // Sin reserva: el orden del grupo manda y el proxy (que va primero)
    // trae la version publica, mayor.
    let before = get(&lab, port, "grp", "lab/lib.bin");
    assert_eq!(before.status, 200);
    assert_eq!(before.body, PUBLIC);
    assert_eq!(before.resolved_from.as_deref(), Some("pxy"));
    let asked = lab.upstream.hits("lab/lib.bin");
    assert!(asked > 0, "the upstream was never asked");

    // La revision de politica reserva el namespace en el grupo y en el
    // proxy; la version de su configuracion sube. El reconfigurador abre la
    // base en su propio proceso: el servicio tiene que estar parado.
    support::stop(&mut service);
    lab.deployment.reconfigure_namespaces("pxy", reserved_lab());
    lab.deployment.reconfigure_namespaces("grp", reserved_lab());
    let mut service = lab.deployment.start(&[(
        "HODEI_PROXY__ALLOWED_HOSTS",
        format!("127.0.0.1:{};private;http", lab.upstream.port).as_str(),
    )]);
    let port = service.port;
    let hits_after_revision = lab.upstream.hits("lab/lib.bin");

    // El proxy directo ya no sale al upstream ni sirve lo que tiene: la
    // negativa nombra la reserva y el upstream que queda fuera.
    let direct = get(&lab, port, "pxy", "lab/lib.bin");
    assert_eq!(direct.status, 403, "{}", direct.text());
    assert!(direct.text().contains("lab"), "{}", direct.text());
    assert!(
        direct.text().contains("reserved"),
        "the denial does not name the reservation: {}",
        direct.text()
    );
    assert!(
        direct.text().contains("public upstream lab"),
        "the denial does not name the source: {}",
        direct.text()
    );

    // En el grupo, lo interno sigue sirviendose: la reserva no toca al
    // hosted, y no hay peticion nueva al upstream.
    let internal = get(&lab, port, "grp", "lab/lib.bin");
    assert_eq!(internal.status, 200, "{}", internal.text());
    assert_eq!(internal.body, INTERNAL);
    assert_eq!(internal.resolved_from.as_deref(), Some("hos"));
    assert_eq!(
        lab.upstream.hits("lab/lib.bin"),
        hits_after_revision,
        "the upstream was asked for a reserved path"
    );

    // Lo que solo tiene el upstream, reservado, es una negativa explicada:
    // el upstream ni se entera de que se pidio.
    let denied = get(&lab, port, "grp", "lab/only-upstream.bin");
    assert_eq!(denied.status, 403, "{}", denied.text());
    assert!(denied.text().contains("lab"), "{}", denied.text());
    assert_eq!(lab.upstream.hits("lab/only-upstream.bin"), 0);

    // Un path fuera de la reserva sigue resolviendo contra el upstream.
    lab.upstream.put("other/ok.bin", PUBLIC, None);
    let free = get(&lab, port, "grp", "other/ok.bin");
    assert_eq!(free.status, 200, "{}", free.text());
    assert_eq!(free.body, PUBLIC);
    support::stop(&mut service);
}

/// AC#2: la negativa no se transforma en fallback cuando el grupo tiene
/// otro espejo publico detras.
#[test]
fn a_second_public_mirror_does_not_turn_the_deny_into_a_fallback() {
    let lab = Lab::new(
        &["hos"],
        &[
            ("pxy-one", serde_json::Value::Null),
            ("pxy-two", serde_json::Value::Null),
        ],
        &[("grp", &["hos", "pxy-one", "pxy-two"], reserved_lab())],
    );
    lab.upstream.put("lab/ghost.bin", PUBLIC, None);
    let mut service = lab.start();
    let port = service.port;

    let denied = get(&lab, port, "grp", "lab/ghost.bin");
    assert_eq!(denied.status, 403, "{}", denied.text());
    assert!(
        denied.text().contains("pxy-one") && denied.text().contains("pxy-two"),
        "the denial does not name both sources: {}",
        denied.text()
    );
    // Ningun upstream recibio nada por la ruta reservada.
    assert_eq!(lab.upstream.hits("lab/ghost.bin"), 0);

    // Sin la reserva, el segundo espejo si sirve: es el comportamiento de
    // siempre para lo no reservado. El reconfigurador necesita el servicio
    // parado.
    support::stop(&mut service);
    lab.deployment
        .reconfigure_namespaces("grp", serde_json::Value::Null);
    let mut free = lab.deployment.start(&[(
        "HODEI_PROXY__ALLOWED_HOSTS",
        format!("127.0.0.1:{};private;http", lab.upstream.port).as_str(),
    )]);
    let served = get(&lab, free.port, "grp", "lab/ghost.bin");
    assert_eq!(served.status, 200, "{}", served.text());
    assert_eq!(served.body, PUBLIC);
    support::stop(&mut free);
}

/// AC#3: una revision de politica invalida la resolucion que la cache
/// sostiene; y el modo offline no la esquiva.
#[test]
fn a_policy_revision_invalidates_the_resolution_the_cache_holds() {
    let lab = Lab::new(&[], &[("pxy", serde_json::Value::Null)], &[]);
    lab.upstream.put("lab/cached.bin", PUBLIC, Some("\"v1\""));
    let allowed = format!("127.0.0.1:{};private;http", lab.upstream.port);
    let mut service = lab
        .deployment
        .start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    // La resolucion entra en la cache como hit.
    let first = get(&lab, port, "pxy", "lab/cached.bin");
    assert_eq!(first.status, 200);
    let cached = get(&lab, port, "pxy", "lab/cached.bin");
    assert_eq!(cached.status, 200);
    let hits = lab.upstream.hits("lab/cached.bin");
    let again = get(&lab, port, "pxy", "lab/cached.bin");
    assert_eq!(again.status, 200);
    assert_eq!(
        lab.upstream.hits("lab/cached.bin"),
        hits,
        "the cache did not hold the resolution"
    );
    support::stop(&mut service);

    // La revision reserva el namespace: la resolucion guardada ya no vale.
    lab.deployment.reconfigure_namespaces("pxy", reserved_lab());
    let mut revised = lab
        .deployment
        .start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let refused = get(&lab, revised.port, "pxy", "lab/cached.bin");
    assert_eq!(refused.status, 403, "{}", refused.text());
    assert_eq!(
        lab.upstream.hits("lab/cached.bin"),
        hits,
        "the revised policy went to the upstream"
    );

    // Y sin red tampoco se esquiva: el offline sirve lo que tiene, pero una
    // ruta reservada no es "lo que tiene".
    support::stop(&mut revised);
    let mut offline = lab.deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let offline_denied = get(&lab, offline.port, "pxy", "lab/cached.bin");
    assert_eq!(offline_denied.status, 403, "{}", offline_denied.text());
    support::stop(&mut offline);

    // Quitar la reserva vuelve a servir lo cacheado: la politica es la que
    // manda, en las dos direcciones.
    lab.deployment
        .reconfigure_namespaces("pxy", serde_json::Value::Null);
    let mut restored = lab
        .deployment
        .start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let back = get(&lab, restored.port, "pxy", "lab/cached.bin");
    assert_eq!(back.status, 200, "{}", back.text());
    assert_eq!(back.body, PUBLIC);
    support::stop(&mut restored);
}

/// AC#2: el alias normalizado de un nombre reservado no esquiva la
/// reserva.
///
/// La reserva es del nombre normalizado (PEP 503): `Lab_Utils`,
/// `lab.utils` y `lab-utils` son el mismo namespace, y la peticion que
/// llega escrita como alias recibe la misma negativa que el nombre
/// canonico. La superficie PyPI normaliza el nombre de la ruta, asi que el
/// veredicto del dominio es el que decide.
#[test]
fn the_normalized_alias_of_a_reserved_name_is_denied_like_the_canonical_one() {
    let upstream = Upstream::start();
    let deployment = Deployment::new();
    deployment.seed_repositories(&[serde_json::json!({
        "key": "pxy",
        "format": "pypi",
        "proxy": proxy_of(&upstream),
        "namespaces": reserved_lab(),
    })]);
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // El alias normaliza igual: la misma negativa que el nombre canonico.
    for project in ["lab", "Lab", "LAB"] {
        let into = deployment.directory.path().join("pypi-response.txt");
        let answer = Curl::call(
            port,
            "GET",
            &format!("/pypi/pxy/simple/{project}"),
            &alice,
            None,
            None,
            Some(&into),
        )
        .run();
        assert_eq!(answer.status, 403, "{project}");
        let body = std::fs::read_to_string(&into).unwrap_or_default();
        assert!(
            body.contains("lab"),
            "the denial does not name the reservation: {body}"
        );
    }
    support::stop(&mut service);
}

/// AC#4: SSRF de configuracion. La URL del upstream no puede ser la puerta
/// de salida a los metadatos internos de la nube: el egress la bloquea
/// antes de conectar, y no queda recibo de nada.
#[test]
fn an_upstream_url_pointing_at_the_metadata_service_never_leaves() {
    let lab = Lab::new(&[], &[("pxy", serde_json::Value::Null)], &[]);
    // Se reescribe la URL del upstream hacia el servicio de metadatos: la
    // configuracion maliciosa no se detecta al sembrar (es una URL http
    // valida), se bloquea al salir.
    lab.deployment.reconfigure_proxy(
        "pxy",
        serde_json::json!({
            "upstream": {
                "id": "lab",
                "url": "http://169.254.169.254/latest/meta-data/",
                "scopes": ["/"],
                "max_bytes": 1024,
            },
            "cache": { "ttl": { "metadata_secs": 3600 } },
        }),
    );
    // El host no esta en la lista de salida: nada sale sin autorizacion.
    let mut service = lab.deployment.start(&[]);
    let port = service.port;
    let answer = get(&lab, port, "pxy", "latest/meta-data");
    assert_ne!(answer.status, 200, "{}", answer.text());
    assert!(
        receipts(&lab, port, "pxy", "latest/meta-data").is_empty(),
        "the metadata attempt left a receipt"
    );
    support::stop(&mut service);
}

/// AC#4: sin filtracion entre cuentas. El grupo y sus miembros de una
/// cuenta no existen para la otra, reserva o no reserva.
#[test]
fn a_repository_of_another_account_does_not_exist_for_this_one() {
    let lab = Lab::new(
        &["hos"],
        &[("pxy", serde_json::Value::Null)],
        &[("grp", &["hos", "pxy"], reserved_lab())],
    );
    lab.upstream.put("lab/lib.bin", PUBLIC, None);
    let mut service = lab.start();
    let port = service.port;
    assert!((200..300).contains(&lab.publish(port, "hos", "lab/lib.bin", INTERNAL)));

    // La cuenta de al lado no ve el grupo ni sus miembros, ni con el
    // namespace reservado ni fuera de el.
    for (repository, path) in [
        ("grp", "lab/lib.bin"),
        ("hos", "lab/lib.bin"),
        ("pxy", "other/ok.bin"),
    ] {
        let status = get_as(&lab, port, SECOND_ACCOUNT, repository, path);
        assert_eq!(status, 404, "{repository}/{path}: {status}");
    }
    support::stop(&mut service);
}
