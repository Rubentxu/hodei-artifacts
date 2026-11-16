//! El corte de consulta y adjuntos de OCI Distribution (HSP-102 / HAR-08),
//! contra el wire.
//!
//! El protocolo de la distribucion no es solo subir y bajar: tambien es
//! **preguntar**. Que etiquetas tiene el repositorio, que cuelga de este
//! manifiesto, y que imagenes multi-plataforma hay.
//!
//! Y las pruebas van contra el wire, con un proceso de verdad, un catalogo
//! real y un almacen de bytes real. El ultimo caso usa un cliente de
//! verdad --`oras`, el cliente de los artefactos-- que cuelga un SBOM de
//! un manifiesto y lo vuelve a encontrar: un protocolo que solo funciona
//! con el cliente que escribio el servidor no es un protocolo.
//!
//! Los fixtures (blobs, manifiestos, etiquetas) se siembran en un proceso
//! aparte, como el resto del catalogo: el motor de disco mantiene el
//! bloqueo del directorio mientras el proceso lo tiene abierto, y el
//! servicio es otro proceso.

mod support;

use std::path::Path;
use std::process::Command;

use hodei_catalog::CatalogReader;
use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// El media type de un manifiesto de imagen OCI.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// El media type de un indice multi-plataforma OCI.
const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";

/// El media type de un manifiesto de artefacto OCI.
const OCI_ARTIFACT: &str = "application/vnd.oci.artifact.manifest.v1+json";

/// El tipo de artefacto del SBOM que se cuelga.
const SBOM_TYPE: &str = "application/vnd.example.sbom.v1";

/// El tipo de artefacto de la firma que se cuelga.
const SIGNATURE_TYPE: &str = "application/vnd.example.signature.v1";

/// La ruta de la base, que el proceso hijo necesita.
const SEED_DB: &str = "HODEI_OCI_INDEX_SEED_DB";

/// Los digests que el inspector tiene que mirar, que viajan con el.
const DIGESTS: &str = "HODEI_OCI_INDEX_DIGESTS";

/// El contenido de una capa de ejemplo.
fn a_layer(seed: u8) -> Vec<u8> {
    (0u16..2048)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// El contenido de la configuracion de la imagen.
fn a_config(architecture: &str) -> Vec<u8> {
    format!(r#"{{"architecture":"{architecture}","os":"linux","rootfs":{{"type":"layers"}}}}"#)
        .into_bytes()
}

/// El manifiesto que referencia la configuracion y la capa.
fn a_manifest(config_digest: &str, layer_digest: &str, size: usize) -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config_digest}"),
            "size": size
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer_digest}"),
            "size": size
        }]
    })
    .to_string()
    .into_bytes()
}

/// El indice multi-plataforma, con sus hijos.
///
/// Los hijos llevan su plataforma, porque un indice sin plataforma no
/// dice que imagen es la de cada arquitectura.
fn a_index(children: &[(&str, &str, &str)]) -> Vec<u8> {
    let manifests: Vec<serde_json::Value> = children
        .iter()
        .map(|(digest, os, architecture)| {
            serde_json::json!({
                "mediaType": OCI_MANIFEST,
                "digest": format!("sha256:{digest}"),
                "size": 1,
                "platform": { "os": os, "architecture": architecture }
            })
        })
        .collect();
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_INDEX,
        "manifests": manifests
    })
    .to_string()
    .into_bytes()
}

/// El manifiesto de artefacto, con su `subject`.
///
/// El `subject` es el manifiesto al que se cuelga: una firma o un SBOM
/// que va al lado sin tocarlo.
fn an_artifact(artifact_type: &str, blob: &str, subject: &str) -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_ARTIFACT,
        "artifactType": artifact_type,
        "blobs": [{
            "mediaType": "application/vnd.example.blob",
            "digest": format!("sha256:{blob}"),
            "size": 1
        }],
        "subject": {
            "mediaType": OCI_MANIFEST,
            "digest": format!("sha256:{subject}"),
            "size": 1
        }
    })
    .to_string()
    .into_bytes()
}

/// El digest de un contenido, sin el prefijo del algoritmo.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// La ruta OCI de un recurso del repositorio de la primera cuenta.
fn the_oci_path(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
}

/// Una peticion de la distribucion.
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

/// Un despliegue con el catalogo sembrado.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// El caso del informe, antes de ejecutarlo.
///
/// Se anota dos veces: al principio, con lo que se espera, y al final,
/// con lo que se ha medido. Si la prueba se muere entre medias, el
/// informe se queda con la primera anotacion, que dice `FALLO`.
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

/// Publica un blob y devuelve su digest.
///
/// El blob es lo que el manifiesto nombra: sin el, el manifiesto no se
/// puede descargar.
fn publish_blob(deployment: &Deployment, port: u16, token: &str, content: &[u8]) -> String {
    let digest = the_digest(content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, content).expect("the blob is written");
    let answer = call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    );
    assert_eq!(
        answer.status,
        201,
        "the blob was not published:\n{}",
        deployment.the_log()
    );
    digest
}

/// Publica un manifiesto y devuelve la respuesta.
fn publish_manifest(
    deployment: &Deployment,
    port: u16,
    token: &str,
    reference: &str,
    bytes: &[u8],
    media_type: &str,
) -> support::Answer {
    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, bytes).expect("the manifest is written");
    call(
        port,
        "PUT",
        &the_oci_path("manifests", reference),
        token,
        Some(&file),
        Some(media_type),
        None,
    )
}

/// Una peticion que deja el cuerpo de la respuesta en un fichero, y lo
/// devuelve como texto.
///
/// El cuerpo de un error es lo que dice el error: sin el, una prueba
/// solo puede afirmar el codigo y no **que** error fue.
fn call_with_body(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Path>,
    content_type: Option<&str>,
) -> (support::Answer, String) {
    let into = deployment.directory.path().join("response.bin");
    let answer = call(port, method, path, token, body, content_type, Some(&into));
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer, text)
}

/// Un `GET` que deja el cuerpo en un fichero, y lo devuelve parseado.
fn get_json(
    port: u16,
    path: &str,
    token: &str,
    into: &Path,
) -> (support::Answer, serde_json::Value) {
    let answer = support::Curl::download(port, path, token, Some(into)).run();
    let body = std::fs::read(into).unwrap_or_default();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (answer, json)
}

/// El seeder del catalogo (repositorios por cuenta).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// El indice multi-plataforma.
// ---------------------------------------------------------------------------

/// Un indice multi-plataforma se publica y se sirve byte a byte.
///
/// El indice es lo que dice que hay una imagen para cada plataforma, y
/// lo que el cliente de contenedores lee para saber cual tiene que
/// bajar. Sus hijos son manifiestos, y tienen que estar.
///
/// Y los bytes que vuelven son **los que se publicaron**: un manifiesto
/// reescrito es un manifiesto con otro digest, y el no-go del protocolo
/// es justo ese.
#[test]
fn the_multi_platform_index_is_published_and_served_byte_by_byte() {
    let mut case = the_case(
        "OCI-INDEX-01 (indice)",
        "dos manifiestos de imagen, uno por plataforma, ya publicados",
        "se publica un indice que los nombra y se pide por etiqueta y por digest",
        "curl --request PUT /v2/releases/manifests/multi (Content-Type: application/vnd.oci.image.index.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Las dos imagenes, con su configuracion y su capa.
    let mut children: Vec<(String, String, String)> = Vec::new();
    for (architecture, seed) in [("amd64", 31u8), ("arm64", 41u8)] {
        let config = a_config(architecture);
        let layer = a_layer(seed);
        let config_digest = publish_blob(&deployment, service.port, &token, &config);
        let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
        let manifest = a_manifest(&config_digest, &layer_digest, config.len());
        let manifest_digest = the_digest(&manifest);
        let answer = publish_manifest(
            &deployment,
            service.port,
            &token,
            &manifest_digest,
            &manifest,
            OCI_MANIFEST,
        );
        assert_eq!(
            answer.status,
            201,
            "the image was not published:\n{}",
            deployment.the_log()
        );
        children.push((
            manifest_digest,
            "linux".to_string(),
            architecture.to_string(),
        ));
    }

    // Y el indice que los nombra.
    let borrowed: Vec<(&str, &str, &str)> = children
        .iter()
        .map(|(digest, os, architecture)| (digest.as_str(), os.as_str(), architecture.as_str()))
        .collect();
    let index = a_index(&borrowed);
    let index_digest = the_digest(&index);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "multi",
        &index,
        OCI_INDEX,
    );
    assert_eq!(
        answer.status,
        201,
        "the index was not published:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("docker-content-digest"),
        Some(format!("sha256:{index_digest}")),
        "the answer does not name the digest of the index"
    );

    // Por etiqueta y por digest, los bytes son los mismos.
    for reference in ["multi".to_string(), format!("sha256:{index_digest}")] {
        let into = deployment.directory.path().join("served.json");
        let answer = support::Curl::with_header(
            service.port,
            &the_oci_path("manifests", &reference),
            &token,
            ("accept", OCI_INDEX),
            &into,
        )
        .run();
        assert_eq!(
            answer.status,
            200,
            "the index is not served by {reference}:\n{}",
            deployment.the_log()
        );
        assert_eq!(
            std::fs::read(&into).expect("the bytes are read"),
            index,
            "the index that came back is not the index that was published"
        );
        assert_eq!(
            answer.header("content-type"),
            Some(OCI_INDEX.to_string()),
            "the index is not served with its media type"
        );
    }

    // Y los hijos que nombra son los que se publicaron, con su digest y
    // su media type.
    let served: serde_json::Value = serde_json::from_slice(&index).expect("the index is JSON");
    let described = served
        .get("manifests")
        .and_then(|manifests| manifests.as_array())
        .expect("the index names its manifests");
    assert_eq!(described.len(), 2, "the index names both platforms");
    for (child, (digest, os, architecture)) in described.iter().zip(children.iter()) {
        assert_eq!(
            child.get("digest").and_then(|value| value.as_str()),
            Some(format!("sha256:{digest}").as_str()),
            "the index does not name the child it published"
        );
        assert_eq!(
            child.get("mediaType").and_then(|value| value.as_str()),
            Some(OCI_MANIFEST),
            "the index does not keep the media type of the child"
        );
        assert_eq!(
            child
                .get("platform")
                .and_then(|platform| platform.get("architecture"))
                .and_then(|value| value.as_str()),
            Some(architecture.as_str()),
            "the index does not keep the platform of the child"
        );
        assert_eq!(
            child
                .get("platform")
                .and_then(|platform| platform.get("os"))
                .and_then(|value| value.as_str()),
            Some(os.as_str()),
            "the index does not keep the platform of the child"
        );
    }

    case.outcome =
        "201 con el digest del indice, y los bytes servidos son los publicados, con sus dos plataformas"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un indice que nombra un manifiesto que no esta no se publica.
///
/// Un indice con un hijo que no se puede descargar es un indice roto,
/// y el protocolo lo dice: el error es `MANIFEST_BLOB_UNKNOWN`.
#[test]
fn an_index_whose_child_is_missing_is_refused() {
    let mut case = the_case(
        "OCI-INDEX-01 (hijos)",
        "un indice que nombra un manifiesto que no esta publicado",
        "se publica el indice",
        "curl --request PUT /v2/releases/manifests/multi (Content-Type: application/vnd.oci.image.index.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let index = a_index(&[(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "linux",
        "amd64",
    )]);
    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "PUT",
        &the_oci_path("manifests", "multi"),
        &token,
        Some(&{
            let file = deployment.directory.path().join("index.json");
            std::fs::write(&file, &index).expect("the index is written");
            file
        }),
        Some(OCI_INDEX),
    );
    assert_eq!(
        answer.status,
        404,
        "an index with a child that is not there was published:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("MANIFEST_BLOB_UNKNOWN"),
        "the error is not the one of the protocol: {body}"
    );

    case.outcome = "404 MANIFEST_BLOB_UNKNOWN: el indice no se publica".to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// El `subject`: lo que cuelga de un manifiesto.
// ---------------------------------------------------------------------------

/// Un manifiesto con un `subject` que todavia no esta se acepta.
///
/// El protocolo lo dice explicitly: un registro tiene que aceptar el
/// manifiesto con un `subject` que no existe todavia, porque un cliente
/// puede subir el referrer **antes** que el sujeto.
#[test]
fn a_subject_that_is_not_there_yet_is_accepted() {
    let mut case = the_case(
        "OCI-INDEX-03 (subject)",
        "un manifiesto de artefacto cuyo subject no esta publicado",
        "se publica el manifiesto con el subject que no existe todavia",
        "curl --request PUT /v2/releases/manifests/sbom (Content-Type: application/vnd.oci.artifact.manifest.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let blob = b"the sbom".to_vec();
    let blob_digest = publish_blob(&deployment, service.port, &token, &blob);
    let artifact = an_artifact(
        SBOM_TYPE,
        &blob_digest,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "sbom",
        &artifact,
        OCI_ARTIFACT,
    );
    assert_eq!(
        answer.status,
        201,
        "a manifest with a subject that is not there yet was refused:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("oci-subject"),
        Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
        "the answer does not name the subject"
    );

    case.outcome =
        "201 con la cabecera OCI-Subject: el manifiesto con un subject que no existe todavia se acepta"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un manifiesto con la configuracion **vacia** se publica sin que
/// ese blob este en el almacen.
///
/// El descriptor vacio dice "esta imagen no tiene configuracion", y un
/// cliente puede mandarlo en linea --con su `data`-- sin subirlo nunca.
/// Un registro que lo exigiera en el almacen rechazaria un manifiesto
/// que el protocolo dice que es valido, y el cliente real que lo hace
/// (`oras`) dejaria de poder adjuntar nada.
#[test]
fn a_manifest_with_the_empty_config_is_published_without_it() {
    let mut case = the_case(
        "OCI-INDEX-03 (configuracion vacia)",
        "un manifiesto de imagen cuyo config es el descriptor vacio, que no esta en el almacen",
        "se publica el manifiesto",
        "curl --request PUT /v2/releases/manifests/v1 (Content-Type: application/vnd.oci.image.manifest.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // La capa si esta en el almacen: es lo que el manifiesto nombra de
    // verdad.
    let layer = a_layer(111);
    let layer_digest = publish_blob(&deployment, service.port, &token, &layer);

    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "artifactType": SBOM_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
            "size": 2,
            "data": "e30="
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer_digest}"),
            "size": layer.len()
        }]
    })
    .to_string()
    .into_bytes();

    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "v1",
        &manifest,
        OCI_MANIFEST,
    );
    assert_eq!(
        answer.status,
        201,
        "a manifest with the empty config was refused:\n{}",
        deployment.the_log()
    );

    case.outcome = "201: el descriptor vacio no se exige en el almacen, y el manifiesto se publica"
        .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un referrer que nombra un blob que no esta no se publica.
///
/// El `subject` no se exige, pero lo que el manifiesto **trae** si: un
/// referrer cuyo SBOM no se puede descargar no es un referrer.
#[test]
fn a_referrer_that_names_a_missing_blob_is_refused() {
    let mut case = the_case(
        "OCI-INDEX-03 (blobs del referrer)",
        "un manifiesto de artefacto que nombra un blob que no esta publicado",
        "se publica el manifiesto",
        "curl --request PUT /v2/releases/manifests/sbom (Content-Type: application/vnd.oci.artifact.manifest.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let artifact = an_artifact(
        SBOM_TYPE,
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "sbom",
        &artifact,
        OCI_ARTIFACT,
    );
    assert_eq!(
        answer.status,
        404,
        "a referrer with a blob that is not there was published:\n{}",
        deployment.the_log()
    );

    case.outcome = "404 MANIFEST_BLOB_UNKNOWN: el referrer no se publica".to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La lista de etiquetas.
// ---------------------------------------------------------------------------

/// La lista de etiquetas es lexical, y pagina.
///
/// El protocolo define el orden como alfanumerico **sin distinguir
/// mayusculas**, y pagina con `n` y `last`. El `Link` dice si hay mas, y
/// con `n = 0` el protocolo dice que no lo hay.
#[test]
fn the_list_of_tags_is_lexical_and_pages() {
    let mut case = the_case(
        "OCI-INDEX-02 (etiquetas)",
        "un repositorio con cuatro etiquetas",
        "se pide la lista entera, una pagina y la pagina de cero",
        "curl /v2/releases/tags/list ; curl /v2/releases/tags/list?n=2 ; curl /v2/releases/tags/list?n=0",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Cuatro imagenes, con cuatro etiquetas.
    for (tag, seed) in [("v1", 51u8), ("v2", 52), ("Beta", 53), ("alpha", 54)] {
        let config = a_config("amd64");
        let layer = a_layer(seed);
        let config_digest = publish_blob(&deployment, service.port, &token, &config);
        let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
        let manifest = a_manifest(&config_digest, &layer_digest, config.len());
        let answer = publish_manifest(
            &deployment,
            service.port,
            &token,
            tag,
            &manifest,
            OCI_MANIFEST,
        );
        assert_eq!(
            answer.status,
            201,
            "the image was not published:\n{}",
            deployment.the_log()
        );
    }

    let into = deployment.directory.path().join("tags.json");

    // La lista entera, en orden lexicografico.
    let (answer, body) = get_json(service.port, &the_oci_path("tags", "list"), &token, &into);
    assert_eq!(answer.status, 200, "the list of tags is not served");
    assert_eq!(
        body.get("name").and_then(|value| value.as_str()),
        Some(FIRST_REPOSITORY)
    );
    let tags: Vec<&str> = body
        .get("tags")
        .and_then(|tags| tags.as_array())
        .expect("the list has tags")
        .iter()
        .filter_map(|tag| tag.as_str())
        .collect();
    assert_eq!(
        tags,
        vec!["alpha", "Beta", "v1", "v2"],
        "the tags are not in lexical order"
    );

    // Una pagina de dos: la primera pagina y el `Link` que dice que hay
    // mas.
    let (answer, body) = get_json(
        service.port,
        &format!("{}?n=2", the_oci_path("tags", "list")),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the page of tags is not served");
    let page: Vec<&str> = body
        .get("tags")
        .and_then(|tags| tags.as_array())
        .expect("the page has tags")
        .iter()
        .filter_map(|tag| tag.as_str())
        .collect();
    assert_eq!(page, vec!["alpha", "Beta"], "the page is not the first one");
    let link = answer
        .header("link")
        .expect("the page does not say that there is more");
    assert!(
        link.contains("rel=\"next\""),
        "the link does not say that there is more: {link}"
    );
    assert!(
        link.contains("last=Beta"),
        "the link does not name the last tag of the page: {link}"
    );

    // La segunda pagina, con el cursor.
    let (answer, body) = get_json(
        service.port,
        &format!("{}?n=2&last=Beta", the_oci_path("tags", "list")),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the second page is not served");
    let page: Vec<&str> = body
        .get("tags")
        .and_then(|tags| tags.as_array())
        .expect("the page has tags")
        .iter()
        .filter_map(|tag| tag.as_str())
        .collect();
    assert_eq!(
        page,
        vec!["v1", "v2"],
        "the cursor does not continue the list"
    );

    // Y `n = 0` es una lista vacia, **sin** `Link`.
    let (answer, body) = get_json(
        service.port,
        &format!("{}?n=0", the_oci_path("tags", "list")),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the empty page is not served");
    assert_eq!(
        body.get("tags").and_then(|tags| tags.as_array()),
        Some(&vec![]),
        "the page of zero is not empty"
    );
    assert!(
        answer.header("link").is_none(),
        "the page of zero has a Link, and the protocol says that it must not"
    );

    case.outcome =
        "200 con las cuatro etiquetas en orden lexicografico, la pagina con su Link, y n=0 sin Link"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// La etiqueta es mutable: dos publicaciones concurrentes no pierden
/// entradas.
///
/// La etiqueta acaba en una de las dos imagenes --la que se escribio
/// ultimo--, pero **las dos** imagenes siguen publicadas con su digest,
/// y la lista de etiquetas no ha perdido ninguna.
#[test]
fn a_tag_moved_concurrently_keeps_both_manifests() {
    let mut case = the_case(
        "OCI-INDEX-02 (concurrencia)",
        "una etiqueta que ya apunta a un manifiesto",
        "dos publicaciones concurrentes con el nombre de la etiqueta",
        "curl --request PUT /v2/releases/manifests/v1 (dos veces, a la vez)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // La imagen que hay detras de la etiqueta.
    let config = a_config("amd64");
    let layer = a_layer(61);
    let config_digest = publish_blob(&deployment, service.port, &token, &config);
    let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
    let first = a_manifest(&config_digest, &layer_digest, config.len());
    let first_digest = the_digest(&first);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "v1",
        &first,
        OCI_MANIFEST,
    );
    assert_eq!(answer.status, 201, "the first image was not published");

    // Dos imagenes mas, que se pelean por la etiqueta.
    let mut others = Vec::new();
    for seed in [71u8, 72u8] {
        let config = a_config("arm64");
        let layer = a_layer(seed);
        let config_digest = publish_blob(&deployment, service.port, &token, &config);
        let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
        let manifest = a_manifest(&config_digest, &layer_digest, config.len());
        others.push((the_digest(&manifest), manifest));
    }

    let port = service.port;
    let token = token.clone();
    let answers: Vec<u16> = std::thread::scope(|scope| {
        let handles: Vec<_> = others
            .iter()
            .map(|(_, manifest)| {
                let token = token.clone();
                scope.spawn(move || {
                    let file = std::env::temp_dir().join(format!(
                        "hodei-oci-index-{}-{}.json",
                        std::process::id(),
                        the_digest(manifest)
                    ));
                    std::fs::write(&file, manifest).expect("the manifest is written");
                    let answer = call(
                        port,
                        "PUT",
                        &the_oci_path("manifests", "v1"),
                        &token,
                        Some(&file),
                        Some(OCI_MANIFEST),
                        None,
                    );
                    let _ = std::fs::remove_file(&file);
                    answer.status
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("the thread finishes"))
            .collect()
    });
    for status in &answers {
        assert_eq!(*status, 201, "one of the concurrent publications failed");
    }

    // La etiqueta apunta a una de las dos, y la lista no ha perdido la
    // etiqueta.
    let into = deployment.directory.path().join("tags.json");
    let (answer, body) = get_json(service.port, &the_oci_path("tags", "list"), &token, &into);
    assert_eq!(answer.status, 200, "the list of tags is not served");
    let tags: Vec<&str> = body
        .get("tags")
        .and_then(|tags| tags.as_array())
        .expect("the list has tags")
        .iter()
        .filter_map(|tag| tag.as_str())
        .collect();
    assert_eq!(tags, vec!["v1"], "the list of tags lost the tag");

    // Y las tres imagenes siguen publicadas con su digest: la que
    // estaba detras de la etiqueta no se ha movido de sitio.
    let into = deployment.directory.path().join("served.json");
    for (digest, manifest) in
        std::iter::once((first_digest.clone(), first.clone())).chain(others.iter().cloned())
    {
        let answer = support::Curl::with_header(
            service.port,
            &the_oci_path("manifests", &format!("sha256:{digest}")),
            &token,
            ("accept", OCI_MANIFEST),
            &into,
        )
        .run();
        assert_eq!(
            answer.status,
            200,
            "the manifest {digest} is not served:\n{}",
            deployment.the_log()
        );
        assert_eq!(
            std::fs::read(&into).expect("the bytes are read"),
            manifest,
            "the manifest that came back is not the manifest that was published"
        );
    }

    case.outcome =
        "las dos publicaciones concurrentas se aceptan y las tres imagenes siguen servidas por su digest"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Los referrers.
// ---------------------------------------------------------------------------

/// Los referrers de un manifiesto que no tiene referrers son una lista
/// vacia, y **no** un 404.
///
/// El protocolo lo dice: un manifiesto sin referrers no es un error, y un
/// cliente que busca la firma de un manifiesto tiene que poder
/// distinguir "no hay firma" de "no hay manifiesto" sin que el registro
/// le diga que no hay manifiesto.
#[test]
fn the_referrers_of_a_manifest_that_has_none_are_an_empty_list() {
    let mut case = the_case(
        "OCI-INDEX-03 (lista vacia)",
        "un manifiesto publicado sin referrers",
        "se pregunta por sus referrers",
        "curl /v2/releases/referrers/sha256:<digest>",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let config = a_config("amd64");
    let layer = a_layer(81);
    let config_digest = publish_blob(&deployment, service.port, &token, &config);
    let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
    let manifest = a_manifest(&config_digest, &layer_digest, config.len());
    let manifest_digest = the_digest(&manifest);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "v1",
        &manifest,
        OCI_MANIFEST,
    );
    assert_eq!(answer.status, 201, "the image was not published");

    let into = deployment.directory.path().join("referrers.json");
    let (answer, body) = get_json(
        service.port,
        &the_oci_path("referrers", &format!("sha256:{manifest_digest}")),
        &token,
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "the referrers of a manifest without referrers are not a 200:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        answer.header("content-type"),
        Some("application/vnd.oci.image.index.v1+json".to_string()),
        "the referrers are not served as an index"
    );
    assert_eq!(
        body.get("manifests")
            .and_then(|manifests| manifests.as_array()),
        Some(&vec![]),
        "the list of referrers is not empty"
    );

    // Y un subject que no existe tampoco es un 404.
    let (answer, body) = get_json(
        service.port,
        &the_oci_path(
            "referrers",
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        ),
        &token,
        &into,
    );
    assert_eq!(
        answer.status,
        200,
        "the referrers of a subject that is not there are not a 200:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        body.get("manifests")
            .and_then(|manifests| manifests.as_array()),
        Some(&vec![]),
        "the list of referrers is not empty"
    );

    case.outcome =
        "200 con la lista vacia, tanto para un manifiesto sin referrers como para un subject que no esta"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un digest que no es un digest es un error del cliente.
///
/// El protocolo lo dice con un `400`: una peticion invalida no es una
/// busqueda sin resultado.
#[test]
fn the_referrers_of_a_digest_that_is_not_a_digest_are_a_bad_request() {
    let mut case = the_case(
        "OCI-INDEX-03 (digest invalido)",
        "una peticion de referrers con un digest que no es un digest",
        "se pregunta por los referrers",
        "curl /v2/releases/referrers/not-a-digest",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "GET",
        &the_oci_path("referrers", "not-a-digest"),
        &token,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        400,
        "a digest that is not a digest is not a 400:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("DIGEST_INVALID"),
        "the error is not the one of the protocol: {body}"
    );

    case.outcome = "400 DIGEST_INVALID: la peticion invalida se dice como invalida".to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Los referrers de otra cuenta no se ven.
///
/// Un repositorio de otra cuenta no existe para esta, y su error es el
/// mismo que el de un repositorio que no existe: un `403` o un `404`
/// que lo distinga filtra la existencia de lo ajeno.
#[test]
fn the_referrers_of_another_account_are_not_visible() {
    let mut case = the_case(
        "OCI-INDEX-03 (aislamiento)",
        "una cuenta que no es la del repositorio",
        "se pregunta por los referrers del repositorio de la otra cuenta",
        "curl /v2/releases/referrers/sha256:<digest>",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let other = support::token(SECOND_ACCOUNT, "bob");

    // El repositorio que **solo** tiene la primera cuenta: el de la
    // segunda cuenta es otro, con el mismo nombre y otro contenido.
    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "GET",
        &format!(
            "/v2/{FIRST_ONLY_REPOSITORY}/referrers/sha256:{}",
            "e".repeat(64)
        ),
        &other,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        404,
        "the referrers of another account are visible:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("NAME_UNKNOWN"),
        "the error is not the one of the protocol: {body}"
    );

    case.outcome =
        "404 NAME_UNKNOWN: el repositorio de otra cuenta no existe para esta".to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// El cliente de verdad: `oras`.
// ---------------------------------------------------------------------------

/// El `oras` cuelga un SBOM de un manifiesto y lo vuelve a encontrar.
///
/// El `oras` es el cliente de los artefactos: cuelga una firma o un SBOM
/// de un manifiesto sin tocarlo, y lo vuelve a encontrar con
/// `discover`. Y lo hace hablando el protocolo entero: el reto, el
/// endpoint de token y la API de referrers.
#[test]
fn oras_attaches_an_artifact_and_discovers_it() {
    let mut case = the_case(
        "OCI-INDEX-03 (oras)",
        "un manifiesto de imagen publicado y un token valido",
        "oras cuelga un SBOM del manifiesto y lo vuelve a encontrar",
        "oras attach --artifact-type application/vnd.example.sbom.v1 127.0.0.1:<port>/releases:v1 sbom.json ; oras discover 127.0.0.1:<port>/releases:v1",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // El manifiesto al que se cuelga el SBOM.
    let config = a_config("amd64");
    let layer = a_layer(91);
    let config_digest = publish_blob(&deployment, service.port, &token, &config);
    let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
    let manifest = a_manifest(&config_digest, &layer_digest, config.len());
    let manifest_digest = the_digest(&manifest);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "v1",
        &manifest,
        OCI_MANIFEST,
    );
    assert_eq!(
        answer.status,
        201,
        "the image was not published:\n{}",
        deployment.the_log()
    );

    // El SBOM que se cuelga.
    let sbom = deployment.directory.path().join("sbom.json");
    std::fs::write(&sbom, br#"{"sbom":"spdx","packages":[]}"#).expect("the sbom is written");

    let oras = support::the_oras();
    let attached = Command::new(&oras)
        .args([
            "attach",
            "--artifact-type",
            SBOM_TYPE,
            "--distribution-spec",
            "v1.1-referrers-api",
            "--plain-http",
            "--username",
            "alice",
            "--password",
            &token,
            "--no-tty",
            // El fichero esta en un directorio temporal, y su camino es
            // absoluto: el `oras` lo comprueba, y aqui es a proposito.
            "--disable-path-validation",
        ])
        .arg(format!("127.0.0.1:{}/{FIRST_REPOSITORY}:v1", service.port))
        .arg(&sbom)
        .output()
        .expect("oras has to be there");
    assert!(
        attached.status.success(),
        "oras could not attach:\n{}\n{}",
        String::from_utf8_lossy(&attached.stdout),
        String::from_utf8_lossy(&attached.stderr)
    );

    // Y el `discover` lo encuentra: el `oras` pregunta por la API de
    // referrers, y la respuesta es lo que el registro tiene.
    let discovered = Command::new(&oras)
        .args([
            "discover",
            "--distribution-spec",
            "v1.1-referrers-api",
            "--plain-http",
            "--username",
            "alice",
            "--password",
            &token,
            "--format",
            "json",
            "--no-tty",
        ])
        .arg(format!("127.0.0.1:{}/{FIRST_REPOSITORY}:v1", service.port))
        .output()
        .expect("oras has to be there");
    assert!(
        discovered.status.success(),
        "oras could not discover:\n{}\n{}",
        String::from_utf8_lossy(&discovered.stdout),
        String::from_utf8_lossy(&discovered.stderr)
    );
    let found: serde_json::Value = serde_json::from_slice(&discovered.stdout).unwrap_or_default();
    let references = found
        .get("manifests")
        .and_then(|manifests| manifests.as_array())
        .unwrap_or_else(|| {
            panic!(
                "the discovery has no manifests: {}",
                String::from_utf8_lossy(&discovered.stdout)
            )
        });
    assert!(
        references.iter().any(|reference| {
            reference
                .get("artifactType")
                .and_then(|value| value.as_str())
                == Some(SBOM_TYPE)
        }),
        "the SBOM is not in the discovery: {found}"
    );

    // Y la API de referrers, preguntada sin cliente, dice lo mismo.
    let into = deployment.directory.path().join("referrers.json");
    let (answer, body) = get_json(
        service.port,
        &the_oci_path("referrers", &format!("sha256:{manifest_digest}")),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the referrers are not served");
    let manifests = body
        .get("manifests")
        .and_then(|manifests| manifests.as_array())
        .expect("the list has manifests");
    assert_eq!(manifests.len(), 1, "the referrer is not the only one");
    assert_eq!(
        manifests[0]
            .get("artifactType")
            .and_then(|value| value.as_str()),
        Some(SBOM_TYPE),
        "the referrer does not say what it is"
    );
    // El `oras` empaca el adjunto como un **manifiesto de imagen** con
    // un `artifactType`: es lo que dice el manifiesto, y el descriptor
    // lo dice como es. Lo que dice **que** es un SBOM es el
    // `artifactType`, que es justo lo que el filtro usa.
    assert_eq!(
        manifests[0]
            .get("mediaType")
            .and_then(|value| value.as_str()),
        Some(OCI_MANIFEST),
        "the referrer is not served with the media type of the manifest: {body}"
    );
    // Y el manifiesto del referrer se sirve con sus bytes, que son los
    // que el `oras` subio.
    let referrer_digest = manifests[0]
        .get("digest")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let into = deployment.directory.path().join("referrer.json");
    let answer = support::Curl::download(
        service.port,
        &the_oci_path("manifests", &referrer_digest),
        &token,
        Some(&into),
    )
    .run();
    assert_eq!(
        answer.status,
        200,
        "the manifest of the referrer is not served:\n{}",
        deployment.the_log()
    );
    let referrer_bytes =
        String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    let referrer: serde_json::Value = serde_json::from_str(&referrer_bytes).expect("it is JSON");
    assert_eq!(
        referrer
            .get("artifactType")
            .and_then(|value| value.as_str()),
        Some(SBOM_TYPE),
        "the referrer does not say what it is: {referrer_bytes}"
    );
    assert_eq!(
        referrer
            .get("subject")
            .and_then(|subject| subject.get("digest"))
            .and_then(|value| value.as_str()),
        Some(format!("sha256:{manifest_digest}").as_str()),
        "the referrer does not point at the manifest it was attached to: {referrer_bytes}"
    );
    assert_eq!(
        manifests[0].get("size").and_then(|value| value.as_u64()),
        Some(referrer_bytes.len() as u64),
        "the descriptor does not say the size of the manifest it describes"
    );

    // El filtro por tipo de artefacto: solo lo que es de ese tipo, y la
    // cabecera que dice que se ha filtrado.
    let (answer, body) = get_json(
        service.port,
        &format!(
            "{}?artifactType={SBOM_TYPE}",
            the_oci_path("referrers", &format!("sha256:{manifest_digest}"))
        ),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the filtered referrers are not served");
    assert_eq!(
        answer.header("oci-filters-applied"),
        Some("artifactType".to_string()),
        "the answer does not say that the filter was applied"
    );
    assert_eq!(
        body.get("manifests")
            .and_then(|manifests| manifests.as_array()),
        Some(&vec![manifests[0].clone()]),
        "the filter did not leave the referrer"
    );

    // Y un filtro que no casa con nada es una lista vacia, no un error.
    let (answer, body) = get_json(
        service.port,
        &format!(
            "{}?artifactType=application/vnd.example.other",
            the_oci_path("referrers", &format!("sha256:{manifest_digest}"))
        ),
        &token,
        &into,
    );
    assert_eq!(answer.status, 200, "the filtered referrers are not served");
    assert_eq!(
        body.get("manifests")
            .and_then(|manifests| manifests.as_array()),
        Some(&vec![]),
        "the filter did not empty the list"
    );

    case.outcome =
        "oras cuelga el SBOM y lo descubre; la API de referrers lo lista con su artifactType, y el filtro dice que se ha aplicado"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La confianza: un referrer no promueve nada.
// ---------------------------------------------------------------------------

/// Una firma colgada no promueve lo que cuelga.
///
/// El referrer de una firma es un manifiesto mas: se publica, se lista y
/// se sirve. Lo que **no** hace es promover el sujeto, porque promover
/// es una decision del motor de confianza y la presencia de un fichero no
/// es una decision.
#[test]
fn a_signature_referrer_does_not_promote_the_subject() {
    let mut case = the_case(
        "OCI-INDEX-04 (confianza)",
        "un manifiesto y una firma colgada de el",
        "se mira el canal de confianza de los dos (en otro proceso)",
        "curl --request PUT /v2/releases/manifests/v1 ; <inspector del canal>",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // El manifiesto.
    let config = a_config("amd64");
    let layer = a_layer(101);
    let config_digest = publish_blob(&deployment, service.port, &token, &config);
    let layer_digest = publish_blob(&deployment, service.port, &token, &layer);
    let manifest = a_manifest(&config_digest, &layer_digest, config.len());
    let manifest_digest = the_digest(&manifest);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        "v1",
        &manifest,
        OCI_MANIFEST,
    );
    assert_eq!(answer.status, 201, "the image was not published");

    // Y la firma, colgada de el.
    let signature = b"a signature that nobody has verified".to_vec();
    let signature_digest = publish_blob(&deployment, service.port, &token, &signature);
    let artifact = an_artifact(SIGNATURE_TYPE, &signature_digest, &manifest_digest);
    let artifact_digest = the_digest(&artifact);
    let answer = publish_manifest(
        &deployment,
        service.port,
        &token,
        &signature_digest,
        &artifact,
        OCI_ARTIFACT,
    );
    assert_eq!(
        answer.status,
        201,
        "the signature was not published:\n{}",
        deployment.the_log()
    );

    // Y el servicio tiene que soltar la base para que otro proceso
    // pueda leerla.
    support::stop(&mut service);

    // El inspector es otro proceso, y no puede ver lo que esta prueba
    // tiene en memoria: los digests viajan por el entorno.
    let status = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "the_signed_manifest_is_still_in_quarantine",
            "--ignored",
            "--nocapture",
        ])
        .env(SEED_DB, &deployment.db)
        .env(DIGESTS, format!("{manifest_digest},{artifact_digest}"))
        .status()
        .expect("the inspector runs");
    assert!(
        status.success(),
        "the signature promoted the manifest:\n{}",
        deployment.the_log()
    );

    case.outcome =
        "el manifiesto y la firma siguen en cuarentena: la presencia de una firma no promueve nada"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Lo que el registro no deja pasar.
// ---------------------------------------------------------------------------

/// Un `subject` que no es un digest no se publica.
///
/// El digest del `subject` viaja al catalogo, y el catalogo lo relee con
/// la regla de un digest: 64 hexadecimales. Si el push aceptara un
/// digest a medio escribir, el manifiesto se publicaria con un 201 y
/// quedaria **ilegible**: la lectura del asset falla como corrupta y el
/// manifiesto no se podria volver a bajar ni por su digest ni por su
/// etiqueta. Un 201 con un manifiesto que no se puede leer es peor que
/// un 400.
#[test]
fn a_subject_that_is_not_a_digest_is_refused() {
    let mut case = the_case(
        "OCI-INDEX-01 (subject mal formado)",
        "un manifiesto de artefacto cuyo subject nombra un digest a medio escribir",
        "se publica el manifiesto",
        "curl --request PUT /v2/releases/manifests/sbom (Content-Type: application/vnd.oci.artifact.manifest.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let blob = b"the sbom".to_vec();
    let blob_digest = publish_blob(&deployment, service.port, &token, &blob);
    // El `subject` es lo que esta mal: un digest a medio escribir.
    let artifact = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_ARTIFACT,
        "artifactType": SBOM_TYPE,
        "blobs": [{
            "mediaType": "application/vnd.example.blob",
            "digest": format!("sha256:{blob_digest}"),
            "size": 1
        }],
        "subject": {
            "mediaType": OCI_MANIFEST,
            "digest": "sha256:aaa",
            "size": 1
        }
    })
    .to_string()
    .into_bytes();

    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &artifact).expect("the manifest is written");
    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "PUT",
        &the_oci_path("manifests", "sbom"),
        &token,
        Some(&file),
        Some(OCI_ARTIFACT),
    );
    assert_eq!(
        answer.status,
        400,
        "a manifest with a subject that is not a digest was published:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("MANIFEST_INVALID"),
        "the error is not the one of the protocol: {body}"
    );

    // Y no queda publicado: un manifiesto que no se puede leer no puede
    // quedar a medio publicar.
    let digest = the_digest(&artifact);
    let (answer, _) = call_with_body(
        &deployment,
        service.port,
        "GET",
        &the_oci_path("manifests", &format!("sha256:{digest}")),
        &token,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        404,
        "the manifest that was refused is served:\n{}",
        deployment.the_log()
    );

    case.outcome =
        "400 MANIFEST_INVALID: un subject que no es un digest no se publica, y el manifiesto no queda a medio publicar"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un descriptor que **dice** que es el vacio pero nombra otro
/// contenido no es el vacio.
///
/// El descriptor vacio son los dos bytes `{}`, y su digest es el de esos
/// dos bytes. Un descriptor que dice "vacio" y nombra un digest que no
/// es el de `{}` esta diciendo una cosa y otra: un registro que lo
/// tomara por el vacio dejaria de exigir un contenido que el manifiesto
/// nombra, y publicaria un manifiesto que no se puede descargar.
#[test]
fn a_descriptor_that_only_says_empty_is_not_the_empty_one() {
    let mut case = the_case(
        "OCI-INDEX-01 (descriptor vacio)",
        "un manifiesto de imagen cuyo config dice que es el descriptor vacio pero nombra otro digest",
        "se publica el manifiesto",
        "curl --request PUT /v2/releases/manifests/v1 (Content-Type: application/vnd.oci.image.manifest.v1+json)",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.empty.v1+json",
            "digest": format!("sha256:{}", "a".repeat(64)),
            "size": 2
        },
        "layers": [],
    })
    .to_string()
    .into_bytes();

    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");
    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "PUT",
        &the_oci_path("manifests", "v1"),
        &token,
        Some(&file),
        Some(OCI_MANIFEST),
    );
    assert_eq!(
        answer.status,
        404,
        "a manifest that names a content that is not there was published:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("MANIFEST_BLOB_UNKNOWN"),
        "the error is not the one of the protocol: {body}"
    );

    case.outcome =
        "404 MANIFEST_BLOB_UNKNOWN: un descriptor que solo dice 'vacio' no es el descriptor vacio, y el contenido que nombra se exige"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Las etiquetas de otra cuenta no se ven.
///
/// La lista de etiquetas sale del catalogo, y el catalogo filtra por
/// cuenta y por repositorio. Una regresion que quitara ese filtro no la
/// detectaria ninguna otra prueba: la suite seguiria en verde y las
/// etiquetas de otra cuenta serian visibles.
#[test]
fn the_tags_of_another_account_are_not_visible() {
    let mut case = the_case(
        "OCI-INDEX-02 (aislamiento)",
        "una cuenta que no es la del repositorio",
        "se pregunta por las etiquetas del repositorio de la otra cuenta",
        "curl /v2/private/tags/list",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let other = support::token(SECOND_ACCOUNT, "bob");

    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "GET",
        &format!("/v2/{FIRST_ONLY_REPOSITORY}/tags/list"),
        &other,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        404,
        "the tags of another account are visible:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("NAME_UNKNOWN"),
        "the error is not the one of the protocol: {body}"
    );

    case.outcome =
        "404 NAME_UNKNOWN: las etiquetas de otra cuenta no existen para esta".to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// Un `n` que no es un numero es un 400, no un 405.
///
/// El parametro es un entero, y un entero que no lo es es una peticion
/// mal escrita: el registro de referencia responde con un 400 y el
/// codigo `PAGINATION_NUMBER_INVALID`, y un cliente que mire el codigo
/// ve lo mismo que veria en cualquier otro registro. Un 405 diria que
/// el registro no sabe atender la peticion, y si los sabe.
#[test]
fn a_n_that_is_not_a_number_is_a_bad_request() {
    let mut case = the_case(
        "OCI-INDEX-02 (n invalido)",
        "una peticion de la lista de etiquetas con un n que no es un numero",
        "se pide la lista",
        "curl /v2/releases/tags/list?n=many",
    );
    support::record_oci_index(case.clone());

    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let (answer, body) = call_with_body(
        &deployment,
        service.port,
        "GET",
        &format!("{}?n=many", the_oci_path("tags", "list")),
        &token,
        None,
        None,
    );
    assert_eq!(
        answer.status,
        400,
        "a malformed n was not a client error:\n{}",
        deployment.the_log()
    );
    assert!(
        body.contains("PAGINATION_NUMBER_INVALID"),
        "the error is not the one of the reference registry: {body}"
    );

    case.outcome =
        "400 PAGINATION_NUMBER_INVALID: un n que no es un numero es un error del cliente"
            .to_string();
    case.passed = true;
    support::record_oci_index(case);

    support::stop(&mut service);
}

/// El inspector del canal, en un proceso aparte.
///
/// La base de disco la tiene el servicio mientras esta vivo, asi que el
/// que mira tiene que ser otro proceso.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo de los tests del indice OCI"]
async fn the_signed_manifest_is_still_in_quarantine() {
    let Some(db_path) = std::env::var(SEED_DB).ok() else {
        return;
    };

    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(Path::new(&db_path))
        .await
        .expect("rocksdb");
    db.use_ns("hodei").await.expect("ns");
    db.use_db("artifacts").await.expect("db");

    let catalog = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        std::sync::Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    catalog.initialize().await.expect("initialize");

    // Lo que importa: **ninguna** de las dos referencias ha salido de la
    // cuarentena. La firma es un referrer mas, y un referrer no promueve
    // a nadie: ni al manifiesto que cuelga de el, ni a si mismo.
    let digests = the_digests_of_the_test();
    assert!(
        !digests.is_empty(),
        "the inspector does not know what to look at: the digests have to travel with it"
    );
    let mut promoted = Vec::new();
    for digest in digests {
        let reference = catalog
            .reference(
                FIRST_ACCOUNT,
                FIRST_REPOSITORY,
                &format!("manifests/{digest}"),
            )
            .await
            .expect("the reference of the manifest");
        if reference.trust_state != hodei_catalog::TrustState::Quarantined {
            promoted.push(digest);
        }
    }
    assert!(
        promoted.is_empty(),
        "the presence of a signature promoted the manifest: {promoted:?}"
    );
}

/// Los digests de los manifiestos que publica la prueba.
///
/// El inspector corre en otro proceso y no puede ver lo que la prueba
/// ha hecho en memoria: los digests van por el entorno.
fn the_digests_of_the_test() -> Vec<String> {
    std::env::var(DIGESTS)
        .ok()
        .map(|digests| {
            digests
                .split(',')
                .filter(|digest| !digest.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
