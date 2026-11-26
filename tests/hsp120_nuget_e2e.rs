//! El NuGet v3 de HSP-120, a nivel HTTP.
//!
//! Se prueba: el service index con direcciones absolutas, la publicacion
//! `PUT` del nupkg, el flat container ensamblado en orden NuGet, la hoja
//! de registros, la inmutabilidad de la version, la autoridad del nuspec
//! sobre la URL, la normalizacion de identidades y el rechazo de lo que
//! el protocolo no sabe hacer. El proxy y su cache sobreviven sin red,
//! byte a byte: un nupkg reescrito dejaria de casar con su `contentHash`.
//!
//! El cliente real (`dotnet`) se prueba en su propio fichero; aqui el
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

/// Una peticion al servicio con token por bearer.
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

/// El push tal y como lo hace `dotnet nuget push`: la credencial viaja
/// en `X-NuGet-ApiKey`, no en el bearer. Aqui el bearer es basura a
/// proposito: si la cabecera del protocolo no valiera, esto fallaria.
fn push_with_api_key(
    port: u16,
    repository: &str,
    api_key: &str,
    id: &str,
    version: &str,
    nupkg: &[u8],
) -> Answer {
    let sent = next("sent");
    std::fs::write(&sent, nupkg).expect("the package is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/nuget/{repository}/package/{id}/{version}"),
        "not-a-token",
        Some(&sent),
        &[
            ("content-type", "application/octet-stream"),
            ("x-nuget-apikey", api_key),
        ],
        Some(&into),
    )
    .run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// El nuspec canonico de un paquete: id y version declarados por dentro.
fn nuspec(id: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>{id}</id>
    <version>{version}</version>
    <authors>Hodei</authors>
    <description>a package for the protocol</description>
  </metadata>
</package>"#
    )
}

/// Un nupkg de prueba: el nuspec y una dll de relleno, en un zip. A
/// nivel HTTP el servidor solo lee el nuspec; el cliente real desarma
/// el resto en su propio test.
fn nupkg(id: &str, version: &str) -> Vec<u8> {
    nupkg_declaring(id, version)
}

/// Un nupkg cuya identidad interior difiere a proposito de la URL.
fn nupkg_declaring(nuspec_id: &str, nuspec_version: &str) -> Vec<u8> {
    nupkg_declaring_tagged(nuspec_id, nuspec_version, "")
}

/// El mismo, con una etiqueta que cambia los bytes sin cambiar la
/// identidad: sirve para hablar de inmutabilidad.
fn nupkg_declaring_tagged(nuspec_id: &str, nuspec_version: &str, tag: &str) -> Vec<u8> {
    use std::io::Write;
    let entry_id = nuspec_id;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entries: Vec<(String, String)> = vec![
        (
            format!("{entry_id}.nuspec"),
            nuspec(nuspec_id, nuspec_version),
        ),
        (
            format!("lib/net8.0/{entry_id}.dll"),
            "a fake dll".to_string(),
        ),
    ];
    if !tag.is_empty() {
        entries.push(("package/tags.txt".to_string(), tag.to_string()));
    }
    for (name, body) in entries {
        writer.start_file(name, options).expect("the entry opens");
        writer
            .write_all(body.as_bytes())
            .expect("the entry is written");
    }
    writer.finish().expect("the zip closes").into_inner()
}

#[test]
fn the_service_index_points_at_the_operations_the_client_needs() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for path in ["/nuget/nuget-hosted/v3/index.json", "/nuget/nuget-hosted"] {
        let answer = call(port, "GET", path, &token, None);
        assert_eq!(answer.status, 200, "{path}: {}", text_of(&answer));
        let body: Value = serde_json::from_slice(&answer.body).expect("json");
        assert_eq!(body["version"], "3.0.0");
        let types: Vec<&str> = body["resources"]
            .as_array()
            .expect("resources")
            .iter()
            .map(|resource| resource["@type"].as_str().expect("a type"))
            .collect();
        for needed in [
            "PackageBaseAddress/3.0.0",
            "RegistrationsBaseUrl/3.6.0",
            "PackagePublish/2.0.0",
        ] {
            assert!(
                types.contains(&needed),
                "{needed} is not declared: {types:?}"
            );
        }
        // Las direcciones son absolutas por el host que pregunto: el
        // cliente no adivina ni el esquema ni el puerto.
        let base = body["resources"][1]["@id"].as_str().expect("an id");
        assert!(
            base.starts_with(&format!("http://127.0.0.1:{port}/nuget/nuget-hosted/")),
            "the flat container address is not absolute on this host: {base}"
        );
    }

    // El indice de servicios es anonimo: `dotnet nuget push` lo carga
    // sin credenciales antes de usar la API key. El documento no
    // revela nada del catalogo; el contenido del feed sigue protegido.
    let anonymous = call(port, "GET", "/nuget/nuget-hosted/v3/index.json", "", None);
    assert_eq!(anonymous.status, 200, "{}", text_of(&anonymous));
    let anonymous_content = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.one/1.0.0/sample.one.nupkg",
        "",
        None,
    );
    assert_eq!(
        anonymous_content.status,
        401,
        "{}",
        text_of(&anonymous_content)
    );

    // Un repositorio de otro formato no es un feed nuget.
    let other_format = call(port, "GET", "/nuget/registry/v3/index.json", &token, None);
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// El push **tal y como lo hace el cliente real**: `PUT` a la
/// direccion de publicacion sin id ni version en la URL, con el nupkg
/// dentro de un `multipart/form-data` y la clave en `X-NuGet-ApiKey`.
fn push_as_form(
    port: u16,
    repository: &str,
    api_key: &str,
    filename: &str,
    nupkg: &[u8],
) -> Answer {
    let mut body = Vec::new();
    body.extend_from_slice(
        b"--bnd-7ca4\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\nignored\r\n",
    );
    body.extend_from_slice(
        b"--bnd-7ca4\r\nContent-Disposition: form-data; name=\"package\"; filename=\"",
    );
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\nContent-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(nupkg);
    body.extend_from_slice(b"\r\n--bnd-7ca4--\r\n");
    let sent = next("sent");
    std::fs::write(&sent, &body).expect("the form is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/nuget/{repository}/package/"),
        "not-a-token",
        Some(&sent),
        &[
            (
                "content-type",
                r#"multipart/form-data; boundary="bnd-7ca4""#,
            ),
            ("x-nuget-apikey", api_key),
        ],
        Some(&into),
    )
    .run()
}

#[test]
fn the_real_client_push_shape_is_a_multipart_form_to_the_publish_address() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-form", serde_json::Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let bytes = nupkg("Sample.Form", "1.0.0");
    let pushed = push_as_form(
        port,
        "nuget-form",
        &token,
        "sample.form.1.0.0.nupkg",
        &bytes,
    );
    assert_eq!(pushed.status, 201, "{}", text_of(&pushed));

    // El paquete vive bajo la identidad del nuspec, no de la URL: la
    // URL del formulario no prometia nada.
    let served = call(
        port,
        "GET",
        "/nuget/nuget-form/flatcontainer/sample.form/1.0.0/sample.form.nupkg",
        &token,
        None,
    );
    assert_eq!(served.status, 200, "{}", text_of(&served));
    assert_eq!(served.body, bytes, "el formulario reescribio el paquete");

    // La lista de versiones y el registro tambien lo saben.
    let versions = call(
        port,
        "GET",
        "/nuget/nuget-form/flatcontainer/sample.form/index.json",
        &token,
        None,
    );
    assert_eq!(versions.status, 200, "{}", text_of(&versions));
    let body: Value = serde_json::from_slice(&versions.body).expect("json");
    assert_eq!(body["versions"][0], "1.0.0");
}

#[test]
fn a_package_is_pushed_and_served_through_the_flat_container() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // El push habla como dotnet: la clave en X-NuGet-ApiKey. La URL va
    // en la forma que el cliente escribe (mixta), y el servidor la
    // normaliza: NuGet es insensible a mayusculas por contrato.
    let bytes = nupkg("Sample.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // La lista de versiones, en la direccion canonica.
    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["count"], 1);
    assert_eq!(body["versions"][0], "1.0.0");

    // El nupkg, byte a byte: el hash que calcule el cliente es el del
    // paquete que subio.
    let served = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(served.status, 200, "{}", text_of(&served));
    assert_eq!(served.body, bytes, "el hosted reescribe el nupkg");

    // HEAD dice el tamano sin cuerpo.
    let head = call(
        port,
        "HEAD",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(head.status, 200);
    assert!(
        head.body.is_empty(),
        "HEAD devolvio {} bytes: {:?}\ncabeceras: {}",
        head.body.len(),
        String::from_utf8_lossy(&head.body),
        head.headers
    );

    // La hoja de registros apunta al flat container con la identidad
    // normalizada.
    let leaf = call(
        port,
        "GET",
        "/nuget/nuget-hosted/registrations/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(leaf.status, 200, "{}", text_of(&leaf));
    let body: Value = serde_json::from_slice(&leaf.body).expect("json");
    assert_eq!(
        body["items"][0]["packageContent"],
        "flatcontainer/sample.lib/1.0.0/sample.lib.nupkg"
    );

    // La ruta sin normalizar no nombra nada: es gramatica, no alias.
    let mixed = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/Sample.Lib/index.json",
        &token,
        None,
    );
    assert_eq!(mixed.status, 404, "{}", text_of(&mixed));
}

#[test]
fn the_nuspec_is_the_authority_and_a_version_is_for_always() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // La URL promete una version que el nuspec no declara: el nuspec
    // manda, y la publicacion se rechaza con el motivo.
    let lying = nupkg_declaring("Sample.Lib", "2.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &lying);
    assert_eq!(answer.status, 400, "{}", text_of(&answer));
    assert!(
        text_of(&answer).contains("2.0.0"),
        "el motivo no cita al nuspec"
    );

    // Lo mismo con el id.
    let lying = nupkg_declaring("Other.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &lying);
    assert_eq!(answer.status, 400, "{}", text_of(&answer));

    // Un cuerpo que no es un nupkg utilizable tampoco.
    let answer = call(
        port,
        "PUT",
        "/nuget/nuget-hosted/package/Sample.Lib/1.0.0",
        &token,
        Some(b"this is not a zip"),
    );
    assert_eq!(answer.status, 400, "{}", text_of(&answer));

    // Publicar de verdad...
    let bytes = nupkg("Sample.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // ...el mismo paquete otra vez es el eco de una publicacion
    // interrumpida: no hay nada que reescribir.
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // ...otras bytes para la misma version son inmutabilidad: 409.
    let changed = nupkg_declaring_tagged("Sample.Lib", "1.0.0", "second");
    let answer = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Sample.Lib",
        "1.0.0",
        &changed,
    );
    assert_eq!(answer.status, 409, "{}", text_of(&answer));
    assert!(text_of(&answer).contains("immutable"));

    // La version declarada con ceros y etiqueta de compilacion se
    // normaliza: `01.0.0+b` es `1.0.0`, que ya existe. Mismo digest,
    // eco; distinto, conflicto.
    let answer = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Sample.Lib",
        "01.0.0+build.7",
        &bytes,
    );
    assert_eq!(answer.status, 201, "{}", text_of(&answer));
    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["count"], 1, "la forma cruda creo una version paralela");
}

#[test]
fn versions_sort_by_the_nuget_rules() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for (id, version) in [
        ("Sample.Lib", "1.10.0"),
        ("Sample.Lib", "1.2.0"),
        ("Sample.Lib", "1.2.0-beta.2"),
        ("Sample.Lib", "1.2.0-beta.10"),
        ("Sample.Lib", "1.2.3.4"),
    ] {
        let bytes = nupkg(id, version);
        let answer = push_with_api_key(port, "nuget-hosted", &token, id, version, &bytes);
        assert_eq!(answer.status, 201, "{version}: {}", text_of(&answer));
    }

    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    let versions: Vec<&str> = body["versions"]
        .as_array()
        .expect("versions")
        .iter()
        .map(|entry| entry.as_str().expect("a version"))
        .collect();
    assert_eq!(
        versions,
        [
            "1.2.0-beta.2",
            "1.2.0-beta.10",
            "1.2.0",
            "1.2.3.4",
            "1.10.0",
        ],
        "los previos van antes que su base, y 1.2.3.4 antes que 1.10.0"
    );

    // La hoja de registros dice la ultima, y cada version apunta a su
    // flat container con la identidad normalizada del push mixto.
    let leaf = call(
        port,
        "GET",
        "/nuget/nuget-hosted/registrations/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&leaf.body).expect("json");
    assert_eq!(body["version"], "1.10.0");
}

#[test]
fn the_protocol_says_no_to_what_it_does_not_know() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    deployment.seed_nuget_proxies(&[(
        "nuget-central",
        json!({"upstream": {"id": "lab", "url": "http://127.0.0.1:1/", "scopes": ["/"], "max_bytes": 4096}}),
    )]);
    deployment.seed_nuget(&[("member", Value::Null)]);
    deployment.seed_nuget_group("nuget-group", &["member"]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Un metodo que el protocolo no tiene para estas rutas.
    let post = call(
        port,
        "POST",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        Some(b""),
    );
    assert_eq!(post.status, 405, "{}", text_of(&post));
    let delete = call(
        port,
        "DELETE",
        "/nuget/nuget-hosted/package/Sample.Lib/1.0.0",
        &token,
        None,
    );
    assert_eq!(delete.status, 405, "{}", text_of(&delete));

    // El flat container no se publica a mano: solo el endpoint Publish.
    let put_flat = call(
        port,
        "PUT",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        Some(&nupkg("Sample.Lib", "1.0.0")),
    );
    assert_eq!(put_flat.status, 405, "{}", text_of(&put_flat));

    // Rutas que no son NuGet v3, ni con trampa de caminos.
    for hostile in [
        "/nuget/nuget-hosted/random/stuff",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/other.nupkg",
        "/nuget/nuget-hosted/../../etc/passwd",
        "/nuget/nuget-hosted/registrations/Sample.Lib/index.json",
        "/nuget/nuget-hosted/search?q=sample",
    ] {
        let answer = call(port, "GET", hostile, &token, None);
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }

    // Un proxy no acepta publicacion: no es un hosted con otro nombre.
    let push_into_proxy = push_with_api_key(
        port,
        "nuget-central",
        &token,
        "Sample.Lib",
        "1.0.0",
        &nupkg("Sample.Lib", "1.0.0"),
    );
    assert_eq!(push_into_proxy.status, 405, "{}", text_of(&push_into_proxy));

    // A nuget group does not accept client writes (PUT to publish returns 405),
    // and reading a missing package from an empty group returns 404.
    let push_into_group = push_with_api_key(
        port,
        "nuget-group",
        &token,
        "Sample.Lib",
        "1.0.0",
        &nupkg("Sample.Lib", "1.0.0"),
    );
    assert_eq!(push_into_group.status, 405, "{}", text_of(&push_into_group));
    let group = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(group.status, 404, "{}", text_of(&group));

    // Un repositorio de otro formato no es un feed, y no se sirve como
    // Raw por equivalencia: no existe para esta superficie.
    let other_format = call(port, "GET", "/nuget/registry/v3/index.json", &token, None);
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// Un upstream NuGet de verdad-en-falso: sirve lo que se le ponga por
/// ruta y cuenta cada pedido.
struct Upstream {
    port: u16,
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

fn start_upstream(id: &str, version: &str, nupkg: Vec<u8>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        format!("/flatcontainer/{id}/index.json"),
        json!({ "count": 1, "versions": [ version ] })
            .to_string()
            .into_bytes(),
    );
    pages.insert(format!("/flatcontainer/{id}/{version}/{id}.nupkg"), nupkg);
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
fn the_proxy_caches_the_package_and_survives_without_network() {
    let bytes = nupkg("Proxied.Lib", "1.0.0");
    let upstream = start_upstream("proxied.lib", "1.0.0", bytes.clone());

    let deployment = Deployment::new();
    deployment.seed_nuget_proxies(&[(
        "nuget-lab",
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

    let nupkg_path = "/nuget/nuget-lab/flatcontainer/proxied.lib/1.0.0/proxied.lib.nupkg";
    let first = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(first.status, 200, "{}", text_of(&first));
    assert_eq!(first.body, bytes, "el proxy reescribe el nupkg");
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));

    // La segunda vez no sale: la cache sirve los mismos bytes, y el
    // contentHash del registro sigue casando.
    let second = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(second.status, 200, "{}", text_of(&second));
    assert_eq!(second.body, bytes);
    assert_eq!(second.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(
        upstream.asked_for("/flatcontainer/proxied.lib/1.0.0/proxied.lib.nupkg"),
        1
    );

    // La lista de versiones, igual que el paquete: por la ruta del
    // protocolo y tal cual la dijo el upstream.
    let list = call(
        port,
        "GET",
        "/nuget/nuget-lab/flatcontainer/proxied.lib/index.json",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["versions"][0], "1.0.0");

    let before = upstream.total();
    support::stop(&mut service);

    // Sin red, lo cacheado se sigue sirviendo; lo que no se cacheo es
    // un 503 que lo dice.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;
    let offline = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(offline.status, 200, "{}", text_of(&offline));
    assert_eq!(offline.body, bytes);
    assert_eq!(offline.header("x-hodei-cache").as_deref(), Some("offline"));
    let unknown = call(
        port,
        "GET",
        "/nuget/nuget-lab/flatcontainer/proxied.lib/9.9.9/proxied.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(unknown.status, 503, "{}", text_of(&unknown));
    support::stop(&mut service);
    assert_eq!(upstream.total(), before, "offline toco la red");
}
