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
//! El grupo es un grupo de **imagen** con miembros `hosted`: el camino del
//! miembro `proxy` dentro de un grupo OCI es el incremento siguiente.

mod support;

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
            "- **Ni un cliente real.** Lo que se ha ejecutado es el camino de la distribución contra el servicio de Hodei: no se ha hablado con `podman`, `skopeo` ni `oras`, y este informe no dice nada de ellos.",
            "- **Ni un upstream real.** Los miembros del grupo son `hosted`: no hay un upstream remoto en este incremento, y el camino del miembro `proxy` dentro de un grupo OCI sigue pendiente.",
            "- **Ni la superficie de push.** Solo se ha medido el pull, y el rechazo de la escritura contra el grupo.",
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
