//! El GOPROXY de HSP-121, a nivel HTTP.
//!
//! Se prueba: la publicacion `PUT` de los tres artefactos de una version,
//! el listado ensamblado en orden semver, `@latest`, las rutas escapadas,
//! la inmutabilidad de la version, el rechazo de lo que el protocolo no
//! sabe hacer y la cache del proxy con el servicio despues sin red.
//!
//! El cliente real (`go`) se prueba en su propio fichero; aqui el
//! protocolo se habla como lo hablaria el cliente.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
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

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// El documento `.info` canonico de una version.
fn info_document(version: &str) -> Vec<u8> {
    json!({ "Version": version, "Time": "2026-11-17T12:00:00Z" })
        .to_string()
        .into_bytes()
}

/// Un `go.mod` que declara el modulo sin escapar: el `!x` de la ruta es
/// solo codificacion de caminos, el go.mod vive dentro y dice la verdad.
fn go_mod(module: &str) -> Vec<u8> {
    let declared = hodei_catalog::gomod::unescape(module);
    format!("module {declared}\n\ngo 1.22\n").into_bytes()
}

/// Los bytes del zip: a nivel HTTP son opacos; el cliente real los
/// descomprime en su propio test.
fn module_zip(tag: &str) -> Vec<u8> {
    format!("the module zip of {tag}").into_bytes()
}

/// Publica la version entera: `.info`, `.mod` y `.zip`.
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

#[test]
fn a_version_is_published_and_served_through_goproxy() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Publicar la 1.0.0: los tres artefactos por su ruta de protocolo.
    let zip = module_zip("hello-1.0.0");
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.0.0",
        &zip,
    );

    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    let info = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.info",
        &token,
        None,
    );
    assert_eq!(info.status, 200, "{}", text_of(&info));
    assert_eq!(
        info.header("content-type").as_deref(),
        Some("application/json"),
        "el .info es JSON"
    );

    let served_mod = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.status, 200, "{}", text_of(&served_mod));
    assert_eq!(served_mod.body, go_mod("example.com/hello"));

    // El zip sale byte a byte: el checksum que calcule el cliente sobre
    // estos bytes es el mismo que el del publicado.
    let download = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, zip);
    assert_eq!(
        download.header("content-type").as_deref(),
        Some("application/zip")
    );

    // Mas versiones: el listado ordena por semver, no por texto.
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.2.0",
        &module_zip("hello-1.2.0"),
    );
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/hello",
        "v1.10.0",
        &module_zip("hello-1.10.0"),
    );
    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(text_of(&list), "v1.0.0\nv1.2.0\nv1.10.0\n");

    // `@latest` senala la ultima, con su documento.
    let latest = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@latest",
        &token,
        None,
    );
    assert_eq!(latest.status, 200, "{}", text_of(&latest));
    let document: Value = serde_json::from_slice(&latest.body).expect("the latest info");
    assert_eq!(document["Version"], json!("v1.10.0"));

    // Republicar los mismos bytes es el eco de una publicacion
    // interrumpida.
    let again = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        Some(&zip),
    );
    assert_eq!(again.status, 200, "{}", text_of(&again));

    // Republicar la misma version con otros bytes es un conflicto.
    let conflict = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.zip",
        &token,
        Some(&module_zip("hello-1.0.0-tampered")),
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));

    // Un `.info` que miente sobre su version no entra.
    let liar = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v2.0.0.info",
        &token,
        Some(&info_document("v1.9.9")),
    );
    assert_eq!(liar.status, 400, "{}", text_of(&liar));

    // Un `.mod` que declara otro modulo tampoco.
    let wrong = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/v2.0.0.mod",
        &token,
        Some(&go_mod("example.com/other")),
    );
    assert_eq!(wrong.status, 400, "{}", text_of(&wrong));

    // Lo que nadie publico no esta.
    let missing_list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/nobody/@v/list",
        &token,
        None,
    );
    assert_eq!(missing_list.status, 404, "{}", text_of(&missing_list));
    let missing_latest = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@latest",
        &token,
        None,
    );
    assert_eq!(missing_latest.status, 200);
    let missing_version = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/v9.9.9.info",
        &token,
        None,
    );
    assert_eq!(missing_version.status, 404, "{}", text_of(&missing_version));

    // Sin credencial no se mira ni la ruta.
    let anonymous = call(
        port,
        "GET",
        "/go/go-hosted/example.com/hello/@v/list",
        "",
        None,
    );
    assert_eq!(anonymous.status, 401, "{}", text_of(&anonymous));
}

#[test]
fn escaped_modules_are_addressed_by_their_canonical_path() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // El escapado canonico del protocolo: mayuscula es `!minuscula`.
    publish_version(
        port,
        "go-hosted",
        &token,
        "example.com/!burnt!sushi/toml",
        "v1.0.0",
        &module_zip("toml-1.0.0"),
    );
    let list = call(
        port,
        "GET",
        "/go/go-hosted/example.com/!burnt!sushi/toml/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    // El `go.mod` declara el modulo sin escapar, y esa es la comprobacion
    // que paso al publicar.
    let served_mod = call(
        port,
        "GET",
        "/go/go-hosted/example.com/!burnt!sushi/toml/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.body, go_mod("example.com/BurntSushi/toml"));

    // La forma sin escapar NO es una ruta: el protocolo habla en escapado.
    let unescaped = call(
        port,
        "GET",
        "/go/go-hosted/example.com/BurntSushi/toml/@v/list",
        &token,
        None,
    );
    assert_eq!(unescaped.status, 404, "{}", text_of(&unescaped));
}

#[test]
fn the_protocol_says_no_to_what_it_does_not_know() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-hosted", Value::Null)]);
    deployment.seed_go_proxies(&[(
        "go-proxy",
        json!({"upstream": {"id": "lab", "url": "http://127.0.0.1:1/", "scopes": ["/"], "max_bytes": 4096}}),
    )]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Un metodo que el protocolo no tiene.
    let post = call(
        port,
        "POST",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        Some(b""),
    );
    assert_eq!(post.status, 405, "{}", text_of(&post));
    let delete = call(
        port,
        "DELETE",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(delete.status, 405, "{}", text_of(&delete));

    // La lista no se publica: solo las tres piezas de una version.
    let put_list = call(
        port,
        "PUT",
        "/go/go-hosted/example.com/hello/@v/list",
        &token,
        Some(b"v1.0.0\n"),
    );
    assert_eq!(put_list.status, 405, "{}", text_of(&put_list));

    // Rutas que no son GOPROXY, ni con trampa de caminos.
    for hostile in [
        "/go/go-hosted/random/stuff",
        "/go/go-hosted/example.com/hello/@v/v1.0.0.exe",
        "/go/go-hosted/../../etc/passwd",
        "/go/go-hosted/example.com/hello/notat@v/v1.0.0.info",
    ] {
        let answer = call(port, "GET", hostile, &token, None);
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }

    // Un proxy no acepta publicacion: no es un hosted con otro nombre.
    let publish_into_proxy = call(
        port,
        "PUT",
        "/go/go-proxy/example.com/hello/@v/v1.0.0.info",
        &token,
        Some(&info_document("v1.0.0")),
    );
    assert_eq!(
        publish_into_proxy.status,
        405,
        "{}",
        text_of(&publish_into_proxy)
    );

    // Un repositorio de otro formato no es un GOPROXY, y no se sirve
    // como Raw por equivalencia: no existe para esta superficie.
    let other_format = call(
        port,
        "GET",
        "/go/registry/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// Un upstream GOPROXY de verdad-en-falso: sirve lo que se le ponga por
/// ruta y cuenta cada pedido.
struct Upstream {
    /// El puerto donde escucha.
    port: u16,
    /// Cuantas peticiones ha atendido, por ruta.
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter of the upstream")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.asked.lock().expect("the counter").values().sum()
    }
}

fn start_upstream(module: &str, version: &str, zip: Vec<u8>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        format!("/{module}/@v/list"),
        format!("{version}\n").into_bytes(),
    );
    pages.insert(
        format!("/{module}/@v/{version}.info"),
        info_document(version),
    );
    pages.insert(format!("/{module}/@v/{version}.mod"), go_mod(module));
    pages.insert(format!("/{module}/@v/{version}.zip"), zip);
    pages.insert(format!("/{module}/@latest"), info_document(version));
    let pages = Arc::new(pages);
    let counter = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let pages = Arc::clone(&pages);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = String::new();
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            read.push_str(String::from_utf8_lossy(&buffer[..n]).as_ref());
                            if read.contains("\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let Some(first) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    return;
                };
                let path = first.to_string();
                *counter
                    .lock()
                    .expect("the counter")
                    .entry(path.clone())
                    .or_default() += 1;
                let answer = match pages.get(&path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body.clone())
                    .collect::<Vec<u8>>(),
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    Upstream { port, asked }
}

#[test]
fn the_proxy_caches_the_module_and_survives_without_network() {
    let zip = module_zip("proxied-1.0.0");
    let upstream = start_upstream("example.com/proxied/hello", "v1.0.0", zip.clone());

    let deployment = Deployment::new();
    deployment.seed_go_proxies(&[(
        "go-central",
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["/"],
                "max_bytes": 65536,
            },
        }),
    )]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);

    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let first = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(first.status, 200, "{}", text_of(&first));
    assert_eq!(first.body, zip, "el proxy reescribe el zip");
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));

    // La segunda vez no sale: la cache sirve los mismos bytes, y el
    // checksum que calcule el cliente no puede ser otro.
    let second = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(second.status, 200, "{}", text_of(&second));
    assert_eq!(second.body, zip);
    assert_eq!(second.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(
        upstream.asked_for("/example.com/proxied/hello/@v/v1.0.0.zip"),
        1
    );

    // La lista y el go.mod, igual que el zip: por la ruta del protocolo.
    let list = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    assert_eq!(text_of(&list), "v1.0.0\n");

    let served_mod = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.mod",
        &token,
        None,
    );
    assert_eq!(served_mod.body, go_mod("example.com/proxied/hello"));

    let before = upstream.total();
    support::stop(&mut service);

    // Sin red, el modulo cacheado se sigue sirviendo tal cual; lo que no
    // se cacheo es un 503 que lo dice.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;
    let offline = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v1.0.0.zip",
        &token,
        None,
    );
    assert_eq!(offline.status, 200, "{}", text_of(&offline));
    assert_eq!(offline.body, zip);
    assert_eq!(offline.header("x-hodei-cache").as_deref(), Some("offline"));
    let unknown = call(
        port,
        "GET",
        "/go/go-central/example.com/proxied/hello/@v/v9.9.9.zip",
        &token,
        None,
    );
    assert_eq!(unknown.status, 503, "{}", text_of(&unknown));
    support::stop(&mut service);
    assert_eq!(upstream.total(), before, "offline toco la red");
}

/// Un grupo go es un grupo go: ensambla la union de las versiones de sus
/// miembros y **no** se convierte en un repositorio crudo que sirva rutas
/// sueltas.
///
/// El grupo que no tiene ningun modulo publicado responde con el `404` del
/// protocolo go --"no hay ningun modulo con ese nombre"-, y no con el `405`
/// de una modalidad que la release no sabe servir: HSP-232 lo sirvio.
#[test]
fn a_go_group_answers_in_the_dialect_of_go_and_never_degrades_to_raw() {
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-member", Value::Null)]);
    deployment.seed_go_group("go-all", &["go-member"]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let answer = call(
        port,
        "GET",
        "/go/go-all/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(answer.status, 404, "{}", text_of(&answer));
    let body = text_of(&answer);
    assert!(
        body.contains("module"),
        "el rechazo debe hablar de modulos: {body}"
    );

    // Y no es la superficie cruda hablando: la misma ruta por `/raw/`
    // no tiene un asset que servir, y el go-group no se registra como
    // repositorio crudo.
    let raw = call(
        port,
        "GET",
        "/raw/go-all/example.com/hello/@v/list",
        &token,
        None,
    );
    assert_eq!(raw.status, 404, "{}", text_of(&raw));
}

/// El cliente `go` autentica con Basic desde su `.netrc` y la credencial
/// viaja en la contrasena. La superficie la normaliza a Bearer antes de
/// preguntar; esto fija el dialecto a nivel de protocolo, que el test del
/// cliente real solo puede ejercer por TLS.
#[test]
fn the_basic_credentials_of_the_go_client_are_understood() {
    use base64::Engine;
    let deployment = Deployment::new();
    deployment.seed_go(&[("go-basic", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    publish_version(
        port,
        "go-basic",
        &token,
        "example.com/hello",
        "v1.0.0",
        b"opaque zip bytes",
    );

    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("alice:{token}"));
    let answer = Curl::call_with_headers(
        port,
        "GET",
        "/go/go-basic/example.com/hello/@v/v1.0.0.info",
        "",
        None,
        &[("authorization", &format!("Basic {encoded}"))],
        Some(&next("basic")),
    )
    .run();
    assert_eq!(answer.status, 200, "basic: {}", text_of(&answer));
    let body: Value = serde_json::from_slice(&answer.body).expect("the info document");
    assert_eq!(body["Version"], "v1.0.0");

    // Basic no es una puerta libre: sin credencial valida no se pasa.
    let junk = base64::engine::general_purpose::STANDARD.encode("alice:not-a-token");
    let denied = Curl::call_with_headers(
        port,
        "GET",
        "/go/go-basic/example.com/hello/@v/v1.0.0.info",
        "",
        None,
        &[("authorization", &format!("Basic {junk}"))],
        Some(&next("basic-denied")),
    )
    .run();
    assert_eq!(denied.status, 401, "junk basic: {}", text_of(&denied));
}
