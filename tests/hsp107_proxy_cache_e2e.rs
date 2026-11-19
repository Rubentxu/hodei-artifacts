//! La cache del repositorio proxy (HSP-107 / HAR-13), contra el wire.
//!
//! La superficie que ejercita es la del formato **raw**: el upstream
//! sirve bytes opacos y la cache los guarda y los vuelve a servir con
//! sus validadores, que es lo que la cache de un proxy raw promete.
//!
//! El servicio es un proceso de verdad con catalogo, almacen de bytes y
//! Cedar reales. El upstream es un servidor local que este fichero
//! controla: cambia lo que sirve, deja de contestar, tarda, y apunta las
//! cabeceras condicionales que le llegan.
//!
//! Lo que se mide es lo que una persona ve: de donde sale cada respuesta
//! (`x-hodei-cache`), cuantas veces se molesta al upstream y que recibo
//! queda. Un upstream que cambia, que cae o que no esta nunca puede acabar
//! presentado como contenido verificado que no es.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::{Curl, Deployment, FIRST_ACCOUNT};

const BODY: &[u8] = b"content that lives upstream";
const NEWER: &[u8] = b"content that the upstream changed";

/// Una peticion que llego al upstream.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    if_none_match: Option<String>,
    if_modified_since: Option<String>,
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
    delay_ms: AtomicU64,
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
            delay_ms: AtomicU64::new(0),
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

    fn remove(&self, path: &str) {
        self.world.docs.lock().expect("docs").remove(path);
    }

    fn fail(&self, failing: bool) {
        self.world.failing.store(failing, Ordering::SeqCst);
    }

    fn delay(&self, millis: u64) {
        self.world.delay_ms.store(millis, Ordering::SeqCst);
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

    fn total(&self) -> usize {
        self.requests().len()
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
    world.seen.lock().expect("the log").push(Seen {
        path: path.clone(),
        if_none_match: if_none_match.clone(),
        if_modified_since: if_modified_since.clone(),
    });

    let delay = world.delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay));
    }

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
fn proxy_of(upstream: &Upstream, scopes: &[&str], cache: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "upstream": {
            "id": "lab",
            "url": format!("http://127.0.0.1:{}/", upstream.port),
            "scopes": scopes,
            "max_bytes": 1024,
        },
        "cache": cache,
    })
}

struct Lab {
    deployment: Deployment,
    upstream: Upstream,
}

impl Lab {
    /// `repositories`: clave del repositorio y su politica de cache.
    fn new(repositories: &[(&str, serde_json::Value)]) -> Self {
        let upstream = Upstream::start();
        let deployment = Deployment::new();
        let proxies: Vec<(&str, serde_json::Value)> = repositories
            .iter()
            .map(|(key, cache)| (*key, proxy_of(&upstream, &["/"], cache.clone())))
            .collect();
        deployment.seed_proxies(&[], &[], &proxies);
        Self {
            deployment,
            upstream,
        }
    }

    fn start(&self) -> support::Service {
        self.start_with(&[])
    }

    fn start_with(&self, extra: &[(&str, &str)]) -> support::Service {
        let allowed = format!("127.0.0.1:{};private;http", self.upstream.port);
        let mut environment = vec![("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())];
        environment.extend_from_slice(extra);
        self.deployment.start(&environment)
    }

    /// Cambia la configuracion de un repositorio con el servicio parado.
    fn reconfigure(&self, key: &str, scopes: &[&str], cache: serde_json::Value) {
        self.deployment
            .reconfigure_proxy(key, proxy_of(&self.upstream, scopes, cache));
    }
}

/// Una respuesta: codigo, cuerpo y cabeceras que interesan.
struct Reply {
    status: u16,
    body: String,
    cache: Option<String>,
    upstream: Option<String>,
    confirmed_at: Option<String>,
}

fn fetch_into(
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
    into: &std::path::Path,
) -> Reply {
    let _ = std::fs::remove_file(into);
    let answer = Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        token,
        None,
        None,
        Some(into),
    )
    .run();
    Reply {
        status: answer.status,
        body: String::from_utf8_lossy(&std::fs::read(into).unwrap_or_default()).into_owned(),
        cache: answer.header("x-hodei-cache"),
        upstream: answer.header("x-hodei-upstream"),
        confirmed_at: answer.header("x-hodei-confirmed-at"),
    }
}

fn get(lab: &Lab, port: u16, repository: &str, path: &str) -> Reply {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("response.bin");
    fetch_into(port, &alice, repository, path, &into)
}

/// Los recibos de una ruta: `(outcome, status)` en el orden en que llegan.
fn outcomes(lab: &Lab, port: u16, repository: &str, path: &str) -> Vec<(String, u64)> {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("receipts.json");
    let _ = std::fs::remove_file(&into);
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
    assert_eq!(answer.status, 200);
    let parsed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&into).expect("the body")).expect("json");
    let mut found: Vec<(String, u64, String)> = parsed["receipts"]
        .as_array()
        .expect("receipts")
        .iter()
        .map(|receipt| {
            (
                receipt["outcome"].as_str().expect("outcome").to_string(),
                receipt["status"].as_u64().expect("status"),
                receipt["fetched_at"].as_str().expect("time").to_string(),
            )
        })
        .collect();
    found.sort_by(|left, right| left.2.cmp(&right.2));
    found
        .into_iter()
        .map(|(outcome, status, _)| (outcome, status))
        .collect()
}

// ---------------------------------------------------------------------------
// TTL, validadores y revalidacion.
// ---------------------------------------------------------------------------

#[test]
fn a_stale_metadata_is_revalidated_with_the_validators_and_a_304_keeps_it() {
    let lab = Lab::new(&[(
        "pxy-meta",
        serde_json::json!({ "ttl": { "metadata_secs": 0 } }),
    )]);
    lab.upstream
        .put("pkg/index.json", BODY, Some("\"m1\""), None);
    lab.upstream.put(
        "pkg/dated/index.json",
        BODY,
        None,
        Some("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    let mut service = lab.start();

    let first = get(&lab, service.port, "pxy-meta", "pkg/index.json");
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.body.as_bytes(), BODY);
    assert_eq!(first.cache.as_deref(), Some("miss"));

    // TTL 0: cada lectura pregunta, y lo que pregunta es condicional.
    let second = get(&lab, service.port, "pxy-meta", "pkg/index.json");
    assert_eq!(second.status, 200);
    assert_eq!(second.body.as_bytes(), BODY);
    assert_eq!(second.cache.as_deref(), Some("revalidated"));
    let seen = lab.upstream.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].if_none_match, None);
    assert_eq!(seen[1].if_none_match.as_deref(), Some("\"m1\""));

    // El recibo de la confirmacion existe y no movio nada.
    assert_eq!(
        outcomes(&lab, service.port, "pxy-meta", "pkg/index.json"),
        vec![
            ("fetched".to_string(), 200),
            ("revalidated".to_string(), 304)
        ]
    );

    // Sin ETag, la condicion es la fecha.
    let _ = get(&lab, service.port, "pxy-meta", "pkg/dated/index.json");
    let again = get(&lab, service.port, "pxy-meta", "pkg/dated/index.json");
    assert_eq!(again.cache.as_deref(), Some("revalidated"));
    assert_eq!(
        lab.upstream
            .requests()
            .last()
            .expect("a request")
            .if_modified_since
            .as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
    support::stop(&mut service);
}

#[test]
fn a_metadata_the_upstream_changed_is_refreshed_and_content_is_served_from_the_cache_while_fresh() {
    let lab = Lab::new(&[(
        "pxy-ttl",
        serde_json::json!({ "ttl": { "metadata_secs": 3 } }),
    )]);
    lab.upstream
        .put("pkg/index.json", BODY, Some("\"m1\""), None);
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    let mut service = lab.start();

    assert_eq!(
        get(&lab, service.port, "pxy-ttl", "pkg/index.json")
            .cache
            .as_deref(),
        Some("miss")
    );
    // Dentro del TTL no se molesta al upstream.
    let fresh = get(&lab, service.port, "pxy-ttl", "pkg/index.json");
    assert_eq!(fresh.cache.as_deref(), Some("hit"));
    assert_eq!(lab.upstream.hits("pkg/index.json"), 1);

    // El contenido no tiene TTL: una vez bajado no caduca.
    let _ = get(&lab, service.port, "pxy-ttl", "pkg/lib.bin");
    std::thread::sleep(Duration::from_millis(3300));
    let lib = get(&lab, service.port, "pxy-ttl", "pkg/lib.bin");
    assert_eq!(lib.cache.as_deref(), Some("hit"));
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 1);

    // Caducada la metadata, y cambiada upstream, se actualiza.
    lab.upstream
        .put("pkg/index.json", NEWER, Some("\"m2\""), None);
    let refreshed = get(&lab, service.port, "pxy-ttl", "pkg/index.json");
    assert_eq!(refreshed.status, 200, "{}", refreshed.body);
    assert_eq!(refreshed.body.as_bytes(), NEWER);
    assert_eq!(refreshed.cache.as_deref(), Some("refreshed"));
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Single-flight.
// ---------------------------------------------------------------------------

#[test]
fn concurrent_requests_for_the_same_content_download_it_once() {
    let lab = Lab::new(&[("pxy-flight", serde_json::json!({}))]);
    lab.upstream.put("pkg/big.bin", BODY, Some("\"b1\""), None);
    // Holgura de sobra para que los ocho `curl` arranquen dentro de una
    // misma descarga aunque la maquina vaya cargada.
    lab.upstream.delay(2000);
    let mut service = lab.start();
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let replies: Vec<Reply> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let into = lab
                    .deployment
                    .directory
                    .path()
                    .join(format!("flight-{index}.bin"));
                let alice = alice.clone();
                let port = service.port;
                scope.spawn(move || fetch_into(port, &alice, "pxy-flight", "pkg/big.bin", &into))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("the request ends"))
            .collect()
    });
    for reply in &replies {
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(reply.body.as_bytes(), BODY);
        // Todas compartieron la misma descarga: ninguna vio una copia ya
        // publicada.
        assert_eq!(reply.cache.as_deref(), Some("miss"));
    }
    assert_eq!(
        lab.upstream.hits("pkg/big.bin"),
        1,
        "the same bytes were downloaded more than once"
    );
    assert_eq!(
        outcomes(&lab, service.port, "pxy-flight", "pkg/big.bin").len(),
        1
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Upstream caido, stale y 404.
// ---------------------------------------------------------------------------

#[test]
fn a_stale_copy_is_served_only_when_the_policy_allows_it_and_says_so() {
    let lab = Lab::new(&[
        (
            "pxy-nostale",
            serde_json::json!({ "ttl": { "content_secs": 0 } }),
        ),
        (
            "pxy-stale",
            serde_json::json!({ "ttl": { "content_secs": 0 }, "stale_if_error_secs": 3600 }),
        ),
    ]);
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    let mut service = lab.start();
    for repository in ["pxy-nostale", "pxy-stale"] {
        assert_eq!(
            get(&lab, service.port, repository, "pkg/lib.bin").status,
            200
        );
    }

    lab.upstream.fail(true);
    let refused = get(&lab, service.port, "pxy-nostale", "pkg/lib.bin");
    assert_eq!(refused.status, 502, "{}", refused.body);
    assert_eq!(refused.cache, None);

    let stale = get(&lab, service.port, "pxy-stale", "pkg/lib.bin");
    assert_eq!(stale.status, 200, "{}", stale.body);
    assert_eq!(stale.body.as_bytes(), BODY);
    assert_eq!(stale.cache.as_deref(), Some("stale"));
    assert_eq!(stale.upstream.as_deref(), Some("lab"));
    assert!(
        stale.confirmed_at.is_some(),
        "a stale answer must say when the upstream last confirmed it"
    );

    // Un 404 del upstream no es un fallo: el contenido ya no existe alli y
    // no se sirve como si siguiera existiendo.
    lab.upstream.fail(false);
    lab.upstream.remove("pkg/lib.bin");
    let gone = get(&lab, service.port, "pxy-stale", "pkg/lib.bin");
    assert_eq!(gone.status, 404, "{}", gone.body);
    support::stop(&mut service);
}

#[test]
fn a_missing_path_is_remembered_only_when_the_policy_says_so() {
    let lab = Lab::new(&[
        ("pxy-neg", serde_json::json!({ "negative_secs": 3600 })),
        ("pxy-noneg", serde_json::json!({})),
    ]);
    let mut service = lab.start();

    for _ in 0..3 {
        assert_eq!(
            get(&lab, service.port, "pxy-neg", "pkg/missing.bin").status,
            404
        );
    }
    assert_eq!(lab.upstream.hits("pkg/missing.bin"), 1);

    for _ in 0..2 {
        assert_eq!(
            get(&lab, service.port, "pxy-noneg", "pkg/missing.bin").status,
            404
        );
    }
    assert_eq!(lab.upstream.hits("pkg/missing.bin"), 3);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Offline determinista.
// ---------------------------------------------------------------------------

#[test]
fn offline_serves_only_verified_cached_content_and_never_touches_the_network() {
    let lab = Lab::new(&[("pxy-off", serde_json::json!({}))]);
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    lab.upstream
        .put("pkg/other.bin", NEWER, Some("\"o1\""), None);
    let mut service = lab.start();
    assert_eq!(
        get(&lab, service.port, "pxy-off", "pkg/lib.bin").status,
        200
    );
    support::stop(&mut service);
    let before = lab.upstream.total();

    // El servicio entero sin red.
    let mut service = lab.start_with(&[("HODEI_PROXY__OFFLINE", "true")]);
    let hit = get(&lab, service.port, "pxy-off", "pkg/lib.bin");
    assert_eq!(hit.status, 200, "{}", hit.body);
    assert_eq!(hit.body.as_bytes(), BODY);
    assert_eq!(hit.cache.as_deref(), Some("offline"));
    assert_eq!(hit.upstream.as_deref(), Some("lab"));
    assert!(hit.confirmed_at.is_some());
    let miss = get(&lab, service.port, "pxy-off", "pkg/other.bin");
    assert_eq!(miss.status, 503, "{}", miss.body);
    assert!(miss.body.contains("offline"), "{}", miss.body);
    assert_eq!(lab.upstream.total(), before, "offline touched the network");
    support::stop(&mut service);

    // El repositorio solo, por su politica.
    lab.reconfigure("pxy-off", &["/"], serde_json::json!({ "offline": true }));
    let mut service = lab.start();
    let hit = get(&lab, service.port, "pxy-off", "pkg/lib.bin");
    assert_eq!(hit.status, 200, "{}", hit.body);
    assert_eq!(hit.cache.as_deref(), Some("offline"));
    assert_eq!(
        get(&lab, service.port, "pxy-off", "pkg/other.bin").status,
        503
    );
    assert_eq!(lab.upstream.total(), before, "offline touched the network");
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Revocacion: la confianza se evalua en cada lectura, tambien las degradadas.
// ---------------------------------------------------------------------------

/// Una peticion JSON `POST` con el token de alice; devuelve el codigo.
fn post_json(lab: &Lab, port: u16, path: &str, body: &serde_json::Value) -> u16 {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let directory = lab.deployment.directory.path();
    let sent = directory.join("post.json");
    std::fs::write(&sent, body.to_string()).expect("the body is written");
    let into = directory.join("post-response.json");
    let _ = std::fs::remove_file(&into);
    Curl::call(
        port,
        "POST",
        path,
        &alice,
        Some(sent.as_path()),
        Some("application/json"),
        Some(&into),
    )
    .run()
    .status
}

#[test]
fn a_revoked_digest_is_not_served_fresh_stale_or_offline() {
    let upstream = Upstream::start();
    let deployment = Deployment::new();
    let cache = serde_json::json!({ "ttl": { "content_secs": 0 }, "stale_if_error_secs": 3600 });
    deployment.seed_proxies(
        &[],
        &["pxy-rev"],
        &[("pxy-rev", proxy_of(&upstream, &["/"], cache))],
    );
    let lab = Lab {
        deployment,
        upstream,
    };
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    let policy_path = lab.deployment.directory.path().join("trust-policy.json");
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&hodei_trust_policy::TrustPolicy::new(
            "default",
            "1",
            Vec::new(),
        ))
        .expect("the policy"),
    )
    .expect("written");
    let policy = policy_path.to_string_lossy().into_owned();
    let digest = hodei_evidence::ContentDigest::of_bytes(BODY).value;
    let promotion = serde_json::json!({
        "repository": "pxy-rev",
        "path": "pkg/lib.bin",
        "digest": format!("sha256:{digest}"),
    });
    let revocation = serde_json::json!({
        "repository": "pxy-rev",
        "digest": format!("sha256:{digest}"),
        "reason": "an operator withdrew it",
    });

    let mut service = lab.start_with(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    // Bajado pero sin promover: el canal trusted no lo sirve.
    assert_eq!(
        get(&lab, service.port, "pxy-rev", "pkg/lib.bin").status,
        404
    );
    assert_eq!(
        post_json(&lab, service.port, "/api/v1/promotions", &promotion),
        200
    );
    let served = get(&lab, service.port, "pxy-rev", "pkg/lib.bin");
    assert_eq!(served.status, 200, "{}", served.body);

    // Revocado: ni revalidando, ni con el upstream caido (stale).
    assert_eq!(
        post_json(&lab, service.port, "/api/v1/promotions/revoke", &revocation),
        200
    );
    let revoked = get(&lab, service.port, "pxy-rev", "pkg/lib.bin");
    assert_eq!(revoked.status, 404, "a revoked digest was served");
    lab.upstream.fail(true);
    let stale = get(&lab, service.port, "pxy-rev", "pkg/lib.bin");
    assert_eq!(stale.status, 404, "a revoked digest was served stale");
    support::stop(&mut service);

    // Ni con el servicio sin red.
    let mut service = lab.start_with(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let offline = get(&lab, service.port, "pxy-rev", "pkg/lib.bin");
    assert_eq!(offline.status, 404, "a revoked digest was served offline");
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Mismatch, cuarentena, cambio de politica.
// ---------------------------------------------------------------------------

#[test]
fn other_bytes_for_immutable_content_are_quarantined_until_the_configuration_changes() {
    let cache = serde_json::json!({ "ttl": { "content_secs": 0 } });
    let lab = Lab::new(&[("pxy-quar", cache.clone())]);
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    let mut service = lab.start();
    assert_eq!(
        get(&lab, service.port, "pxy-quar", "pkg/lib.bin").status,
        200
    );

    // El upstream entrega otros bytes para lo mismo.
    lab.upstream.put("pkg/lib.bin", NEWER, Some("\"c2\""), None);
    let held = get(&lab, service.port, "pxy-quar", "pkg/lib.bin");
    assert_eq!(held.status, 502, "{}", held.body);
    assert!(held.body.contains("withheld"), "{}", held.body);
    assert_eq!(held.cache, None, "withheld content carries no cache status");
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 2);

    // La cuarentena es dura: ni se sirve lo viejo ni se vuelve a preguntar.
    let still = get(&lab, service.port, "pxy-quar", "pkg/lib.bin");
    assert_eq!(still.status, 502);
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 2);
    assert_eq!(
        outcomes(&lab, service.port, "pxy-quar", "pkg/lib.bin"),
        vec![
            ("fetched".to_string(), 200),
            ("quarantined".to_string(), 200)
        ]
    );
    support::stop(&mut service);

    // Una configuracion nueva es la palanca para reabrirla.
    lab.reconfigure("pxy-quar", &["/"], cache);
    let mut service = lab.start();
    let reopened = get(&lab, service.port, "pxy-quar", "pkg/lib.bin");
    assert_eq!(reopened.status, 200, "{}", reopened.body);
    assert_eq!(reopened.body.as_bytes(), NEWER);
    assert_eq!(reopened.cache.as_deref(), Some("refreshed"));
    // Y una vez aceptado, lo nuevo es lo que el upstream confirma: no vuelve
    // a retenerse.
    let again = get(&lab, service.port, "pxy-quar", "pkg/lib.bin");
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.cache.as_deref(), Some("revalidated"));
    support::stop(&mut service);
}

#[test]
fn a_policy_change_that_leaves_a_path_out_of_scope_hides_what_was_cached() {
    let lab = Lab::new(&[("pxy-scope", serde_json::json!({}))]);
    lab.upstream.put("pkg/lib.bin", BODY, Some("\"c1\""), None);
    lab.upstream.put("ok/lib.bin", BODY, Some("\"c1\""), None);
    let mut service = lab.start();
    for path in ["pkg/lib.bin", "ok/lib.bin"] {
        assert_eq!(get(&lab, service.port, "pxy-scope", path).status, 200);
    }
    support::stop(&mut service);

    lab.reconfigure("pxy-scope", &["ok/"], serde_json::json!({}));
    let mut service = lab.start();
    // Lo que ya no cae en el scope no se sirve ni se pregunta.
    assert_eq!(
        get(&lab, service.port, "pxy-scope", "pkg/lib.bin").status,
        404
    );
    assert_eq!(lab.upstream.hits("pkg/lib.bin"), 1);
    // Lo que sigue dentro se revalida una vez bajo la configuracion nueva.
    let kept = get(&lab, service.port, "pxy-scope", "ok/lib.bin");
    assert_eq!(kept.status, 200, "{}", kept.body);
    assert_eq!(kept.cache.as_deref(), Some("revalidated"));
    assert_eq!(lab.upstream.hits("ok/lib.bin"), 2);
    support::stop(&mut service);
}
