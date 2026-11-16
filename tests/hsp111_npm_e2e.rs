//! npm hosted de HSP-111, a nivel HTTP, sin el cliente `npm`.
//!
//! Lo que el cliente real no deja probar con comodidad: los codigos y el
//! cuerpo JSON de cada rechazo, la politica de scopes de un repositorio, que
//! un proxy y un group no aceptan escrituras, y que cada cambio de packument
//! es una instantanea nueva (otro `ETag`) que deja intactos los tarballs ya
//! publicados, y que varias publicaciones simultaneas no se pierden.

mod support;

use std::path::Path;

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Una peticion con cuerpo JSON opcional; devuelve la respuesta y su cuerpo.
fn call(
    dir: &Path,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Answer {
    // Un par de ficheros por peticion: varias pruebas llaman a la vez.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let into = dir.join(format!("answer-{n}.bin"));
    let sent = dir.join(format!("request-{n}.json"));
    let sent = body.map(|body| {
        std::fs::write(&sent, serde_json::to_vec(body).expect("the body")).expect("the body file");
        sent
    });
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &[("content-type", "application/json")],
        Some(&into),
    )
    .run()
}

fn json_of(answer: &Answer) -> Value {
    serde_json::from_slice(&answer.body).unwrap_or_else(|error| {
        panic!(
            "the body is not JSON ({error}): {}",
            String::from_utf8_lossy(&answer.body)
        )
    })
}

/// El cuerpo de un `npm publish`: una version y su tarball en base64.
fn publication(name: &str, version: &str, tarball: &[u8]) -> Value {
    let base = name.rsplit('/').next().expect("a base name");
    let file = format!("{name}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "dist-tags": { "latest": version },
        "versions": { version: { "name": name, "version": version, "_id": format!("{name}@{version}") } },
        "_attachments": { file: {
            "content_type": "application/octet-stream",
            "data": base64::engine::general_purpose::STANDARD.encode(tarball),
            "length": tarball.len(),
        } },
        "access": base,
    })
}

fn the_integrity(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

#[test]
fn a_publication_is_served_back_with_absolute_tarballs_and_its_integrity() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();
    let tarball = b"not a real tarball, the registry does not open it".to_vec();

    let created = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/@acme%2fwidget",
        &token,
        Some(&publication("@acme/widget", "1.0.0", &tarball)),
    );
    assert_eq!(
        created.status,
        201,
        "{}",
        String::from_utf8_lossy(&created.body)
    );
    assert_eq!(json_of(&created)["ok"], json!(true));

    let packument = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/@acme%2fwidget",
        &token,
        None,
    );
    assert_eq!(packument.status, 200);
    let document = json_of(&packument);
    assert_eq!(document["dist-tags"]["latest"], json!("1.0.0"));
    let version = &document["versions"]["1.0.0"];
    assert_eq!(
        version["dist"]["tarball"],
        json!(format!(
            "http://127.0.0.1:{port}/npm/npm-hosted/@acme/widget/-/widget-1.0.0.tgz"
        ))
    );
    assert_eq!(version["dist"]["integrity"], json!(the_integrity(&tarball)));

    let served = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/@acme/widget/-/widget-1.0.0.tgz",
        &token,
        None,
    );
    assert_eq!(served.status, 200);
    assert_eq!(served.body, tarball);

    // Un HEAD dice cuanto traeria el GET, y una condicion vigente es un 304.
    let etag = packument.header("etag").expect("the packument has an etag");
    let unchanged = Curl::call_with_headers(
        port,
        "GET",
        "/npm/npm-hosted/@acme%2fwidget",
        &token,
        None,
        &[("if-none-match", etag.as_str())],
        None,
    )
    .run();
    assert_eq!(unchanged.status, 304);

    // Las rutas de servicio de npm.
    assert_eq!(
        call(dir, port, "GET", "/npm/npm-hosted/-/ping", &token, None).status,
        200
    );
    let whoami = call(dir, port, "GET", "/npm/npm-hosted/-/whoami", &token, None);
    assert_eq!(whoami.status, 200);
    assert!(json_of(&whoami)["username"].is_string());
    // La auditoria de seguridad (un POST) no se ofrece: npm sigue sin ella
    // y tolera el 404.
    let audit = call(
        dir,
        port,
        "POST",
        "/npm/npm-hosted/-/npm/v1/security/advisories/bulk",
        &token,
        Some(&json!({})),
    );
    assert_eq!(audit.status, 404);
    assert!(json_of(&audit)["error"].is_string(), "a 404 without JSON");
    // Un metodo que una ruta conocida no admite: 405, tambien con JSON.
    let post = call(
        dir,
        port,
        "POST",
        "/npm/npm-hosted/widget",
        &token,
        Some(&json!({})),
    );
    assert_eq!(post.status, 405);
    assert!(json_of(&post)["error"].is_string(), "a 405 without JSON");
    support::stop(&mut service);
}

#[test]
fn errors_are_json_and_follow_the_conventions_npm_expects() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();
    let tarball = b"bytes".to_vec();

    // Sin credencial: 401 con el desafio y el motivo en JSON.
    let anonymous = call(dir, port, "GET", "/npm/npm-hosted/widget", "", None);
    assert_eq!(anonymous.status, 401);
    assert!(
        anonymous
            .header("www-authenticate")
            .is_some_and(|value| value.starts_with("Bearer"))
    );
    assert!(json_of(&anonymous)["error"].is_string());

    // Un paquete que no existe es un 404 con `error`.
    let missing = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/nothing-here",
        &token,
        None,
    );
    assert_eq!(missing.status, 404);
    assert!(json_of(&missing)["error"].is_string());

    // Un repositorio que no es npm no se sirve por esta superficie.
    let other = call(dir, port, "GET", "/npm/releases/widget", &token, None);
    assert_eq!(other.status, 404);

    // Sobre-publicar una version es un 403, no un 409: un 409 hace que
    // `libnpmpublish` intente fusionar documentos.
    let first = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget",
        &token,
        Some(&publication("widget", "1.0.0", &tarball)),
    );
    assert_eq!(first.status, 201);
    let again = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget",
        &token,
        Some(&publication("widget", "1.0.0", b"other bytes")),
    );
    assert_eq!(again.status, 403);
    assert!(json_of(&again)["error"].is_string());
    let kept = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget/-/widget-1.0.0.tgz",
        &token,
        None,
    );
    assert_eq!(kept.body, tarball, "the published bytes were rewritten");

    // Un integrity declarado que no es el del tarball se rechaza.
    let mut lying = publication("widget", "2.0.0", &tarball);
    lying["versions"]["2.0.0"]["dist"] = json!({ "integrity": "sha512-AAAA" });
    let lying = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget",
        &token,
        Some(&lying),
    );
    assert_eq!(lying.status, 400);

    // Un nombre que no es un nombre de npm.
    let invalid = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/Not%20A%20Name",
        &token,
        Some(&publication("Not A Name", "1.0.0", &tarball)),
    );
    assert!(matches!(invalid.status, 400 | 404), "{}", invalid.status);
    support::stop(&mut service);
}

#[test]
fn every_packument_change_is_a_new_snapshot_and_published_files_do_not_change() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();
    let old = b"version one".to_vec();

    let packument = |dir: &Path| call(dir, port, "GET", "/npm/npm-hosted/widget", &token, None);
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            "/npm/npm-hosted/widget",
            &token,
            Some(&publication("widget", "1.0.0", &old))
        )
        .status,
        201
    );
    let first = packument(dir);
    let first_etag = first.header("etag").expect("an etag");
    let first_document = json_of(&first);

    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            "/npm/npm-hosted/widget",
            &token,
            Some(&publication("widget", "1.1.0", b"version two"))
        )
        .status,
        201
    );
    let second = packument(dir);
    let second_etag = second.header("etag").expect("an etag");
    assert_ne!(
        first_etag, second_etag,
        "a publication did not make a new snapshot"
    );
    // La version anterior sigue diciendo lo mismo que antes.
    assert_eq!(
        json_of(&second)["versions"]["1.0.0"]["dist"],
        first_document["versions"]["1.0.0"]["dist"]
    );

    // Mover `latest` es otra instantanea, y los tarballs no se tocan.
    let tags = "/npm/npm-hosted/-/package/widget/dist-tags";
    let moved = call(
        dir,
        port,
        "PUT",
        &format!("{tags}/latest"),
        &token,
        Some(&json!("1.0.0")),
    );
    assert_eq!(
        moved.status,
        200,
        "{}",
        String::from_utf8_lossy(&moved.body)
    );
    let third = packument(dir);
    assert_ne!(second_etag, third.header("etag").expect("an etag"));
    assert_eq!(json_of(&third)["dist-tags"]["latest"], json!("1.0.0"));
    let listed = call(dir, port, "GET", tags, &token, None);
    assert_eq!(json_of(&listed)["latest"], json!("1.0.0"));
    let one = call(dir, port, "GET", &format!("{tags}/latest"), &token, None);
    assert_eq!(json_of(&one), json!("1.0.0"));
    let file = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget/-/widget-1.0.0.tgz",
        &token,
        None,
    );
    assert_eq!(file.body, old);

    // `latest` no se quita y un tag no apunta a una version que no existe.
    assert_eq!(
        call(dir, port, "DELETE", &format!("{tags}/latest"), &token, None).status,
        400
    );
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            &format!("{tags}/beta"),
            &token,
            Some(&json!("9.9.9"))
        )
        .status,
        404
    );
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            &format!("{tags}/beta"),
            &token,
            Some(&json!("1.1.0"))
        )
        .status,
        200
    );
    assert_eq!(
        call(dir, port, "DELETE", &format!("{tags}/beta"), &token, None).status,
        200
    );
    assert_eq!(
        call(dir, port, "DELETE", &format!("{tags}/beta"), &token, None).status,
        404
    );
    support::stop(&mut service);
}

#[test]
fn deprecating_is_allowed_and_unpublishing_is_refused_and_leaves_a_trace() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();
    let bytes = b"bytes".to_vec();
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            "/npm/npm-hosted/widget",
            &token,
            Some(&publication("widget", "1.0.0", &bytes))
        )
        .status,
        201
    );

    // `npm deprecate`: el packument completo con `deprecated` y sin adjuntos.
    let mut document = json_of(&call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget",
        &token,
        None,
    ));
    document["versions"]["1.0.0"]["deprecated"] = json!("use 2.0.0");
    let deprecated = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget",
        &token,
        Some(&document),
    );
    assert_eq!(
        deprecated.status,
        200,
        "{}",
        String::from_utf8_lossy(&deprecated.body)
    );
    let after = json_of(&call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget",
        &token,
        None,
    ));
    assert_eq!(after["versions"]["1.0.0"]["deprecated"], json!("use 2.0.0"));

    // Quitar la deprecacion tambien es un cambio permitido.
    let mut document = after.clone();
    document["versions"]["1.0.0"]["deprecated"] = json!("");
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            "/npm/npm-hosted/widget",
            &token,
            Some(&document)
        )
        .status,
        200
    );

    // `npm unpublish`: quitar la version del documento, o borrar por `-rev`.
    let mut removed = after.clone();
    removed["versions"] = json!({});
    let refused = call(
        dir,
        port,
        "PUT",
        "/npm/npm-hosted/widget/-rev/1-abc",
        &token,
        Some(&removed),
    );
    assert_eq!(refused.status, 403);
    assert!(
        json_of(&refused)["error"]
            .as_str()
            .is_some_and(|text| text.contains("deprecate"))
    );
    assert_eq!(
        call(
            dir,
            port,
            "PUT",
            "/npm/npm-hosted/widget",
            &token,
            Some(&removed)
        )
        .status,
        403
    );
    assert_eq!(
        call(
            dir,
            port,
            "DELETE",
            "/npm/npm-hosted/widget/-rev/1-abc",
            &token,
            None
        )
        .status,
        403
    );
    assert_eq!(
        call(
            dir,
            port,
            "DELETE",
            "/npm/npm-hosted/widget/-/widget-1.0.0.tgz/-rev/1-abc",
            &token,
            None
        )
        .status,
        403
    );
    let kept = call(
        dir,
        port,
        "GET",
        "/npm/npm-hosted/widget/-/widget-1.0.0.tgz",
        &token,
        None,
    );
    assert_eq!(kept.body, bytes);

    // El intento queda en el rastro de auditoria del servicio.
    support::stop(&mut service);
    let log = deployment.the_log();
    assert!(
        log.contains("unpublish"),
        "the refused unpublish left no trace in the service log:\n{log}"
    );
}

#[test]
fn a_repository_publishes_only_the_scopes_it_declares() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-acme", json!({ "scopes": ["@acme"] }))]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();
    let bytes = b"bytes".to_vec();
    let put = |name: &str, path: &str| {
        call(
            dir,
            port,
            "PUT",
            path,
            &token,
            Some(&publication(name, "1.0.0", &bytes)),
        )
        .status
    };
    assert_eq!(put("@acme/widget", "/npm/npm-acme/@acme%2fwidget"), 201);
    assert_eq!(put("@other/widget", "/npm/npm-acme/@other%2fwidget"), 403);
    assert_eq!(put("widget", "/npm/npm-acme/widget"), 403);
    // La politica vale tambien para los dist-tags de un paquete ajeno.
    let tags = "/npm/npm-acme/-/package/@other%2fwidget/dist-tags/beta";
    let tag = |method: &str| call(dir, port, method, tags, &token, Some(&json!("1.0.0"))).status;
    assert_eq!(tag("PUT"), 403);
    assert_eq!(tag("DELETE"), 403);
    assert_eq!(
        call(
            dir,
            port,
            "GET",
            "/npm/npm-acme/@other%2fwidget",
            &token,
            None
        )
        .status,
        404,
        "a package the scope policy refused was stored"
    );
    support::stop(&mut service);
}

#[test]
fn proxies_and_groups_do_not_accept_publications() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    deployment.seed_npm_proxies(&[(
        "npm-central",
        json!({ "upstream": { "id": "central", "url": "http://127.0.0.1:9/", "scopes": ["/"] } }),
    )]);
    deployment.seed_npm_groups(&[("npm-all", &["npm-hosted", "npm-central"])]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service =
        deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", "127.0.0.1:9;private;http")]);
    let port = service.port;
    let dir = deployment.directory.path();
    let body = publication("widget", "1.0.0", b"bytes");
    for repository in ["npm-central", "npm-all"] {
        let refused = call(
            dir,
            port,
            "PUT",
            &format!("/npm/{repository}/widget"),
            &token,
            Some(&body),
        );
        assert_eq!(refused.status, 405, "{repository} accepted a publication");
        let tag = call(
            dir,
            port,
            "PUT",
            &format!("/npm/{repository}/-/package/widget/dist-tags/beta"),
            &token,
            Some(&json!("1.0.0")),
        );
        assert_eq!(tag.status, 405, "{repository} accepted a dist-tag change");
    }
    support::stop(&mut service);
}

#[test]
fn simultaneous_publications_of_one_package_are_all_kept() {
    let deployment = Deployment::new();
    deployment.seed_npm(&[("npm-hosted", Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[]);
    let port = service.port;
    let dir = deployment.directory.path();

    // Cuatro versiones a la vez: cada una lee el packument, anade la suya y
    // escribe con precondicion; las que pierden la carrera releen y reintentan.
    let statuses: Vec<u16> = std::thread::scope(|scope| {
        let handles: Vec<_> = (1..=4)
            .map(|n| {
                let token = &token;
                scope.spawn(move || {
                    call(
                        dir,
                        port,
                        "PUT",
                        "/npm/npm-hosted/widget",
                        token,
                        Some(&publication(
                            "widget",
                            &format!("1.0.{n}"),
                            format!("bytes {n}").as_bytes(),
                        )),
                    )
                    .status
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a publisher"))
            .collect()
    });
    assert_eq!(statuses, vec![201; 4], "a concurrent publication was lost");

    let packument = call(dir, port, "GET", "/npm/npm-hosted/widget", &token, None);
    let document = json_of(&packument);
    for n in 1..=4 {
        assert!(
            document["versions"][format!("1.0.{n}")].is_object(),
            "the version 1.0.{n} is missing from the packument"
        );
    }
    support::stop(&mut service);
}
