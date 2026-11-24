//! HSP-229: el grupo OCI resuelve la referencia entre sus miembros.
//!
//! Lo que se demuestra aqui, contra el **service de verdad**:
//!
//! - la precedencia del grupo es la de su configuracion: gana el primer
//!   miembro que tiene la referencia, y la respuesta dice de que miembro
//!   salio (`x-hodei-resolved-from`) y en que version de la configuracion
//!   se resolvio (`x-hodei-group-version`);
//! - un manifiesto se sirve **entero** de un solo miembro, y sus blobs van
//!   direccionados por su digest;
//! - un MISS (un miembro sin la etiqueta) sigue al siguiente miembro, y un
//!   ERROR (la confianza oculta el digest del ganador) **detiene** la
//!   resolucion: el grupo no salta al siguiente;
//! - el `HEAD` recorre exactamente la misma resolucion que el `GET`;
//! - un miembro que el principal no puede leer se omite sin decir que
//!   existe, y un repositorio de otra cuenta es `NAME_UNKNOWN`;
//! - un grupo **no guarda nada**: la escritura contra el se rechaza con el
//!   `UNSUPPORTED` del dialecto.
//!
//! Y, con un miembro **proxy** delante --el incremento 2--:
//!
//! - el proxy se consulta con el **nombre del miembro**, no con el del
//!   grupo, y lo que baja se publica en el namespace del miembro;
//! - un upstream **caido** detiene la resolucion: el grupo no salta al
//!   siguiente miembro, porque un fallo transitorio no puede cambiar en
//!   silencio lo que se descarga;
//! - un upstream que responde `404` es un MISS, y el siguiente miembro gana;
//! - la reserva de namespaces se juzga por el **nombre del repositorio**, y
//!   un miembro proxy cuyo nombre cae en la reserva no se consulta;
//! - un upstream que pide el token en un realm que no es el declarado no
//!   recibe la credencial, ni dentro de un grupo;
//! - un cliente OCI **real** (`skopeo`) saca la imagen del grupo.

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

/// El primer miembro del grupo.
const FIRST: &str = "oci-first";

/// El segundo miembro del grupo.
const SECOND: &str = "oci-second";

/// El grupo.
const GROUP: &str = "oci-group";

/// La etiqueta que se pide.
const TAG: &str = "v1";

/// El tipo de medio del manifiesto de imagen.
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

/// Los casos que una ejecucion completa tiene que ejecutar.
///
/// Se declaran **antes** de correr: una ejecucion que solo llego a algunos
/// dice cuales no corrieron, y un informe con un caso que no corrio nunca
/// es verde.
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
];

/// El seeder del catálogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El digest de unos bytes.
fn the_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// La configuracion de una imagen, con su capa.
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

/// El despliegue con el grupo y sus miembros sembrados.
///
/// `trusted` son los miembros que se siembran en el canal **trusted**: su
/// compuerta de confianza no deja pasar lo que no tiene concesion, y eso
/// es lo que se mide.
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

/// Publica una imagen en un repositorio OCI, por el protocolo.
///
/// Los blobs primero y el manifiesto despues: es el orden del protocolo, y
/// el manifiesto se publica con la etiqueta que lo nombra.
fn publish(deployment: &Deployment, port: u16, repository: &str, layer: &[u8]) -> Vec<u8> {
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
        &format!("/v2/{repository}/manifests/{TAG}"),
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

/// La respuesta de una lectura OCI.
struct Reply {
    /// El codigo de estado.
    status: u16,
    /// Los bytes del cuerpo.
    body: Vec<u8>,
    /// El digest que dice la respuesta.
    content_digest: Option<String>,
    /// Que miembro sirvio el contenido.
    resolved_from: Option<String>,
    /// La version de la configuracion del grupo.
    group_version: Option<String>,
}

/// Una lectura del grupo por la etiqueta.
fn the_manifest(deployment: &Deployment, port: u16, repository: &str) -> Reply {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("answer.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::download(
        port,
        &format!("/v2/{repository}/manifests/{TAG}"),
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

/// Una lectura de un blob por su digest.
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

/// La respuesta de un `HEAD` del grupo por la etiqueta.
fn the_head(port: u16, repository: &str) -> support::Answer {
    let token = support::token(FIRST_ACCOUNT, "alice");
    Curl::describe(port, &format!("/v2/{repository}/manifests/{TAG}"), &token).run()
}

/// Corre un caso del laboratorio.
///
/// El caso se **inscribe antes de correr**, con `NOT_RUN`, y se reemplaza
/// cuando acaba: un caso que se muere a mitad deja un `NOT_RUN` en el
/// informe y no un `PASS` que nadie midio.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    // El informe se describe antes de que corra ningun caso: el titulo y
    // el descargo son de la suite, y un caso que se inscribe antes de que
    // la suite se describa escribiria un informe que no es de esta suite.
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
// AC1: la precedencia, la integridad y la procedencia.
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

            // Los dos miembros tienen la misma etiqueta y distinto digest:
            // gana el primero en el orden configurado.
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

            // Los blobs van direccionados por su digest, y son los del
            // ganador.
            let blob = the_blob(&deployment, port, GROUP, &the_digest(&first_layer));
            assert_eq!(blob.status, 200, "el blob del ganador no se sirve");
            assert_eq!(blob.body, first_layer, "el blob no es el del ganador");
            assert_eq!(blob.resolved_from.as_deref(), Some(FIRST));

            // El blob del segundo miembro tambien se sirve: el grupo no
            // fusiona, y lo que hay en un miembro sigue siendo alcanzable
            // por su digest.
            let other = the_blob(&deployment, port, GROUP, &the_digest(&second_layer));
            assert_eq!(other.status, 200, "el blob del segundo miembro no se sirve");
            assert_eq!(other.body, second_layer);

            // La lista de etiquetas del grupo es la union de las de sus
            // miembros: una entrada por nombre, y no se sirve a medias.
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

            // Y la ruta directa de cada miembro no cambia: el grupo no le
            // da acceso a nada que su propia ruta no tenga.
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

            // Solo el segundo miembro publica la etiqueta: el primero dice
            // con certeza que no la tiene, y la resolucion sigue.
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
// AC2: el HEAD recorre la misma resolucion.
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
// AC3: identidad y permisos.
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

            // El repositorio de la otra cuenta existe, pero no para esta:
            // la respuesta no dice si existe en el otro sitio.
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
            // alice puede leer el grupo y el segundo miembro, pero no el
            // primero.
            deployment.write_policy(&the_policy(&[GROUP, SECOND]));
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let second_layer = b"la-capa-del-segundo-miembro".to_vec();
            publish(&deployment, port, FIRST, &first_layer);
            let second_manifest = publish(&deployment, port, SECOND, &second_layer);

            // El primer miembro tiene la etiqueta, pero alice no puede
            // leerlo: se omite sin decir que existe, y gana el segundo.
            let answer = the_manifest(&deployment, port, GROUP);
            assert_eq!(answer.status, 200, "el grupo no resuelve la etiqueta");
            assert_eq!(answer.body, second_manifest);
            assert_eq!(
                answer.resolved_from.as_deref(),
                Some(SECOND),
                "gana el miembro que el principal puede leer"
            );

            // Y la ruta directa del miembro que no puede leer sigue cerrada.
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

/// La politica de Cedar de la suite: alice puede leer lo que se nombra.
///
/// El principal verificado es `hrn:hodei:iam::<cuenta>:User/<usuario>`: la
/// region vacia se escribe con sus dos puntos, que es como el token lo
/// declara.
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
// R6: la confianza del ganador detiene la resolucion.
// ---------------------------------------------------------------------------

#[test]
fn r6_a_trust_gate_that_hides_the_winner_does_not_make_the_group_jump() {
    a_case(
        "r6-the-trust-gate-stops-the-resolution",
        "la confianza que oculta al ganador detiene la resolucion",
        Scope::Product,
        Expectation::Succeeds,
        || {
            // El primer miembro es el canal **trusted**: su compuerta de
            // confianza no deja pasar lo que no tiene concesion.
            let deployment = the_deployment(&[FIRST]);
            let mut service = deployment.start(&[]);
            let port = service.port;

            let first_layer = b"la-capa-del-primer-miembro".to_vec();
            let second_layer = b"la-capa-del-segundo-miembro".to_vec();
            publish(&deployment, port, FIRST, &first_layer);
            publish(&deployment, port, SECOND, &second_layer);

            // El ganador (el primero) lo oculta la confianza: el grupo
            // contesta que no existe y NO baja al segundo miembro, que es
            // otro contenido.
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
// R10: un grupo no guarda nada.
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

            // El inicio de una subida contra el grupo.
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

            // Y el manifiesto tampoco.
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
// El miembro proxy: el upstream de distribucion del laboratorio.
// ---------------------------------------------------------------------------

/// El nombre del miembro proxy.
const PROXY: &str = "oci-proxy";

/// El grupo que tiene un miembro proxy.
const PROXY_GROUP: &str = "oci-proxy-group";

/// El miembro proxy cuyo upstream esta caido.
const DOWN: &str = "oci-proxy-down";

/// La credencial que el upstream exige en su realm.
const CREDENTIAL: &str = "secreto-del-laboratorio";

/// El token que el upstream emite.
const TOKEN: &str = "token-del-laboratorio";

/// La variable de entorno que lleva la credencial del upstream.
const CREDENTIAL_VARIABLE: &str = "LAB_OCI_CREDENTIAL";

/// La cabecera que dice el digest del contenido servido.
const DOCKER_CONTENT_DIGEST: HeaderName = HeaderName::from_static("docker-content-digest");

/// Como se comporta el upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheBehaviour {
    /// Pide token y sirve lo que tiene.
    Normal,
    /// Su reto apunta a un realm que **no** es el declarado.
    HostileRealm,
}

/// Lo que el upstream ha recibido.
#[derive(Debug, Clone)]
struct Seen {
    /// El camino pedido.
    path: String,
    /// La cabecera `Authorization`, si la hubo.
    authorization: Option<String>,
}

/// El estado del upstream.
struct TheWorld {
    /// Los manifiestos, por referencia.
    manifests: Mutex<HashMap<String, Vec<u8>>>,
    /// Los blobs, por digest hexadecimal.
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    /// Lo que el upstream ha recibido.
    seen: Mutex<Vec<Seen>>,
    /// Como se comporta.
    behaviour: Mutex<TheBehaviour>,
    /// El origen al que apunta el reto hostil.
    elsewhere: Mutex<String>,
    /// El origen de este upstream, para el reto.
    origin: Mutex<String>,
}

/// El upstream de distribucion del laboratorio.
///
/// Pide un token en su realm, lo emite a quien presenta la credencial, y
/// sirve manifiestos y blobs. Es el mismo patron que el de HSP-228: aqui
/// lo que se mide es el camino del **grupo** con ese upstream delante.
struct TheRegistry {
    /// El puerto donde escucha.
    port: u16,
    /// El estado que comparte.
    world: Arc<TheWorld>,
}

impl TheRegistry {
    /// Arranca un upstream vacio.
    fn start() -> Self {
        let world = Arc::new(TheWorld {
            manifests: Mutex::new(HashMap::new()),
            blobs: Mutex::new(HashMap::new()),
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

    /// El puerto donde escucha.
    fn port(&self) -> u16 {
        self.port
    }

    /// El origen del upstream.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// La base del upstream, con barra final.
    fn base(&self) -> String {
        format!("{}/", self.origin())
    }

    /// Publica un manifiesto, bajo una referencia.
    fn publish(&self, reference: &str, bytes: Vec<u8>) {
        self.world
            .manifests
            .lock()
            .expect("the manifests")
            .insert(reference.to_string(), bytes);
    }

    /// Publica un blob.
    fn publish_blob(&self, digest: &str, bytes: Vec<u8>) {
        self.world
            .blobs
            .lock()
            .expect("the blobs")
            .insert(digest.to_string(), bytes);
    }

    /// Cambia el comportamiento del upstream.
    fn behave(&self, behaviour: TheBehaviour) {
        *self.world.behaviour.lock().expect("the behaviour") = behaviour;
    }

    /// A donde apunta el reto hostil.
    fn elsewhere(&self, origin: &str) {
        *self.world.elsewhere.lock().expect("the elsewhere") = origin.to_string();
    }

    /// Los caminos que le han pedido.
    fn paths(&self) -> Vec<String> {
        self.world
            .seen
            .lock()
            .expect("the log")
            .iter()
            .map(|one| one.path.clone())
            .collect()
    }

    /// Cuantas veces le han pedido un camino.
    ///
    /// El camino es el que va **despues** de `/v2/`: el upstream se pide por
    /// `v2/<nombre>/manifests/<referencia>`, y lo que se registra es lo que
    /// viene de la ruta.
    fn hits(&self, path: &str) -> usize {
        self.paths().iter().filter(|one| *one == path).count()
    }
}

/// El origen hostil: solo registra lo que le llega.
///
/// Es el sitio al que un upstream puede mandar el reto del token cuando no
/// es el declarado: la credencial no puede viajar ahi.
struct TheHostile {
    /// El puerto donde escucha.
    port: u16,
    /// Lo que ha recibido.
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TheHostile {
    /// Arranca un origen que solo registra lo que le llega.
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

    /// El origen del hostil.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Lo que el hostil ha recibido.
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log").clone()
    }
}

/// Arranca un servidor con el router del upstream y devuelve el puerto.
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

/// El endpoint de token del upstream.
///
/// Solo entrega el token a quien presenta la credencial.
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

/// La superficie de la distribucion: manifiestos y blobs.
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

    // El reto: el upstream pide un token en su realm. El upstream hostil lo
    // pide en el realm de **otro**, que es lo que un upstream puede hacer.
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

    // `v2/<nombre>/manifests/<referencia>` y `v2/<nombre>/blobs/<digest>`.
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

/// Un texto en base64.
fn base64_of(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// Un puerto donde no escucha nadie: el upstream caido.
fn a_closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("the port binds");
    let port = listener.local_addr().expect("the address").port();
    drop(listener);
    port
}

/// La configuracion de un repositorio proxy, con su upstream.
///
/// La base es la del upstream y el realm del token: los dos son el mismo
/// origen, porque un upstream que pide el token en otro sitio no es el
/// upstream declarado.
fn the_proxy(base: &str) -> serde_json::Value {
    json!({
        "upstream": {
            "id": "lab",
            "url": base,
            "scopes": ["v2/"],
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

/// El laboratorio con el upstream de distribucion delante.
struct TheLab {
    /// El despliegue con el catalogo sembrado.
    deployment: Deployment,
    /// El upstream de distribucion.
    registry: TheRegistry,
    /// El origen hostil.
    hostile: TheHostile,
    /// El puerto donde no escucha nadie: el upstream caido.
    down_port: u16,
}

impl TheLab {
    /// El puerto del origen hostil.
    fn hostile_port(&self) -> u16 {
        self.hostile.port
    }

    /// Un despliegue con los miembros sembrados y el upstream delante.
    ///
    /// Los miembros del grupo son los que se nombran; el resto esta
    /// sembrado tambien, para que una suite pueda reordenar el grupo sin
    /// volver a sembrar.
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

    /// El entorno del servicio: la salida permitida y la credencial.
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

    /// Arranca el servicio con la salida permitida.
    fn start(&self) -> support::Service {
        let environment = self.the_environment();
        let borrowed: Vec<(&str, &str)> = environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        self.deployment.start(&borrowed)
    }

    /// Publica en el upstream una imagen, con su etiqueta.
    fn publish_upstream(&self, layer: &[u8]) -> Vec<u8> {
        let (manifest, config) = the_image(layer);
        self.registry.publish(TAG, manifest.clone());
        self.registry
            .publish_blob(&the_digest(&config), config.clone());
        self.registry
            .publish_blob(&the_digest(layer), layer.to_vec());
        manifest
    }
}

// ---------------------------------------------------------------------------
// AC3: el miembro proxy dentro del grupo.
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

            // Los tres miembros tienen la misma etiqueta y distinto digest.
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

            // Gana el primero en el orden configurado.
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
            // Y el proxy no se ha consultado: la precedencia es la del orden.
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

            // Los dos miembros tienen la misma etiqueta y distinto digest.
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

            // El upstream se consulta con el **nombre del miembro**, no con
            // el del grupo: el grupo no renombra a su miembro.
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

            // Y el blob del proxy se sirve por su digest, desde el miembro.
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
            // El primer miembro es un proxy cuyo upstream no esta: el
            // segundo miembro tiene la etiqueta.
            let lab = TheLab::new(&[DOWN, FIRST], None);
            let mut service = lab.start();
            let port = service.port;

            let hosted_manifest =
                publish(&lab.deployment, port, FIRST, b"la-capa-del-primer-miembro");

            // El upstream caido es un ERROR: la resolucion se detiene y se
            // contesta el error. No se salta al siguiente miembro, porque un
            // fallo transitorio no puede cambiar en silencio lo que se
            // descarga.
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

            // El upstream **no** tiene la etiqueta: lo dice con certeza, y
            // es un MISS. El segundo miembro la tiene.
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
            // La reserva del grupo reserva el nombre del miembro proxy: un
            // nombre reservado no sale de un upstream publico.
            let lab = TheLab::new(
                &[PROXY, FIRST],
                Some(json!({ "reserved": [PROXY], "on_conflict": "deny" })),
            );
            let mut service = lab.start();
            let port = service.port;

            // El upstream tiene la etiqueta, y el segundo miembro tambien.
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
            // El upstream no se ha consultado: el juicio de la reserva es
            // sobre el nombre del repositorio, y se hace antes.
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
            // El upstream pide el token en un realm que **no** es el
            // declarado: la credencial no puede viajar ahi.
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

            // El origen hostil no ha recibido la credencial.
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
            // El cliente real: `skopeo` primero, y `oras` si no esta.
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
// El cliente OCI real.
// ---------------------------------------------------------------------------

/// Lo que un cliente OCI real contesta.
struct TheClientAnswer {
    /// Si la peticion salio bien.
    ok: bool,
    /// Lo que el cliente dijo.
    detail: String,
    /// Lo que el cliente devolvio.
    body: String,
}

/// Un cliente OCI real del entorno.
enum TheClient {
    /// `skopeo`.
    Skopeo,
    /// `oras`.
    Oras,
}

impl TheClient {
    /// El nombre del comando.
    fn the_command(&self) -> &'static str {
        match self {
            Self::Skopeo => "skopeo",
            Self::Oras => "oras",
        }
    }

    /// Le pide al cliente que mire el manifiesto del grupo.
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

/// El cliente OCI real del entorno, si hay alguno.
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
