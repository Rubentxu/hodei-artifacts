//! HSP-229: the OCI group resolves the reference among its members.
//!
//! What is demonstrated here, against the **real service**:
//!
//! - the group's precedence is that of its configuration: the first
//!   member that has the reference wins, and the response says which member
//!   it came from (`x-hodei-resolved-from`) and in which version of the configuration
//!   it was resolved (`x-hodei-group-version`);
//! - a manifest is served **whole** from a single member, and its blobs are
//!   addressed by their digest;
//! - a MISS (a member without the tag) moves on to the next member, and an
//!   ERROR (trust hides the winner's digest) **stops** the
//!   resolution: the group does not skip to the next one;
//! - `HEAD` walks exactly the same resolution as `GET`;
//! - a member that the principal cannot read is skipped without saying that it
//!   exists, and a repository of another account is `NAME_UNKNOWN`;
//! - a group **stores nothing**: a write against it is rejected with the
//!   dialect's `UNSUPPORTED`.
//!
//! And, with a **proxy** member in front --increment 2--:
//!
//! - the proxy is queried with the **member's name**, not the
//!   group's, and what it downloads is published in the member's namespace;
//! - a **down** upstream stops the resolution: the group does not skip to the
//!   next member, because a transient failure cannot silently change
//!   what is downloaded;
//! - an upstream that answers `404` is a MISS, and the next member wins;
//! - namespace reservation is judged by the **repository name**, and
//!   a proxy member whose name falls in the reservation is not queried;
//! - an upstream that asks for the token in a realm that is not the declared one does not
//!   receive the credential, not even inside a group;
//! - a **real** OCI client (`skopeo`) pulls the image from the group.
//!
//! And, with the tag listing --increment 3--:
//!
//! - the group's listing brings the tags of the hosted ones **and** those of the
//!   upstream of its proxy member, which is queried with the member's
//!   name;
//! - a tag that two members have comes out **once**, and the listing and
//!   the resolution tell the same story: what the listing says is
//!   downloaded from the member that won;
//! - a **down** upstream does not let the listing be served halfway: the
//!   whole listing is an error.
//!
//! And, with the listing of a **direct** proxy --increment 4--:
//!
//! - the listing of a direct proxy brings the tags of its **upstream**,
//!   which is queried with the proxy's name;
//! - the listing and the resolution tell the same story: what the listing
//!   says is downloaded from the upstream;
//! - a **down** upstream is not served as an empty list: the
//!   whole listing is an error;
//! - an upstream whose **scope** does not cover the listing makes a proxy that
//!   cannot enumerate, and its listing is the empty list.
//!
//! And, with the **honest** listing --increment 5--:
//!
//! - the listing only announces the tags that the resolution accepts: a
//!   name that the resolution rejects is not announced;
//! - a member whose scope does not cover the listing does not appear in the group's
//!   listing, and the decision leaves a trace;
//! - an upstream that **paginates** its list does not serve a listing halfway: the
//!   listing is not served.

mod support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use sha2::{Digest, Sha256};
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Curl, Deployment, FIRST_ACCOUNT, SECOND_REPOSITORY};

/// The first member of the group.
const FIRST: &str = "oci-first";

/// The second member of the group.
const SECOND: &str = "oci-second";

/// The group.
const GROUP: &str = "oci-group";

/// The tag that is requested.
const TAG: &str = "v1";

/// The media type of the image manifest.
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// The cases that a complete run has to execute.
///
/// They are declared **before** running: a run that only reached some
/// says which did not run, and a report with a case that never ran
/// is green.
const THE_CASES: &[(&str, &str)] = &[
    (
        "ac1-precedence-and-provenance",
        "el primer miembro con la etiqueta gana, y dice de que miembro salio",
    ),
    (
        "ac1-a-miss-goes-to-the-next-member",
        "un miembro sin la etiqueta es un MISS, y gana el que la tiene",
    ),
    (
        "ac1-a-tag-that-nobody-has",
        "una etiqueta que nadie tiene es un manifiesto desconocido",
    ),
    (
        "ac2-head-and-get-take-the-same-resolution",
        "el HEAD y el GET recorren la misma resolucion",
    ),
    (
        "ac3-another-account-is-name-unknown",
        "un repositorio de otra cuenta es NAME_UNKNOWN y no dice nada",
    ),
    (
        "ac3-a-member-the-principal-cannot-read-is-skipped",
        "un miembro que no se puede leer se omite sin decir que existe",
    ),
    (
        "r6-the-trust-gate-stops-the-resolution",
        "la confianza que oculta al ganador detiene la resolucion",
    ),
    (
        "r10-a-group-does-not-accept-writes",
        "escribir contra un grupo se rechaza con UNSUPPORTED",
    ),
    (
        "ac3-the-first-member-wins-with-a-proxy-in-the-group",
        "con un miembro proxy, gana el primer miembro del orden",
    ),
    (
        "ac3-the-proxy-wins-when-it-is-the-first-member",
        "si el primer miembro es el proxy, gana el proxy",
    ),
    (
        "ac3-an-upstream-that-is-down-stops-the-resolution",
        "un upstream caido detiene la resolucion y no salta al siguiente miembro",
    ),
    (
        "ac3-an-upstream-without-the-reference-is-a-miss",
        "un upstream que no tiene la referencia es un MISS, y gana el siguiente",
    ),
    (
        "ac3-a-reserved-name-is-not-served-from-the-proxy",
        "un nombre reservado no se sirve desde el miembro proxy",
    ),
    (
        "ac3-the-credential-does-not-travel-to-a-foreign-realm",
        "un realm que no es el declarado no recibe la credencial",
    ),
    (
        "ac3-a-real-oci-client-pulls-from-the-group",
        "un cliente OCI real saca la imagen del grupo",
    ),
    (
        "ac3-the-referrers-of-the-group-are-the-ones-of-its-members",
        "los referrers del grupo son los de sus miembros",
    ),
    (
        "ac4-the-listing-of-the-group-has-the-tags-of-the-proxy",
        "el listado del grupo trae las etiquetas de los hosted y las del proxy",
    ),
    (
        "ac4-the-list-has-one-entry-per-name",
        "una etiqueta que tienen dos miembros sale una vez",
    ),
    (
        "ac4-a-tag-of-the-listing-can-be-pulled-from-the-group",
        "una etiqueta del listado se descarga y devuelve el digest del miembro que gano",
    ),
    (
        "ac4-an-upstream-that-is-down-does-not-serve-a-partial-listing",
        "un upstream caido no sirve el listado a medias",
    ),
    (
        "ac5-the-listing-of-a-direct-proxy-has-the-tags-of-the-upstream",
        "el listado de un proxy directo trae las etiquetas de su upstream",
    ),
    (
        "ac5-a-tag-of-the-listing-can-be-pulled-from-the-direct-proxy",
        "una etiqueta del listado de un proxy directo se descarga de su upstream",
    ),
    (
        "ac5-an-upstream-that-is-down-does-not-serve-an-empty-listing",
        "un upstream caido no hace que un proxy directo sirva una lista vacia",
    ),
    (
        "ac5-a-proxy-whose-scope-does-not-cover-the-listing-serves-an-empty-list",
        "un proxy cuyo ambito no cubre el listado sirve la lista vacia",
    ),
    (
        "ac6-the-listing-only-announces-the-tags-that-can-be-pulled",
        "el listado solo anuncia las etiquetas que la resolucion acepta",
    ),
    (
        "ac6-a-member-whose-scope-does-not-cover-the-listing-is-not-listed",
        "un miembro que no puede enumerar no aparece en el listado, y deja traza",
    ),
    (
        "ac6-a-paginated-upstream-does-not-serve-a-partial-listing",
        "un upstream que pagina su lista no sirve un listado a medias",
    ),
];

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The digest of some bytes.
fn the_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// The configuration of an image, with its layer.
fn the_image(layer: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let config = b"{\"architecture\":\"amd64\",\"os\":\"linux\"}".to_vec();
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{}", the_digest(&config)),
            "size": config.len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": format!("sha256:{}", the_digest(layer)),
            "size": layer.len()
        }]
    })
    .to_string()
    .into_bytes();
    (manifest, config)
}

/// The deployment with the group and its members seeded.
///
/// `trusted` are the members seeded on the **trusted** channel: their
/// trust gate does not let through what has no grant, and that
/// is what is measured.
fn the_deployment(trusted: &[&str]) -> Deployment {
    let deployment = Deployment::new();
    let repositories: Vec<serde_json::Value> = [FIRST, SECOND]
        .iter()
        .map(|key| {
            json!({
                "key": key,
                "format": "oci",
                "trusted": trusted.contains(key),
            })
        })
        .collect();
    deployment.seed_repositories(&repositories);
    deployment.seed_groups_raw(&[json!({
        "key": GROUP,
        "members": [FIRST, SECOND],
        "format": "oci",
    })]);
    deployment
}

/// Publishes an image to an OCI repository, by the protocol.
///
/// The blobs first and the manifest after: it is the protocol's order, and
/// the manifest is published with the tag that names it.
fn publish(deployment: &Deployment, port: u16, repository: &str, layer: &[u8]) -> Vec<u8> {
    publish_as(deployment, port, repository, TAG, layer)
}

/// Publishes an image to an OCI repository, with a tag of its own.
fn publish_as(
    deployment: &Deployment,
    port: u16,
    repository: &str,
    tag: &str,
    layer: &[u8],
) -> Vec<u8> {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let (manifest, config) = the_image(layer);

    for content in [&config, layer] {
        let file = deployment.directory.path().join("blob.bin");
        std::fs::write(&file, content).expect("the blob is written");
        let answer = Curl::call(
            port,
            "POST",
            &format!(
                "/v2/{repository}/blobs/uploads/?digest=sha256:{}",
                the_digest(content)
            ),
            &token,
            Some(&file),
            None,
            Some(&deployment.directory.path().join("answer.bin")),
        )
        .run();
        assert!(
            (200..300).contains(&answer.status),
            "the blob was not published: {}\n{}\n{}",
            answer.status,
            String::from_utf8_lossy(&answer.body),
            deployment.the_log()
        );
    }

    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");
    let answer = Curl::call(
        port,
        "PUT",
        &format!("/v2/{repository}/manifests/{tag}"),
        &token,
        Some(&file),
        Some(MANIFEST_MEDIA_TYPE),
        None,
    )
    .run();
    assert!(
        (200..300).contains(&answer.status),
        "the manifest was not published: {}\n{}",
        answer.status,
        deployment.the_log()
    );
    manifest
}

/// The response of an OCI read.
struct Reply {
    /// The status code.
    status: u16,
    /// The body bytes.
    body: Vec<u8>,
    /// The digest that the response states.
    content_digest: Option<String>,
    /// Which member served the content.
    resolved_from: Option<String>,
    /// The version of the group's configuration.
    group_version: Option<String>,
}

/// A read of the group by tag.
fn the_manifest(deployment: &Deployment, port: u16, repository: &str) -> Reply {
    the_manifest_of(deployment, port, repository, TAG)
}

/// A read of the group by a tag.
fn the_manifest_of(deployment: &Deployment, port: u16, repository: &str, tag: &str) -> Reply {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("answer.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::download(
        port,
        &format!("/v2/{repository}/manifests/{tag}"),
        &token,
        Some(&into),
    )
    .run();
    Reply {
        status: answer.status,
        body: std::fs::read(&into).unwrap_or_default(),
        content_digest: answer.header("docker-content-digest"),
        resolved_from: answer.header("x-hodei-resolved-from"),
        group_version: answer.header("x-hodei-group-version"),
    }
}

/// The tag list of a repository: the status, the names and the
/// raw body.
///
/// The raw body matters: a listing served halfway would be recognized
/// by what it **says**, not by what it leaves unsaid.
fn the_tags(deployment: &Deployment, port: u16, repository: &str) -> (u16, Vec<String>, String) {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("tags-list.json");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::download(
        port,
        &format!("/v2/{repository}/tags/list"),
        &token,
        Some(&into),
    )
    .run();
    let body = std::fs::read_to_string(&into).unwrap_or_default();
    let tags = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("tags").and_then(|tags| tags.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tag| tag.as_str().map(str::to_string))
        .collect();
    (answer.status, tags, body)
}

/// A read of a blob by its digest.
fn the_blob(deployment: &Deployment, port: u16, repository: &str, digest: &str) -> Reply {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("blob-answer.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::download(
        port,
        &format!("/v2/{repository}/blobs/sha256:{digest}"),
        &token,
        Some(&into),
    )
    .run();
    Reply {
        status: answer.status,
        body: std::fs::read(&into).unwrap_or_default(),
        content_digest: answer.header("docker-content-digest"),
        resolved_from: answer.header("x-hodei-resolved-from"),
        group_version: answer.header("x-hodei-group-version"),
    }
}

/// The response of a `HEAD` of the group by tag.
fn the_head(port: u16, repository: &str) -> support::Answer {
    let token = support::token(FIRST_ACCOUNT, "alice");
    Curl::describe(port, &format!("/v2/{repository}/manifests/{TAG}"), &token).run()
}

/// Runs a lab case.
///
/// The case is **registered before running**, as `NOT_RUN`, and is replaced
/// when it finishes: a case that dies midway leaves a `NOT_RUN` in the
/// report and not a `PASS` that nobody measured.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    // The report is described before any case runs: the title and
    // the disclaimer belong to the suite, and a case registered before
    // the suite is described would write a report that is not this suite's.
    describe_registry_lab(
        "Laboratorio del grupo OCI — informe",
        &[
            "- **Un upstream, no una red.** El upstream de distribucion es el registro del laboratorio, en local: lo que se mide es el camino del grupo con un miembro proxy, no la red de una distribucion real.",
            "- **Ni la superficie de push.** Solo se ha medido el pull, y el rechazo de la escritura contra el grupo.",
            "- **El cliente real es `skopeo` (o `oras`).** Si el entorno no los tiene, el caso se declara `NOT_RUN` con el motivo: nunca `PASS` por una salida vacia.",
        ],
    );
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

// ---------------------------------------------------------------------------
// AC1: precedence, integrity and provenance.
// ---------------------------------------------------------------------------

#[test]
fn ac1_the_first_member_with_the_tag_wins_and_the_provenance_is_said() {
    a_case(
        "ac1-precedence-and-provenance",
        "el primer miembro con la etiqueta gana, y dice de que miembro salio",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let second_layer = b"la-capa-del-segundo-miembro".to_vec();
            let first_manifest = publish(&deployment, port, FIRST, &first_layer);
            let second_manifest = publish(&deployment, port, SECOND, &second_layer);
            assert_ne!(
                first_manifest, second_manifest,
                "los dos miembros publican imagenes distintas"
            );

            // Both members have the same tag and a different digest:
            // the first in the configured order wins.
            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(
                answer.status,
                200,
                "el grupo no sirve el manifiesto:\n{}",
                deployment.the_log()
            );
            assert_eq!(
                answer.body, first_manifest,
                "el manifiesto no es el del miembro que gano"
            );
            assert_eq!(
                answer.content_digest.as_deref(),
                Some(format!("sha256:{}", the_digest(&first_manifest)).as_str()),
                "la respuesta no dice el digest del contenido servido"
            );
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(FIRST),
                "la respuesta no dice que miembro sirvio el contenido"
            );
            assert!(
                answer.group_version.is_some(),
                "la respuesta no dice la version de la configuracion del grupo"
            );

            // The blobs are addressed by their digest, and are those of the
            // winner.
            let blob = the_blob(&deployment, port, GROUP, &the_digest(&first_layer));
            assert_eq!(blob.status, 200, "el blob del ganador no se sirve");
            assert_eq!(blob.body, first_layer, "el blob no es el del ganador");
            assert_eq!(blob.resolved_from.as_deref(), Some(FIRST));

            // The second member's blob is also served: the group does not
            // merge, and what is in a member remains reachable
            // by its digest.
            let other = the_blob(&deployment, port, GROUP, &the_digest(&second_layer));
            assert_eq!(other.status, 200, "el blob del segundo miembro no se sirve");
            assert_eq!(other.body, second_layer);

            // The group's tag list is the union of its members':
            // one entry per name, and it is not served halfway.
            let token = support::token(FIRST_ACCOUNT, "alice");
            let into = deployment.directory.path().join("tags.json");
            let tags =
                Curl::download(port, &format!("/v2/{GROUP}/tags/list"), &token, Some(&into)).run();
            assert_eq!(tags.status, 200, "el grupo no lista sus etiquetas");
            let body = std::fs::read_to_string(&into).unwrap_or_default();
            assert!(
                body.contains(TAG),
                "la etiqueta del miembro no esta en la lista del grupo: {body}"
            );

            // And each member's direct path does not change: the group does not
            // give it access to anything its own path does not have.
            let direct = the_manifest(&deployment, port, FIRST);
            assert_eq!(direct.body, first_manifest);
            assert_eq!(direct.resolved_from, None);

            support::stop(&mut service);
            LabCase::new(
                "ac1-precedence-and-provenance",
                "el primer miembro con la etiqueta gana, y dice de que miembro salio",
            )
            .passed()
        },
    );
}

#[test]
fn ac1_a_member_without_the_tag_is_a_miss_and_the_next_one_wins() {
    a_case(
        "ac1-a-miss-goes-to-the-next-member",
        "un miembro sin la etiqueta es un MISS, y gana el que la tiene",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            // Only the second member publishes the tag: the first says
            // with certainty that it does not have it, and the resolution continues.
            let second_manifest =
                publish(&deployment, port, SECOND, b"la-capa-del-segundo-miembro");

            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(answer.status, 200, "el grupo no resuelve la etiqueta");
            assert_eq!(answer.body, second_manifest);
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(SECOND),
                "gana el miembro que tiene la etiqueta"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-a-miss-goes-to-the-next-member",
                "un miembro sin la etiqueta es un MISS, y gana el que la tiene",
            )
            .passed()
        },
    );
}

#[test]
fn ac1_a_tag_that_nobody_has_is_an_unknown_manifest() {
    a_case(
        "ac1-a-tag-that-nobody-has",
        "una etiqueta que nadie tiene es un manifiesto desconocido",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(
                answer.status, 404,
                "una etiqueta que nadie tiene es un manifiesto desconocido"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac1-a-tag-that-nobody-has",
                "una etiqueta que nadie tiene es un manifiesto desconocido",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC2: HEAD walks the same resolution.
// ---------------------------------------------------------------------------

#[test]
fn ac2_the_head_and_the_get_take_the_same_resolution() {
    a_case(
        "ac2-head-and-get-take-the-same-resolution",
        "el HEAD y el GET recorren la misma resolucion",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let first_manifest = publish(&deployment, port, FIRST, &first_layer);

            let get = the_manifest(&deployment, port, GROUP);
            assert_eq!(get.status, 200);

            let head = the_head(port, GROUP);
            assert_eq!(head.status, 200, "el HEAD del grupo no responde");
            assert_eq!(head.size, 0, "el HEAD trajo cuerpo");
            assert_eq!(
                head.header("docker-content-digest"),
                get.content_digest,
                "el HEAD no dice el mismo digest que el GET"
            );
            assert_eq!(
                head.header("x-hodei-resolved-from"),
                get.resolved_from,
                "el HEAD no dice el mismo miembro que el GET"
            );
            assert_eq!(
                head.header("x-hodei-group-version"),
                get.group_version,
                "el HEAD no dice la misma version del grupo que el GET"
            );
            assert_eq!(
                the_digest(&first_manifest),
                get.content_digest.unwrap().replace("sha256:", "")
            );

            support::stop(&mut service);
            LabCase::new(
                "ac2-head-and-get-take-the-same-resolution",
                "el HEAD y el GET recorren la misma resolucion",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC3: identity and permissions.
// ---------------------------------------------------------------------------

#[test]
fn ac3_a_repository_of_another_account_is_unknown_and_says_nothing() {
    a_case(
        "ac3-another-account-is-name-unknown",
        "un repositorio de otra cuenta es NAME_UNKNOWN y no dice nada",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            // The other account's repository exists, but not for this one:
            // the response does not say whether it exists elsewhere.
            let token = support::token(FIRST_ACCOUNT, "alice");
            let into = deployment.directory.path().join("answer.bin");
            let answer = Curl::download(
                port,
                &format!("/v2/{SECOND_REPOSITORY}/manifests/{TAG}"),
                &token,
                Some(&into),
            )
            .run();
            assert_eq!(answer.status, 404, "la cuenta ajena no se ve");
            let body = std::fs::read_to_string(&into).unwrap_or_default();
            assert!(
                body.contains("NAME_UNKNOWN"),
                "la respuesta no habla el dialecto de la distribucion: {body}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-another-account-is-name-unknown",
                "un repositorio de otra cuenta es NAME_UNKNOWN y no dice nada",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_a_member_the_principal_cannot_read_is_skipped_without_saying_it_exists() {
    a_case(
        "ac3-a-member-the-principal-cannot-read-is-skipped",
        "un miembro que no se puede leer se omite sin decir que existe",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            // alice can read the group and the second member, but not the
            // first.
            deployment.write_policy(&the_policy(&[GROUP, SECOND]));
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let second_layer = b"la-capa-del-segundo-miembro".to_vec();
            publish(&deployment, port, FIRST, &first_layer);
            let second_manifest = publish(&deployment, port, SECOND, &second_layer);

            // The first member has the tag, but alice cannot
            // read it: it is skipped without saying that it exists, and the second wins.
            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(answer.status, 200, "el grupo no resuelve la etiqueta");
            assert_eq!(answer.body, second_manifest);
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(SECOND),
                "gana el miembro que el principal puede leer"
            );

            // And the direct path of the member she cannot read stays closed.
            let token = support::token(FIRST_ACCOUNT, "alice");
            let into = deployment.directory.path().join("answer.bin");
            let direct = Curl::download(
                port,
                &format!("/v2/{FIRST}/manifests/{TAG}"),
                &token,
                Some(&into),
            )
            .run();
            assert!(
                [403, 404].contains(&direct.status),
                "la ruta directa del miembro no se cierra: {}",
                direct.status
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-member-the-principal-cannot-read-is-skipped",
                "un miembro que no se puede leer se omite sin decir que existe",
            )
            .passed()
        },
    );
}

/// The suite's Cedar policy: alice can read what is named.
///
/// The verified principal is `hrn:hodei:iam::<account>:User/<user>`: the
/// empty region is written with its two colons, which is how the token
/// declares it.
fn the_policy(downloads: &[&str]) -> String {
    let mut text = String::new();
    let principal = format!("hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice");
    text.push_str(&format!(
        "permit(principal == Iam::User::\"{principal}\", action == Artifacts::Action::\"UploadAsset\", resource);\n"
    ));
    for key in downloads {
        text.push_str(&format!(
            "permit(principal == Iam::User::\"{principal}\", action == Artifacts::Action::\"DownloadAsset\", resource == Artifacts::Repository::\"hrn:hodei:artifacts:eu:{FIRST_ACCOUNT}:repository/{key}\");\n"
        ));
    }
    text
}

// ---------------------------------------------------------------------------
// R6: the winner's trust stops the resolution.
// ---------------------------------------------------------------------------

#[test]
fn r6_a_trust_gate_that_hides_the_winner_does_not_make_the_group_jump() {
    a_case(
        "r6-the-trust-gate-stops-the-resolution",
        "la confianza que oculta al ganador detiene la resolucion",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The first member is the **trusted** channel: its trust
            // gate does not let through what has no grant.
            let deployment = the_deployment(&[FIRST]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let second_layer = b"la-capa-del-segundo-miembro".to_vec();
            publish(&deployment, port, FIRST, &first_layer);
            publish(&deployment, port, SECOND, &second_layer);

            // The winner (the first) is hidden by trust: the group
            // answers that it does not exist and does NOT go down to the second member, which is
            // other content.
            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(
                answer.status, 404,
                "lo que la confianza oculta responde como inexistente"
            );
            assert_eq!(
                answer.resolved_from, None,
                "la respuesta no nombra el miembro que se salto"
            );

            support::stop(&mut service);
            LabCase::new(
                "r6-the-trust-gate-stops-the-resolution",
                "la confianza que oculta al ganador detiene la resolucion",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// R10: a group stores nothing.
// ---------------------------------------------------------------------------

#[test]
fn r10_a_write_against_a_group_is_rejected_as_unsupported() {
    a_case(
        "r10-a-group-does-not-accept-writes",
        "escribir contra un grupo se rechaza con UNSUPPORTED",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let token = support::token(FIRST_ACCOUNT, "alice");
            let file = deployment.directory.path().join("upload.bin");
            std::fs::write(&file, b"contenido").expect("the body is written");
            let into = deployment.directory.path().join("answer.bin");

            // The start of an upload against the group.
            let upload = Curl::call(
                port,
                "POST",
                &format!("/v2/{GROUP}/blobs/uploads/"),
                &token,
                Some(&file),
                None,
                Some(&into),
            )
            .run();
            assert_eq!(
                upload.status,
                405,
                "escribir contra un grupo no se rechaza:\n{}",
                deployment.the_log()
            );
            let text = String::from_utf8_lossy(&upload.body).to_string();
            assert!(
                text.contains("UNSUPPORTED"),
                "la respuesta no habla el dialecto de la distribucion: {text}"
            );

            // And the manifest neither.
            let manifest = Curl::call(
                port,
                "PUT",
                &format!("/v2/{GROUP}/manifests/{TAG}"),
                &token,
                Some(&file),
                Some(MANIFEST_MEDIA_TYPE),
                Some(&into),
            )
            .run();
            assert_eq!(manifest.status, 405, "el manifiesto del grupo se acepta");

            support::stop(&mut service);
            LabCase::new(
                "r10-a-group-does-not-accept-writes",
                "escribir contra un grupo se rechaza con UNSUPPORTED",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// The proxy member: the lab's distribution upstream.
// ---------------------------------------------------------------------------

/// The name of the proxy member.
const PROXY: &str = "oci-proxy";

/// The group that has a proxy member.
const PROXY_GROUP: &str = "oci-proxy-group";

/// The proxy member whose upstream is down.
const DOWN: &str = "oci-proxy-down";

/// The direct proxy whose scope does not cover the listing.
///
/// Its upstream has a scope that does not reach `v2/<name>/tags/list`: the
/// repository **cannot enumerate**, and its listing is the empty list.
const SCOPED: &str = "oci-proxy-scoped";

/// The credential that the upstream requires in its realm.
const CREDENTIAL: &str = "secreto-del-laboratorio";

/// The token that the upstream issues.
const TOKEN: &str = "token-del-laboratorio";

/// The environment variable that carries the upstream's credential.
const CREDENTIAL_VARIABLE: &str = "LAB_OCI_CREDENTIAL";

/// The header that states the digest of the served content.
const DOCKER_CONTENT_DIGEST: HeaderName = HeaderName::from_static("docker-content-digest");

/// How the upstream behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheBehaviour {
    /// Asks for a token and serves what it has.
    Normal,
    /// Its challenge points to a realm that is **not** the declared one.
    HostileRealm,
    /// Its tag list is **paginated**: it answers with the `Link` header,
    /// which says there are more pages.
    Paginated,
}

/// What the upstream has received.
#[derive(Debug, Clone)]
struct Seen {
    /// The requested path.
    path: String,
    /// The `Authorization` header, if there was one.
    authorization: Option<String>,
}

/// The state of the upstream.
struct TheWorld {
    /// The manifests, by reference.
    manifests: Mutex<HashMap<String, Vec<u8>>>,
    /// The blobs, by hexadecimal digest.
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    /// The tags of each repository of the upstream.
    tags: Mutex<HashMap<String, Vec<String>>>,
    /// What the upstream has received.
    seen: Mutex<Vec<Seen>>,
    /// How it behaves.
    behaviour: Mutex<TheBehaviour>,
    /// The origin that the hostile challenge points to.
    elsewhere: Mutex<String>,
    /// The origin of this upstream, for the challenge.
    origin: Mutex<String>,
}

/// The lab's distribution upstream.
///
/// It asks for a token in its realm, issues it to whoever presents the credential, and
/// serves manifests and blobs. It is the same pattern as HSP-228's: here
/// what is measured is the path of the **group** with this upstream in front.
struct TheRegistry {
    /// The port it listens on.
    port: u16,
    /// The state it shares.
    world: Arc<TheWorld>,
}

impl TheRegistry {
    /// Starts an empty upstream.
    fn start() -> Self {
        let world = Arc::new(TheWorld {
            manifests: Mutex::new(HashMap::new()),
            blobs: Mutex::new(HashMap::new()),
            tags: Mutex::new(HashMap::new()),
            seen: Mutex::new(Vec::new()),
            behaviour: Mutex::new(TheBehaviour::Normal),
            elsewhere: Mutex::new(String::new()),
            origin: Mutex::new(String::new()),
        });
        let port = serve(Arc::clone(&world));
        let registry = Self { port, world };
        *registry.world.origin.lock().expect("the origin") = registry.origin();
        registry
    }

    /// The port it listens on.
    fn port(&self) -> u16 {
        self.port
    }

    /// The origin of the upstream.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The base of the upstream, with a trailing slash.
    fn base(&self) -> String {
        format!("{}/", self.origin())
    }

    /// Publishes a manifest, under a reference.
    fn publish(&self, reference: &str, bytes: Vec<u8>) {
        self.world
            .manifests
            .lock()
            .expect("the manifests")
            .insert(reference.to_string(), bytes);
    }

    /// Publishes a blob.
    fn publish_blob(&self, digest: &str, bytes: Vec<u8>) {
        self.world
            .blobs
            .lock()
            .expect("the blobs")
            .insert(digest.to_string(), bytes);
    }

    /// Registers a tag in a repository of the upstream.
    ///
    /// It is what the upstream answers in its `tags/list`: the list of what
    /// each repository has.
    fn tag(&self, repository: &str, tag: &str) {
        let mut tags = self.world.tags.lock().expect("the tags");
        let entry = tags.entry(repository.to_string()).or_default();
        if !entry.iter().any(|one| one == tag) {
            entry.push(tag.to_string());
        }
    }

    /// Changes the upstream's behaviour.
    fn behave(&self, behaviour: TheBehaviour) {
        *self.world.behaviour.lock().expect("the behaviour") = behaviour;
    }

    /// Where the hostile challenge points.
    fn elsewhere(&self, origin: &str) {
        *self.world.elsewhere.lock().expect("the elsewhere") = origin.to_string();
    }

    /// The paths that have been requested of it.
    fn paths(&self) -> Vec<String> {
        self.world
            .seen
            .lock()
            .expect("the log")
            .iter()
            .map(|one| one.path.clone())
            .collect()
    }

    /// How many times a path has been requested.
    ///
    /// The path is the one that goes **after** `/v2/`: the upstream is requested by
    /// `v2/<name>/manifests/<reference>`, and what is recorded is what
    /// comes from the route.
    fn hits(&self, path: &str) -> usize {
        self.paths().iter().filter(|one| *one == path).count()
    }
}

/// The hostile origin: it only records what reaches it.
///
/// It is the place to which an upstream can send the token challenge when it is not
/// the declared one: the credential cannot travel there.
struct TheHostile {
    /// The port it listens on.
    port: u16,
    /// What it has received.
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TheHostile {
    /// Starts an origin that only records what reaches it.
    fn start() -> Self {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route(
                "/{*path}",
                get({
                    let seen = Arc::clone(&seen);
                    async move |Path(path): Path<String>, headers: HeaderMap| {
                        seen.lock().expect("the log").push(Seen {
                            path,
                            authorization: headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string),
                        });
                        (StatusCode::OK, "robado".to_string())
                    }
                }),
            )
            .with_state(());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the hostile binds");
        let port = listener.local_addr().expect("the address").port();
        listener.set_nonblocking(true).expect("the hostile accepts");
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("the runtime of the hostile");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).expect("the listener");
                let _ = axum::serve(listener, app).await;
            });
        });
        Self { port, seen }
    }

    /// The hostile one's origin.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// What the hostile one has received.
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }
}

/// Starts a server with the upstream's router and returns the port.
fn serve(world: Arc<TheWorld>) -> u16 {
    let app = Router::new()
        .route("/token", get(the_token_endpoint))
        .route("/v2/{*path}", get(the_distribution))
        .with_state(world);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    listener
        .set_nonblocking(true)
        .expect("the upstream accepts");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the runtime of the upstream");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("the listener");
            let _ = axum::serve(listener, app).await;
        });
    });
    port
}

/// The upstream's token endpoint.
///
/// It only hands the token to whoever presents the credential.
async fn the_token_endpoint(State(_world): State<Arc<TheWorld>>, headers: HeaderMap) -> Response {
    let expected = format!("Basic {}", base64_of(&format!("lab:{CREDENTIAL}")));
    match headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if value == expected => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "token": TOKEN, "expires_in": 300 }).to_string(),
        )
            .into_response(),
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"registry\"")],
            "{}".to_string(),
        )
            .into_response(),
    }
}

/// The distribution surface: manifests and blobs.
async fn the_distribution(
    State(world): State<Arc<TheWorld>>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    world.seen.lock().expect("the log").push(Seen {
        path: path.clone(),
        authorization: authorization.clone(),
    });

    let behaviour = *world.behaviour.lock().expect("the behaviour");
    let origin = world.origin.lock().expect("the origin").clone();
    let elsewhere = world.elsewhere.lock().expect("the elsewhere").clone();

    // The challenge: the upstream asks for a token in its realm. The hostile upstream
    // asks for it in the realm of **another**, which is what an upstream can do.
    let realm = match behaviour {
        TheBehaviour::HostileRealm => format!("{elsewhere}/token"),
        _ => format!("{origin}/token"),
    };
    let challenge = format!("Bearer realm=\"{realm}\",service=\"lab\"");

    let authorized = authorization.as_deref() == Some(&format!("Bearer {TOKEN}"));
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, challenge.as_str())],
            "{}".to_string(),
        )
            .into_response();
    }

    // `v2/<name>/tags/list`: the repository's tag list.
    if let Some((name, tail)) = path.split_once("/tags/list")
        && tail.is_empty()
    {
        let tags = world
            .tags
            .lock()
            .expect("the tags")
            .get(name)
            .cloned()
            .unwrap_or_default();
        let mut answer = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "name": name, "tags": tags }).to_string(),
        )
            .into_response();
        // A **paginated** list: the upstream says so with the
        // `Link` header, which announces the next page. What is received is the
        // first page, not the whole list.
        if behaviour == TheBehaviour::Paginated {
            answer.headers_mut().insert(
                header::LINK,
                header::HeaderValue::from_static("</v2/lab/tags/list?n=1&last=v1>; rel=\"next\""),
            );
        }
        return answer;
    }

    // `v2/<name>/manifests/<reference>` and `v2/<name>/blobs/<digest>`.
    if let Some((_name, reference)) = path.split_once("/manifests/") {
        let manifest = world
            .manifests
            .lock()
            .expect("the manifests")
            .get(reference)
            .cloned();
        let Some(bytes) = manifest else {
            return (StatusCode::NOT_FOUND, "{}".to_string()).into_response();
        };
        let digest = the_digest(&bytes);
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, MANIFEST_MEDIA_TYPE.to_string()),
                (header::ETAG, format!("\"{digest}\"")),
                (DOCKER_CONTENT_DIGEST, format!("sha256:{digest}")),
            ],
            bytes,
        )
            .into_response();
    }

    if let Some((_name, reference)) = path.split_once("/blobs/") {
        let digest = reference.strip_prefix("sha256:").unwrap_or(reference);
        let blob = world.blobs.lock().expect("the blobs").get(digest).cloned();
        let Some(bytes) = blob else {
            return (StatusCode::NOT_FOUND, "{}".to_string()).into_response();
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    DOCKER_CONTENT_DIGEST,
                    format!("sha256:{}", the_digest(&bytes)),
                ),
            ],
            bytes,
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "{}".to_string()).into_response()
}

/// A text in base64.
fn base64_of(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// A port where nobody listens: the down upstream.
fn a_closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the port binds");
    let port = listener.local_addr().expect("the address").port();
    drop(listener);
    port
}

/// The configuration of a proxy repository, with its upstream.
///
/// The base is the upstream's and the token's realm: both are the same
/// origin, because an upstream that asks for the token elsewhere is not the
/// declared upstream.
fn the_proxy(base: &str) -> serde_json::Value {
    the_proxy_with_scopes(base, &["v2/"])
}

/// The configuration of a proxy repository, with the scopes that are
/// declared for it.
///
/// The scope is what decides whether a path can be requested from the upstream: what
/// does not fall in a scope is not requested. A scope that does not cover the listing
/// makes a proxy that **cannot enumerate**.
fn the_proxy_with_scopes(base: &str, scopes: &[&str]) -> serde_json::Value {
    json!({
        "upstream": {
            "id": "lab",
            "url": base,
            "scopes": scopes,
            "credential_ref": format!("env:{CREDENTIAL_VARIABLE}"),
            "auth": {
                "realm": format!("{base}token"),
                "service": "lab",
                "username": "lab"
            },
            "redirects": { "max": 3, "cross_origin": false }
        },
        "cache": {
            "ttl": { "content_secs": 0, "metadata_secs": 0 },
            "negative_secs": 0
        }
    })
}

/// The lab with the distribution upstream in front.
struct TheLab {
    /// The deployment with the catalog seeded.
    deployment: Deployment,
    /// The distribution upstream.
    registry: TheRegistry,
    /// The hostile origin.
    hostile: TheHostile,
    /// The port where nobody listens: the down upstream.
    down_port: u16,
}

impl TheLab {
    /// The hostile origin's port.
    fn hostile_port(&self) -> u16 {
        self.hostile.port
    }

    /// A deployment with the members seeded and the upstream in front.
    ///
    /// The group's members are the ones named; the rest are
    /// seeded too, so that a suite can reorder the group without
    /// seeding again.
    fn new(members: &[&str], namespaces: Option<serde_json::Value>) -> Self {
        let deployment = Deployment::new();
        let registry = TheRegistry::start();
        let hostile = TheHostile::start();
        let down_port = a_closed_port();
        let down_base = format!("http://127.0.0.1:{down_port}/");
        deployment.seed_repositories(&[
            json!({ "key": FIRST, "format": "oci" }),
            json!({ "key": SECOND, "format": "oci" }),
            json!({
                "key": PROXY,
                "format": "oci",
                "proxy": the_proxy(&registry.base()),
            }),
            json!({
                "key": DOWN,
                "format": "oci",
                "proxy": the_proxy(&down_base),
            }),
            // This proxy's scope does not cover the listing: the
            // repository **cannot enumerate**.
            json!({
                "key": SCOPED,
                "format": "oci",
                "proxy": the_proxy_with_scopes(&registry.base(), &["v2/manifests/"]),
            }),
        ]);
        let mut group = json!({
            "key": PROXY_GROUP,
            "members": members,
            "format": "oci",
        });
        if let Some(namespaces) = namespaces {
            group["namespaces"] = namespaces;
        }
        deployment.seed_groups_raw(&[group]);
        Self {
            deployment,
            registry,
            hostile,
            down_port,
        }
    }

    /// The service's environment: the allowed egress and the credential.
    fn the_environment(&self) -> Vec<(String, String)> {
        let allowed = format!(
            "127.0.0.1:{};private;http,127.0.0.1:{};private;http,127.0.0.1:{};private;http",
            self.registry.port(),
            self.hostile.port,
            self.down_port
        );
        vec![
            ("HODEI_PROXY__ALLOWED_HOSTS".to_string(), allowed),
            (
                "HODEI_PROXY__ALLOWED_CREDENTIALS".to_string(),
                CREDENTIAL_VARIABLE.to_string(),
            ),
            (CREDENTIAL_VARIABLE.to_string(), CREDENTIAL.to_string()),
        ]
    }

    /// Starts the service with the allowed egress.
    fn start(&self) -> support::Service {
        let environment = self.the_environment();
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start(&borrowed)
    }

    /// Publishes an image to the upstream, with its tag.
    fn publish_upstream(&self, layer: &[u8]) -> Vec<u8> {
        self.publish_upstream_tag(TAG, layer)
    }

    /// Publishes an image to the upstream, with a tag of its own.
    ///
    /// The tag is also registered in the upstream's list: it is what
    /// the group enumerates when it asks its proxy member.
    fn publish_upstream_tag(&self, tag: &str, layer: &[u8]) -> Vec<u8> {
        let (manifest, config) = the_image(layer);
        self.registry.publish(tag, manifest.clone());
        self.registry
            .publish_blob(&the_digest(&config), config.clone());
        self.registry
            .publish_blob(&the_digest(layer), layer.to_vec());
        self.registry.tag(PROXY, tag);
        manifest
    }
}

// ---------------------------------------------------------------------------
// AC3: the proxy member inside the group.
// ---------------------------------------------------------------------------

#[test]
fn ac3_with_a_proxy_in_the_group_the_first_member_wins() {
    a_case(
        "ac3-the-first-member-wins-with-a-proxy-in-the-group",
        "con un miembro proxy, gana el primer miembro del orden",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, SECOND, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // The three members have the same tag and a different digest.
            let first_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            publish(
                &lab.deployment,
                port,
                SECOND,
                b"la-capa-del-segundo-miembro",
            );
            let proxy_manifest = lab.publish_upstream(b"la-capa-del-upstream");
            assert_ne!(first_manifest, proxy_manifest);

            // The first in the configured order wins.
            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                200,
                "el grupo no resuelve la etiqueta:\n{}",
                lab.deployment.the_log()
            );
            assert_eq!(
                answer.body, first_manifest,
                "el manifiesto no es el del miembro que gano"
            );
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(FIRST),
                "la respuesta no dice que miembro sirvio el contenido"
            );
            // And the proxy has not been queried: precedence is by order.
            assert_eq!(
                lab.registry.hits(&format!("{PROXY}/manifests/{TAG}")),
                0,
                "el upstream se consulted antes de tiempo: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-first-member-wins-with-a-proxy-in-the-group",
                "con un miembro proxy, gana el primer miembro del orden",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_the_proxy_wins_when_it_is_the_first_member() {
    a_case(
        "ac3-the-proxy-wins-when-it-is-the-first-member",
        "si el primer miembro es el proxy, gana el proxy",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[PROXY, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            // Both members have the same tag and a different digest.
            let proxy_manifest = lab.publish_upstream(b"la-capa-del-upstream");
            let hosted_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            assert_ne!(proxy_manifest, hosted_manifest);

            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                200,
                "el grupo no resuelve la etiqueta:\n{}",
                lab.deployment.the_log()
            );
            assert_eq!(
                answer.body, proxy_manifest,
                "el manifiesto no es el del miembro proxy que gano"
            );
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(PROXY),
                "la respuesta no dice que miembro sirvio el contenido"
            );
            assert_eq!(
                answer.content_digest.as_deref(),
                Some(format!("sha256:{}", the_digest(&proxy_manifest)).as_str()),
                "la respuesta no dice el digest del contenido servido"
            );

            // The upstream is queried with the **member's name**, not with
            // the group's: the group does not rename its member.
            assert!(
                lab.registry.hits(&format!("{PROXY}/manifests/{TAG}")) > 0,
                "el upstream no se consulto con el nombre del miembro: {:?}",
                lab.registry.paths()
            );
            assert_eq!(
                lab.registry.hits(&format!("{PROXY_GROUP}/manifests/{TAG}")),
                0,
                "el upstream se consulto con el nombre del grupo: {:?}",
                lab.registry.paths()
            );

            // And the proxy's blob is served by its digest, from the member.
            let layer = b"la-capa-del-upstream".to_vec();
            let blob = the_blob(&lab.deployment, port, PROXY_GROUP, &the_digest(&layer));
            assert_eq!(blob.status, 200, "el blob del proxy no se sirve");
            assert_eq!(blob.body, layer);
            assert_eq!(blob.resolved_from.as_deref(), Some(PROXY));

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-proxy-wins-when-it-is-the-first-member",
                "si el primer miembro es el proxy, gana el proxy",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_an_upstream_that_is_down_stops_the_resolution() {
    a_case(
        "ac3-an-upstream-that-is-down-stops-the-resolution",
        "un upstream caido detiene la resolucion y no salta al siguiente miembro",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The first member is a proxy whose upstream is not there: the
            // second member has the tag.
            let lab = TheLab::new(&[DOWN, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            let hosted_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");

            // The down upstream is an ERROR: the resolution stops and the
            // error is answered. It does not skip to the next member, because a
            // transient failure cannot silently change what is
            // downloaded.
            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                503,
                "un upstream caido no detiene la resolucion:\n{}\n{}",
                answer.body.len(),
                lab.deployment.the_log()
            );
            assert_ne!(
                answer.body, hosted_manifest,
                "el grupo salto al siguiente miembro con el upstream caido"
            );
            assert_eq!(
                answer.resolved_from, None,
                "la respuesta nombra un miembro que no sirvio el contenido"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-an-upstream-that-is-down-stops-the-resolution",
                "un upstream caido detiene la resolucion y no salta al siguiente miembro",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_an_upstream_without_the_reference_is_a_miss() {
    a_case(
        "ac3-an-upstream-without-the-reference-is-a-miss",
        "un upstream que no tiene la referencia es un MISS, y gana el siguiente",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[PROXY, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            // The upstream does **not** have the tag: it says so with certainty, and
            // it is a MISS. The second member has it.
            let hosted_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");

            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                200,
                "el grupo no resuelve la etiqueta:\n{}",
                lab.deployment.the_log()
            );
            assert_eq!(answer.body, hosted_manifest);
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(FIRST),
                "gana el miembro que tiene la referencia"
            );
            assert!(
                lab.registry.hits(&format!("{PROXY}/manifests/{TAG}")) > 0,
                "el upstream no se consulto: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-an-upstream-without-the-reference-is-a-miss",
                "un upstream que no tiene la referencia es un MISS, y gana el siguiente",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_a_reserved_name_is_not_served_from_the_proxy() {
    a_case(
        "ac3-a-reserved-name-is-not-served-from-the-proxy",
        "un nombre reservado no se sirve desde el miembro proxy",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The group's reservation reserves the proxy member's name: a
            // reserved name does not come out of a public upstream.
            let lab = TheLab::new(
                &[PROXY, FIRST],
                Some(json!({ "reserved": [PROXY], "on_conflict": "deny" })),
            );
            let mut service = lab.start();
            let port = service.port;

            // The upstream has the tag, and the second member too.
            lab.publish_upstream(b"la-capa-del-upstream");
            let hosted_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");

            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                200,
                "el grupo no resuelve la etiqueta:\n{}",
                lab.deployment.the_log()
            );
            assert_eq!(
                answer.body, hosted_manifest,
                "el contenido reservado salio del upstream"
            );
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(FIRST),
                "gana el miembro que no esta reservado"
            );
            // The upstream has not been queried: the reservation judgment is
            // on the repository name, and is made first.
            assert_eq!(
                lab.registry.hits(&format!("{PROXY}/manifests/{TAG}")),
                0,
                "el upstream reservado se consulto: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-reserved-name-is-not-served-from-the-proxy",
                "un nombre reservado no se sirve desde el miembro proxy",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_the_credential_does_not_travel_to_a_foreign_realm() {
    a_case(
        "ac3-the-credential-does-not-travel-to-a-foreign-realm",
        "un realm que no es el declarado no recibe la credencial",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[PROXY, FIRST], None);
            // The upstream asks for the token in a realm that is **not** the
            // declared one: the credential cannot travel there.
            lab.hostile_port();
            lab.registry.behave(TheBehaviour::HostileRealm);
            lab.registry.elsewhere(&lab.hostile.origin());
            let mut service = lab.start();
            let port = service.port;

            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");

            let answer = the_manifest(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                answer.status,
                503,
                "un realm que no es el declarado no detiene la resolucion:\n{}",
                lab.deployment.the_log()
            );

            // The hostile origin has not received the credential.
            for seen in lab.hostile.seen() {
                assert!(
                    !seen
                        .authorization
                        .as_deref()
                        .is_some_and(|value| value.contains(CREDENTIAL)),
                    "la credencial viajo a un realm que no es el declarado"
                );
            }

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-credential-does-not-travel-to-a-foreign-realm",
                "un realm que no es el declarado no recibe la credencial",
            )
            .passed()
        },
    );
}

#[test]
fn ac3_a_real_oci_client_pulls_from_the_group() {
    a_case(
        "ac3-a-real-oci-client-pulls-from-the-group",
        "un cliente OCI real saca la imagen del grupo",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The real client: `skopeo` first, and `oras` if it is not there.
            let client = match the_real_client() {
                Some(client) => client,
                None => {
                    return LabCase::new(
                        "ac3-a-real-oci-client-pulls-from-the-group",
                        "un cliente OCI real saca la imagen del grupo",
                    )
                    .not_run("no hay cliente OCI real en el entorno (ni skopeo ni oras)");
                }
            };

            let lab = TheLab::new(&[PROXY, FIRST], None);
            let mut service = lab.start();
            let port = service.port;
            let proxy_manifest = lab.publish_upstream(b"la-capa-del-upstream");

            let token = support::token(FIRST_ACCOUNT, "alice");
            let answer = client.inspect(port, &token, PROXY_GROUP, TAG);
            assert!(
                answer.ok,
                "el cliente real no pudo sacar la imagen: {}\n{}",
                answer.detail,
                lab.deployment.the_log()
            );
            assert!(
                answer.body.contains(&the_digest(&proxy_manifest)),
                "el cliente real no vio el digest del manifiesto servido: {}",
                answer.detail
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-a-real-oci-client-pulls-from-the-group",
                "un cliente OCI real saca la imagen del grupo",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// The real OCI client.
// ---------------------------------------------------------------------------

/// What a real OCI client answers.
struct TheClientAnswer {
    /// Whether the request went well.
    ok: bool,
    /// What the client said.
    detail: String,
    /// What the client returned.
    body: String,
}

/// A real OCI client of the environment.
enum TheClient {
    /// `skopeo`.
    Skopeo,
    /// `oras`.
    Oras,
}

impl TheClient {
    /// The command's name.
    fn the_command(&self) -> &'static str {
        match self {
            Self::Skopeo => "skopeo",
            Self::Oras => "oras",
        }
    }

    /// Asks the client to look at the group's manifest.
    fn inspect(&self, port: u16, token: &str, repository: &str, tag: &str) -> TheClientAnswer {
        let reference = format!("127.0.0.1:{port}/{repository}:{tag}");
        let output = match self {
            Self::Skopeo => std::process::Command::new("skopeo")
                .args([
                    "inspect",
                    "--tls-verify=false",
                    "--creds",
                    &format!("alice:{token}"),
                    &format!("docker://{reference}"),
                ])
                .output(),
            Self::Oras => std::process::Command::new("oras")
                .args([
                    "manifest",
                    "fetch",
                    "--plain-http",
                    "--username",
                    "alice",
                    "--password",
                    token,
                    &reference,
                ])
                .output(),
        };
        match output {
            Ok(output) => TheClientAnswer {
                ok: output.status.success(),
                detail: format!(
                    "{}: {}",
                    String::from_utf8_lossy(&output.stderr),
                    String::from_utf8_lossy(&output.stdout)
                ),
                body: String::from_utf8_lossy(&output.stdout).to_string(),
            },
            Err(error) => TheClientAnswer {
                ok: false,
                detail: error.to_string(),
                body: String::new(),
            },
        }
    }
}

/// The environment's real OCI client, if there is one.
fn the_real_client() -> Option<TheClient> {
    for candidate in [TheClient::Skopeo, TheClient::Oras] {
        let output = std::process::Command::new(candidate.the_command())
            .arg("--version")
            .output();
        if output.is_ok_and(|output| output.status.success()) {
            return Some(candidate);
        }
    }
    None
}

/// Publishes an arbitrary manifest to an OCI repository.
///
/// It is what is needed to hang a referrer: a manifest with
/// `subject` is what the protocol calls a referrer.
fn publish_manifest(
    deployment: &Deployment,
    port: u16,
    repository: &str,
    reference: &str,
    bytes: &[u8],
) {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let file = deployment.directory.path().join("referrer.json");
    std::fs::write(&file, bytes).expect("the manifest is written");
    let answer = Curl::call(
        port,
        "PUT",
        &format!("/v2/{repository}/manifests/{reference}"),
        &token,
        Some(&file),
        Some(MANIFEST_MEDIA_TYPE),
        None,
    )
    .run();
    assert!(
        (200..300).contains(&answer.status),
        "el referrer no se publico: {}\n{}",
        answer.status,
        deployment.the_log()
    );
}

#[test]
fn ac3_the_referrers_of_the_group_are_the_ones_of_its_members() {
    a_case(
        "ac3-the-referrers-of-the-group-are-the-ones-of-its-members",
        "los referrers del grupo son los de sus miembros",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let deployment = the_deployment(&[]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            // The subject: an image in the first member.
            let layer = b"la-capa-del-primer-miembro".to_vec();
            let subject = publish(&deployment, port, FIRST, &layer);
            let subject_digest = the_digest(&subject);

            // The referrer: a manifest that hangs from the subject, with the
            // same blobs (which are already in the repository).
            let config = b"{\"architecture\":\"amd64\",\"os\":\"linux\"}".to_vec();
            let referrer = json!({
                "schemaVersion": 2,
                "mediaType": MANIFEST_MEDIA_TYPE,
                "artifactType": "application/vnd.hodei.signature.v1+json",
                "config": {
                    "mediaType": "application/vnd.oci.image.config.v1+json",
                    "digest": format!("sha256:{}", the_digest(&config)),
                    "size": config.len()
                },
                "layers": [{
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": format!("sha256:{}", the_digest(&layer)),
                    "size": layer.len()
                }],
                "subject": {
                    "mediaType": MANIFEST_MEDIA_TYPE,
                    "digest": format!("sha256:{subject_digest}"),
                    "size": subject.len()
                }
            })
            .to_string()
            .into_bytes();
            publish_manifest(&deployment, port, FIRST, "firma", &referrer);

            // The group answers with the referrers of **its members**: a
            // group stores nothing, so it has none of its own.
            let token = support::token(FIRST_ACCOUNT, "alice");
            let into = deployment.directory.path().join("referrers.json");
            let answer = Curl::download(
                port,
                &format!("/v2/{GROUP}/referrers/sha256:{subject_digest}"),
                &token,
                Some(&into),
            )
            .run();
            assert_eq!(answer.status, 200, "el grupo no responde los referrers");
            let body = std::fs::read_to_string(&into).unwrap_or_default();
            assert!(
                body.contains(&the_digest(&referrer)),
                "los referrers del grupo no son los de sus miembros: {body}"
            );
            assert!(
                body.contains("application/vnd.hodei.signature.v1+json"),
                "el referrer no dice su artifactType: {body}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac3-the-referrers-of-the-group-are-the-ones-of-its-members",
                "los referrers del grupo son los de sus miembros",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC4: the group's tag listing, with a proxy member in front.
// ---------------------------------------------------------------------------

#[test]
fn ac4_the_listing_of_the_group_has_the_tags_of_the_proxy() {
    a_case(
        "ac4-the-listing-of-the-group-has-the-tags-of-the-proxy",
        "el listado del grupo trae las etiquetas de los hosted y las del proxy",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // The hosted member has its tag, and the upstream has one
            // that only exists in the upstream.
            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            lab.publish_upstream_tag("solo-en-el-proxy", b"la-capa-del-upstream");

            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                200,
                "el grupo no lista sus etiquetas:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                tags.contains(&TAG.to_string()),
                "la etiqueta del miembro hosted no esta en la lista: {tags:?}"
            );
            assert!(
                tags.contains(&"solo-en-el-proxy".to_string()),
                "la etiqueta que solo tiene el proxy no esta en la lista: {tags:?}"
            );

            // And the upstream is queried with the **member's** name, not with
            // the group's: the group does not rename its member.
            assert!(
                lab.registry.hits(&format!("{PROXY}/tags/list")) > 0,
                "el upstream no se consulto por su lista: {:?}",
                lab.registry.paths()
            );
            assert_eq!(
                lab.registry.hits(&format!("{PROXY_GROUP}/tags/list")),
                0,
                "el upstream se consulto con el nombre del grupo: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac4-the-listing-of-the-group-has-the-tags-of-the-proxy",
                "el listado del grupo trae las etiquetas de los hosted y las del proxy",
            )
            .passed()
        },
    );
}

#[test]
fn ac4_the_listing_has_one_entry_per_name() {
    a_case(
        "ac4-the-list-has-one-entry-per-name",
        "una etiqueta que tienen dos miembros sale una vez",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // Both members have the same tag: the first's in
            // the configured order is the one that counts.
            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            lab.publish_upstream(b"la-capa-del-upstream");

            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                200,
                "el grupo no lista sus etiquetas:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert_eq!(
                tags.iter().filter(|one| one.as_str() == TAG).count(),
                1,
                "la etiqueta que tienen dos miembros sale mas de una vez: {tags:?}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac4-the-list-has-one-entry-per-name",
                "una etiqueta que tienen dos miembros sale una vez",
            )
            .passed()
        },
    );
}

#[test]
fn ac4_a_tag_of_the_listing_can_be_pulled_from_the_group() {
    a_case(
        "ac4-a-tag-of-the-listing-can-be-pulled-from-the-group",
        "una etiqueta del listado se descarga y devuelve el digest del miembro que gano",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            let hosted = publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            let proxied = lab.publish_upstream_tag("solo-en-el-proxy", b"la-capa-del-upstream");

            // The listing brings both tags.
            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(status, 200, "el grupo no lista sus etiquetas:\n{body}");
            assert!(
                tags.contains(&"solo-en-el-proxy".to_string()),
                "la etiqueta que solo tiene el proxy no esta en la lista: {tags:?}"
            );

            // And what the listing says can be downloaded: the tag that
            // only the proxy has comes out of the proxy, with its digest, and the
            // response says which member it came from.
            let answer = the_manifest_of(&lab.deployment, port, PROXY_GROUP, "solo-en-el-proxy");
            assert_eq!(
                answer.status,
                200,
                "la etiqueta del listado no se descarga:\n{}\n{}",
                lab.deployment.the_log(),
                String::from_utf8_lossy(&answer.body)
            );
            assert_eq!(
                answer.body, proxied,
                "el manifiesto no es el del miembro que gano"
            );
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(PROXY),
                "la respuesta no dice que miembro sirvio el contenido"
            );
            assert_eq!(
                answer.content_digest.as_deref(),
                Some(format!("sha256:{}", the_digest(&proxied)).as_str()),
                "la respuesta no dice el digest del contenido servido"
            );

            // And the hosted's comes out of the hosted: the listing and the resolution
            // tell the same story.
            let other = the_manifest_of(&lab.deployment, port, PROXY_GROUP, TAG);
            assert_eq!(other.status, 200, "la etiqueta del hosted no se descarga");
            assert_eq!(other.body, hosted, "el manifiesto no es el del hosted");
            assert_eq!(other.resolved_from.as_deref(), Some(FIRST));

            support::stop(&mut service);
            LabCase::new(
                "ac4-a-tag-of-the-listing-can-be-pulled-from-the-group",
                "una etiqueta del listado se descarga y devuelve el digest del miembro que gano",
            )
            .passed()
        },
    );
}

#[test]
fn ac4_an_upstream_that_is_down_does_not_serve_a_partial_listing() {
    a_case(
        "ac4-an-upstream-that-is-down-does-not-serve-a-partial-listing",
        "un upstream caido no sirve el listado a medias",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The first member is a proxy whose upstream is not there: the
            // second member has a tag.
            let lab = TheLab::new(&[DOWN, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            // The hosted member has its tag: if the listing were served
            // halfway, it would appear alone.
            publish_as(
                &lab.deployment,
                port,
                FIRST,
                "solo-en-el-hosted",
                b"la-capa-del-primer-miembro",
            );

            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                503,
                "un upstream caido no detiene el listado:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                !body.contains("solo-en-el-hosted"),
                "el listado se sirvio a medias: {body}"
            );
            assert!(
                !tags.contains(&"solo-en-el-hosted".to_string()),
                "el listado se sirvio a medias: {tags:?}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac4-an-upstream-that-is-down-does-not-serve-a-partial-listing",
                "un upstream caido no sirve el listado a medias",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC5: the tag listing of a **direct** proxy.
// ---------------------------------------------------------------------------

#[test]
fn ac5_the_listing_of_a_direct_proxy_has_the_tags_of_the_upstream() {
    a_case(
        "ac5-the-listing-of-a-direct-proxy-has-the-tags-of-the-upstream",
        "el listado de un proxy directo trae las etiquetas de su upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // The upstream has a tag that the catalog does not have: a
            // proxy does not store tag rows.
            lab.publish_upstream_tag("solo-en-el-upstream", b"la-capa-del-upstream");

            // The **direct** proxy's listing: the same path as the group's,
            // but without a group in front.
            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY);
            assert_eq!(
                status,
                200,
                "el proxy directo no lista sus etiquetas:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                tags.contains(&"solo-en-el-upstream".to_string()),
                "la etiqueta que solo tiene el upstream no esta en la lista: {tags:?}"
            );

            // And the upstream is queried with the **proxy's** name, not with
            // the group's.
            assert!(
                lab.registry.hits(&format!("{PROXY}/tags/list")) > 0,
                "el upstream no se consulto por su lista: {:?}",
                lab.registry.paths()
            );
            assert_eq!(
                lab.registry.hits(&format!("{PROXY_GROUP}/tags/list")),
                0,
                "el upstream se consulto con el nombre del grupo: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac5-the-listing-of-a-direct-proxy-has-the-tags-of-the-upstream",
                "el listado de un proxy directo trae las etiquetas de su upstream",
            )
            .passed()
        },
    );
}

#[test]
fn ac5_a_tag_of_the_listing_can_be_pulled_from_the_direct_proxy() {
    a_case(
        "ac5-a-tag-of-the-listing-can-be-pulled-from-the-direct-proxy",
        "una etiqueta del listado de un proxy directo se descarga de su upstream",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            let proxied = lab.publish_upstream_tag("solo-en-el-upstream", b"la-capa-del-upstream");

            // The listing brings the tag.
            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY);
            assert_eq!(
                status, 200,
                "el proxy directo no lista sus etiquetas:\n{body}"
            );
            assert!(
                tags.contains(&"solo-en-el-upstream".to_string()),
                "la etiqueta que solo tiene el upstream no esta en la lista: {tags:?}"
            );

            // And what the listing says can be downloaded: the listing and the
            // resolution tell the same story.
            let answer = the_manifest_of(&lab.deployment, port, PROXY, "solo-en-el-upstream");
            assert_eq!(
                answer.status,
                200,
                "la etiqueta del listado no se descarga:\n{}\n{}",
                lab.deployment.the_log(),
                String::from_utf8_lossy(&answer.body)
            );
            assert_eq!(answer.body, proxied, "el manifiesto no es el del upstream");
            assert_eq!(
                answer.content_digest.as_deref(),
                Some(format!("sha256:{}", the_digest(&proxied)).as_str()),
                "la respuesta no dice el digest del contenido servido"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac5-a-tag-of-the-listing-can-be-pulled-from-the-direct-proxy",
                "una etiqueta del listado de un proxy directo se descarga de su upstream",
            )
            .passed()
        },
    );
}

#[test]
fn ac5_an_upstream_that_is_down_does_not_serve_an_empty_listing() {
    a_case(
        "ac5-an-upstream-that-is-down-does-not-serve-an-empty-listing",
        "un upstream caido no hace que un proxy directo sirva una lista vacia",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // The down proxy's upstream does not answer: the listing is **not**
            // served partially. An empty list would look complete, and the
            // client would believe that the upstream has nothing.
            let (status, tags, body) = the_tags(&lab.deployment, port, DOWN);
            assert_eq!(
                status,
                503,
                "un upstream caido no detiene el listado:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                !body.contains("\"tags\""),
                "el listado se sirvio como una lista vacia: {body}"
            );
            assert!(tags.is_empty(), "el listado se sirvio a medias: {tags:?}");

            support::stop(&mut service);
            LabCase::new(
                "ac5-an-upstream-that-is-down-does-not-serve-an-empty-listing",
                "un upstream caido no hace que un proxy directo sirva una lista vacia",
            )
            .passed()
        },
    );
}

#[test]
fn ac5_a_proxy_whose_scope_does_not_cover_the_listing_serves_an_empty_list() {
    a_case(
        "ac5-a-proxy-whose-scope-does-not-cover-the-listing-serves-an-empty-list",
        "un proxy cuyo ambito no cubre el listado sirve la lista vacia",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // This proxy's scope does not cover `v2/<name>/tags/list`: the
            // repository **cannot enumerate**. In a direct proxy there are
            // no "others" to list, so the empty list is the
            // only honest answer.
            let (status, tags, body) = the_tags(&lab.deployment, port, SCOPED);
            assert_eq!(
                status,
                200,
                "un proxy que no puede enumerar no deberia fallar:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                tags.is_empty(),
                "un proxy que no puede enumerar no deberia listar etiquetas: {tags:?}"
            );

            // And what does not fall in the scope is not asked of the upstream.
            assert_eq!(
                lab.registry.hits(&format!("{SCOPED}/tags/list")),
                0,
                "se pidio al upstream una ruta fuera de su ambito: {:?}",
                lab.registry.paths()
            );

            support::stop(&mut service);
            LabCase::new(
                "ac5-a-proxy-whose-scope-does-not-cover-the-listing-serves-an-empty-list",
                "un proxy cuyo ambito no cubre el listado sirve la lista vacia",
            )
            .passed()
        },
    );
}

// ---------------------------------------------------------------------------
// AC6: the proxy's listing, honest and complete.
// ---------------------------------------------------------------------------

#[test]
fn ac6_the_listing_only_announces_the_tags_that_can_be_pulled() {
    a_case(
        "ac6-the-listing-only-announces-the-tags-that-can-be-pulled",
        "el listado solo anuncia las etiquetas que la resolucion acepta",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            let mut service = lab.start();
            let port = service.port;

            // The upstream has a good tag and one that is **not** a
            // tag: it carries a slash, and the resolution rejects it.
            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            lab.publish_upstream_tag("solo-en-el-proxy", b"la-capa-del-upstream");
            lab.registry.tag(PROXY, "con/barra");

            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                200,
                "el grupo no lista sus etiquetas:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                tags.contains(&"solo-en-el-proxy".to_string()),
                "la etiqueta que el upstream si tiene no esta en la lista: {tags:?}"
            );
            // The listing only promises what can be downloaded: a name
            // that the resolution rejects --with a slash-- is not announced.
            assert!(
                !tags.iter().any(|one| one.contains('/')),
                "el listado anuncia una etiqueta que la resolucion rechaza: {tags:?}"
            );
            assert!(
                !body.contains("con/barra"),
                "el listado anuncia una etiqueta que la resolucion rechaza: {body}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac6-the-listing-only-announces-the-tags-that-can-be-pulled",
                "el listado solo anuncia las etiquetas que la resolucion acepta",
            )
            .passed()
        },
    );
}

#[test]
fn ac6_a_member_whose_scope_does_not_cover_the_listing_is_not_listed() {
    a_case(
        "ac6-a-member-whose-scope-does-not-cover-the-listing-is-not-listed",
        "un miembro que no puede enumerar no aparece en el listado, y deja traza",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // The first member is a proxy whose **scope** does not cover the
            // listing: its list cannot be asked for.
            let lab = TheLab::new(&[SCOPED, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            // The second member has a tag: the group's listing
            // is **not served halfway**, it is served with what can
            // be enumerated.
            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            // And the upstream has something under the member's name that it cannot
            // enumerate: it is not announced, because it cannot be asked for.
            lab.registry.tag(SCOPED, "solo-en-el-scoped");

            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                200,
                "el grupo no lista sus etiquetas:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                tags.contains(&TAG.to_string()),
                "la etiqueta del miembro que si se puede enumerar no esta: {tags:?}"
            );
            assert!(
                !tags.contains(&"solo-en-el-scoped".to_string()),
                "un miembro que no puede enumerar no aporta etiquetas al listado: {tags:?}"
            );

            // What does not fall in the scope is not asked of the upstream.
            assert_eq!(
                lab.registry.hits(&format!("{SCOPED}/tags/list")),
                0,
                "se pidio al upstream una ruta fuera de su ambito: {:?}",
                lab.registry.paths()
            );

            // And the decision is **seen**: the service warns that a
            // member cannot enumerate and why.
            let log = lab.deployment.the_log();
            assert!(
                log.contains("cannot be enumerated"),
                "la decision no ha dejado traza:\n{log}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac6-a-member-whose-scope-does-not-cover-the-listing-is-not-listed",
                "un miembro que no puede enumerar no aparece en el listado, y deja traza",
            )
            .passed()
        },
    );
}

#[test]
fn ac6_a_paginated_upstream_does_not_serve_a_partial_listing() {
    a_case(
        "ac6-a-paginated-upstream-does-not-serve-a-partial-listing",
        "un upstream que pagina su lista no sirve un listado a medias",
        Scope::Product,
        Expectation::Succeeds,
        || {
            let lab = TheLab::new(&[FIRST, PROXY], None);
            // The upstream paginates its list: what it answers is the first
            // page, and it says so with the `Link` header.
            lab.registry.behave(TheBehaviour::Paginated);
            let mut service = lab.start();
            let port = service.port;

            publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");
            lab.publish_upstream_tag("solo-en-el-proxy", b"la-capa-del-upstream");

            // The listing is **not served**: a first page presented as
            // the whole list is the defect this closes.
            let (status, tags, body) = the_tags(&lab.deployment, port, PROXY_GROUP);
            assert_eq!(
                status,
                503,
                "una lista paginada no detiene el listado:\n{}\n{body}",
                lab.deployment.the_log()
            );
            assert!(
                !body.contains("solo-en-el-proxy"),
                "el listado se sirvio a medias: {body}"
            );
            assert!(
                !tags.contains(&"solo-en-el-proxy".to_string()),
                "el listado se sirvio a medias: {tags:?}"
            );

            support::stop(&mut service);
            LabCase::new(
                "ac6-a-paginated-upstream-does-not-serve-a-partial-listing",
                "un upstream que pagina su lista no sirve un listado a medias",
            )
            .passed()
        },
    );
}
