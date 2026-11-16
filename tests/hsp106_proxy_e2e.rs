//! El repository proxy remoto (HSP-106 / HAR-12), contra el wire.
//!
//! El servicio es un proceso de verdad con catalogo, almacen de bytes y
//! Cedar reales. El upstream es un servidor local que este fichero
//! controla: uno **autorizado** (el que la politica de salida deja
//! alcanzar) y las respuestas hostiles que simula -- redirects hacia redes
//! prohibidas, cuerpos truncados, checksums falsos, exceso de bytes.
//!
//! Lo que se mide es lo que una persona vera: que se sirve, que no, y que
//! recibo queda. Nada de lo que falla puede acabar presentado como
//! contenido verificado.

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use support::{Curl, Deployment, FIRST_ACCOUNT};

/// Lo que sirve el upstream autorizado.
const BODY: &[u8] = b"content that lives upstream";

/// El token que el servicio lee del entorno por referencia.
const UPSTREAM_SECRET: &str = "lab-secret-value-9f3a";

/// Una peticion que llego al upstream.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    authorization: Option<String>,
}

/// Un upstream local con respuestas escritas a mano.
struct Upstream {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Upstream {
    /// Arranca un upstream. `redirects_to` es el puerto de otro upstream (o
    /// de un puerto cerrado) al que apuntan algunos redirects.
    fn start(redirects_to: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener.local_addr().expect("the address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, redirects_to, &log));
            }
        });
        Self { port, seen }
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }

    fn hits(&self, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|one| one.path == path)
            .count()
    }
}

fn serve(mut stream: TcpStream, other_port: u16, log: &Mutex<Vec<Seen>>) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
        // Un ClientHello de TLS no es una peticion HTTP: un servidor plano
        // lo contesta con basura y cierra, y el handshake no se completa.
        if request.first() == Some(&0x16) {
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            return;
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
    let authorization = text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.trim().to_string())
    });
    log.lock().expect("the log").push(Seen {
        path: path.clone(),
        authorization,
    });

    let redirect = |location: &str| -> Vec<u8> {
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    };
    let ok = |body: &[u8], extra: &str| -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    };
    let checksum = |body: &[u8]| {
        format!(
            "X-Checksum-Sha256: {}\r\n",
            hodei_evidence::ContentDigest::of_bytes(body).value
        )
    };

    let response: Vec<u8> = match path.as_str() {
        "ok/app.bin" => ok(BODY, &format!("{}ETag: \"v1\"\r\n", checksum(BODY))),
        "ok/plain.bin" => ok(BODY, ""),
        "bad/checksum.bin" => ok(BODY, &checksum(b"something else entirely")),
        "bad/truncated.bin" => {
            // Promete mil bytes y entrega diez.
            let mut out =
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n".to_vec();
            out.extend_from_slice(&BODY[..10]);
            out
        }
        "big/huge.bin" => ok(&vec![b'x'; 8192], ""),
        "status/500.bin" => {
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec()
        }
        "redir/same.bin" => redirect("/ok/app.bin"),
        "redir/loop.bin" => redirect("/redir/loop.bin"),
        "redir/loopback.bin" => redirect(&format!("http://127.0.0.1:{other_port}/ok/app.bin")),
        "redir/metadata.bin" => redirect("http://169.254.169.254/latest/meta-data/"),
        "redir/private.bin" => redirect("http://10.0.0.5/ok/app.bin"),
        "redir/stranger.bin" => redirect("http://evil.example/ok/app.bin"),
        _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

/// La configuracion de un upstream de repositorio.
fn upstream_of(id: &str, url: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut config = serde_json::json!({
        "id": id,
        "url": url,
        "scopes": ["/"],
        "credential_ref": "env:LAB_TOKEN",
        "max_bytes": 1024,
    });
    for (key, value) in extra.as_object().expect("an object") {
        config[key] = value.clone();
    }
    config
}

/// La lista de salida que autoriza a los dos upstreams locales.
fn allow(ports: &[u16]) -> String {
    ports
        .iter()
        .map(|port| format!("127.0.0.1:{port};private;http"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Una peticion cualquiera: codigo y cuerpo.
fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let file = body.map(|bytes| {
        std::fs::write(&sent, bytes).expect("the body is written");
        sent.as_path()
    });
    let answer = Curl::call(
        port,
        method,
        path,
        token,
        file,
        body.map(|_| "application/octet-stream"),
        Some(&into),
    )
    .run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

fn get(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
) -> (u16, String) {
    call(
        deployment,
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        token,
        None,
    )
}

/// Los recibos que el servicio dice tener de una ruta.
fn receipts(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
) -> Vec<serde_json::Value> {
    let (status, body) = call(
        deployment,
        port,
        "GET",
        &format!("/api/v1/provenance?repository={repository}&path={path}"),
        token,
        None,
    );
    assert_eq!(status, 200, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
    parsed["receipts"].as_array().expect("receipts").clone()
}

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Un despliegue con un upstream autorizado, otro para los redirects y los
/// repositorios proxy de la prueba.
struct Lab {
    deployment: Deployment,
    primary: Upstream,
    secondary: Upstream,
}

impl Lab {
    fn new(trusted: &[&str]) -> Self {
        let closed = support::free_port();
        let secondary = Upstream::start(closed);
        let primary = Upstream::start(secondary.port);
        let base = |upstream: &Upstream| format!("http://127.0.0.1:{}/", upstream.port);
        let deployment = Deployment::new();
        deployment.seed_proxies(
            &[],
            trusted,
            &[
                (
                    "pxy",
                    upstream_of("lab", &base(&primary), serde_json::json!({})),
                ),
                (
                    "pxy-strict",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "require_checksum": true }),
                    ),
                ),
                (
                    "pxy-cross",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "redirects": { "max": 3, "cross_origin": true } }),
                    ),
                ),
                (
                    "pxy-scoped",
                    upstream_of(
                        "lab",
                        &base(&primary),
                        serde_json::json!({ "scopes": ["ok/"] }),
                    ),
                ),
                (
                    "pxy-tls",
                    upstream_of(
                        "lab",
                        &format!("https://127.0.0.1:{}/", primary.port),
                        serde_json::json!({}),
                    ),
                ),
                (
                    "pxy-trusted",
                    upstream_of("lab", &base(&primary), serde_json::json!({})),
                ),
            ],
        );
        Self {
            deployment,
            primary,
            secondary,
        }
    }

    /// Arranca el servicio con la salida autorizada hacia los dos upstreams.
    fn start(&self) -> support::Service {
        self.start_with(&[])
    }

    fn start_with(&self, extra: &[(&str, &str)]) -> support::Service {
        let allowed = allow(&[self.primary.port, self.secondary.port]);
        let mut environment = vec![
            ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
            ("HODEI_PROXY__ALLOWED_CREDENTIALS", "LAB_TOKEN"),
            ("LAB_TOKEN", UPSTREAM_SECRET),
        ];
        environment.extend_from_slice(extra);
        self.deployment.start(&environment)
    }
}

// ---------------------------------------------------------------------------
// PROXY-01: lo que se baja se verifica, se guarda y deja recibo.
// ---------------------------------------------------------------------------

#[test]
fn the_first_resolution_is_verified_receipted_and_then_served_as_ours() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 200, "{body}");
    assert_eq!(body.as_bytes(), BODY);

    // El recibo dice de donde vino y con que se comprobo.
    let found = receipts(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(found.len(), 1);
    let receipt = &found[0];
    assert_eq!(receipt["upstream"], "lab");
    assert_eq!(receipt["status"], 200);
    assert_eq!(receipt["redirects"], 0);
    assert_eq!(receipt["size_bytes"], BODY.len());
    assert_eq!(receipt["checksum"]["kind"], "declared");
    assert_eq!(
        receipt["digest"],
        format!(
            "sha256:{}",
            hodei_evidence::ContentDigest::of_bytes(BODY).value
        )
    );
    assert!(
        receipt["source_url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/ok/app.bin") && !url.contains('@')),
        "{receipt}"
    );

    // La credencial se envio al upstream autorizado -- y no sale en ningun
    // sitio que se pueda leer.
    let first = lab.primary.requests();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].authorization.as_deref(),
        Some(format!("Bearer {UPSTREAM_SECRET}").as_str())
    );
    assert!(!receipt.to_string().contains(UPSTREAM_SECRET));
    assert!(!lab.deployment.the_log().contains(UPSTREAM_SECRET));
    assert!(!lab.deployment.the_log().contains(&alice));

    // Una vez resuelto es contenido propio: no se vuelve a pedir.
    let (status, again) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 200);
    assert_eq!(again.as_bytes(), BODY);
    assert_eq!(lab.primary.hits("ok/app.bin"), 1);
    assert_eq!(
        receipts(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin").len(),
        1
    );
    support::stop(&mut service);
}

#[test]
fn a_content_without_a_declared_checksum_is_marked_as_computed_only() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // Sin exigirlo, se sirve -- pero el recibo no finge una verificacion
    // que no hubo.
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/plain.bin");
    assert_eq!(status, 200);
    let found = receipts(&lab.deployment, service.port, &alice, "pxy", "ok/plain.bin");
    assert_eq!(found[0]["checksum"]["kind"], "computed_only");

    // Exigiendolo, no se sirve.
    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-strict",
        "ok/plain.bin",
    );
    assert_eq!(status, 502);
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-strict",
            "ok/plain.bin"
        )
        .is_empty()
    );
    support::stop(&mut service);
}

#[test]
fn a_proxy_does_not_accept_uploads() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = call(
        &lab.deployment,
        service.port,
        "PUT",
        "/repository/pxy/mine.bin",
        &alice,
        Some(b"local bytes"),
    );
    assert_eq!(status, 405);
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "mine.bin");
    assert_ne!(status, 200);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// PROXY-02: un upstream hostil no deja contenido ni recibo.
// ---------------------------------------------------------------------------

#[test]
fn a_hostile_upstream_never_leaves_verified_content_behind() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    for (path, expected) in [
        // Un checksum que no coincide con los bytes.
        ("bad/checksum.bin", 502),
        // Promete mil bytes y entrega diez.
        ("bad/truncated.bin", 502),
        // Mas bytes que el presupuesto del upstream.
        ("big/huge.bin", 502),
        // Un fallo del upstream no se disfraza.
        ("status/500.bin", 502),
        // Lo que no existe, no existe.
        ("missing/none.bin", 404),
    ] {
        let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(status, expected, "{path}: {body}");
        // Ni el mensaje ni el log filtran la credencial.
        assert!(!body.contains(UPSTREAM_SECRET));
        // Y no queda nada que parezca verificado: ni recibo, ni contenido.
        assert!(
            receipts(&lab.deployment, service.port, &alice, "pxy", path).is_empty(),
            "{path} left a receipt"
        );
        let (again, _) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(again, expected, "{path} is served on the second request");
    }
    assert!(!lab.deployment.the_log().contains(UPSTREAM_SECRET));
    support::stop(&mut service);
}

#[test]
fn a_path_outside_the_scopes_is_not_asked_upstream() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-scoped",
        "bad/checksum.bin",
    );
    assert_eq!(status, 404);
    assert!(lab.primary.requests().is_empty(), "the upstream was asked");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-scoped",
        "ok/app.bin",
    );
    assert_eq!(status, 200);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// SSRF: la salida solo va donde se ha autorizado.
// ---------------------------------------------------------------------------

#[test]
fn a_credential_the_operator_did_not_allow_is_never_read_nor_sent() {
    let lab = Lab::new(&[]);
    let allowed = allow(&[lab.primary.port, lab.secondary.port]);
    // El host esta permitido y el secreto existe en el entorno, pero la
    // variable no esta en `allowed_credentials`: el repositorio la nombra
    // y no basta.
    let mut service = lab.deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("LAB_TOKEN", UPSTREAM_SECRET),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(
        lab.primary.requests().is_empty(),
        "the upstream was asked without its credential"
    );
    support::stop(&mut service);
}

#[test]
fn nothing_leaves_the_service_without_an_allow_list() {
    let lab = Lab::new(&[]);
    // Sin lista de salida: el valor por defecto es no salir.
    let mut service = lab.deployment.start(&[("LAB_TOKEN", UPSTREAM_SECRET)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(lab.primary.requests().is_empty());
    support::stop(&mut service);

    // Con el host en la lista pero sin el permiso administrativo explicito
    // para redes privadas, el loopback sigue prohibido.
    let entry = format!("127.0.0.1:{}", lab.primary.port);
    let mut service = lab.deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", entry.as_str()),
        ("LAB_TOKEN", UPSTREAM_SECRET),
    ]);
    let (status, _) = get(&lab.deployment, service.port, &alice, "pxy", "ok/app.bin");
    assert_eq!(status, 502);
    assert!(lab.primary.requests().is_empty());
    support::stop(&mut service);
}

#[test]
fn a_redirect_towards_a_forbidden_place_is_refused_before_it_is_followed() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    for path in [
        // Otro origen en el loopback (otro puerto autorizado, pero un
        // redirect entre origenes no esta permitido en este repositorio).
        "redir/loopback.bin",
        // El servicio de metadatos de la nube.
        "redir/metadata.bin",
        // Una red privada.
        "redir/private.bin",
        // Un host que nadie autorizo.
        "redir/stranger.bin",
        // Un bucle.
        "redir/loop.bin",
    ] {
        let (status, body) = get(&lab.deployment, service.port, &alice, "pxy", path);
        assert_eq!(status, 502, "{path}: {body}");
        assert!(
            receipts(&lab.deployment, service.port, &alice, "pxy", path).is_empty(),
            "{path} left a receipt"
        );
    }
    // El otro origen no recibio nada: el redirect no se siguio.
    assert!(lab.secondary.requests().is_empty());

    // Un redirect dentro del mismo origen si se sigue, y el recibo lo dice.
    let (status, body) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy",
        "redir/same.bin",
    );
    assert_eq!(status, 200, "{body}");
    let found = receipts(
        &lab.deployment,
        service.port,
        &alice,
        "pxy",
        "redir/same.bin",
    );
    assert_eq!(found[0]["redirects"], 1);
    assert!(
        found[0]["source_url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/ok/app.bin"))
    );
    support::stop(&mut service);
}

#[test]
fn a_cross_origin_redirect_never_carries_the_credential() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // Este repositorio si permite el redirect hacia otro origen autorizado.
    let (status, body) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-cross",
        "redir/loopback.bin",
    );
    assert_eq!(status, 200, "{body}");
    let received = lab.secondary.requests();
    assert_eq!(received.len(), 1, "the redirect was not followed");
    assert!(
        received[0].authorization.is_none(),
        "the credential crossed to another origin: {received:?}"
    );
    // El origen primero si la recibio, y el recibo cuenta el salto.
    assert!(lab.primary.requests()[0].authorization.is_some());
    let found = receipts(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-cross",
        "redir/loopback.bin",
    );
    assert_eq!(found[0]["redirects"], 1);
    assert!(
        found[0]["source_url"]
            .as_str()
            .is_some_and(|url| url.contains(&lab.secondary.port.to_string()))
    );
    support::stop(&mut service);
}

#[test]
fn an_upstream_with_an_invalid_tls_endpoint_is_refused() {
    let lab = Lab::new(&[]);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // El upstream habla http plano en un puerto que se configuro como
    // https: el handshake de TLS no puede completarse, y no hay vuelta
    // atras a un canal sin cifrar.
    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-tls",
        "ok/app.bin",
    );
    assert_eq!(status, 502);
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-tls",
            "ok/app.bin"
        )
        .is_empty()
    );
    assert!(
        lab.primary
            .requests()
            .iter()
            .all(|one| one.authorization.is_none())
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Confianza: lo recien bajado no es trusted por haber llegado.
// ---------------------------------------------------------------------------

#[test]
fn what_was_just_downloaded_does_not_reach_the_trusted_channel_by_itself() {
    let lab = Lab::new(&["pxy-trusted"]);
    let policy = lab.deployment.directory.path().join("trust-policy.json");
    let permissive = hodei_trust_policy::TrustPolicy::new("default", "1", Vec::new());
    std::fs::write(
        &policy,
        serde_json::to_vec(&permissive).expect("the policy"),
    )
    .expect("written");
    let policy = policy.to_string_lossy().into_owned();
    let mut service = lab.start_with(&[("HODEI_TRUST__POLICY_FILE", policy.as_str())]);
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (status, _) = get(
        &lab.deployment,
        service.port,
        &alice,
        "pxy-trusted",
        "ok/app.bin",
    );
    assert_eq!(status, 404, "an unpromoted download was served as trusted");
    // El 404 no es un fallo de resolucion: el upstream si fue consultado.
    assert!(
        !lab.primary.requests().is_empty(),
        "the content was never fetched"
    );
    // Y el recibo habla de un digest que el canal no sirve: tampoco dice
    // que existe.
    assert!(
        receipts(
            &lab.deployment,
            service.port,
            &alice,
            "pxy-trusted",
            "ok/app.bin"
        )
        .is_empty(),
        "the provenance told about content the channel does not serve"
    );
    support::stop(&mut service);
}
