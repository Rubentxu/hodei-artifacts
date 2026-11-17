//! Cargo sparse hosted de HSP-119, a nivel HTTP.
//!
//! Se prueba: la configuracion del indice, el camino sparse
//! determinista, la publicacion `PUT api/v1/crates/new`, el download
//! byte a byte, el ETag de lectura, la inmutabilidad de la version y la
//! idempotencia de una republicacion identical.
//!
//! El cliente real (`cargo`) esta fuera de alcance aqui: el protocolo
//! sparse se habla contra el servicio igual que lo hablaria el cliente.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El sha256 en hex minusculas: el mismo `cksum` que el cliente verifica.
fn the_cksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// El cuerpo de una publicacion: cabecera JSON y fichero, cada uno con su
/// longitud u32 little-endian.
fn the_publish_body(header: &Value, file: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(header).expect("the header is JSON");
    let mut body = Vec::new();
    body.extend_from_slice(&(json.len() as u32).to_le_bytes());
    body.extend_from_slice(&json);
    body.extend_from_slice(&(file.len() as u32).to_le_bytes());
    body.extend_from_slice(file);
    body
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// Una peticion al servicio.
fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
    extra: &[(&str, &str)],
) -> Answer {
    // Un `HEAD` se pide con `--head`: con `-X HEAD` el `curl` se queda
    // esperando el cuerpo que un `HEAD` nunca manda.
    if method == "HEAD" {
        return Curl::describe(port, path, token).run();
    }
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = extra.to_vec();
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

/// Publica un crate: cabecera y fichero por el camino del protocolo.
fn publish(port: u16, repository: &str, token: &str, header: &Value, file: &[u8]) -> Answer {
    call(
        port,
        "PUT",
        &format!("/cargo/{repository}/api/v1/crates/new"),
        token,
        Some(&the_publish_body(header, file)),
        &[],
    )
}

/// El texto de una respuesta.
fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

fn the_header(name: &str, version: &str) -> Value {
    json!({
        "name": name,
        "vers": version,
        "deps": [{"name": "libc", "optional": false}],
        "cksum": "",
        "features": {"std": []},
        "yanked": false,
    })
}

#[test]
fn a_crate_is_published_and_served_through_the_sparse_protocol() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // La configuracion del indice dice donde buscar la descarga y la API.
    let config = call(
        port,
        "GET",
        "/cargo/cargo-hosted/config.json",
        &token,
        None,
        &[],
    );
    assert_eq!(config.status, 200, "{}", text_of(&config));
    let config: Value = serde_json::from_slice(&config.body).expect("the config");
    assert_eq!(
        config["dl"].as_str().expect("the dl"),
        format!(
            "http://127.0.0.1:{port}/cargo/cargo-hosted/api/v1/crates/{{crate}}/{{version}}/download"
        )
    );
    assert_eq!(
        config["api"].as_str().map(str::to_string),
        Some(format!("http://127.0.0.1:{port}/cargo/cargo-hosted"))
    );
    assert_eq!(
        config["auth-required"], true,
        "cargo solo manda el token a las descargas si el registro lo declara"
    );

    let file = b"not really a gzipped crate, but bytes".to_vec();
    let digest = the_cksum(&file);

    // Publicar la version 0.1.0.
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // El indice sparse: el camino determinista de un nombre de cuatro.
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200, "{}", text_of(&index));
    let line: Value = serde_json::from_slice(&index.body).expect("the NDJSON line");
    assert_eq!(line["name"], json!("hodei-pkg"));
    assert_eq!(line["vers"], json!("0.1.0"));
    assert_eq!(line["cksum"], json!(digest));
    assert_eq!(line["yanked"], json!(false));

    // El download entrega los bytes exactos que se publicaron.
    let download = call(
        port,
        "GET",
        "/cargo/cargo-hosted/api/v1/crates/hodei-pkg/0.1.0/download",
        &token,
        None,
        &[],
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, file);

    // Publicar la 0.2.0: el indice ordena las versiones por semver.
    let second = b"a different blob for the next version".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.2.0"),
        &second,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200);
    let lines: Vec<Value> = String::from_utf8_lossy(&index.body)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["vers"], json!("0.1.0"));
    assert_eq!(lines[1]["vers"], json!("0.2.0"));

    // Republicar los mismos bytes es el eco de una publicacion
    // interrumpida: no es una reescritura.
    let again = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        &file,
    );
    assert_eq!(again.status, 200, "{}", text_of(&again));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ho/de/hodei-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(String::from_utf8_lossy(&index.body).lines().count(), 2);

    // Republicar la misma version con otros bytes es un conflicto: la
    // version publicada es inmutable.
    let conflict = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("hodei-pkg", "0.1.0"),
        b"the same version with different bytes".as_slice(),
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));
}

#[test]
fn the_index_serves_a_conditional_read_and_a_head() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let file = b"bytes behind an etag".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("etag-pkg", "1.0.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[],
    );
    let etag = index.header("etag").expect("the index has an ETag");

    // If-None-Match con el ETag vigente: un 304 sin cuerpo.
    let not_modified = call(
        port,
        "GET",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[("if-none-match", &etag)],
    );
    assert_eq!(not_modified.status, 304, "{}", text_of(&not_modified));

    // HEAD: el mismo indice, sin el cuerpo, anunciando su tamano.
    let head = call(
        port,
        "HEAD",
        "/cargo/cargo-hosted/et/ag/etag-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(head.status, 200, "{}", text_of(&head));
    assert!(head.body.is_empty());
    assert_eq!(head.size, 0);
    assert_eq!(
        head.header("content-length").as_deref(),
        Some(index.body.len().to_string().as_str())
    );
    assert_eq!(head.header("etag").as_deref(), Some(etag.as_str()));
}

#[test]
fn names_are_normalized_and_the_unknown_crate_is_not_found() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // Un nombre con mayusculas se normaliza a minusculas al publicar.
    let file = b"an uppercase crate publishes normalized".to_vec();
    let published = publish(
        port,
        "cargo-hosted",
        &token,
        &the_header("Case-Pkg", "1.0.0"),
        &file,
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));
    let index = call(
        port,
        "GET",
        "/cargo/cargo-hosted/ca/se/case-pkg",
        &token,
        None,
        &[],
    );
    assert_eq!(index.status, 200, "{}", text_of(&index));
    let line: Value = serde_json::from_slice(&index.body).expect("the line");
    assert_eq!(line["name"], json!("case-pkg"));

    // Un crate que nadie publico no esta en el indice.
    let missing = call(
        port,
        "GET",
        "/cargo/cargo-hosted/mi/ss/missing",
        &token,
        None,
        &[],
    );
    assert_eq!(missing.status, 404, "{}", text_of(&missing));

    // Una publicacion sin credencial se rechaza.
    let anonymous = publish(
        port,
        "cargo-hosted",
        "",
        &the_header("hodei-pkg", "9.9.9"),
        &file,
    );
    assert_eq!(anonymous.status, 401, "{}", text_of(&anonymous));
}

#[test]
fn a_wild_path_is_not_part_of_the_cargo_protocol() {
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for hostile in ["../se/serde", "se/../serde", "api/v1/crates/../../etc"] {
        let answer = call(
            port,
            "GET",
            &format!("/cargo/cargo-hosted/{hostile}"),
            &token,
            None,
            &[],
        );
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }
}
