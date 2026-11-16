//! PyPI hosted, proxy y group de HSP-112, a nivel HTTP.
//!
//! Lo que se prueba: la subida con el protocolo que habla `twine`, el
//! indice que negocia HTML y JSON, el nombre normalizado, el hash que se
//! comprueba contra los bytes que han llegado, el estado *yanked* de una
//! version, el grupo que junta los proyectos de sus miembros y el proxy
//! que trae el indice de su upstream sin devolver el HTML de este.
//!
//! El cliente real (`twine` y `pip`) esta en `hsp112_pypi_cli.rs`.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El separador de la subida.
const BOUNDARY: &str = "hodeipypiboundary";

/// El cuerpo de una subida de `twine`: los campos y el fichero.
///
/// Los campos son los que manda `twine`: la accion, la version del
/// protocolo, el nombre, la version y el hash.
fn the_upload(name: &str, version: &str, filename: &str, file: &[u8], sha256: &str) -> Vec<u8> {
    the_upload_with(name, version, filename, file, sha256, None)
}

/// El cuerpo de una subida, con firma si se pide.
///
/// El `twine upload --sign` manda la firma como otra parte con nombre de
/// fichero: no es una segunda distribucion.
fn the_upload_with(
    name: &str,
    version: &str,
    filename: &str,
    file: &[u8],
    sha256: &str,
    signature: Option<&str>,
) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", name),
        ("version", version),
        ("metadata_version", "2.1"),
        ("requires_python", ">=3.9"),
        ("sha256_digest", sha256),
    ] {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\n\
                 Content-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\n\
             Content-Disposition: form-data; name=\"content\"; filename=\"{filename}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n");
    if let Some(signature) = signature {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\n\
                 Content-Disposition: form-data; name=\"gpg_signature\"; filename=\"{filename}.asc\"\r\n\
                 Content-Type: application/pgp-signature\r\n\r\n{signature}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// El hash de unos bytes, en hexadecimal.
fn the_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Una peticion al servicio.
///
/// El cuerpo se deja siempre en un fichero: una respuesta sin cuerpo no
/// dice **que** sirvio el servicio, y solo dice que dijo algo.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    extra: &[(&str, &str)],
    into: Option<&Path>,
) -> Answer {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let target = match into {
        Some(file) => file.to_path_buf(),
        None => dir.join(format!("answer-{n}.bin")),
    };
    Curl::call_with_headers(port, method, path, token, None, extra, Some(&target)).run()
}

/// Una subida al servicio.
fn upload(dir: &Path, port: u16, repository: &str, token: &str, body: Vec<u8>) -> Answer {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let sent = dir.join(format!("upload-{n}.bin"));
    let into = dir.join(format!("upload-answer-{n}.bin"));
    std::fs::write(&sent, body).expect("the upload body is written");
    Curl::call(
        port,
        "POST",
        &format!("/pypi/{repository}/"),
        token,
        Some(&sent),
        Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
        Some(&into),
    )
    .run()
}

/// El JSON de una respuesta.
fn json_of(answer: &Answer) -> Value {
    serde_json::from_slice(&answer.body).unwrap_or_else(|error| {
        panic!(
            "the body is not JSON ({error}): {}",
            String::from_utf8_lossy(&answer.body)
        )
    })
}

/// El texto de una respuesta.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

#[test]
fn a_distribution_is_published_and_served_in_the_index() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", Some(">=3.9"));
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    assert_eq!(json_of(&published)["ok"], json!(true));

    // El indice en JSON: el hash, la URL y la version de Python.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[(
            "accept",
            "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.1",
        )],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(
        page.header("content-type").as_deref(),
        Some("application/vnd.pypi.simple.v1+json")
    );
    let document = json_of(&page);
    assert_eq!(document["name"], json!("labpkg"));
    assert_eq!(document["meta"]["api-version"], json!("1.0"));
    assert_eq!(document["versions"], json!(["1.0"]));
    let files = document["files"].as_array().expect("the files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], json!("labpkg-1.0-py3-none-any.whl"));
    assert_eq!(files[0]["hashes"]["sha256"], json!(hash));
    assert_eq!(files[0]["requires-python"], json!(">=3.9"));
    assert_eq!(files[0]["yanked"], json!(false));
    let url = files[0]["url"].as_str().expect("the url");
    assert!(url.contains("/pypi/pypi-hosted/files/labpkg/"), "{url}");

    // El indice en HTML: el mismo hash, en el fragmento.
    let html = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    assert_eq!(html.status, 200);
    assert_eq!(
        html.header("content-type").as_deref(),
        Some("text/html; charset=utf-8")
    );
    let page = text_of(&html);
    assert!(page.contains(&format!("#sha256={hash}")), "{page}");
    assert!(page.contains("data-requires-python=\"&gt;=3.9\""), "{page}");

    // El indice lleva su ETag, y con el no se sirve otra vez.
    let etag = html.header("etag").expect("the index has an ETag");
    let again = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html"), ("if-none-match", &etag)],
        None,
    );
    assert_eq!(again.status, 304);

    // El fichero se descarga tal cual se publico.
    let into = dir.join("downloaded.whl");
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200);
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);
}

#[test]
fn the_project_name_is_normalized_the_same_way_everywhere() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    // El nombre del metadata y el del fichero no son el mismo: los dos son
    // el mismo proyecto.
    let wheel = support::a_wheel("Lab.Pkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "Lab.Pkg",
            "1.0",
            "lab_pkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    for name in ["lab-pkg", "lab_pkg", "Lab.Pkg", "LAB--PKG"] {
        let page = call(
            dir,
            port,
            "GET",
            &format!("/pypi/pypi-hosted/simple/{name}/"),
            &token,
            &[("accept", "application/vnd.pypi.simple.v1+json")],
            None,
        );
        assert_eq!(
            page.status,
            200,
            "the project {name} is not served: {}",
            text_of(&page)
        );
        assert_eq!(json_of(&page)["name"], json!("lab-pkg"));
    }
    // Un proyecto que no existe no se inventa.
    let missing = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[],
        None,
    );
    assert_eq!(missing.status, 404);
}

#[test]
fn an_upload_whose_hash_is_not_the_one_of_its_content_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let lying = "0".repeat(64);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &lying,
        ),
    );
    assert_eq!(published.status, 400, "{}", text_of(&published));
    // Lo que no se publico no sale en el indice.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[],
        None,
    );
    assert_eq!(page.status, 404);
}

#[test]
fn an_upload_that_carries_two_distributions_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    // Dos partes de fichero: no se elige una, se dice que no.
    let mut body = the_upload(
        "labpkg",
        "1.0",
        "labpkg-1.0-py3-none-any.whl",
        &wheel,
        &hash,
    );
    let second = format!(
        "--{BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"content\"; filename=\"labpkg-2.0-py3-none-any.whl\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    );
    let end = body.len() - format!("\r\n--{BOUNDARY}--\r\n").len();
    body.truncate(end);
    body.extend_from_slice(second.as_bytes());
    body.extend_from_slice(&wheel);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    let published = upload(dir, port, "pypi-hosted", &token, body);
    assert_eq!(published.status, 400, "{}", text_of(&published));
}

#[test]
fn a_yanked_version_is_marked_and_still_downloadable() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    for version in ["1.0", "2.0"] {
        let wheel = support::a_wheel("labpkg", version, None);
        let hash = the_sha256(&wheel);
        let published = upload(
            dir,
            port,
            "pypi-hosted",
            &token,
            the_upload(
                "labpkg",
                version,
                &format!("labpkg-{version}-py3-none-any.whl"),
                &wheel,
                &hash,
            ),
        );
        assert_eq!(published.status, 200, "{}", text_of(&published));
    }

    // Retirar la version 2.0.
    let into = dir.join("yank.json");
    std::fs::write(
        &into,
        serde_json::to_vec(&json!({
            "project": "labpkg",
            "version": "2.0",
            "yanked": true,
            "reason": "the wheel was broken",
        }))
        .expect("the body"),
    )
    .expect("the body is written");
    let yank = Curl::call(
        port,
        "POST",
        "/pypi/pypi-hosted/yank",
        &token,
        Some(&into),
        Some("application/json"),
        Some(&dir.join("yank-answer.bin")),
    )
    .run();
    assert_eq!(yank.status, 200, "{}", text_of(&yank));

    // El indice dice que 2.0 esta retirada, y 1.0 no.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200);
    let document = json_of(&page);
    let files = document["files"].as_array().expect("the files");
    let yanked: Vec<&Value> = files
        .iter()
        .filter(|file| file["yanked"].is_string())
        .collect();
    assert_eq!(yanked.len(), 1, "{document}");
    assert_eq!(yanked[0]["filename"], json!("labpkg-2.0-py3-none-any.whl"));
    assert_eq!(yanked[0]["yanked"], json!("the wheel was broken"));

    // Y el HTML lo dice de la otra manera.
    let html = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    let page = text_of(&html);
    assert!(
        page.contains("data-yanked=\"the wheel was broken\""),
        "{page}"
    );

    // Lo retirado se sigue descargando: se pide exactamente esa version.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-2.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 200);
}

#[test]
fn a_group_joins_the_projects_of_its_members() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-one", Value::Null), ("pypi-two", Value::Null)]);
    deployment.seed_pypi_groups(&[("pypi-all", &["pypi-one", "pypi-two"])]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    for (repository, name) in [("pypi-one", "onepkg"), ("pypi-two", "twopkg")] {
        let wheel = support::a_wheel(name, "1.0", None);
        let hash = the_sha256(&wheel);
        let published = upload(
            dir,
            port,
            repository,
            &token,
            the_upload(
                name,
                "1.0",
                &format!("{name}-1.0-py3-none-any.whl"),
                &wheel,
                &hash,
            ),
        );
        assert_eq!(published.status, 200, "{}", text_of(&published));
    }

    // El grupo tiene los dos proyectos.
    let index = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-all/simple/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(index.status, 200);
    let document = json_of(&index);
    let names: Vec<&str> = document["projects"]
        .as_array()
        .expect("the projects")
        .iter()
        .filter_map(|project| project["name"].as_str())
        .collect();
    assert!(names.contains(&"onepkg"), "{document}");
    assert!(names.contains(&"twopkg"), "{document}");

    // Y el fichero sale del miembro que lo tiene.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-all/files/twopkg/twopkg-1.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
}

#[test]
fn a_proxy_serves_the_index_of_its_upstream_without_its_html() {
    let deployment = Deployment::new();
    let root = deployment.directory.path();

    // El upstream sirve el JSON de la Simple API cuando se le pide el JSON,
    // y su propio HTML cuando no. Lo que sale de aqui es siempre el
    // nuestro: el HTML del upstream no se devuelve, y sus valores se
    // escapan.
    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let upstream = start_upstream_on(|port| {
        let mut pages: Pages = HashMap::new();
        pages.insert(
            "/simple/labpkg".to_string(),
            (
                serde_json::to_vec(&json!({
                    "meta": {"api-version": "1.0"},
                    "name": "labpkg",
                    "files": [{
                        "filename": "labpkg-1.0-py3-none-any.whl",
                        "url": format!("http://127.0.0.1:{port}/packages/labpkg-1.0-py3-none-any.whl"),
                        "hashes": {"sha256": hash},
                        "requires-python": "\"><script>alert(1)</script>",
                    }],
                }))
                .expect("the document"),
                b"<html><body><script>alert('the upstream')</script></body></html>".to_vec(),
            ),
        );
        pages.insert(
            "/packages/labpkg-1.0-py3-none-any.whl".to_string(),
            (wheel.clone(), wheel.clone()),
        );
        pages
    });
    let port = upstream.port;

    deployment.seed_pypi_proxies(&[(
        "pypi-central",
        json!({
            "upstream": {
                "id": "central",
                "url": format!("http://127.0.0.1:{port}/simple/"),
                "scopes": ["/"],
                "max_bytes": 1048576,
            },
        }),
    )]);
    let allowed = format!("127.0.0.1:{port};private;http");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let service_port = service.port;
    let dir = deployment.directory.path();

    // El indice del proxy lo escribe el servicio, no el upstream.
    let page = call(
        dir,
        service_port,
        "GET",
        "/pypi/pypi-central/simple/labpkg/",
        &token,
        &[("accept", "text/html")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    let html = text_of(&page);
    assert!(
        !html.contains("<script>"),
        "the upstream wrote HTML: {html}"
    );
    assert!(html.contains("&lt;script&gt;"), "{html}");
    // Y el HTML es nuestro: la URL es de esta superficie.
    assert!(html.contains("/pypi/pypi-central/files/labpkg/"), "{html}");

    // El fichero sale del upstream, y su hash es el que dijo el indice.
    let into = root.join("from-proxy.whl");
    let download = call(
        dir,
        service_port,
        "GET",
        "/pypi/pypi-central/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);

    // El upstream recibio una peticion por ruta: la pagina del proyecto se
    // pidio una vez, y la descarga del fichero no la volvio a pedir.
    assert_eq!(upstream.asked_for("/simple/labpkg"), 1);
    assert_eq!(
        upstream.asked_for("/packages/labpkg-1.0-py3-none-any.whl"),
        1
    );
}

#[test]
fn a_write_without_a_credential_is_refused() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        "",
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 401);
}

#[test]
fn a_proxy_does_not_accept_writes() {
    let deployment = Deployment::new();
    deployment.seed_pypi_proxies(&[(
        "pypi-central",
        json!({
            "upstream": {
                "id": "central",
                "url": "http://127.0.0.1:1/simple/",
                "scopes": ["/"],
            },
        }),
    )]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-central",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 405);
}

/// Una distribucion de mas de dos megas se publica.
///
/// El limite por defecto del servidor web es de dos megas, y una
/// distribucion de verdad no cabe en dos. El limite que manda es el
/// configurado, y lo aplica el servicio.
#[test]
fn a_distribution_bigger_than_two_megabytes_is_published() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_big_wheel("bigpkg", "1.0", None, 3 * 1024 * 1024);
    assert!(wheel.len() > 2 * 1024 * 1024, "the wheel is not big enough");
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "bigpkg",
            "1.0",
            "bigpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // Y se descarga igual que se publico.
    let into = dir.join("big.whl");
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/bigpkg/bigpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        Some(&into),
    );
    assert_eq!(download.status, 200);
    assert_eq!(std::fs::read(&into).expect("the download"), wheel);
}

/// Un tramo del contenido se sirve como un tramo.
///
/// El `pip` de hoy pide un tramo para no bajar dos veces lo mismo, y un
/// tramo que no cabe se dice que no cabe.
#[test]
fn a_range_asks_for_a_part_of_the_distribution() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // El principio del fichero.
    let into = dir.join("part.whl");
    let part = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[("range", "bytes=0-9")],
        Some(&into),
    );
    assert_eq!(part.status, 206);
    assert_eq!(
        part.header("content-range").as_deref(),
        Some(format!("bytes 0-9/{}", wheel.len()).as_str())
    );
    assert_eq!(std::fs::read(&into).expect("the part"), wheel[..10]);

    // Un tramo que no cabe: se dice que no.
    let impossible = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[("range", "bytes=99999999-")],
        None,
    );
    assert_eq!(impossible.status, 416);
}

/// El canal de confianza no anuncia lo que no deja servir.
///
/// El canal `trusted` solo sirve los digests con concesion activa. Un
/// indice que anuncia una distribucion que no se puede descargar es un
/// indice roto.
#[test]
fn the_trusted_channel_does_not_announce_what_it_does_not_serve() {
    let deployment = Deployment::new();
    deployment.seed_pypi_trusted(&["pypi-trusted"], &[("pypi-trusted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-trusted",
        &token,
        the_upload(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // El indice no anuncia la distribucion: el canal no la admite.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-trusted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    let document = json_of(&page);
    let files = document["files"].as_array().expect("the files");
    assert!(files.is_empty(), "the index announces what it cannot serve");

    // Y la distribucion no se descarga.
    let download = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-trusted/files/labpkg/labpkg-1.0-py3-none-any.whl",
        &token,
        &[],
        None,
    );
    assert_eq!(download.status, 404);
}

/// La version se compara en su forma canonica.
///
/// El cliente puede escribir la version como le plazca y el nombre del
/// fichero la lleva canonica: `1.0-2` y `1.0.post2` son la misma version.
#[test]
fn a_version_that_is_not_canonical_is_accepted() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let sdist = support::an_sdist("labpkg", "1.0.post2", None);
    let hash = the_sha256(&sdist);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload("labpkg", "1.0-2", "labpkg-1.0.post2.tar.gz", &sdist, &hash),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // Y el indice lo dice con la version canonica.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(json_of(&page)["versions"], json!(["1.0.post2"]));
}

/// Una subida firmada no se toma por dos distribuciones.
///
/// El `twine upload --sign` manda la firma como otra parte con nombre de
/// fichero. No es una segunda distribucion: la superficie no guarda
/// firmas, y una subida firmada se publica como las demas.
#[test]
fn a_signed_upload_is_not_taken_for_two_distributions() {
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    let wheel = support::a_wheel("labpkg", "1.0", None);
    let hash = the_sha256(&wheel);
    let published = upload(
        dir,
        port,
        "pypi-hosted",
        &token,
        the_upload_with(
            "labpkg",
            "1.0",
            "labpkg-1.0-py3-none-any.whl",
            &wheel,
            &hash,
            Some("-----BEGIN PGP SIGNATURE-----\nnot-a-signature\n-----END PGP SIGNATURE-----\n"),
        ),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // Y la distribucion se sirve como las demas.
    let page = call(
        dir,
        port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        None,
    );
    assert_eq!(page.status, 200, "{}", text_of(&page));
    assert_eq!(json_of(&page)["versions"], json!(["1.0"]));
}

/// Las paginas de un upstream: la del JSON y la del HTML.
type Pages = HashMap<String, (Vec<u8>, Vec<u8>)>;

/// Un upstream de laboratorio: responde lo que se le pide, por ruta.
struct Upstream {
    /// El puerto donde escucha.
    port: u16,
    /// Cuantas peticiones ha atendido, por ruta.
    asked: Arc<std::sync::Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    /// Cuantas veces le han pedido esa ruta.
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter of the upstream")
            .get(path)
            .copied()
            .unwrap_or(0)
    }
}

/// Un upstream que se levanta en un puerto libre y sirve las paginas dadas.
///
/// Cada pagina tiene dos formas: la del JSON y la del HTML. El upstream
/// contesta con la del JSON cuando la peticion pide el JSON, y con la del
/// HTML cuando no: asi se puede ver que lo que sale de la superficie es
/// siempre el JSON republished, nunca el HTML del upstream.
fn start_upstream_on<F>(pages: F) -> Upstream
where
    F: Fn(u16) -> Pages,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<std::sync::Mutex<HashMap<String, usize>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let counter = Arc::clone(&asked);
    let served = Arc::new(std::sync::Mutex::new(pages(port)));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let served = Arc::clone(&served);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                *counter
                    .lock()
                    .expect("the counter of the upstream")
                    .entry(path.clone())
                    .or_insert(0) += 1;
                let wants_json = request.contains("application/vnd.pypi.simple.v1+json");
                let pages = served.lock().expect("the pages");
                let (json, html) = pages
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| (b"not found".to_vec(), b"not found".to_vec()));
                let body = if wants_json { json } else { html };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            });
        }
    });
    Upstream { port, asked }
}
