//! The OCI Distribution push cut (HSP-101 / HAR-07), against the wire.
//!
//! The push protocol is tested the way it is used: a **real** process,
//! a real catalog, a real byte store and a real `curl`, over
//! the `/v2` endpoints of the OCI distribution.
//!
//! And the last two cases do not use `curl`: they use a **real client** --
//! `skopeo` and `podman`, the two clients the container ecosystem
//! really uses -- which push and pull without anyone telling them how. A
//! protocol that only works with the client that wrote the server
//! is not a protocol.
//!
//! The fixtures (blobs, manifest, tag) are seeded in a **separate
//! process**, like the rest of the catalog, against the deployment's Dgraph
//! namespace (the graph profile, the production one).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;

use hodei_catalog::CatalogReader;
use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// The media type of an OCI manifest.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// The graph tenant locator, which the child process needs.
const SEED_DB: &str = "HODEI_OCI_PUSH_SEED_DB";

/// The content of a sample layer.
fn a_layer(seed: u8) -> Vec<u8> {
    (0u16..2048)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// The content of the image configuration.
fn a_config() -> Vec<u8> {
    br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers"}}"#.to_vec()
}

/// The manifest that references the configuration and the layer.
fn a_manifest(config_digest: &str, layer_digest: &str) -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config_digest}"),
            "size": a_config().len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer_digest}"),
            "size": a_layer(1).len()
        }]
    })
    .to_string()
    .into_bytes()
}

/// The digest of a content, without the algorithm prefix.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// The OCI path of a resource in the first account's repository.
fn the_oci_path(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
}

/// The path that starts an upload.
fn the_upload_start() -> String {
    format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/")
}

/// A distribution request.
fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Path>,
    content_type: Option<&str>,
    into: Option<&Path>,
) -> support::Answer {
    support::Curl::call(port, method, path, token, body, content_type, into).run()
}

/// The `Location` of an upload response.
fn the_location(answer: &support::Answer) -> String {
    answer
        .header("location")
        .expect("the answer has no Location")
}

/// A deployment with the catalog seeded.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// The report case, before running it.
///
/// It is recorded twice: at the start, with what is expected, and at the end,
/// with what was measured. If the test dies in between, the
/// report is left with the first entry, which says `FAIL`.
fn the_case(id: &str, given: &str, when: &str, command: &str) -> support::Case {
    support::Case {
        id: id.to_string(),
        given: given.to_string(),
        when: when.to_string(),
        command: command.to_string(),
        outcome: "en curso".to_string(),
        passed: false,
    }
}

/// The catalog seeder (repositories per account).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The upload session: POST, PATCH, GET, PUT and DELETE.
// ---------------------------------------------------------------------------

/// The session opens with the headers the protocol states.
///
/// `POST /v2/<name>/blobs/uploads/` answers `202 Accepted` with the
/// session's `Location`, the `Range` of what is there (nothing) and the
/// session identifier.
#[test]
fn the_upload_session_opens_with_the_headers_the_protocol_says() {
    let mut case = the_case(
        "OCI-PUSH-01 (apertura)",
        "un repositorio hosted limpio y un token valido",
        "se abre una sesion de subida con POST",
        "curl --request POST /v2/releases/blobs/uploads/",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        202,
        "the upload session did not open:\n{}",
        deployment.the_log()
    );
    let location = the_location(&answer);
    assert!(
        location.starts_with(&format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/")),
        "the location of the session is not the one of the protocol: {location}"
    );
    assert_eq!(
        answer.header("range"),
        Some("0-0".to_string()),
        "the session does not say how much it holds"
    );
    let session = location.rsplit('/').next().expect("the session id");
    assert_eq!(
        answer.header("docker-upload-uuid"),
        Some(session.to_string()),
        "the session id and the header do not agree"
    );
    assert_eq!(
        answer.header("docker-distribution-api-version"),
        Some("registry/2.0".to_string()),
        "the session does not announce the version of the API"
    );

    case.outcome = "202 con Location, Range 0-0 y Docker-Upload-UUID".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The chunk is appended and the `Range` says how much there is.
///
/// The protocol's `Range` is `0-<last byte received>`, and a client
/// that reads it knows where to continue.
#[test]
fn the_chunk_is_appended_and_the_range_says_how_much_there_is() {
    let mut case = the_case(
        "OCI-PUSH-01 (trozo)",
        "una sesion de subida abierta",
        "se manda un trozo con PATCH y se pregunta por el estado",
        "curl --request PATCH <location> ; curl <location>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    assert_eq!(answer.status, 202);
    let location = the_location(&answer);

    let chunk = a_layer(1);
    let file = deployment.directory.path().join("chunk.bin");
    std::fs::write(&file, &chunk).expect("the chunk is written");
    let answer = call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(
        answer.status,
        202,
        "the chunk was not accepted:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", chunk.len() - 1)),
        "the range does not say how much the session holds"
    );
    assert_eq!(
        answer.header("location"),
        Some(location.clone()),
        "the session moved after the chunk"
    );

    case.outcome = "202 con Range 0-<bytes>, y el estado dice lo mismo".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The session state is queried with a `GET`.
///
/// The protocol says so: a session `GET` answers `204` with the
/// `Range` of what is there, and it is what a client uses to recover the
/// offset when a chunk does not fit.
#[test]
fn the_status_of_the_session_says_how_much_there_is() {
    let mut case = the_case(
        "OCI-PUSH-01 (estado)",
        "una sesion de subida con un trozo",
        "se pregunta por el estado de la sesion",
        "curl <location>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);

    let chunk = a_layer(1);
    let file = deployment.directory.path().join("chunk.bin");
    std::fs::write(&file, &chunk).expect("the chunk is written");
    call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );

    let answer = call(service.port, "GET", &location, &token, None, None, None);
    assert_eq!(
        answer.status,
        204,
        "the status of the session did not answer:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", chunk.len() - 1)),
        "the status does not say how much the session holds"
    );

    case.outcome = "204 con Location y Range 0-<bytes>".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The blob is published when the session closes, and is served exactly.
///
/// The `PUT` with the digest is the commit: what comes out are the bytes that
/// went in, and the digest that is stated is that of those bytes.
#[test]
fn the_blob_is_published_when_the_session_closes() {
    let mut case = the_case(
        "OCI-PUSH-01 (cierre)",
        "una sesion de subida con el contenido entero",
        "se cierra con el PUT que lleva el digest",
        "curl --request PUT \"<location>?digest=sha256:<digest>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = a_layer(7);
    let digest = the_digest(&content);

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );

    let answer = call(
        service.port,
        "PUT",
        &format!("{location}?digest=sha256:{digest}"),
        &token,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the blob was not published:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the answer does not name the digest of what was published"
    );
    assert_eq!(
        answer.header("location"),
        Some(the_oci_path("blobs", &format!("sha256:{digest}"))),
        "the location of the published blob is not the one of the protocol"
    );

    // And now the blob is served, and it is the bytes that went in.
    let into = deployment.directory.path().join("served.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(
        answer.status,
        200,
        "the published blob is not served:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        content,
        "the bytes that came back are not the bytes that went in"
    );

    case.outcome =
        "201 con Location y Docker-Content-Digest, y el blob se sirve exacto".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A digest that does not match publishes nothing.
///
/// A `PUT` with a digest that is not the content's is a client
/// error, and what is answered is an error: neither the blob, nor a
/// visible reference.
#[test]
fn a_digest_that_does_not_match_publishes_nothing() {
    let mut case = the_case(
        "OCI-PUSH-01 (digest incorrecto)",
        "una sesion de subida con un contenido",
        "se cierra declarando el digest de OTRO contenido",
        "curl --request PUT \"<location>?digest=sha256:<otro>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = a_layer(7);
    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );

    // The declared digest is that of OTHER content.
    let wrong = the_digest(b"other content");
    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "PUT",
        &format!("{location}?digest=sha256:{wrong}"),
        &token,
        None,
        None,
        Some(&into),
    );
    assert_eq!(
        answer.status,
        400,
        "a digest that does not match was accepted:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("DIGEST_INVALID"),
        "the error does not speak OCI: {body}"
    );

    // And the blob does not exist: neither the real content's nor the declared
    // digest's.
    for probed in [the_digest(&content), wrong.clone()] {
        let into = deployment.directory.path().join("probed.bin");
        let answer = support::Curl::download(
            service.port,
            &the_oci_path("blobs", &format!("sha256:{probed}")),
            &token,
            Some(&into),
        )
        .run();
        assert_eq!(
            answer.status, 404,
            "a blob that was not published is served"
        );
    }

    case.outcome = "400 DIGEST_INVALID, y no hay blob ni referencia visible".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The upload can be cancelled, and what is cancelled is not published.
///
/// The `DELETE` closes the session, and what had been written is left
/// unpublished: the client receives an explicit error when it asks
/// about the session again.
#[test]
fn the_upload_can_be_cancelled_and_nothing_is_published() {
    let mut case = the_case(
        "OCI-PUSH-02 (cancelacion)",
        "una sesion de subida con bytes escritos",
        "se cancela la sesion con DELETE",
        "curl --request DELETE <location>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = a_layer(7);
    let digest = the_digest(&content);
    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );

    let answer = call(service.port, "DELETE", &location, &token, None, None, None);
    assert_eq!(
        answer.status,
        204,
        "the upload could not be cancelled:\n{}",
        deployment.the_log()
    );

    // The session no longer exists, and the close says no.
    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "PUT",
        &format!("{location}?digest=sha256:{digest}"),
        &token,
        None,
        None,
        Some(&into),
    );
    assert_eq!(answer.status, 404, "a cancelled session could be closed");
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("BLOB_UPLOAD_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    // And the blob is not there.
    let into = deployment.directory.path().join("probed.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 404, "a cancelled upload published its blob");

    case.outcome = "204, y lo cancelado no se publica".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The monolithic upload publishes the blob in a single request.
///
/// The protocol defines it as a `POST` with `?digest=` and the entire
/// content, and the server checks that what arrived is what the
/// client declared.
#[test]
fn the_monolithic_upload_publishes_the_blob_in_one_request() {
    let mut case = the_case(
        "OCI-PUSH-01 (monolitica)",
        "un contenido y su digest",
        "se publica entero en una sola peticion",
        "curl --request POST \"<upload>?digest=sha256:<digest>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = a_layer(3);
    let digest = the_digest(&content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");

    let answer = call(
        service.port,
        "POST",
        &format!("{}?digest=sha256:{digest}", the_upload_start()),
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the monolithic upload did not publish:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the answer does not name the digest of what was published"
    );

    let into = deployment.directory.path().join("served.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the published blob is not served");
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        content,
        "the bytes that came back are not the bytes that went in"
    );

    case.outcome = "201 con Location y Docker-Content-Digest".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// The manifest.
// ---------------------------------------------------------------------------

/// The manifest is published with its exact bytes.
///
/// The manifest is just another blob: its digest is that of its own bytes, and
/// what is stored are the bytes the client sent, without rewriting
/// a single byte.
#[test]
fn the_manifest_is_published_with_its_exact_bytes() {
    let mut case = the_case(
        "OCI-PUSH-01 (manifiesto)",
        "un manifiesto con su config y su capa, ya publicados",
        "se publica el manifiesto con el PUT de su referencia",
        "curl --request PUT /v2/releases/manifests/v1 (Content-Type: application/vnd.oci.image.manifest.v1+json)",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The manifest's two pieces are published first as blobs.
    let config = a_config();
    let layer = a_layer(1);
    for content in [&config, &layer] {
        let digest = the_digest(content);
        let file = deployment.directory.path().join("piece.bin");
        std::fs::write(&file, content).expect("the piece is written");
        let answer = call(
            service.port,
            "POST",
            &format!("{}?digest=sha256:{digest}", the_upload_start()),
            &token,
            Some(&file),
            Some("application/octet-stream"),
            None,
        );
        assert_eq!(answer.status, 201, "the piece was not published");
    }

    // And now the manifest, which references them.
    let manifest = a_manifest(&the_digest(&config), &the_digest(&layer));
    let digest = the_digest(&manifest);
    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");

    let answer = call(
        service.port,
        "PUT",
        &the_oci_path("manifests", "v1"),
        &token,
        Some(&file),
        Some(OCI_MANIFEST),
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the manifest was not published:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the answer does not name the digest of the manifest"
    );
    assert_eq!(
        answer.header("location"),
        Some(the_oci_path("manifests", &format!("sha256:{digest}"))),
        "the location of the manifest is not the one of the protocol"
    );

    // By digest and by tag, the bytes are the same.
    for reference in [format!("sha256:{digest}"), "v1".to_string()] {
        let into = deployment.directory.path().join("served.json");
        let answer = support::Curl::with_header(
            service.port,
            &the_oci_path("manifests", &reference),
            &token,
            ("accept", OCI_MANIFEST),
            &into,
        )
        .run();
        assert_eq!(
            answer.status,
            200,
            "the manifest is not served by {reference}:\n{}",
            deployment.the_log()
        );
        assert_eq!(
            std::fs::read(&into).expect("the bytes are read"),
            manifest,
            "the manifest that came back is not the manifest that was published"
        );
        assert_eq!(
            answer.header("content-type"),
            Some(OCI_MANIFEST.to_string()),
            "the manifest is not served with its media type"
        );
    }

    case.outcome =
        "201 con Location y Docker-Content-Digest, y los bytes servidos son los publicados"
            .to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A tag **moves** when the manifest behind it changes.
///
/// A tag is a mutable reference, and the normal operation is
/// retagging: what is behind the previous tag stays
/// published with its digest, and the tag points to the new one.
#[test]
fn a_tag_moves_when_the_manifest_behind_it_changes() {
    let mut case = the_case(
        "OCI-PUSH-01 (etiqueta)",
        "una etiqueta que ya apunta a un manifiesto",
        "se publica otro manifiesto con el nombre de la etiqueta",
        "curl --request PUT /v2/releases/manifests/v1",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Two different manifests, with the same tag.
    let mut digests = Vec::new();
    for seed in [1u8, 2u8] {
        let layer = a_layer(seed);
        let config = a_config();
        for content in [&config, &layer] {
            let digest = the_digest(content);
            let file = deployment.directory.path().join("piece.bin");
            std::fs::write(&file, content).expect("the piece is written");
            let answer = call(
                service.port,
                "POST",
                &format!("{}?digest=sha256:{digest}", the_upload_start()),
                &token,
                Some(&file),
                Some("application/octet-stream"),
                None,
            );
            assert_eq!(answer.status, 201, "the piece was not published");
        }
        let manifest = a_manifest(&the_digest(&config), &the_digest(&layer));
        digests.push(the_digest(&manifest));

        let file = deployment.directory.path().join("manifest.json");
        std::fs::write(&file, &manifest).expect("the manifest is written");
        let answer = call(
            service.port,
            "PUT",
            &the_oci_path("manifests", "v1"),
            &token,
            Some(&file),
            Some(OCI_MANIFEST),
            None,
        );
        assert_eq!(
            answer.status,
            201,
            "the manifest was not published:\n{}",
            deployment.the_log()
        );
    }

    // The tag points to the second, and the first is still published by its
    // digest.
    let into = deployment.directory.path().join("tagged.json");
    let answer = support::Curl::with_header(
        service.port,
        &the_oci_path("manifests", "v1"),
        &token,
        ("accept", OCI_MANIFEST),
        &into,
    )
    .run();
    assert_eq!(answer.status, 200, "the tag does not resolve");
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{}", digests[1])),
        "the tag did not move to the manifest that was published last"
    );

    let into = deployment.directory.path().join("by-digest.json");
    let answer = support::Curl::with_header(
        service.port,
        &the_oci_path("manifests", &format!("sha256:{}", digests[0])),
        &token,
        ("accept", OCI_MANIFEST),
        &into,
    )
    .run();
    assert_eq!(
        answer.status, 200,
        "the manifest that the tag used to point at is gone"
    );

    case.outcome =
        "la etiqueta se mueve al manifiesto nuevo, y el anterior sigue publicado con su digest"
            .to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A manifest with a digest that does not match is neither published nor creates the
/// tag.
///
/// The tag is what a client sees, and a tag that points to a
/// manifest that was not published is a tag that lies.
#[test]
fn a_manifest_with_a_wrong_digest_publishes_nothing_and_creates_no_tag() {
    let mut case = the_case(
        "OCI-PUSH-01 (manifiesto con digest incorrecto)",
        "un manifiesto y un digest que no es el suyo",
        "se publica por una referencia de digest que no cuadra",
        "curl --request PUT /v2/releases/manifests/sha256:<otro>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let manifest = a_manifest(&the_digest(&a_config()), &the_digest(&a_layer(1)));
    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");

    let wrong = "0".repeat(64);
    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "PUT",
        &the_oci_path("manifests", &format!("sha256:{wrong}")),
        &token,
        Some(&file),
        Some(OCI_MANIFEST),
        Some(&into),
    );
    assert_eq!(
        answer.status,
        400,
        "a manifest with a wrong digest was accepted:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("DIGEST_INVALID"),
        "the error does not speak OCI: {body}"
    );

    // The tag does not exist: there is no manifest to resolve.
    let into = deployment.directory.path().join("probed.bin");
    let answer = support::Curl::with_header(
        service.port,
        &the_oci_path("manifests", "v1"),
        &token,
        ("accept", OCI_MANIFEST),
        &into,
    )
    .run();
    assert_eq!(
        answer.status, 404,
        "a tag was created for a manifest that was not published"
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("MANIFEST_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    case.outcome = "400 DIGEST_INVALID, y no se publica ni se crea etiqueta".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A manifest that requires a blob that is not there is not published.
///
/// The required references are the configuration and the layers: a
/// manifest that points to something the repository does not have is a
/// manifest that cannot be downloaded.
#[test]
fn a_manifest_that_references_a_missing_blob_is_refused() {
    let mut case = the_case(
        "OCI-PUSH-02 (referencias exigidas)",
        "un manifiesto que exige una capa que no esta",
        "se publica el manifiesto",
        "curl --request PUT /v2/releases/manifests/v1",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The layer is never published.
    let manifest = a_manifest(&the_digest(&a_config()), &the_digest(&a_layer(1)));
    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");

    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "PUT",
        &the_oci_path("manifests", "v1"),
        &token,
        Some(&file),
        Some(OCI_MANIFEST),
        Some(&into),
    );
    assert_eq!(
        answer.status,
        404,
        "a manifest with a missing blob was accepted:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("MANIFEST_BLOB_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    // And the tag was not created.
    let into = deployment.directory.path().join("probed.bin");
    let answer = support::Curl::with_header(
        service.port,
        &the_oci_path("manifests", "v1"),
        &token,
        ("accept", OCI_MANIFEST),
        &into,
    )
    .run();
    assert_eq!(
        answer.status, 404,
        "a tag was created for a manifest that was not published"
    );

    case.outcome = "404 MANIFEST_BLOB_UNKNOWN, y la etiqueta no se crea".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Isolation between accounts.
// ---------------------------------------------------------------------------

/// Another account's session does not exist.
///
/// The session identifier is opaque, but the account is also
/// part of its identity: a session that exists in another account is not
/// a session that can be used from this one.
#[test]
fn the_session_of_another_account_does_not_exist() {
    let mut case = the_case(
        "OCI-PUSH-02 (aislamiento)",
        "una sesion de subida de una cuenta",
        "otra cuenta pregunta por ella",
        "curl <location> (con el token de otra cuenta)",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &first,
        None,
        None,
        None,
    );
    let location = the_location(&answer);

    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "GET",
        &location,
        &second,
        None,
        None,
        Some(&into),
    );
    assert_eq!(
        answer.status,
        404,
        "the session of another account was visible:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("BLOB_UPLOAD_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    case.outcome = "404 BLOB_UPLOAD_UNKNOWN: la sesion de otra cuenta no existe".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The blob can be mounted from another repository of the same account.
///
/// Mounting does not copy bytes: the CAS is a common tree, and what is
/// written is the destination repository's reference.
#[test]
fn the_blob_can_be_mounted_from_another_repository_of_the_same_account() {
    let mut case = the_case(
        "OCI-PUSH-02 (montaje)",
        "un blob publicado en un repositorio",
        "se monta desde otro repositorio de la misma cuenta",
        "curl --request POST \"<upload>?mount=sha256:<digest>&from=<otro>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The blob is published in one repository.
    let content = a_layer(9);
    let digest = the_digest(&content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    let answer = call(
        service.port,
        "POST",
        &format!("/v2/{FIRST_ONLY_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 201, "the blob was not published");

    // And it is mounted in the other.
    let answer = call(
        service.port,
        "POST",
        &format!(
            "{}?mount=sha256:{digest}&from={FIRST_ONLY_REPOSITORY}",
            the_upload_start()
        ),
        &token,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the blob was not mounted:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the answer does not name the digest of what was mounted"
    );

    // And at the destination it is served, with the same bytes.
    let into = deployment.directory.path().join("mounted.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the mounted blob is not served");
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        content,
        "the mounted blob is not the blob that was mounted"
    );

    case.outcome = "201 con Location, y el blob se sirve en el repositorio destino".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// Mounting from another account mounts nothing.
///
/// A blob from another account is not copied to this one: mounting is not possible
/// and the request falls back to a normal upload, which is what a registry
/// that does not mount does. And the other account's `404` does not turn into a
/// `201` saying the blob exists.
#[test]
fn the_blob_cannot_be_mounted_from_another_account() {
    let mut case = the_case(
        "OCI-PUSH-02 (montaje entre cuentas)",
        "un blob de una cuenta",
        "otra cuenta intenta montarlo desde el repositorio de la primera",
        "curl --request POST \"<upload>?mount=sha256:<digest>&from=<otro>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");

    // The other account publishes its blob.
    let content = b"the bytes of another account".to_vec();
    let digest = the_digest(&content);
    let file = deployment.directory.path().join("secret.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    let answer = call(
        service.port,
        "POST",
        &format!("/v2/{FIRST_ONLY_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        &first,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 201, "the blob was not published");

    // The first account tries to mount it from the other's
    // repository. The other's repository does not exist for the first, so
    // mounting is not possible.
    let answer = call(
        service.port,
        "POST",
        &format!(
            "{}?mount=sha256:{digest}&from={FIRST_ONLY_REPOSITORY}",
            the_upload_start()
        ),
        &second,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        202,
        "the mount of another account was accepted:\n{}",
        deployment.the_log()
    );
    assert!(
        the_location(&answer).contains("/blobs/uploads/"),
        "the mount of another account was not a normal session"
    );

    // And the second account does not see the first's blob.
    let into = deployment.directory.path().join("probed.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &second,
        Some(&into),
    )
    .run();
    assert_eq!(
        answer.status,
        404,
        "the blob of another account was served:\n{}",
        deployment.the_log()
    );

    case.outcome =
        "no monta: cae a una subida normal, y el blob no se ve desde la otra cuenta".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// Two uploads at once do not get mixed.
///
/// Each session writes into its own staging, and a client that uploads two
/// things at once -- which is what a `podman` with several layers does -- cannot
/// end up with the content of one in the place of the other.
///
/// The two uploads really go at the same time, each on its own thread: a
/// test that mixes them in turns says nothing about what happens when
/// the registry has two live sessions.
#[test]
fn two_concurrent_uploads_do_not_mix() {
    let mut case = the_case(
        "OCI-PUSH-02 (concurrencia)",
        "dos contenidos y un token valido",
        "dos subidas a la vez, cada una en su hilo",
        "curl POST/PATCH/PUT x2 (en paralelo)",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let first = a_layer(31);
    let second = a_layer(41);
    let first_file = deployment.directory.path().join("first.bin");
    let second_file = deployment.directory.path().join("second.bin");
    std::fs::write(&first_file, &first).expect("the first blob is written");
    std::fs::write(&second_file, &second).expect("the second blob is written");
    let first_digest = the_digest(&first);
    let second_digest = the_digest(&second);

    // The two sessions are opened, written and closed at once.
    let (first_done, second_done) = std::thread::scope(|scope| {
        let first = scope.spawn(|| a_full_upload(service.port, &token, &first_file, &first_digest));
        let second =
            scope.spawn(|| a_full_upload(service.port, &token, &second_file, &second_digest));
        (
            first.join().expect("the first upload"),
            second.join().expect("the second upload"),
        )
    });
    assert!(
        first_done && second_done,
        "one of the concurrent uploads did not finish:\n{}",
        deployment.the_log()
    );

    // And each blob is served with its content: that of one has not ended up
    // in the place of the other.
    for (content, digest) in [(&first, &first_digest), (&second, &second_digest)] {
        let into = deployment.directory.path().join("served.bin");
        let answer = support::Curl::download(
            service.port,
            &the_oci_path("blobs", &format!("sha256:{digest}")),
            &token,
            Some(&into),
        )
        .run();
        assert_eq!(
            answer.status,
            200,
            "the published blob is not served:\n{}",
            deployment.the_log()
        );
        assert_eq!(
            std::fs::read(&into).expect("the bytes are read"),
            *content,
            "a concurrent upload published the bytes of the other"
        );
    }

    case.outcome = "las dos se publican, y cada blob tiene su contenido".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A complete upload: open, write and close.
///
/// It lives outside the test because two uploads go at once, each on its
/// own thread, and the registry has to know how to carry two live sessions.
fn a_full_upload(port: u16, token: &str, file: &Path, digest: &str) -> bool {
    let answer = call(port, "POST", &the_upload_start(), token, None, None, None);
    if answer.status != 202 {
        return false;
    }
    let location = the_location(&answer);
    let answer = call(
        port,
        "PATCH",
        &location,
        token,
        Some(file),
        Some("application/octet-stream"),
        None,
    );
    if answer.status != 202 {
        return false;
    }
    let answer = call(
        port,
        "PUT",
        &format!("{location}?digest=sha256:{digest}"),
        token,
        None,
        None,
        None,
    );
    answer.status == 201
}

// ---------------------------------------------------------------------------
// Chunks that do not fit, restart and quarantine.
// ---------------------------------------------------------------------------

/// A chunk that does not fit the offset receives a 416.
///
/// The protocol says chunks go in order, and that a client whose
/// chunk does not fit recovers the offset with a `GET`.
#[test]
fn a_chunk_that_does_not_fit_the_offset_is_refused() {
    let mut case = the_case(
        "OCI-PUSH-02 (trozo descuadrado)",
        "una sesion de subida con un trozo",
        "se manda un trozo que vuelve a empezar en 0",
        "curl --request PATCH <location> (Content-Range: 0-<bytes>)",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);

    let chunk = a_layer(1);
    let file = deployment.directory.path().join("chunk.bin");
    std::fs::write(&file, &chunk).expect("the chunk is written");

    // The first chunk goes in whole.
    let answer = call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 202, "the first chunk was refused");

    // A second chunk that starts again at 0 does not fit.
    let answer = support::Curl::call_with_headers(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        &[
            ("content-type", "application/octet-stream"),
            ("content-range", format!("0-{}", chunk.len() - 1).as_str()),
        ],
        None,
    )
    .run();
    assert_eq!(
        answer.status,
        416,
        "a chunk that does not fit the offset was accepted:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", chunk.len() - 1)),
        "the error does not say where the upload is"
    );

    case.outcome = "416, y la respuesta dice por donde sigue la subida".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// A chunk that is retried is not duplicated.
///
/// A client whose `PATCH` is lost sends it again, and the registry cannot
/// duplicate the bytes: the second chunk with the range that already passed
/// receives the `416` that says where to continue, the client recovers the
/// offset with a `GET` and continues.
///
/// And what is published has the content **once**: a retry that
/// duplicated the bytes would publish a blob that is not the one the client
/// declared, with a digest the client had not sent.
#[test]
fn a_chunk_that_is_retried_does_not_duplicate_the_bytes() {
    let mut case = the_case(
        "OCI-PUSH-02 (reintento)",
        "una sesion de subida con un trozo",
        "se reintenta el trozo y se cierra la sesion",
        "curl --request PATCH <location> ; curl --request PUT \"<location>?digest=sha256:<digest>\"",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);

    let chunk = a_layer(3);
    let file = deployment.directory.path().join("chunk.bin");
    std::fs::write(&file, &chunk).expect("the chunk is written");

    // The first chunk goes in whole.
    let answer = call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 202, "the first chunk was refused");

    // The client does not know whether it arrived, and sends it again.
    let answer = support::Curl::call_with_headers(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        &[
            ("content-type", "application/octet-stream"),
            ("content-range", format!("0-{}", chunk.len() - 1).as_str()),
        ],
        None,
    )
    .run();
    assert_eq!(
        answer.status,
        416,
        "a retried chunk was accepted:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", chunk.len() - 1)),
        "the error does not say where the upload is"
    );

    // The client asks how far the upload has got, and what it says is the
    // truth: the chunk is written once.
    let answer = call(service.port, "GET", &location, &token, None, None, None);
    assert_eq!(answer.status, 204, "the session does not answer");
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", chunk.len() - 1)),
        "the status does not say how much there is"
    );

    // And it closes: the blob that comes out has the content once.
    let digest = the_digest(&chunk);
    let answer = call(
        service.port,
        "PUT",
        &format!("{location}?digest=sha256:{digest}"),
        &token,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the blob was not published:\n{}",
        deployment.the_log()
    );

    let into = deployment.directory.path().join("served.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the published blob is not served");
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        chunk,
        "the retried chunk was written twice"
    );

    case.outcome = "416 al reintentar, y el blob publicado tiene el contenido una vez".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// Opens an upload, sends its first chunk, and kills the service.
///
/// Returns the `Location` of the session, the token and the first chunk.
/// The service ran with `environment`, so a case can give the session the
/// life it needs.
fn an_upload_cut_by_a_crash(
    deployment: &Deployment,
    environment: &[(&str, &str)],
) -> (String, String, Vec<u8>) {
    let mut service = deployment.start(environment);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let answer = call(
        service.port,
        "POST",
        &the_upload_start(),
        &token,
        None,
        None,
        None,
    );
    let location = the_location(&answer);

    let chunk = a_layer(1);
    let file = deployment.directory.path().join("chunk.bin");
    std::fs::write(&file, &chunk).expect("the chunk is written");
    let answer = call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 202, "the first chunk was refused");
    assert!(
        !deployment.staging_entries().is_empty(),
        "the upload did not leave a staging behind"
    );

    // The process dies at once.
    support::kill(&mut service);
    (location, token, chunk)
}

/// Finishes the upload on the restarted service: asks where it is, sends
/// the second chunk, closes it and reads the blob back.
fn the_client_finishes_the_upload(
    deployment: &Deployment,
    service: &support::Service,
    location: &str,
    token: &str,
    first: &[u8],
) {
    // The status says where the upload is, and it is the truth.
    let answer = call(service.port, "GET", location, token, None, None, None);
    assert_eq!(
        answer.status,
        204,
        "the upload did not survive the restart:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("range"),
        Some(format!("0-{}", first.len() - 1)),
        "the status after the restart does not say how much there is"
    );

    // The client sends the rest where the status said.
    let second = a_layer(2);
    let file = deployment.directory.path().join("second.bin");
    std::fs::write(&file, &second).expect("the second chunk is written");
    let answer = call(
        service.port,
        "PATCH",
        location,
        token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(
        answer.status,
        202,
        "the chunk after the restart was refused:\n{}",
        deployment.the_log()
    );

    let whole: Vec<u8> = first.iter().chain(second.iter()).copied().collect();
    let digest = the_digest(&whole);
    let answer = call(
        service.port,
        "PUT",
        &format!("{location}?digest=sha256:{digest}"),
        token,
        None,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the upload that crossed the restart was not published:\n{}",
        deployment.the_log()
    );

    // And the blob is the two chunks, once each.
    let into = deployment.directory.path().join("served.bin");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("blobs", &format!("sha256:{digest}")),
        token,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the published blob is not served");
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        whole,
        "the blob that crossed the restart is not the two chunks"
    );
}

/// A restart does not lose the upload: the session is durable (HSP-201).
///
/// In the graph profile the session and its offset live in the journal, and the
/// bytes in the CAS staging: a process that dies in the middle of an upload
/// does not take the session with it. The client asks how far it got, continues from there
/// and the upload is published with the digest of what it sent, and startup does not
/// touch the staging of a session that is still alive.
#[test]
fn a_restart_keeps_the_upload_and_it_completes() {
    let mut case = the_case(
        "OCI-PUSH-02 (reinicio)",
        "una sesion de subida a medias",
        "el proceso se muere de golpe, vuelve, y el cliente sigue",
        "curl --request POST <upload> ; kill -9 <servicio> ; <servicio> ; curl --request PATCH <upload> ; curl --request PUT <upload>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let (location, token, first) = an_upload_cut_by_a_crash(&deployment, &[]);
    let staging = deployment.staging_entries();

    let mut service = deployment.start(&[]);
    assert_eq!(
        deployment.staging_entries(),
        staging,
        "the boot touched the staging of a live session"
    );
    the_client_finishes_the_upload(&deployment, &service, &location, &token, &first);

    case.outcome =
        "el offset sobrevive al reinicio, la subida se completa y el blob es exacto".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// Startup cleanup does not remove the staging of a live session.
///
/// The journal holds the session open for `oci_upload_ttl_secs`, and the
/// staging is where the resumption writes: the cleanup age is never
/// shorter than that TTL. Here the TTL is greater than `staging_max_age_secs`
/// (the harness sets it to an hour) and the staging is two hours old: it is still standing,
/// and the upload completes.
#[test]
fn the_boot_cleanup_keeps_the_staging_of_a_live_session() {
    let mut case = the_case(
        "OCI-PUSH-02 (limpieza del arranque)",
        "una sesion viva cuyo staging es mas viejo que staging_max_age_secs",
        "el proceso se muere y vuelve, y el cliente sigue",
        "curl --request POST <upload> ; kill -9 <servicio> ; <servicio> ; curl --request PATCH <upload> ; curl --request PUT <upload>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let long_life = [("HODEI_SECURITY__OCI_UPLOAD_TTL_SECS", "86400")];
    let (location, token, first) = an_upload_cut_by_a_crash(&deployment, &long_life);
    deployment.age_the_staging(std::time::Duration::from_secs(7200));

    let mut service = deployment.start(&long_life);
    assert_eq!(
        deployment.staging_entries().len(),
        1,
        "the boot removed the staging of a session that is still live"
    );
    the_client_finishes_the_upload(&deployment, &service, &location, &token, &first);

    case.outcome =
        "el staging de la sesion viva sobrevive al arranque y la subida se completa".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// An upload that expired does not exist after the restart, with the explicit error.
///
/// The TTL is the protocol's promise: once past it, the client receives
/// `BLOB_UPLOAD_UNKNOWN`, not a `204` with a `Range` that then cannot be
/// written. And the staging it left, already old, is cleaned at startup.
#[test]
fn an_upload_past_its_ttl_is_unknown_after_a_restart() {
    let mut case = the_case(
        "OCI-PUSH-02 (caducidad)",
        "una sesion de subida a medias cuyo TTL vence mientras el proceso esta caido",
        "el proceso vuelve y el cliente pregunta por la sesion",
        "curl --request POST <upload> ; kill -9 <servicio> ; <servicio> ; curl <upload>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let short_life = [("HODEI_SECURITY__OCI_UPLOAD_TTL_SECS", "2")];
    let (location, token, _first) = an_upload_cut_by_a_crash(&deployment, &short_life);

    // The session expires while the process is down, and the staging it
    // left grows old: the cleanup of the boot is the recovery operation.
    std::thread::sleep(std::time::Duration::from_secs(3));
    deployment.age_the_staging(std::time::Duration::from_secs(7200));

    let mut service = deployment.start(&short_life);
    let into = deployment.directory.path().join("error.bin");
    let answer = call(
        service.port,
        "GET",
        &location,
        &token,
        None,
        None,
        Some(&into),
    );
    assert_eq!(
        answer.status,
        404,
        "an expired upload is still announced:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("BLOB_UPLOAD_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    // The chunk of a client that did not read the status is refused the
    // same way, not with a retryable error.
    let file = deployment.directory.path().join("late.bin");
    std::fs::write(&file, a_layer(2)).expect("the chunk is written");
    let answer = call(
        service.port,
        "PATCH",
        &location,
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 404, "a chunk of an expired upload was taken");

    assert!(
        deployment.staging_entries().is_empty(),
        "the staging of the interrupted upload was left behind: {:?}",
        deployment.staging_entries()
    );

    case.outcome =
        "BLOB_UPLOAD_UNKNOWN pasado el TTL, y el staging envejecido se limpia al arrancar"
            .to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The admitted artifact is left in quarantine.
///
/// Admission writes the channel into the catalog reference, and
/// writes it as quarantine: a newly admitted artifact has not been
/// promoted by anyone, and push has no path that promotes it.
#[test]
fn the_admitted_artifact_is_in_quarantine() {
    let mut case = the_case(
        "OCI-PUSH-02 (cuarentena)",
        "un blob admitido",
        "se publica y se mira el canal en que queda (en otro proceso)",
        "curl --request POST \"<upload>?digest=sha256:<digest>\" ; <inspector del canal>",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = a_layer(11);
    let digest = the_digest(&content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    let answer = call(
        service.port,
        "POST",
        &format!("{}?digest=sha256:{digest}", the_upload_start()),
        &token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(answer.status, 201, "the blob was not published");

    // The service has to release the database so that another process can
    // read it.
    case.outcome = "el artefacto admitido queda en cuarentena".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);

    let status = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "the_admitted_artifact_can_be_inspected",
            "--ignored",
            "--nocapture",
        ])
        .env(SEED_DB, &deployment.db)
        .status()
        .expect("the inspector runs");
    assert!(
        status.success(),
        "the admitted artifact is not in quarantine:\n{}",
        deployment.the_log()
    );
}

/// The channel inspector, in a separate process.
///
/// The disk database is held by the service while it is alive, so the
/// one looking has to be another process.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo de los tests de push OCI"]
async fn the_admitted_artifact_can_be_inspected() {
    let Some(db_path) = std::env::var(SEED_DB).ok() else {
        return;
    };

    // The real Dgraph catalog store of the deployment's own namespace.
    let tenant = support::graph_profile::GraphTenant::from_locator(&db_path)
        .expect("the locator names a graph tenant");
    let catalog = tenant.catalog_store().await;

    // The admitted blob's asset, and the channel it is in.
    let reference = catalog
        .reference(
            FIRST_ACCOUNT,
            FIRST_REPOSITORY,
            &format!("blobs/{}", the_digest(&a_layer(11))),
        )
        .await
        .expect("the reference of the admitted blob");
    assert_eq!(
        reference.trust_state,
        hodei_catalog::TrustState::Quarantined,
        "the admitted artifact is not in quarantine"
    );
}

// ---------------------------------------------------------------------------
// The Docker clients' door.
// ---------------------------------------------------------------------------

/// The ping says how to authenticate.
///
/// A Docker client arrives, asks and reads the `WWW-Authenticate`: without
/// it, it does not know where the token endpoint is and there is nowhere to
/// start.
#[test]
fn the_ping_says_how_to_authenticate() {
    let mut case = the_case(
        "OCI-TOKEN-01 (reto)",
        "un cliente de Docker sin credencial",
        "pregunta por la version de la API de distribucion",
        "curl /v2/",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);

    let answer = call(service.port, "GET", "/v2/", "", None, None, None);
    assert_eq!(
        answer.status,
        401,
        "the ping without credentials did not answer:\n{}",
        deployment.the_log()
    );
    let challenge = answer
        .header("www-authenticate")
        .expect("the ping does not say how to authenticate");
    assert!(
        challenge.contains("Bearer"),
        "the challenge does not name the scheme: {challenge}"
    );
    assert!(
        challenge.contains(&format!(
            "realm=\"http://127.0.0.1:{}/token\"",
            service.port
        )),
        "the challenge does not name the realm: {challenge}"
    );
    assert!(
        challenge.contains("service="),
        "the challenge does not name the service: {challenge}"
    );

    case.outcome =
        "401 con WWW-Authenticate: Bearer realm=\"http://<host>/token\",service=\"hodei\""
            .to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The token endpoint returns the verified credential, and only that.
///
/// Without a credential there is no token: what is answered is the same challenge.
/// And a credential that does not verify does not buy a token.
#[test]
fn the_token_endpoint_returns_the_verified_credential_and_nothing_else() {
    let mut case = the_case(
        "OCI-TOKEN-02 (token)",
        "unas credenciales verificadas y otra que no verifica",
        "se pide un token en el endpoint",
        "curl /token?service=hodei&scope=repository:releases:pull,push",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Without a credential: the same challenge.
    let answer = call(service.port, "GET", "/token", "", None, None, None);
    assert_eq!(answer.status, 401, "the token endpoint gave a token away");

    // With a credential that does not verify: neither.
    let forged = "Basic YWxpY2U6bm90LWEtdG9rZW4=".to_string();
    let answer = support::Curl::call_with_headers(
        service.port,
        "GET",
        "/token",
        "",
        None,
        &[("authorization", forged.as_str())],
        None,
    )
    .run();
    assert_eq!(
        answer.status, 401,
        "a credential that does not verify got a token"
    );

    // With the good credential: the token, and it is the same.
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("alice:{token}"))
    );
    let into = deployment.directory.path().join("token.json");
    let answer = support::Curl::call_with_headers(
        service.port,
        "GET",
        "/token?service=hodei&scope=repository:releases:pull,push",
        "",
        None,
        &[("authorization", basic.as_str())],
        Some(&into),
    )
    .run();
    assert_eq!(
        answer.status,
        200,
        "the token endpoint did not answer:\n{}",
        deployment.the_log()
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    let granted: serde_json::Value = serde_json::from_str(&body).expect("the token is JSON");
    assert_eq!(
        granted.get("token").and_then(|value| value.as_str()),
        Some(token.as_str()),
        "the token that came back is not the credential that was verified"
    );

    // And the token works for the surface.
    let answer = call(service.port, "GET", "/v2/", &token, None, None, None);
    assert_eq!(answer.status, 200, "the token does not open the surface");

    case.outcome = "200 con la credencial verificada; y 401 con la que no verifica".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// The real clients.
// ---------------------------------------------------------------------------

/// An OCI layout with a single-platform image.
///
/// The layout is what skopeo and podman read: an `index.json` that
/// names a manifest, and the blobs in `blobs/sha256/<digest>`.
fn an_oci_layout(directory: &Path, layer: &[u8]) -> (String, String, String) {
    let blobs = directory.join("blobs").join("sha256");
    std::fs::create_dir_all(&blobs).expect("the blobs directory");

    let config = a_config();
    let config_digest = the_digest(&config);
    std::fs::write(blobs.join(&config_digest), &config).expect("the config is written");

    let layer_digest = the_digest(layer);
    std::fs::write(blobs.join(&layer_digest), layer).expect("the layer is written");

    let manifest = a_manifest(&config_digest, &layer_digest);
    let manifest_digest = the_digest(&manifest);
    std::fs::write(blobs.join(&manifest_digest), &manifest).expect("the manifest is written");

    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": OCI_MANIFEST,
            "digest": format!("sha256:{manifest_digest}"),
            "size": manifest.len(),
            "annotations": { "org.opencontainers.image.ref.name": "demo" }
        }]
    })
    .to_string();
    std::fs::write(directory.join("index.json"), index).expect("the index is written");
    std::fs::write(
        directory.join("oci-layout"),
        r#"{"imageLayoutVersion":"1.0.0"}"#,
    )
    .expect("the layout is written");

    (config_digest, layer_digest, manifest_digest)
}

/// The manifest the registry serves for a tag: the bytes and the
/// digest.
///
/// What is compared is what is there: the bytes are downloaded from the registry and
/// the digest is read from its header. A client that compares them with what it
/// uploaded is not comparing with its memory, but with what the registry
/// has.
///
/// The `Accept` is the "whatever there is works for me" one: the manifest declares its
/// own type, and what is checked is that the digest the registry
/// names is that of the bytes it serves.
fn the_manifest_of_the_tag(
    deployment: &Deployment,
    port: u16,
    token: &str,
    tag: &str,
) -> (Vec<u8>, String) {
    let into = deployment
        .directory
        .path()
        .join(format!("served-{tag}.manifest"));
    let answer = support::Curl::with_header(
        port,
        &the_oci_path("manifests", tag),
        token,
        ("accept", "*/*"),
        &into,
    )
    .run();
    assert_eq!(
        answer.status,
        200,
        "the tag {tag} does not resolve:\n{}",
        deployment.the_log()
    );
    let digest = answer
        .header("docker-content-digest")
        .expect("the answer does not name the digest");
    (std::fs::read(&into).expect("the manifest is read"), digest)
}

/// The blob the registry serves by its digest.
fn the_blob_of(deployment: &Deployment, port: u16, token: &str, digest: &str) -> Vec<u8> {
    let into = deployment.directory.path().join("served.blob");
    let answer =
        support::Curl::download(port, &the_oci_path("blobs", digest), token, Some(&into)).run();
    assert_eq!(
        answer.status,
        200,
        "the blob {digest} is not served:\n{}",
        deployment.the_log()
    );
    std::fs::read(&into).expect("the blob is read")
}

/// The digest of the configuration a manifest names.
///
/// An image's identifier is the digest of its configuration: it is what
/// `podman` says about the image it has, and what the manifest
/// names as its configuration.
fn the_config_digest_of(manifest: &[u8]) -> String {
    let manifest: serde_json::Value =
        serde_json::from_slice(manifest).expect("the manifest is JSON");
    manifest
        .get("config")
        .and_then(|config| config.get("digest"))
        .and_then(|digest| digest.as_str())
        .expect("the manifest names a config")
        .trim_start_matches("sha256:")
        .to_string()
}

/// The digest of the layer a manifest names.
fn the_layer_digest_of(manifest: &[u8]) -> String {
    let manifest: serde_json::Value =
        serde_json::from_slice(manifest).expect("the manifest is JSON");
    manifest
        .get("layers")
        .and_then(|layers| layers.get(0))
        .and_then(|layer| layer.get("digest"))
        .and_then(|digest| digest.as_str())
        .expect("the manifest names a layer")
        .trim_start_matches("sha256:")
        .to_string()
}

/// The layer the pull returned.
///
/// The downloaded layout names the layer in the manifest, and the digest is
/// the file's. The layout file is named by its digest, so
/// the digest is looked up in the manifest and the file with that
/// name is read.
fn the_layer_of_the_pulled_layout(pulled: &Path) -> (String, PathBuf) {
    let index: serde_json::Value = serde_json::from_slice(
        &std::fs::read(pulled.join("index.json")).expect("the index is read"),
    )
    .expect("the index is JSON");
    let manifest_digest = index
        .get("manifests")
        .and_then(|manifests| manifests.get(0))
        .and_then(|manifest| manifest.get("digest"))
        .and_then(|digest| digest.as_str())
        .expect("the index names a manifest")
        .trim_start_matches("sha256:")
        .to_string();
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(pulled.join("blobs").join("sha256").join(&manifest_digest))
            .expect("the manifest is read"),
    )
    .expect("the manifest is JSON");
    let layer_digest = manifest
        .get("layers")
        .and_then(|layers| layers.get(0))
        .and_then(|layer| layer.get("digest"))
        .and_then(|digest| digest.as_str())
        .expect("the manifest names a layer")
        .trim_start_matches("sha256:")
        .to_string();
    let path = pulled.join("blobs").join("sha256").join(&layer_digest);
    (layer_digest, path)
}

/// `skopeo` pushes and pulls, and what comes back are the bytes that are there.
///
/// The client receives no instruction: `skopeo` decides how to
/// talk to the registry, and the registry has to talk the way the
/// protocol says.
///
/// And the comparison is not with the source layout: a real client
/// **rewrites** the manifest it uploads -- converting it to the format the
/// destination understands --, so the source digest is not the digest that
/// ends up in the registry. What has to be the same are the bytes the
/// registry serves and those that come back from the pull.
#[test]
fn skopeo_pushes_and_pulls_the_same_bytes() {
    let mut case = the_case(
        "OCI-PUSH-03 (skopeo)",
        "un layout OCI de una sola plataforma y un token valido",
        "skopeo hace push y despues pull, y se comparan los bytes del manifiesto y de la capa",
        "skopeo copy --dest-registry-token <alice> oci:<layout> docker://127.0.0.1:<port>/releases:v1",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let layout = deployment.directory.path().join("layout");
    let layer = a_layer(21);
    an_oci_layout(&layout, &layer);

    let pushed = Command::new("skopeo")
        .args([
            "copy",
            "--dest-registry-token",
            &token,
            "--dest-tls-verify=false",
        ])
        .arg(format!("oci:{}", layout.display()))
        .arg(format!(
            "docker://127.0.0.1:{}/{FIRST_REPOSITORY}:v1",
            service.port
        ))
        .output()
        .expect("skopeo has to be there");
    assert!(
        pushed.status.success(),
        "skopeo could not push:\n{}\n{}",
        String::from_utf8_lossy(&pushed.stdout),
        String::from_utf8_lossy(&pushed.stderr)
    );

    // What the registry serves: the manifest bytes and the digest it
    // names for them. If the digest is not that of the bytes, the registry is
    // lying to itself.
    let (served, served_digest) = the_manifest_of_the_tag(&deployment, service.port, &token, "v1");
    assert_eq!(
        the_digest(&served),
        served_digest.trim_start_matches("sha256:"),
        "the digest the registry names is not the digest of the bytes it serves"
    );

    // And the pull returns the same bytes.
    let pulled = deployment.directory.path().join("pulled");
    let pulled_answer = Command::new("skopeo")
        .args([
            "copy",
            "--src-registry-token",
            &token,
            "--src-tls-verify=false",
        ])
        .arg(format!(
            "docker://127.0.0.1:{}/{FIRST_REPOSITORY}:v1",
            service.port
        ))
        .arg(format!("oci:{}", pulled.display()))
        .output()
        .expect("skopeo has to be there");
    assert!(
        pulled_answer.status.success(),
        "skopeo could not pull:\n{}\n{}",
        String::from_utf8_lossy(&pulled_answer.stdout),
        String::from_utf8_lossy(&pulled_answer.stderr)
    );

    // The downloaded layout's index names the registry's manifest.
    let index: serde_json::Value = serde_json::from_slice(
        &std::fs::read(pulled.join("index.json")).expect("the index is read"),
    )
    .expect("the index is JSON");
    let described = index
        .get("manifests")
        .and_then(|manifests| manifests.get(0))
        .and_then(|manifest| manifest.get("digest"))
        .and_then(|digest| digest.as_str())
        .expect("the index names a manifest");
    assert_eq!(
        described, served_digest,
        "the pulled index does not name the manifest that the registry serves"
    );

    // And the manifest bytes are the same, byte for byte.
    let pulled_manifest = pulled
        .join("blobs")
        .join("sha256")
        .join(served_digest.trim_start_matches("sha256:"));
    assert_eq!(
        std::fs::read(&pulled_manifest).expect("the manifest is read"),
        served,
        "the manifest that came back is not the manifest that was served"
    );

    // The layer: the one the manifest names is the one the registry serves.
    let layer_digest = the_layer_digest_of(&served);
    let served_layer = the_blob_of(
        &deployment,
        service.port,
        &token,
        &format!("sha256:{layer_digest}"),
    );
    assert_eq!(
        the_digest(&served_layer),
        layer_digest,
        "the registry does not serve the layer that the manifest names"
    );

    // And the layer that comes back from the pull is that same layer.
    let (pulled_layer_digest, layer_file) = the_layer_of_the_pulled_layout(&pulled);
    assert!(
        layer_file.exists(),
        "the pulled layout does not have the layer: {:?}",
        pulled.join("blobs").join("sha256")
    );
    assert_eq!(
        pulled_layer_digest, layer_digest,
        "the pulled layer is not the layer that the manifest names"
    );
    assert_eq!(
        support::sha256_of(&layer_file),
        layer_digest,
        "the layer that came back is not the layer that went in"
    );

    case.outcome =
        "push y pull con skopeo: los bytes que vuelve son los que el registro servia".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}

/// The `podman` command, with its own store.
///
/// The user's store is not touched: the test's `podman` uses its own
/// root and runroot, and the `vfs` driver, which is the one that works
/// where there are no block permissions.
fn podman(root: &Path, runroot: &Path) -> Command {
    let mut command = Command::new("podman");
    command
        .args(["--root", root.to_str().expect("the root")])
        .args(["--runroot", runroot.to_str().expect("the runroot")])
        .args(["--storage-driver", "vfs"])
        .env("XDG_RUNTIME_DIR", runroot);
    command
}

/// `podman` pushes and pulls against the registry.
///
/// `podman` is the client closest to the end user, and the one that
/// speaks the whole Docker protocol: the challenge, the token endpoint and
/// the upload session.
#[test]
fn podman_pushes_and_pulls_the_same_bytes() {
    let mut case = the_case(
        "OCI-PUSH-03 (podman)",
        "un registro con el protocolo de Docker entero y un token valido",
        "podman hace push y pull, y la imagen que vuelve es la que el registro tiene",
        "podman push --tls-verify=false --authfile <credenciales> 127.0.0.1:<port>/releases:v1",
    );
    support::record_oci_push(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // `podman` talks to its own store: the user's is not touched.
    let root = deployment.directory.path().join("podman-root");
    let runroot = deployment.directory.path().join("podman-runroot");
    std::fs::create_dir_all(&root).expect("the podman root");
    std::fs::create_dir_all(&runroot).expect("the podman runroot");

    // The protocol's credentials file: the user is
    // informational and the password is the platform's credential.
    let auth = deployment.directory.path().join("auth.json");
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("alice:{token}"));
    std::fs::write(
        &auth,
        serde_json::json!({
            "auths": { format!("127.0.0.1:{}", service.port): { "auth": encoded } }
        })
        .to_string(),
    )
    .expect("the credentials are written");

    // The content that is imported: a minimal tree in a tar, which is what
    // `podman` knows how to turn into an image.
    let tree = deployment.directory.path().join("rootfs");
    std::fs::create_dir_all(&tree).expect("the tree");
    std::fs::write(tree.join("hello.txt"), b"hodei\n").expect("the file");
    let archive = deployment.directory.path().join("rootfs.tar");
    let tarred = Command::new("tar")
        .args([
            "-cf",
            archive.to_str().expect("the archive"),
            "-C",
            tree.to_str().expect("the tree"),
            ".",
        ])
        .output()
        .expect("tar has to be there");
    assert!(
        tarred.status.success(),
        "the archive could not be written:\n{}",
        String::from_utf8_lossy(&tarred.stderr)
    );

    let reference = format!("127.0.0.1:{}/{FIRST_REPOSITORY}:v1", service.port);

    // The import: a local image, which is where what gets pushed comes from.
    let imported = podman(&root, &runroot)
        .arg("import")
        .arg(&archive)
        .arg(&reference)
        .output()
        .expect("podman has to be there");
    assert!(
        imported.status.success(),
        "podman could not import:\n{}\n{}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );

    // The push: `podman` pushes what it has, with the challenge and the token
    // endpoint in between.
    let pushed = podman(&root, &runroot)
        .arg("push")
        .args(["--tls-verify=false", "--authfile"])
        .arg(&auth)
        .arg(&reference)
        .output()
        .expect("podman has to be there");
    assert!(
        pushed.status.success(),
        "podman could not push:\n{}\n{}",
        String::from_utf8_lossy(&pushed.stdout),
        String::from_utf8_lossy(&pushed.stderr)
    );

    // The registry has the manifest, and the digest it names is that of
    // the bytes it serves.
    let (served, served_digest) = the_manifest_of_the_tag(&deployment, service.port, &token, "v1");
    assert_eq!(
        the_digest(&served),
        served_digest.trim_start_matches("sha256:"),
        "the digest the registry names is not the digest of the bytes it serves"
    );
    let config_digest = the_config_digest_of(&served);

    // The pull: `podman` authenticates by itself, with the challenge and the token
    // endpoint.
    let pulled = podman(&root, &runroot)
        .arg("pull")
        .args(["--tls-verify=false", "--authfile"])
        .arg(&auth)
        .arg(&reference)
        .output()
        .expect("podman has to be there");
    assert!(
        pulled.status.success(),
        "podman could not pull:\n{}\n{}",
        String::from_utf8_lossy(&pulled.stdout),
        String::from_utf8_lossy(&pulled.stderr)
    );

    // The image that comes back is the same: its identifier is the digest of
    // the configuration the manifest names.
    let inspected = podman(&root, &runroot)
        .args(["image", "inspect", "--format", "{{.Id}}"])
        .arg(&reference)
        .output()
        .expect("podman has to be there");
    assert!(
        inspected.status.success(),
        "podman could not inspect the image:\n{}\n{}",
        String::from_utf8_lossy(&inspected.stdout),
        String::from_utf8_lossy(&inspected.stderr)
    );
    let identifier = String::from_utf8_lossy(&inspected.stdout)
        .trim()
        .to_string();
    assert_eq!(
        identifier.trim_start_matches("sha256:"),
        config_digest,
        "the image that came back is not the image that was pushed"
    );

    // And the new tag exists and points to the same image.
    let retagged = format!("127.0.0.1:{}/{FIRST_REPOSITORY}:v2", service.port);
    let tagged = podman(&root, &runroot)
        .arg("tag")
        .arg(&reference)
        .arg(&retagged)
        .output()
        .expect("podman has to be there");
    assert!(
        tagged.status.success(),
        "podman could not tag:\n{}",
        String::from_utf8_lossy(&tagged.stderr)
    );
    let repushed = podman(&root, &runroot)
        .arg("push")
        .args(["--tls-verify=false", "--authfile"])
        .arg(&auth)
        .arg(&retagged)
        .output()
        .expect("podman has to be there");
    assert!(
        repushed.status.success(),
        "podman could not push the second tag:\n{}\n{}",
        String::from_utf8_lossy(&repushed.stdout),
        String::from_utf8_lossy(&repushed.stderr)
    );

    let (moved, moved_digest) = the_manifest_of_the_tag(&deployment, service.port, &token, "v2");
    assert_eq!(
        the_config_digest_of(&moved),
        config_digest,
        "the tag that podman pushed does not point at the image it pushed"
    );
    assert!(
        moved_digest.starts_with("sha256:"),
        "the registry did not name the digest of the manifest"
    );

    case.outcome =
        "push y pull con podman: la imagen que vuelve es la que el registro tiene".to_string();
    case.passed = true;
    support::record_oci_push(case);

    support::stop(&mut service);
}
