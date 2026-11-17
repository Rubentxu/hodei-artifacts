//! La migracion de HSP-123 contra **origenes con la forma real de sus
//! APIs**: servidores fake que responden el JSON exacto de Nexus
//! (`/service/rest/v1/search`, `/service/rest/v1/repositories`) y de
//! Artifactory (`/api/repositories`, `/api/storage/{key}`), y el hub
//! real arrancado como proceso con la superficie administrativa abierta.
//!
//! Lo que se comprueba no es el parser solo: es el ciclo completo del
//! criterio de aceptacion — inventario con diferencias explicitas,
//! import verificado por SHA, reimport que no mueve bytes, corrupto que
//! no publica nada, resolucion nativa por path antes y despues, y la
//! prueba estructural de que al origen solo le hicieron `GET`s.

mod support;

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha1::Digest as _;
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// El log de peticiones del origen: cada linea es `METODO path`. La
/// prueba la exige de lectura pura.
type Log = Arc<Mutex<Vec<String>>>;

/// Un artefacto del origen fake: donde vive, que formato declara y que
/// bytes sirve. `honest=false` predica un sha256 que no es el suyo.
struct FakeAsset {
    repository: &'static str,
    format: &'static str,
    path: &'static str,
    bytes: Vec<u8>,
    honest: bool,
}

impl FakeAsset {
    fn sha256(&self) -> String {
        if self.honest {
            hex(&sha2::Sha256::digest(&self.bytes))
        } else {
            "0".repeat(64)
        }
    }

    fn sha1(&self) -> String {
        hex(&sha1::Sha1::digest(&self.bytes))
    }

    /// El documento de busqueda de Nexus para este artefacto: el pom
    /// declara solo sha1, como en un Nexus viejo; el resto, ambos.
    fn nexus_item(&self, base: &str) -> Value {
        let checksums = if self.path.ends_with(".pom") {
            json!({"sha1": self.sha1()})
        } else {
            json!({"sha1": self.sha1(), "sha256": self.sha256()})
        };
        json!({
            "repository": self.repository,
            "format": self.format,
            "name": self.path,
            "assets": [{
                "repository": self.repository,
                "format": self.format,
                "path": self.path,
                "downloadUrl": format!("{base}/repository/{}/{}", self.repository, self.path),
                "checksum": checksums,
                "size": self.bytes.len(),
            }],
        })
    }

    fn download_path(&self, kind: &str) -> String {
        match kind {
            "nexus" => format!("/repository/{}/{}", self.repository, self.path),
            _ => format!("/{}/{}", self.repository, self.path),
        }
    }
}

/// El origen fake completo: ruta exacta (con query) contra respuesta.
/// Se construye con el puerto ya conocido, como lo haria el propio
/// Nexus con su `downloadUrl`.
fn spawn_origin(kind: &str, assets: Vec<FakeAsset>) -> (u16, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port for the fake origin");
    let port = listener.local_addr().expect("the address").port();
    let base = format!("http://127.0.0.1:{port}");
    let log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut routes: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    let serve_json = |routes: &mut HashMap<String, (String, Vec<u8>)>, path: &str, body: Value| {
        routes.insert(
            path.to_string(),
            (
                "application/json".to_string(),
                body.to_string().into_bytes(),
            ),
        );
    };

    let json_route = |name: &str| -> (String, Vec<u8>) {
        for asset in &assets {
            if asset.download_path(kind) == name {
                return ("application/octet-stream".to_string(), asset.bytes.clone());
            }
        }
        panic!("the fake origin does not serve {name}")
    };

    if kind == "nexus" {
        serve_json(
            &mut routes,
            "/service/rest/v1/repositories",
            json!([
                {"name": "maven-releases", "format": "maven2", "type": "hosted"},
                {"name": "raw-hosted", "format": "raw", "type": "hosted"},
                {"name": "npm-proxy", "format": "npm", "type": "proxy"},
            ]),
        );
        let first: Vec<Value> = assets
            .iter()
            .take(3)
            .map(|asset| asset.nexus_item(&base))
            .collect();
        let second: Vec<Value> = assets
            .iter()
            .skip(3)
            .map(|asset| asset.nexus_item(&base))
            .collect();
        let mut page = json!({"items": first});
        if !second.is_empty() {
            page["continuationToken"] = json!("page2");
        }
        serve_json(&mut routes, "/service/rest/v1/search?pageSize=1000", page);
        if !second.is_empty() {
            serve_json(
                &mut routes,
                "/service/rest/v1/search?pageSize=1000&continuationToken=page2",
                json!({"items": second}),
            );
        }
        for asset in &assets {
            routes.insert(
                asset.download_path(kind),
                json_route(&asset.download_path(kind)),
            );
        }
    } else {
        serve_json(
            &mut routes,
            "/api/repositories",
            json!([
                {"key": "libs", "packageType": "maven", "type": "local"},
                {"key": "remote-maven", "packageType": "maven", "type": "remote"},
            ]),
        );
        serve_json(
            &mut routes,
            "/api/storage/libs?list",
            json!({"path": "/", "children": [{"uri": "/com", "folder": true}]}),
        );
        serve_json(
            &mut routes,
            "/api/storage/libs/com?list",
            json!({"path": "/com", "children": [{"uri": "/demo-1.0.jar", "folder": false}]}),
        );
        let jar = assets.first().expect("the artifactory fake has one asset");
        serve_json(
            &mut routes,
            "/api/storage/libs/com/demo-1.0.jar",
            json!({
                "downloadUri": format!("{base}/libs/{}", jar.path),
                "size": jar.bytes.len(),
                "checksums": {"sha1": jar.sha1(), "sha256": jar.sha256()},
            }),
        );
        for asset in &assets {
            routes.insert(
                asset.download_path(kind),
                json_route(&asset.download_path(kind)),
            );
        }
    }

    let thread_log = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let log = thread_log.clone();
            let routes = routes.clone();
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&buffer).into_owned();
                let Some(head) = request.lines().next() else {
                    return;
                };
                let mut pieces = head.split_whitespace();
                let (Some(method), Some(path)) = (pieces.next(), pieces.next()) else {
                    return;
                };
                log.lock()
                    .expect("the log")
                    .push(format!("{method} {path}"));
                let (status, content_type, body) = match routes.get(path) {
                    Some((content_type, body)) => {
                        ("200 OK".to_string(), content_type.clone(), body.clone())
                    }
                    None => (
                        "404 Not Found".to_string(),
                        "text/plain".to_string(),
                        b"no such origin document".to_vec(),
                    ),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                );
                let mut response = head.into_bytes();
                response.extend_from_slice(&body);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            });
        }
    });
    (port, log)
}

/// El despliegue con la superficie administrativa abierta: el esquema
/// con su namespace `Admin` y la politica que solo deja pasar a alice,
/// exactamente el montaje de HSP-115.
fn admin_deployment() -> Deployment {
    let deployment = Deployment::new();
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        },
    });
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        schema.to_string(),
    )
    .expect("the admin schema is written");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        format!(
            "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{}:User/alice\", action == Admin::Action::\"AccessAdminApi\", resource);\n",
            support::the_policy(),
            support::FIRST_ACCOUNT,
        ),
    )
    .expect("the admin policy is written");
    deployment
}

/// El cuerpo JSON de la peticion de migracion contra un origen.
fn migration_body(kind: &str, port: u16) -> Value {
    json!({
        "source": {
            "kind": kind,
            "base_url": format!("http://127.0.0.1:{port}"),
        },
    })
}

fn post_json(port: u16, path: &str, token: &str, body: &Value) -> Answer {
    let sent = next("sent");
    std::fs::write(&sent, body.to_string()).expect("the body is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "POST",
        path,
        token,
        Some(&sent),
        &[("content-type", "application/json")],
        Some(&into),
    )
    .run()
}

fn body_of(answer: &Answer) -> Value {
    serde_json::from_slice(&answer.body).unwrap_or(Value::Null)
}

/// Un `GET` de la prueba al origen fake, sin `curl`: los bytes exactos
/// que el origen predica, medidos por el propio arnés.
fn origin_get(path: &str, port: u16) -> Vec<u8> {
    use std::io::{Read as _, Write as _};
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("the fake origin answers");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: origin\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .expect("the request goes out");
    let mut buffer = Vec::new();
    stream
        .read_to_end(&mut buffer)
        .expect("the response comes in");
    let head_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head")
        + 4;
    buffer[head_end..].to_vec()
}

/// Que el origen solo vio lecturas, y cuantas peticiones de contenido.
fn assert_read_only(log: &Log) {
    let entries = log.lock().expect("the log").clone();
    for entry in entries {
        assert!(
            entry.starts_with("GET "),
            "the origin only ever sees reads, and it saw: {entry}"
        );
    }
}

fn content_gets(log: &Log, kind: &str) -> usize {
    let prefix = match kind {
        "nexus" => "GET /repository/",
        _ => "GET /libs/",
    };
    log.lock()
        .expect("the log")
        .iter()
        .filter(|entry| entry.starts_with(prefix))
        .count()
}

fn asset(
    repository: &'static str,
    format: &'static str,
    path: &'static str,
    seed: u8,
) -> FakeAsset {
    let last = 64u8 + seed;
    FakeAsset {
        repository,
        format,
        path,
        bytes: (0..last).map(|index| index ^ seed).collect(),
        honest: true,
    }
}

/// Los cuatro artefactos del origen Nexus de la suite: dos maven (el
/// pom solo declara sha1), uno raw y uno npm que no es migrable.
fn nexus_assets() -> Vec<FakeAsset> {
    vec![
        asset(
            "maven-releases",
            "maven2",
            "com/example/demo/1.0/demo-1.0.jar",
            1,
        ),
        asset(
            "maven-releases",
            "maven2",
            "com/example/demo/1.0/demo-1.0.pom",
            2,
        ),
        asset("raw-hosted", "raw", "tools/hello.bin", 3),
        asset("npm-proxy", "npm", "demo/-/demo-1.0.0.tgz", 4),
    ]
}

/// AC1: el inventario suma bytes y rutas, dice la modalidad del origen,
/// la permission del destino y escribe cada diferencia en claro.
#[test]
fn the_inventory_reports_everything_and_spells_out_every_difference() {
    let (origin, _log) = spawn_origin("nexus", nexus_assets());
    let deployment = admin_deployment();
    deployment.seed_with(&["raw-hosted"]);
    deployment.seed_maven(&[("maven-releases", Value::Null)]);
    let mut service = deployment.start(&[("HODEI_SECURITY__ADMIN_API_ENABLED", "true")]);
    let port = service.port;
    let alice = support::token(support::FIRST_ACCOUNT, "alice");
    let bob = support::token(support::FIRST_ACCOUNT, "bob");

    let answer = post_json(
        port,
        "/api/v1/migrations/inventory",
        &alice,
        &migration_body("nexus", origin),
    );
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    let report = body_of(&answer);

    assert_eq!(report["totals"]["assets"], 4);
    assert_eq!(report["totals"]["planned"], 3, "the npm tgz is not planned");
    let maven = report["repositories"]
        .as_array()
        .expect("repositories")
        .iter()
        .find(|entry| entry["repository"] == "maven-releases")
        .expect("the maven repository is reported");
    assert_eq!(
        maven["mode"], "hosted",
        "the modality comes from the origin"
    );
    assert_eq!(maven["planned"], 2);

    let incompatibilities: Vec<(String, String)> = report["incompatibilities"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| {
            (
                entry["repository"].as_str().unwrap_or_default().to_string(),
                entry["reason"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let reasons = incompatibilities
        .iter()
        .map(|(_, reason)| reason.as_str())
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(reasons.contains("the format npm is resolved"), "{reasons}");
    assert!(
        incompatibilities
            .iter()
            .any(|(repository, reason)| repository == "npm-proxy" && reason.contains("proxy")),
        "the proxy modality is an explicit difference: {reasons}"
    );

    // La superficie es administrativa de verdad: sin credencial 401, y
    // bob, que puede subir assets pero no acceder a la admin, 403.
    let anonymous = post_json(
        port,
        "/api/v1/migrations/inventory",
        "",
        &migration_body("nexus", origin),
    );
    assert_eq!(anonymous.status, 401);
    let denied = post_json(
        port,
        "/api/v1/migrations/inventory",
        &bob,
        &migration_body("nexus", origin),
    );
    assert_eq!(denied.status, 403);

    support::stop(&mut service);
}

/// AC2 + AC3: el import publica lo verificado, el origen solo fue leido
/// y el cliente nativo resuelve el path antes/despues con los mismos
/// bytes. Un segundo import no mueve ni un byte de los declarados.
#[test]
fn the_import_verifies_publishes_and_is_incrementally_idempotent() {
    let (origin, log) = spawn_origin("nexus", nexus_assets());
    let deployment = admin_deployment();
    deployment.seed_with(&["raw-hosted"]);
    deployment.seed_maven(&[("maven-releases", Value::Null)]);
    let mut service = deployment.start(&[("HODEI_SECURITY__ADMIN_API_ENABLED", "true")]);
    let port = service.port;
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    // Antes: el cliente nativo no encuentra nada.
    let jar = "com/example/demo/1.0/demo-1.0.jar";
    let before = Curl::call(
        port,
        "GET",
        &format!("/repository/maven-releases/{jar}"),
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(before.status, 404, "the path is empty before the import");

    let answer = post_json(
        port,
        "/api/v1/migrations/import",
        &token,
        &migration_body("nexus", origin),
    );
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    let report = body_of(&answer);
    assert_eq!(report["totals"]["imported"], 3, "{report}");
    assert_eq!(report["totals"]["skipped"], 1, "the npm tgz is skipped");

    let assets = nexus_assets();
    for source in assets
        .iter()
        .filter(|asset| asset.honest && asset.format != "npm")
    {
        let origin_bytes = origin_get(&source.download_path("nexus"), origin);
        let into = next("hub-get");
        let served = Curl::call(
            port,
            "GET",
            &format!("/repository/{}/{}", source.repository, source.path),
            &token,
            None,
            None,
            Some(&into),
        )
        .run();
        assert_eq!(served.status, 200, "{}", source.path);
        assert_eq!(served.body, origin_bytes, "the bytes must be identical");
        assert_eq!(
            hex(&sha2::Sha256::digest(&served.body)),
            source.sha256(),
            "the native resolution serves exactly what the origin promised"
        );
    }

    // Incremental de verdad: el segundo import reconoce por digest los
    // dos artefactos con sha256 declarado sin volver a bajarlos, y el
    // pom (solo sha1) vuelve a descargarse pero no duplica nada.
    let gets_before = content_gets(&log, "nexus");
    let again = post_json(
        port,
        "/api/v1/migrations/import",
        &token,
        &migration_body("nexus", origin),
    );
    assert_eq!(again.status, 200);
    let second = body_of(&again);
    assert_eq!(second["totals"]["already_present"], 2, "{second}");
    let moved = content_gets(&log, "nexus") - gets_before;
    assert_eq!(moved, 1, "only the sha1-only pom must be fetched again");
    assert!(
        second["imported"]
            .as_array()
            .is_none_or(|entries| entries.iter().all(|entry| entry["created"] == json!(false))),
        "a re-import must not create a second version: {second}"
    );

    assert_read_only(&log);
    support::stop(&mut service);
}

/// AC2: un artefacto que no cumple el sha256 declarado aborta la
/// publicacion **antes** de escribir el catalogo: el destino sigue
/// vacio y el origen, intacto.
#[test]
fn a_corrupted_artifact_stops_the_run_and_publishes_nothing() {
    let mut assets = nexus_assets();
    assets[0].honest = false;
    let (origin, log) = spawn_origin("nexus", assets);
    let deployment = admin_deployment();
    deployment.seed_with(&["raw-hosted"]);
    deployment.seed_maven(&[("maven-releases", Value::Null)]);
    let mut service = deployment.start(&[("HODEI_SECURITY__ADMIN_API_ENABLED", "true")]);
    let port = service.port;
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    let answer = post_json(
        port,
        "/api/v1/migrations/import",
        &token,
        &migration_body("nexus", origin),
    );
    assert_eq!(
        answer.status,
        502,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    let body = String::from_utf8_lossy(&answer.body).into_owned();
    assert!(
        body.contains("does not match the checksum"),
        "the refusal must say why: {body}"
    );

    let jar = Curl::call(
        port,
        "GET",
        "/repository/maven-releases/com/example/demo/1.0/demo-1.0.jar",
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(jar.status, 404, "nothing may have been published");
    let pom = Curl::call(
        port,
        "GET",
        "/repository/maven-releases/com/example/demo/1.0/demo-1.0.pom",
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(
        pom.status, 404,
        "the verified ones wait too: phase one failed"
    );
    assert_read_only(&log);
    support::stop(&mut service);
}

/// El otro origen, la otra forma del documento: listing + info de
/// Artifactory, con su remote declarado como diferencia.
#[test]
fn the_artifactory_surface_is_inventoried_and_imported_too() {
    let (origin, log) = spawn_origin(
        "artifactory",
        vec![asset("libs", "maven", "com/demo-1.0.jar", 7)],
    );
    let deployment = admin_deployment();
    deployment.seed_maven(&[("libs", Value::Null)]);
    deployment.seed();
    let mut service = deployment.start(&[("HODEI_SECURITY__ADMIN_API_ENABLED", "true")]);
    let port = service.port;
    let token = support::token(support::FIRST_ACCOUNT, "alice");

    let inventory = post_json(
        port,
        "/api/v1/migrations/inventory",
        &token,
        &migration_body("artifactory", origin),
    );
    assert_eq!(
        inventory.status,
        200,
        "{}",
        String::from_utf8_lossy(&inventory.body)
    );
    let report = body_of(&inventory);
    assert_eq!(report["totals"]["assets"], 1);
    assert_eq!(report["totals"]["planned"], 1);
    let joined: String = report["incompatibilities"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|entry| entry["reason"].as_str())
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        joined.contains("remote"),
        "the remote is an explicit difference: {joined}"
    );

    let answer = post_json(
        port,
        "/api/v1/migrations/import",
        &token,
        &migration_body("artifactory", origin),
    );
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    let report = body_of(&answer);
    assert_eq!(report["totals"]["imported"], 1, "{report}");

    let into = next("art-get");
    let served = Curl::call(
        port,
        "GET",
        "/repository/libs/com/demo-1.0.jar",
        &token,
        None,
        None,
        Some(&into),
    )
    .run();
    assert_eq!(served.status, 200);
    assert_eq!(served.body, origin_get("/libs/com/demo-1.0.jar", origin));
    assert_eq!(
        hex(&sha2::Sha256::digest(&served.body)),
        asset("libs", "maven", "com/demo-1.0.jar", 7).sha256(),
    );
    assert_read_only(&log);
    support::stop(&mut service);
}
