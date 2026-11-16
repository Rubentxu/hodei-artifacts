//! Las politicas de confianza en la distribucion (HSP-105 / HAR-11), contra
//! el wire.
//!
//! Cedar decide **quien** puede promover; la politica de confianza decide
//! **que software** merece el canal trusted. Un artefacto que nadie ha
//! promovido no aparece por ningun protocolo, ni por una etiqueta que se
//! movio, ni por un alias, ni desde otra cuenta.
//!
//! Las pruebas hablan con un proceso de verdad, un catalogo real, un
//! almacen de bytes real y el motor de Cedar real. El canal del
//! repositorio se fija al sembrar, como lo haria quien opera el despliegue.

mod support;

use hodei_trust_policy::{OnFailure, Requirement, RequirementKind, TrustPolicy};
use support::{Curl, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY, SECOND_ACCOUNT};

const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_ARTIFACT: &str = "application/vnd.oci.artifact.manifest.v1+json";

/// El repositorio que se queda en el canal abierto.
const OPEN_REPOSITORY: &str = "lab";

/// El digest de un contenido, sin el prefijo del algoritmo.
fn hex_of(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Una peticion cualquiera, con el cuerpo de la respuesta en un fichero.
fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<(&[u8], &str)>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let (file, content_type) = match body {
        Some((bytes, content_type)) => {
            std::fs::write(&sent, bytes).expect("the body is written");
            (Some(sent.as_path()), Some(content_type))
        }
        None => (None, None),
    };
    // `--request HEAD` espera un cuerpo que no llega: HEAD se pide con
    // `--head`.
    let answer = if method == "HEAD" {
        Curl::describe(port, path, token).run()
    } else {
        Curl::call(port, method, path, token, file, content_type, Some(&into)).run()
    };
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

/// Sube un fichero crudo.
fn put_raw(deployment: &Deployment, port: u16, token: &str, path: &str, bytes: &[u8]) -> u16 {
    call(
        deployment,
        port,
        "PUT",
        path,
        token,
        Some((bytes, "application/octet-stream")),
    )
    .0
}

/// Pide a promover y devuelve el codigo y el cuerpo.
fn promote(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": repository,
        "path": path,
        "digest": format!("sha256:{digest_hex}"),
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// Revoca por digest.
fn revoke(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": repository,
        "digest": format!("sha256:{digest_hex}"),
        "reason": "an operator withdrew it",
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions/revoke",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// Escribe un fichero de politica de confianza y devuelve su ruta.
fn trust_policy_file(deployment: &Deployment, requirements: Vec<Requirement>) -> String {
    let policy = TrustPolicy::new("default", "1", requirements);
    let path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&path, serde_json::to_vec(&policy).expect("the policy")).expect("written");
    path.to_string_lossy().into_owned()
}

/// Una politica que todo software cumple.
fn permissive(deployment: &Deployment) -> String {
    trust_policy_file(deployment, Vec::new())
}

/// Una politica que ninguna evidencia vacia cumple.
fn demanding(deployment: &Deployment) -> String {
    trust_policy_file(
        deployment,
        vec![Requirement {
            kind: RequirementKind::RiskEstablished,
            on_failure: OnFailure::Deny,
            overridable_by: None,
        }],
    )
}

/// Un despliegue con `releases` en el canal trusted y `lab` abierto.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed_trusted(&[OPEN_REPOSITORY], &[FIRST_REPOSITORY]);
    deployment
}

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn raw(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

// ---------------------------------------------------------------------------
// SEC-01: sin credencial no hay promocion.
// ---------------------------------------------------------------------------

#[test]
fn promoting_and_revoking_need_a_credential() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let digest = hex_of(b"anything");

    let (status, _) = promote(
        &deployment,
        service.port,
        "",
        FIRST_REPOSITORY,
        "a",
        &digest,
    );
    assert_eq!(status, 401);
    let (status, _) = promote(
        &deployment,
        service.port,
        "not-a-token",
        FIRST_REPOSITORY,
        "a",
        &digest,
    );
    assert_eq!(status, 401);
    let (status, _) = revoke(&deployment, service.port, "", FIRST_REPOSITORY, &digest);
    assert_eq!(status, 401);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// SEC-03 y SEC-05: lo no promovido no sale por el canal trusted.
// ---------------------------------------------------------------------------

#[test]
fn an_unpromoted_upload_is_not_served_by_the_trusted_channel() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bytes = b"quarantined build".to_vec();

    // Se puede subir: la cuarentena no impide publicar.
    let trusted = raw(FIRST_REPOSITORY, "app/build.bin");
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &trusted, &bytes),
        201
    );
    // Pero no se sirve, ni siquiera a quien lo subio.
    let (status, _) = call(&deployment, service.port, "GET", &trusted, &alice, None);
    assert_eq!(status, 404, "the quarantined upload was served");
    let described = Curl::describe(service.port, &trusted, &alice).run();
    assert_eq!(
        described.status, 404,
        "the quarantined upload was described"
    );

    // El repositorio abierto no cambia: es el canal de siempre.
    let open = raw(OPEN_REPOSITORY, "app/build.bin");
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &open, &bytes),
        201
    );
    let (status, body) = call(&deployment, service.port, "GET", &open, &alice, None);
    assert_eq!(status, 200);
    assert_eq!(body.as_bytes(), bytes.as_slice());
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC1 y AC2: quien promueve, y que se sirve despues.
// ---------------------------------------------------------------------------

#[test]
fn a_promoted_digest_is_served_and_a_revoked_one_stops_at_once() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw(FIRST_REPOSITORY, "app/release.bin");
    let bytes = b"release candidate".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/release.bin",
        &digest,
    );
    assert_eq!(status, 200, "{body}");
    let promoted: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(promoted["promoted"], true);
    assert!(
        promoted["decision_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(!body.contains(&alice), "the token is in the response");

    let (status, served) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200);
    assert_eq!(served.as_bytes(), bytes.as_slice());

    // La revocacion tiene efecto en la peticion siguiente: no hay cache.
    let (status, body) = revoke(&deployment, service.port, &alice, FIRST_REPOSITORY, &digest);
    assert_eq!(status, 200, "{body}");
    let revoked: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(revoked["revoked"], 1);
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404, "a revoked digest was served");

    // Y una nueva promocion lo vuelve a conceder, con una decision nueva.
    let (status, _) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/release.bin",
        &digest,
    );
    assert_eq!(status, 200);
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200);

    // El log no lleva el token (SEC-08).
    assert!(!deployment.the_log().contains(&alice));
    support::stop(&mut service);
}

#[test]
fn only_who_cedar_allows_can_promote_or_revoke() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bob = support::token(SECOND_ACCOUNT, "bob");
    let path = raw(FIRST_REPOSITORY, "app/secret.bin");
    let bytes = b"somebody else's build".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    // Bob es de otra cuenta: su `releases` es otro repositorio. Lo que
    // diga el cuerpo no cambia la cuenta, y la respuesta es la misma que
    // si la ruta existiera: no hay canal lateral.
    let (existing, _) = promote(
        &deployment,
        service.port,
        &bob,
        FIRST_REPOSITORY,
        "app/secret.bin",
        &digest,
    );
    let (missing, _) = promote(
        &deployment,
        service.port,
        &bob,
        FIRST_REPOSITORY,
        "app/nothing.bin",
        &digest,
    );
    assert_eq!(existing, 403);
    assert_eq!(existing, missing);

    // Un repositorio que no existe en su cuenta es un 404.
    let (status, _) = promote(
        &deployment,
        service.port,
        &bob,
        "private",
        "app/secret.bin",
        &digest,
    );
    assert_eq!(status, 404);

    let (status, _) = revoke(&deployment, service.port, &bob, FIRST_REPOSITORY, &digest);
    assert_eq!(status, 403);

    // Nada de lo anterior concedio nada.
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404);
    support::stop(&mut service);
}

#[test]
fn a_header_does_not_confer_an_account_or_a_scope() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bob = support::token(SECOND_ACCOUNT, "bob");
    let path = raw(FIRST_REPOSITORY, "app/header.bin");
    let bytes = b"header spoof".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "path": "app/header.bin",
        "digest": format!("sha256:{digest}"),
    })
    .to_string();
    let file = deployment.directory.path().join("spoof.json");
    std::fs::write(&file, body).expect("written");
    let answer = Curl::call_with_headers(
        service.port,
        "POST",
        "/api/v1/promotions",
        &bob,
        Some(&file),
        &[
            ("content-type", "application/json"),
            ("x-account", FIRST_ACCOUNT),
            ("x-hodei-account", FIRST_ACCOUNT),
            ("x-user", "alice"),
            ("x-scope", "promote"),
        ],
        None,
    )
    .run();
    assert_eq!(answer.status, 403);
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La politica de confianza decide que software.
// ---------------------------------------------------------------------------

#[test]
fn a_demanding_policy_refuses_with_reasons_and_serves_nothing() {
    let deployment = a_deployment();
    let policy = demanding(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw(FIRST_REPOSITORY, "app/risky.bin");
    let bytes = b"no risk assessment".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    // Cedar permite a alice; aun asi, la politica no ve el riesgo
    // establecido y rechaza: el permiso no es el visto bueno.
    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/risky.bin",
        &digest,
    );
    assert_eq!(status, 422, "{body}");
    let refused: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(refused["promoted"], false);
    assert!(
        refused["reasons"]
            .as_array()
            .is_some_and(|reasons| !reasons.is_empty()),
        "{body}"
    );
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404);
    support::stop(&mut service);
}

#[test]
fn without_a_trust_policy_nothing_is_promoted() {
    let deployment = a_deployment();
    // Sin `HODEI_TRUST__POLICY_FILE`: el registro esta vacio.
    let mut service = deployment.start(&[]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw(FIRST_REPOSITORY, "app/orphan.bin");
    let bytes = b"no policy".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    let (status, _) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/orphan.bin",
        &digest,
    );
    assert_eq!(status, 422, "a missing policy promoted");
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404);
    support::stop(&mut service);
}

#[test]
fn a_stale_digest_is_a_conflict_and_a_malformed_one_is_a_bad_request() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw(FIRST_REPOSITORY, "app/moving.bin");
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, b"current"),
        201
    );

    // Se pide promover un contenido que la ruta no tiene: no se promueve
    // lo que el cliente vio, ni tampoco lo que hay ahora.
    let stale = hex_of(b"what the client saw");
    let (status, _) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/moving.bin",
        &stale,
    );
    assert_eq!(status, 409);
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404);

    for bad in ["sha256:ABC", "md5:abc", "nope", ""] {
        let body = serde_json::json!({
            "repository": FIRST_REPOSITORY, "path": "app/moving.bin", "digest": bad
        })
        .to_string();
        let (status, _) = call(
            &deployment,
            service.port,
            "POST",
            "/api/v1/promotions",
            &alice,
            Some((body.as_bytes(), "application/json")),
        );
        assert_eq!(status, 400, "{bad}");
    }
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC4: tag mutable, manifiesto alternativo, referrers y alias, por OCI.
// ---------------------------------------------------------------------------

fn oci(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
}

fn push_blob(deployment: &Deployment, port: u16, token: &str, bytes: &[u8]) -> String {
    let digest = hex_of(bytes);
    let (status, body) = call(
        deployment,
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        token,
        Some((bytes, "application/octet-stream")),
    );
    assert_eq!(status, 201, "{body}");
    digest
}

fn image(config: &str, layer: &str) -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config}"), "size": 1
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer}"), "size": 1
        }]
    })
    .to_string()
    .into_bytes()
}

/// Publica una imagen bajo una etiqueta y devuelve el digest del
/// manifiesto, el de su configuracion y el de su capa.
fn push_image(
    deployment: &Deployment,
    port: u16,
    token: &str,
    tag: &str,
    seed: &str,
) -> (String, String, String) {
    let config = push_blob(
        deployment,
        port,
        token,
        format!(r#"{{"os":"linux","rootfs":{{"type":"layers"}},"seed":"{seed}"}}"#).as_bytes(),
    );
    let layer = push_blob(deployment, port, token, format!("layer-{seed}").as_bytes());
    let manifest = image(&config, &layer);
    let digest = hex_of(&manifest);
    let (status, body) = call(
        deployment,
        port,
        "PUT",
        &oci("manifests", tag),
        token,
        Some((&manifest, OCI_MANIFEST)),
    );
    assert_eq!(status, 201, "{body}");
    (digest, config, layer)
}

#[test]
fn a_moved_tag_does_not_inherit_the_decision_of_the_digest_it_left() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (good, good_config, good_layer) = push_image(&deployment, port, &alice, "latest", "good");
    // Antes de promover, ni la etiqueta ni el digest ni sus blobs salen.
    for path in [
        oci("manifests", "latest"),
        oci("manifests", &format!("sha256:{good}")),
        oci("blobs", &format!("sha256:{good_layer}")),
    ] {
        let (status, _) = call(&deployment, port, "GET", &path, &alice, None);
        assert_eq!(status, 404, "{path} was served before promotion");
    }

    // Se promueve el manifiesto: abarca su configuracion y sus capas.
    let (status, body) = promote(
        &deployment,
        port,
        &alice,
        FIRST_REPOSITORY,
        &format!("manifests/{good}"),
        &good,
    );
    assert_eq!(status, 200, "{body}");
    for path in [
        oci("manifests", "latest"),
        oci("manifests", &format!("sha256:{good}")),
        oci("blobs", &format!("sha256:{good_layer}")),
        oci("blobs", &format!("sha256:{good_config}")),
    ] {
        let (status, _) = call(&deployment, port, "GET", &path, &alice, None);
        assert_eq!(status, 200, "{path} was not served after promotion");
    }

    // La etiqueta se mueve a otra imagen: el digest de ayer sigue
    // servido, y el de hoy, que nadie ha promovido, no.
    let (evil, _, evil_layer) = push_image(&deployment, port, &alice, "latest", "evil");
    assert_ne!(good, evil);
    let (status, _) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", "latest"),
        &alice,
        None,
    );
    assert_eq!(status, 404, "the moved tag inherited the old decision");
    let (status, _) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", &format!("sha256:{evil}")),
        &alice,
        None,
    );
    assert_eq!(status, 404, "the alternative manifest was served by digest");
    let (status, _) = call(
        &deployment,
        port,
        "GET",
        &oci("blobs", &format!("sha256:{evil_layer}")),
        &alice,
        None,
    );
    assert_eq!(status, 404, "the alternative layer was served");
    let (status, _) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", &format!("sha256:{good}")),
        &alice,
        None,
    );
    assert_eq!(status, 200, "the promoted digest stopped being served");

    // El listado de etiquetas no nombra lo que no se sirve.
    let (status, tags) = call(&deployment, port, "GET", &oci("tags", "list"), &alice, None);
    assert_eq!(status, 200, "{tags}");
    assert!(
        !tags.contains("latest"),
        "a tag of an unpromoted digest is listed: {tags}"
    );
    support::stop(&mut service);
}

#[test]
fn an_attachment_is_not_listed_until_it_is_promoted() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (subject, _, _) = push_image(&deployment, port, &alice, "v1", "subject");
    let (status, _) = promote(
        &deployment,
        port,
        &alice,
        FIRST_REPOSITORY,
        &format!("manifests/{subject}"),
        &subject,
    );
    assert_eq!(status, 200);

    // Un SBOM que cuelga del manifiesto promovido, sin promover.
    let blob = push_blob(&deployment, port, &alice, b"sbom document");
    let attachment = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_ARTIFACT,
        "artifactType": "application/vnd.example.sbom.v1",
        "blobs": [{
            "mediaType": "application/vnd.example.blob",
            "digest": format!("sha256:{blob}"), "size": 1
        }],
        "subject": {
            "mediaType": OCI_MANIFEST,
            "digest": format!("sha256:{subject}"), "size": 1
        }
    })
    .to_string()
    .into_bytes();
    let attachment_digest = hex_of(&attachment);
    let (status, body) = call(
        &deployment,
        port,
        "PUT",
        &oci("manifests", &format!("sha256:{attachment_digest}")),
        &alice,
        Some((&attachment, OCI_ARTIFACT)),
    );
    assert_eq!(status, 201, "{body}");

    let referrers = oci("referrers", &format!("sha256:{subject}"));
    let (status, listed) = call(&deployment, port, "GET", &referrers, &alice, None);
    assert_eq!(status, 200, "{listed}");
    assert!(
        !listed.contains(&attachment_digest),
        "an unpromoted attachment is listed: {listed}"
    );
    let (status, _) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", &format!("sha256:{attachment_digest}")),
        &alice,
        None,
    );
    assert_eq!(status, 404);

    let (status, _) = promote(
        &deployment,
        port,
        &alice,
        FIRST_REPOSITORY,
        &format!("manifests/{attachment_digest}"),
        &attachment_digest,
    );
    assert_eq!(status, 200);
    let (_, listed) = call(&deployment, port, "GET", &referrers, &alice, None);
    assert!(listed.contains(&attachment_digest), "{listed}");
    support::stop(&mut service);
}

#[test]
fn a_virtual_alias_of_the_same_bytes_is_the_same_decision_and_different_bytes_are_not() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bytes = b"one artifact, two names".to_vec();
    let digest = hex_of(&bytes);
    let canonical = raw(FIRST_REPOSITORY, "app/one.bin");
    let alias = raw(FIRST_REPOSITORY, "mirror/alias/one.bin");
    let other = raw(FIRST_REPOSITORY, "mirror/alias/two.bin");
    for (path, content) in [
        (&canonical, &bytes[..]),
        (&alias, &bytes[..]),
        (&other, &b"different"[..]),
    ] {
        assert_eq!(
            put_raw(&deployment, service.port, &alice, path, content),
            201
        );
    }
    let (status, _) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/one.bin",
        &digest,
    );
    assert_eq!(status, 200);

    // La concesion es del contenido: el mismo contenido bajo otro nombre
    // es el mismo software. Otro contenido bajo un nombre parecido no.
    let (status, _) = call(&deployment, service.port, "GET", &alias, &alice, None);
    assert_eq!(status, 200);
    let (status, _) = call(&deployment, service.port, "GET", &other, &alice, None);
    assert_eq!(status, 404);

    // Una ruta con disfraz de traversal no abre nada.
    let (status, _) = call(
        &deployment,
        service.port,
        "GET",
        &raw(FIRST_REPOSITORY, "app/%2e%2e/mirror/alias/two.bin"),
        &alice,
        None,
    );
    assert_ne!(status, 200);
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC1: cada ruta de lectura, escritura, etiqueta, borrado y promocion exige
// una identidad verificada y un permiso.
// ---------------------------------------------------------------------------

/// Cada ruta, con su metodo. `exact` marca las que **tienen** que responder
/// `401` sin credencial; en el resto un `405` tambien es una negativa.
fn the_surface() -> Vec<(&'static str, String, bool)> {
    let repository = FIRST_REPOSITORY;
    let digest = format!("sha256:{}", hex_of(b"anything"));
    vec![
        ("GET", format!("/repository/{repository}/a.bin"), true),
        ("PUT", format!("/repository/{repository}/a.bin"), true),
        ("DELETE", format!("/repository/{repository}/a.bin"), false),
        ("GET", format!("/v2/{repository}/manifests/latest"), true),
        ("PUT", format!("/v2/{repository}/manifests/latest"), true),
        (
            "DELETE",
            format!("/v2/{repository}/manifests/latest"),
            false,
        ),
        ("GET", format!("/v2/{repository}/blobs/{digest}"), true),
        ("DELETE", format!("/v2/{repository}/blobs/{digest}"), false),
        ("POST", format!("/v2/{repository}/blobs/uploads/"), true),
        (
            "POST",
            format!("/v2/{repository}/blobs/uploads/?mount={digest}&from={OPEN_REPOSITORY}"),
            true,
        ),
        (
            "PATCH",
            format!("/v2/{repository}/blobs/uploads/00000000-0000-4000-8000-000000000000"),
            true,
        ),
        (
            "PUT",
            format!("/v2/{repository}/blobs/uploads/00000000-0000-4000-8000-000000000000"),
            true,
        ),
        (
            "GET",
            format!("/v2/{repository}/blobs/uploads/00000000-0000-4000-8000-000000000000"),
            true,
        ),
        (
            "DELETE",
            format!("/v2/{repository}/blobs/uploads/00000000-0000-4000-8000-000000000000"),
            false,
        ),
        ("HEAD", format!("/repository/{repository}/a.bin"), true),
        ("HEAD", format!("/v2/{repository}/manifests/latest"), true),
        ("HEAD", format!("/v2/{repository}/blobs/{digest}"), true),
        ("GET", format!("/v2/{repository}/tags/list"), true),
        ("GET", format!("/v2/{repository}/referrers/{digest}"), true),
        ("POST", "/api/v1/promotions".to_string(), true),
        ("POST", "/api/v1/promotions/revoke".to_string(), true),
    ]
}

#[test]
fn every_route_asks_for_an_identity_and_a_permission() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    // Una identidad verificada de la cuenta, a la que ninguna politica da
    // permiso: existe, pero no puede nada.
    let nobody = support::token(FIRST_ACCOUNT, "nobody");
    let digest = format!("sha256:{}", hex_of(b"anything"));
    let promote_body = serde_json::json!({
        "repository": FIRST_REPOSITORY, "path": "a.bin", "digest": digest
    })
    .to_string();
    let revoke_body = serde_json::json!({
        "repository": FIRST_REPOSITORY, "digest": digest, "reason": "x"
    })
    .to_string();
    let generic = br#"{}"#.to_vec();

    for (method, path, exact) in the_surface() {
        // Cada ruta de promocion lleva un cuerpo **valido**: con uno
        // invalido la respuesta seria un 400 antes de llegar a Cedar y el
        // test no mediria el permiso.
        let (body, promotion) = if path == "/api/v1/promotions" {
            (promote_body.as_bytes().to_vec(), true)
        } else if path == "/api/v1/promotions/revoke" {
            (revoke_body.as_bytes().to_vec(), true)
        } else {
            (generic.clone(), false)
        };
        let payload = (method != "GET" && method != "HEAD" && method != "DELETE")
            .then_some((&body[..], "application/json"));

        let (status, _) = call(&deployment, service.port, method, &path, "", payload);
        if exact {
            assert_eq!(status, 401, "{method} {path} without a credential");
        } else {
            assert!(
                matches!(status, 401 | 405),
                "{method} {path} without a credential answered {status}"
            );
        }

        let (status, _) = call(&deployment, service.port, method, &path, &nobody, payload);
        if promotion {
            assert_eq!(
                status, 403,
                "{method} {path} for an identity without permission"
            );
        } else {
            assert!(
                matches!(status, 400 | 401 | 403 | 404 | 405),
                "{method} {path} for an identity without permission answered {status}"
            );
            assert!(
                !(200..300).contains(&status),
                "{method} {path} let it through"
            );
        }
    }
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Los huecos que encontro la revision independiente.
// ---------------------------------------------------------------------------

#[test]
fn a_mount_does_not_carry_a_quarantined_blob_out_of_a_trusted_repository() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    // Un blob en cuarentena en el repositorio trusted.
    let blob = push_blob(&deployment, port, &alice, b"quarantined layer");
    let mount =
        format!("/v2/{OPEN_REPOSITORY}/blobs/uploads/?mount=sha256:{blob}&from={FIRST_REPOSITORY}");

    // Montarlo en el repositorio abierto no lo saca: cae a una subida
    // normal (202), exactamente como si el blob no existiera, y el
    // destino no lo sirve.
    let (status, body) = call(&deployment, port, "POST", &mount, &alice, None);
    assert_eq!(status, 202, "a quarantined blob was mounted: {body}");
    let leaked = format!("/v2/{OPEN_REPOSITORY}/blobs/sha256:{blob}");
    let (status, _) = call(&deployment, port, "GET", &leaked, &alice, None);
    assert_eq!(status, 404, "the mounted blob was served");

    // Promovido, es el mismo software y el montaje funciona.
    let (status, body) = promote(
        &deployment,
        port,
        &alice,
        FIRST_REPOSITORY,
        &format!("blobs/{blob}"),
        &blob,
    );
    assert_eq!(status, 200, "{body}");
    let (status, body) = call(&deployment, port, "POST", &mount, &alice, None);
    assert_eq!(status, 201, "{body}");
    let (status, _) = call(&deployment, port, "GET", &leaked, &alice, None);
    assert_eq!(status, 200);
    support::stop(&mut service);
}

#[test]
fn the_not_found_of_a_manifest_does_not_say_it_is_in_quarantine() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (digest, _, _) = push_image(&deployment, port, &alice, "latest", "quiet");
    let (status, by_tag) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", "latest"),
        &alice,
        None,
    );
    assert_eq!(status, 404);
    let (status, by_digest) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", &format!("sha256:{digest}")),
        &alice,
        None,
    );
    assert_eq!(status, 404);
    // Ni el digest al que apunta la etiqueta, ni una frase que distinga
    // la cuarentena de lo que no existe.
    assert!(
        !by_tag.contains(&digest),
        "the tag answer names {digest}: {by_tag}"
    );
    for body in [&by_tag, &by_digest] {
        assert!(
            !body.contains("does not reference"),
            "the answer tells quarantine from absence: {body}"
        );
    }
    let (_, missing) = call(
        &deployment,
        port,
        "GET",
        &oci("manifests", "latest-missing"),
        &alice,
        None,
    );
    assert_eq!(
        by_tag.replace("latest", "x"),
        missing.replace("latest-missing", "x"),
        "a quarantined tag answers differently from a missing one"
    );
    support::stop(&mut service);
}

#[test]
fn revoking_an_image_withdraws_what_its_promotion_granted() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let (image_digest, config, layer) = push_image(&deployment, port, &alice, "v1", "whole");
    let (status, body) = promote(
        &deployment,
        port,
        &alice,
        FIRST_REPOSITORY,
        &format!("manifests/{image_digest}"),
        &image_digest,
    );
    assert_eq!(status, 200, "{body}");
    let parts = [
        oci("manifests", &format!("sha256:{image_digest}")),
        oci("blobs", &format!("sha256:{config}")),
        oci("blobs", &format!("sha256:{layer}")),
    ];
    for path in &parts {
        let (status, _) = call(&deployment, port, "GET", path, &alice, None);
        assert_eq!(status, 200, "{path} before the revocation");
    }

    // Se revoca por el digest del manifiesto: todo lo que concedio cae.
    let (status, body) = revoke(&deployment, port, &alice, FIRST_REPOSITORY, &image_digest);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"revoked\":3"), "{body}");
    for path in &parts {
        let (status, _) = call(&deployment, port, "GET", path, &alice, None);
        assert_eq!(status, 404, "{path} survived the revocation");
    }
    support::stop(&mut service);
}

#[test]
fn a_same_account_principal_without_permission_learns_nothing() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[("HODEI_TRUST__POLICY_FILE", &policy)]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let nobody = support::token(FIRST_ACCOUNT, "nobody");
    let bytes = b"in the same account".to_vec();
    let digest = hex_of(&bytes);
    let path = raw(FIRST_REPOSITORY, "app/inside.bin");
    assert_eq!(put_raw(&deployment, port, &alice, &path, &bytes), 201);

    // Existe y no existe: la misma respuesta, y nada se concede.
    let (existing, existing_body) = promote(
        &deployment,
        port,
        &nobody,
        FIRST_REPOSITORY,
        "app/inside.bin",
        &digest,
    );
    let (missing, missing_body) = promote(
        &deployment,
        port,
        &nobody,
        FIRST_REPOSITORY,
        "app/nothing.bin",
        &digest,
    );
    assert_eq!(existing, 403);
    assert_eq!(existing, missing);
    assert_eq!(existing_body, missing_body);
    let (status, _) = call(&deployment, port, "GET", &path, &alice, None);
    assert_eq!(status, 404);
    support::stop(&mut service);
}
