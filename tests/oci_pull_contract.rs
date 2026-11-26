//! El corte pull de OCI Distribution (HSP-100 / HAR-06), contra el wire.
//!
//! `hodei-hub::oci` ya lee manifiestos como **tipos**; esta suite prueba el
//! protocolo **de verdad**: un binario real, un catálogo real, un almacén
//! de bytes real y `curl` de verdad, sobre los endpoints `/v2` de la
//! distribución OCI.
//!
//! El corte es **pull**: `GET /v2/`, blobs y manifests por digest y por
//! tag, con negociación de tipos. El push no se anuncia todavia.
//!
//! Los fixtures (blobs, manifest, tag) se siembran en un **proceso
//! aparte**, como el resto del catálogo, contra el namespace Dgraph del
//! despliegue.

mod support;

use std::path::Path;

use hodei_catalog::CatalogWriter;
use hodei_hub::blob::BlobStore;
use support::{
    Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// El media type de un manifiesto OCI.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
/// El media type de un manifiesto Docker schema2.
const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
/// Un media type que este registro NO soporta en este corte (index).
const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// Un tipo que este registro **no** sirve: la firma de Notary.
///
/// El indice de OCI ya se sirve --lo publico HSP-102--, asi que el
/// ejemplo de "un tipo que no se sirve" tiene que ser uno que no se vaya
/// a servir nunca, y no el que antes lo era.
const UNSERVED_MEDIA_TYPE: &str = "application/vnd.cncf.notary.signature";

/// El seeder del catálogo (repositorios por cuenta).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// El localizador del tenant (base) y la ruta del CAS que el fixture de OCI usa.
const SEED_DB: &str = "HODEI_OCI_SEED_DB";
const SEED_CAS: &str = "HODEI_OCI_SEED_CAS";

/// El contenido de una capa de ejemplo.
fn a_layer(seed: u8) -> Vec<u8> {
    (0u16..2048)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// El contenido de la configuración de la imagen.
fn a_config() -> Vec<u8> {
    br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers"}}"#.to_vec()
}

/// El manifiesto que referencia la configuración y la capa.
///
/// El manifiesto es un blob mas: su digest es el de sus propios bytes,
/// y sus descriptores apuntan a los digests de la configuración y de la
/// capa. Los bytes que se sirven son EXACTAMENTE estos.
fn a_manifest(config_digest: &str, layer_digest: &str, media_type: &str) -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": media_type,
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

/// El digest de un contenido, sin el prefijo del algoritmo.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// La ruta del fixture de OCI, en su propio proceso.
///
/// Igual que el semillero del catálogo: el arnés le pasa la ruta de la
/// base y del CAS por el entorno, este la escribe y se acaba.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo de los tests de pull OCI"]
async fn the_oci_fixture_can_be_seeded() {
    let (Some(db_path), Some(cas)) = (std::env::var(SEED_DB).ok(), std::env::var(SEED_CAS).ok())
    else {
        return;
    };

    let store = std::sync::Arc::new(
        hodei_hub::blob::FileBlobStore::new(Path::new(&cas)).expect("the store"),
    );
    let probe = std::sync::Arc::new(hodei_artifacts_api::blob_probe::BlobStoreProbe::new(
        store.clone(),
    ));

    // `HODEI_OCI_SEED_DB` carries the locator of the deployment's graph
    // tenant: the fixture goes through the real Dgraph catalog store.
    let tenant = support::graph_profile::GraphTenant::from_locator(&db_path)
        .expect("the locator names a graph tenant");
    let catalog = tenant.catalog_store().await.with_probe(probe);

    // La configuración, la capa y el manifiesto OCI, como blobs del
    // almacén y como assets del repositorio de la primera cuenta.
    let config = a_config();
    let layer = a_layer(1);
    let manifest = a_manifest(&the_digest(&config), &the_digest(&layer), OCI_MANIFEST);

    // La etiqueta v1 nace con la publicacion del manifiesto: apunta a
    // las coordenadas de ese asset. La identidad del contenido sigue
    // siendo el digest del manifiesto; la etiqueta es la referencia
    // mutable que lo resuelve.
    // El registro tiene los dos manifiestos: el OCI y el Docker schema2.
    // Un cliente pide el que quiere por su digest, y el registro lo sirve
    // con el tipo de medio que el propio manifiesto declara.
    let manifest_digest = the_digest(&manifest);
    let docker_manifest = a_manifest(&the_digest(&config), &the_digest(&layer), DOCKER_MANIFEST);
    let docker_digest = the_digest(&docker_manifest);
    for (path, content, tags) in [
        (
            format!("blobs/{}", the_digest(&config)),
            &config,
            Vec::new(),
        ),
        (format!("blobs/{}", the_digest(&layer)), &layer, Vec::new()),
        (
            format!("manifests/{manifest_digest}"),
            &manifest,
            vec!["v1".to_string()],
        ),
        (
            format!("manifests/{docker_digest}"),
            &docker_manifest,
            Vec::new(),
        ),
    ] {
        let digest = store.put(content).await.expect("the blob is stored");
        let coordinates = hodei_catalog::PackageCoordinates {
            package_name: path.clone(),
            package_version: digest.value.clone(),
        };
        catalog
            .publish(
                FIRST_ACCOUNT,
                hodei_catalog::Publication {
                    repository: FIRST_REPOSITORY.to_string(),
                    asset_path: path.clone(),
                    coordinates: coordinates.clone(),
                    digest,
                    size_bytes: content.len() as u64,
                    tags: Vec::new(),
                    actor: "alice".to_string(),
                    correlation_id: "oci-fixture".to_string(),
                    overwrite: false,
                    precondition: None,
                    subject: None,
                },
            )
            .await
            .expect("the blob asset is published");
        // The tag is created the way a client push creates it: moved onto
        // the version that already exists.
        for tag in tags {
            catalog
                .move_tag(FIRST_ACCOUNT, FIRST_REPOSITORY, &tag, coordinates.clone())
                .await
                .expect("the tag is created");
        }
    }
}

/// Siembra el catálogo y los fixtures de OCI en procesos aparte.
fn a_oci_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();

    let binary = std::env::current_exe().expect("the test binary");
    let status = std::process::Command::new(binary)
        .args([
            "--exact",
            "the_oci_fixture_can_be_seeded",
            "--ignored",
            "--nocapture",
        ])
        .env(SEED_DB, &deployment.db)
        .env(SEED_CAS, &deployment.cas)
        .status()
        .expect("the fixture seeder runs");
    assert!(status.success(), "the OCI fixture could not be seeded");
    deployment
}

/// La ruta OCI de un recurso del repositorio de la primera cuenta.
fn the_oci_path(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
}

/// La referencia de un blob o un manifest por digest.
///
/// El digest lleva su algoritmo: `sha256:<hex>`. Es lo que dice el spec
/// y lo que envian los clientes reales.
fn the_digest_reference(digest: &str) -> String {
    format!("sha256:{digest}")
}

/// Descarga la respuesta a un fichero y la devuelve.
fn get_with_accept(
    _deployment: &Deployment,
    service_port: u16,
    token: &str,
    path: &str,
    accept: Option<&str>,
    into: &Path,
) -> support::Answer {
    let curl = if let Some(accept) = accept {
        support::Curl::with_header(service_port, path, token, ("accept", accept), into)
    } else {
        support::Curl::download(service_port, path, token, Some(into))
    };
    curl.run()
}

/// La cabecera de versión de la API de distribución.
#[test]
fn oci_version_check_answers_the_wire_ping() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        "/v2/",
        None,
        &deployment.directory.path().join("v2.bin"),
    );
    assert_eq!(
        answer.status,
        200,
        "GET /v2/ is the wire ping of the distribution:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-distribution-api-version"),
        Some("registry/2.0".to_string()),
        "the version check does not announce the registry API version"
    );

    support::stop(&mut service);
}

/// Un blob referenciado por el repositorio se sirve por su digest, con
/// sus cabeceras OCI; el HEAD lo describe sin traerlo.
#[test]
fn a_referenced_blob_is_served_by_digest_with_oci_headers() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let layer = a_layer(1);
    let digest = the_digest(&layer);
    let into = deployment.directory.path().join("layer.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("blobs", &the_digest_reference(&digest)),
        None,
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "the referenced blob is not served: {}\n{}",
        std::fs::read_to_string(&into).unwrap_or_default(),
        deployment.the_log()
    );
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        layer,
        "the bytes that came back are not the bytes of the blob"
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the response does not name the digest of the content"
    );
    assert_eq!(
        answer.header("content-length"),
        Some((layer.len()).to_string()),
        "the response does not say the length of the blob"
    );

    // Y el HEAD lo describe sin traerlo.
    let described = support::Curl::describe(
        service.port,
        &the_oci_path("blobs", &the_digest_reference(&digest)),
        &token,
    );
    let head = described.run();
    assert_eq!(head.status, 200, "the HEAD of a served blob must answer");
    assert_eq!(head.size, 0, "the HEAD brought a body");
    assert_eq!(
        head.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the HEAD does not name the digest"
    );

    support::stop(&mut service);
}

/// El manifiesto se sirve por su digest, con su tipo de medio.
#[test]
fn a_manifest_is_served_by_digest_with_its_media_type() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let config_digest = the_digest(&a_config());
    let layer_digest = the_digest(&a_layer(1));
    let manifest = a_manifest(&config_digest, &layer_digest, OCI_MANIFEST);
    let digest = the_digest(&manifest);

    let into = deployment.directory.path().join("manifest.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", &the_digest_reference(&digest)),
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "the manifest is not served by digest: {}\n{}",
        std::fs::read_to_string(&into).unwrap_or_default(),
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
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the manifest response does not name its own digest"
    );

    support::stop(&mut service);
}

/// La etiqueta resuelve al contenido sin convertirse en él.
///
/// El oráculo de AC#2: servir por tag devuelve EXACTAMENTE los mismos
/// bytes y el mismo digest que servir por el digest del manifiesto. La
/// etiqueta es la referencia mutable; la identidad del contenido es el
/// digest, y la respuesta lo dice.
#[test]
fn a_tag_resolves_to_the_manifest_and_the_answer_says_the_digest() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let config_digest = the_digest(&a_config());
    let layer_digest = the_digest(&a_layer(1));
    let manifest = a_manifest(&config_digest, &layer_digest, OCI_MANIFEST);
    let digest = the_digest(&manifest);

    let into = deployment.directory.path().join("by-tag.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", "v1"),
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "the tag does not resolve:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        std::fs::read(&into).expect("the bytes are read"),
        manifest,
        "the tag resolved to other bytes"
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{digest}")),
        "the answer for a tag does not say the digest of the manifest it resolved to"
    );

    support::stop(&mut service);
}

/// La negociación de tipos es explícita: OCI y Docker schema2 se sirven;
/// un cliente que solo acepta tipos no soportados recibe el error OCI.
#[test]
fn the_accept_negotiation_is_explicit_and_unsupported_types_fail_with_oci_errors() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Docker schema2 también se sirve (la negociación lo nombra).
    let docker_manifest = a_manifest(
        &the_digest(&a_config()),
        &the_digest(&a_layer(1)),
        DOCKER_MANIFEST,
    );
    let docker_digest = the_digest(&docker_manifest);
    let into = deployment.directory.path().join("docker-manifest.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", &the_digest_reference(&docker_digest)),
        Some(DOCKER_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "a Docker schema2 manifest is not served:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("content-type"),
        Some(DOCKER_MANIFEST.to_string()),
        "the Docker manifest is not served with its media type"
    );

    // Y un cliente que SOLO acepta un tipo no soportado recibe el error
    // OCI de manifiesto desconocido, no un 200 engañoso.
    let refused = deployment.directory.path().join("refused.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", &the_digest_reference(&docker_digest)),
        Some(INDEX_MEDIA_TYPE),
        &refused,
    );
    assert_eq!(
        answer.status, 404,
        "a client that only accepts unsupported media got a response"
    );
    let body = std::fs::read(&refused).expect("the error body is read");
    let body = String::from_utf8_lossy(&body).into_owned();
    assert!(
        body.contains("MANIFEST_UNKNOWN"),
        "the error is not the OCI code for an unknown manifest: {body}"
    );

    support::stop(&mut service);
}

/// Los fallos hablan el dialecto OCI: envelope con code, y sin filtrar
/// la existencia de lo ajeno.
#[test]
fn the_errors_speak_oci_and_do_not_leak_the_other_account() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");

    // Blob desconocido: 404 con el código BLOB_UNKNOWN.
    let into = deployment.directory.path().join("unknown-blob.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &first,
        &the_oci_path("blobs", &the_digest_reference(&"0".repeat(64))),
        None,
        &into,
    );
    assert_eq!(answer.status, 404);
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("BLOB_UNKNOWN"),
        "an unknown blob does not speak OCI: {body}"
    );

    // Manifest desconocido: 404 con MANIFEST_UNKNOWN.
    let into = deployment.directory.path().join("unknown-manifest.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &first,
        &the_oci_path("manifests", &the_digest_reference(&"1".repeat(64))),
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(answer.status, 404);
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("MANIFEST_UNKNOWN"),
        "an unknown manifest does not speak OCI: {body}"
    );

    // Repositorio que la cuenta no tiene: 404 NAME_UNKNOWN, sin decir
    // si existe en otra cuenta.
    let into = deployment.directory.path().join("other-repo.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &second,
        &format!("/v2/{FIRST_ONLY_REPOSITORY}/manifests/v1"),
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status, 404,
        "the other account saw a repository it does not have"
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("NAME_UNKNOWN"),
        "a foreign repository does not speak OCI: {body}"
    );
    assert!(
        !body.contains(FIRST_ACCOUNT),
        "the error leaks the account that owns the repository: {body}"
    );

    support::stop(&mut service);
}

/// Los paths malformados se rechazan con códigos OCI, no con un 500 ni
/// con un 404 de router que confunde la ausencia con el error.
#[test]
fn malformed_names_and_digests_get_oci_client_errors() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Un nombre con traversal: 400 NAME_INVALID.
    let into = deployment.directory.path().join("bad-name.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        "/v2/..%2Fescape/manifests/v1",
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status, 400,
        "a traversal name was not refused as a client error"
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("NAME_INVALID"),
        "a malformed name does not speak OCI: {body}"
    );

    // Un digest mal formado: 400 DIGEST_INVALID.
    let into = deployment.directory.path().join("bad-digest.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("blobs", "not-a-digest"),
        None,
        &into,
    );
    assert_eq!(
        answer.status, 400,
        "a malformed digest was not refused as a client error"
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("DIGEST_INVALID"),
        "a malformed digest does not speak OCI: {body}"
    );

    support::stop(&mut service);
}

/// Un blob que esta en el almacén de bytes pero que el repositorio no
/// referencia NO se sirve.
///
/// AC#4: la referencia del catálogo es la que autoriza el servir. Los
/// bytes huerfanos del CAS no son contenido del repositorio.
#[tokio::test]
async fn a_blob_that_the_repository_does_not_reference_is_not_served() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Bytes **reales** en el CAS (el almacen los verifica al leer, asi que
    // un fichero a mano con el nombre del digest serviria de poco), pero
    // sin fila en el catálogo del repositorio que los referencia.
    let orphan = b"orphan bytes that no manifest references".to_vec();
    let orphan_digest = the_digest(&orphan);
    let store = hodei_hub::blob::FileBlobStore::new(Path::new(&deployment.cas)).expect("the store");
    store
        .put(&orphan)
        .await
        .expect("the orphan bytes are stored");

    let into = deployment.directory.path().join("orphan.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("blobs", &the_digest_reference(&orphan_digest)),
        None,
        &into,
    );
    assert_eq!(
        answer.status, 404,
        "an unreferenced blob was served as content of the repository"
    );
    let body = String::from_utf8_lossy(&std::fs::read(&into).expect("the body")).into_owned();
    assert!(
        body.contains("BLOB_UNKNOWN"),
        "an unreferenced blob does not speak OCI: {body}"
    );
    assert_ne!(
        answer.size,
        orphan.len() as u64,
        "the orphan bytes leaked in the response"
    );

    support::stop(&mut service);
}

/// Una fila que solo NOMBRA la ruta no autoriza a servir ese digest.
///
/// El CAS es un arbol comun: los bytes de una cuenta estan ahi, junto a
/// los de otra. Si el pull sirviera los bytes del digest **pedido** en
/// vez de los que la fila **referencia**, una fila que nombra una ruta
/// serviria el contenido de otro blob, que puede ser de otra cuenta.
///
/// Es el reverso del caso del blob huerfano: aqui la fila existe y el
/// contenido es de otro.
#[tokio::test]
async fn a_blob_that_the_repository_only_names_by_path_is_not_served() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let acme = support::token(FIRST_ACCOUNT, "alice");
    let other = support::token(SECOND_ACCOUNT, "bob");

    // El contenido de otra cuenta: vive en el CAS global.
    let secret = b"the bytes of another account".to_vec();
    let secret_digest = the_digest(&secret);
    let secret_file = deployment.directory.path().join("secret.bin");
    std::fs::write(&secret_file, &secret).expect("the secret is written");
    let published = support::Curl::upload(
        service.port,
        &format!("/repository/{SECOND_REPOSITORY}/secret"),
        &other,
        &secret_file,
        None,
    )
    .run();
    assert_eq!(published.status, 201, "the other account could not publish");

    // La cuenta que lo pide solo NOMBRA la ruta; la fila apunta a otro
    // contenido. Mismo tamano a proposito: la comprobacion de tamano no
    // debe tapar el fallo.
    let filler = vec![b'x'; secret.len()];
    let filler_file = deployment.directory.path().join("filler.bin");
    std::fs::write(&filler_file, &filler).expect("the filler is written");
    let named = support::Curl::upload(
        service.port,
        &format!("/repository/{FIRST_REPOSITORY}/blobs/{secret_digest}"),
        &acme,
        &filler_file,
        None,
    )
    .run();
    assert_eq!(named.status, 201, "the naming row could not be published");

    // El pull por ese digest NO devuelve los bytes ajenos.
    let into = deployment.directory.path().join("stolen.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &acme,
        &the_oci_path("blobs", &the_digest_reference(&secret_digest)),
        None,
        &into,
    );
    assert_eq!(
        answer.status, 404,
        "a blob the repository does not reference was served"
    );
    assert_ne!(
        std::fs::read_to_string(&into).unwrap_or_default(),
        String::from_utf8_lossy(&secret),
        "the pull returned the bytes of another account"
    );
    let body = std::fs::read_to_string(&into).unwrap_or_default();
    assert!(
        body.contains("BLOB_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    support::stop(&mut service);
}

/// Un manifiesto que declara un tipo que este registro NO sirve recibe
/// error OCI, no un 200 con otro tipo.
///
/// Servirlo con un tipo distinto es un 200 que finge ser lo que el
/// cliente pidio, y el cliente que lo pidio no lo recibio.
#[tokio::test]
async fn a_manifest_that_declares_an_unsupported_media_type_is_not_served_as_another_type() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let unserved = a_manifest(
        &the_digest(&a_config()),
        &the_digest(&a_layer(1)),
        UNSERVED_MEDIA_TYPE,
    );
    let digest = the_digest(&unserved);
    let file = deployment.directory.path().join("unserved.bin");
    std::fs::write(&file, &unserved).expect("the manifest is written");
    let published = support::Curl::upload(
        service.port,
        &format!("/repository/{FIRST_REPOSITORY}/manifests/{digest}"),
        &token,
        &file,
        None,
    )
    .run();
    assert_eq!(published.status, 201, "the index could not be published");

    // Y con un `Accept` que no lo limita, el registro lo rechaza igual:
    // el tipo que declara el manifiesto es el que decide.
    let into = deployment.directory.path().join("unserved-out.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", &the_digest_reference(&digest)),
        Some("*/*"),
        &into,
    );
    assert_eq!(
        answer.status,
        404,
        "an unsupported media type was served: {} body={}",
        answer.header("content-type").unwrap_or_default(),
        std::fs::read_to_string(&into).unwrap_or_default()
    );
    let body = std::fs::read_to_string(&into).unwrap_or_default();
    assert!(
        body.contains("MANIFEST_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    support::stop(&mut service);
}

/// La negociacion limita el tipo que se sirve.
///
/// Un cliente que solo acepta el tipo OCI no recibe un manifiesto Docker
/// schema2 con un 200: recibe el error OCI. Servirle otro tipo seria
/// responder con algo que no pidio.
#[tokio::test]
async fn a_client_that_only_accepts_one_type_is_not_served_another() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // El manifiesto Docker schema2 esta sembrado en el fixture.
    let docker_manifest = a_manifest(
        &the_digest(&a_config()),
        &the_digest(&a_layer(1)),
        DOCKER_MANIFEST,
    );
    let docker_digest = the_digest(&docker_manifest);

    let into = deployment.directory.path().join("wrong-type.bin");
    let answer = get_with_accept(
        &deployment,
        service.port,
        &token,
        &the_oci_path("manifests", &the_digest_reference(&docker_digest)),
        Some(OCI_MANIFEST),
        &into,
    );
    assert_eq!(
        answer.status, 404,
        "a client that only accepts the OCI type was served the Docker one"
    );
    let body = std::fs::read_to_string(&into).unwrap_or_default();
    assert!(
        body.contains("MANIFEST_UNKNOWN"),
        "the error does not speak OCI: {body}"
    );

    support::stop(&mut service);
}
