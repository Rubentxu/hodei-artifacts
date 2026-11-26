//! The query and attachments cut of OCI Distribution (HSP-102 / HAR-08),
//! against the wire.
//!
//! The distribution protocol is not only upload and download: it is also
//! **asking**. Which tags the repository has, what hangs from this
//! manifest, and which multi-platform images there are.
//!
//! And the tests go against the wire, with a real process, a real catalog
//! and a real byte store. The last case uses a real client
//! --`oras`, the artifacts client-- that hangs an SBOM from
//! a manifest and finds it again: a protocol that only works
//! with the client that the server's author wrote is not a protocol.
//!
//! The fixtures (blobs, manifests, tags) are seeded in a separate
//! process, like the rest of the catalog, against the deployment's
//! Dgraph namespace.

mod support;

use std::path::Path;
use std::process::Command;

use hodei_catalog::CatalogReader;
use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// The media type of an OCI image manifest.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// The media type of an OCI multi-platform index.
const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";

/// The media type of an OCI artifact manifest.
const OCI_ARTIFACT: &str = "application/vnd.oci.artifact.manifest.v1+json";

/// The artifact type of the SBOM that gets attached.
const SBOM_TYPE: &str = "application/vnd.example.sbom.v1";

/// The artifact type of the signature that gets attached.
const SIGNATURE_TYPE: &str = "application/vnd.example.signature.v1";

/// The locator of the Dgraph tenant, which the child process needs.
const SEED_DB: &str = "HODEI_OCI_INDEX_SEED_DB";

/// The digests that the inspector has to look at, which travel with it.
const DIGESTS: &str = "HODEI_OCI_INDEX_DIGESTS";

/// The content of a sample layer.
fn a_layer(seed: u8) -> Vec<u8> {
    (0u16..2048)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// The content of the image's configuration.
fn a_config(architecture: &str) -> Vec<u8> {
    format!(r#"{{"architecture":"{architecture}","os":"linux","rootfs":{{"type":"layers"}}}}"#)
        .into_bytes()
}

/// The manifest that references the configuration and the layer.
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

/// The multi-platform index, with its children.
///
/// The children carry their platform, because an index without a platform does not
/// say which image is the one for each architecture.
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

/// The artifact manifest, with its `subject`.
///
/// The `subject` is the manifest it is attached to: a signature or an SBOM
/// that goes alongside without touching it.
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

/// The digest of a content, without the algorithm prefix.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// The OCI path of a resource of the first account's repository.
fn the_oci_path(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
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

/// A deployment with the catalog seeded.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// The report case, before executing it.
///
/// It is recorded twice: at the start, with what is expected, and at the end,
/// with what was measured. If the test dies in between, the
/// report keeps the first record, which says `FALLO`.
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

/// Publishes a blob and returns its digest.
///
/// The blob is what the manifest names: without it, the manifest
/// cannot be downloaded.
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

/// Publishes a manifest and returns the response.
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

/// A request that leaves the response body in a file, and
/// returns it as text.
///
/// The body of an error is what the error says: without it, a test
/// can only assert the code and not **which** error it was.
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

/// A `GET` that leaves the body in a file, and returns it parsed.
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

/// The catalog seeder (repositories per account).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The multi-platform index.
// ---------------------------------------------------------------------------

/// A multi-platform index is published and served byte for byte.
///
/// The index is what says that there is an image for each platform, and
/// what the container client reads to know which one it has to
/// download. Its children are manifests, and they have to be there.
///
/// And the bytes that come back are **those that were published**: a rewritten
/// manifest is a manifest with another digest, and the protocol's no-go
/// is exactly that.
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

    // The two images, with their configuration and their layer.
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

    // And the index that names them.
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

    // By tag and by digest, the bytes are the same.
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

    // And the children it names are those that were published, with their digest and
    // their media type.
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

/// An index that names a manifest that is not there is not published.
///
/// An index with a child that cannot be downloaded is a broken index,
/// and the protocol says so: the error is `MANIFEST_BLOB_UNKNOWN`.
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
// The `subject`: what hangs from a manifest.
// ---------------------------------------------------------------------------

/// A manifest with a `subject` that is not there yet is accepted.
///
/// The protocol says so explicitly: a registry has to accept the
/// manifest with a `subject` that does not exist yet, because a client
/// may upload the referrer **before** the subject.
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

/// A manifest with the **empty** configuration is published without
/// that blob being in the store.
///
/// The empty descriptor says "this image has no configuration", and a
/// client may send it inline --with its `data`-- without ever uploading it.
/// A registry that required it in the store would reject a manifest
/// that the protocol says is valid, and the real client that does it
/// (`oras`) would no longer be able to attach anything.
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

    // The layer is in the store: it is what the manifest really
    // names.
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

/// A referrer that names a blob that is not there is not published.
///
/// The `subject` is not required, but what the manifest **carries** is: a
/// referrer whose SBOM cannot be downloaded is not a referrer.
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
// The tag list.
// ---------------------------------------------------------------------------

/// The tag list is lexical, and paginates.
///
/// The protocol defines the order as alphanumeric **case-insensitive**,
/// and paginates with `n` and `last`. The `Link` says whether there is more, and
/// with `n = 0` the protocol says there is none.
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

    // Four images, with four tags.
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

    // The whole list, in lexicographic order.
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

    // A page of two: the first page and the `Link` that says there is
    // more.
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

    // The second page, with the cursor.
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

    // And `n = 0` is an empty list, **without** `Link`.
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

/// The tag is mutable: two concurrent publications do not lose
/// entries.
///
/// The tag ends up on one of the two images --the one written
/// last--, but **both** images remain published with their digest,
/// and the tag list has not lost any.
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

    // The image behind the tag.
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

    // Two more images, which fight over the tag.
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

    // The tag points to one of the two, and the list has not lost the
    // tag.
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

    // And the three images remain published with their digest: the one that
    // was behind the tag has not moved from its place.
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
// The referrers.
// ---------------------------------------------------------------------------

/// The referrers of a manifest that has no referrers are an empty
/// list, and **not** a 404.
///
/// The protocol says so: a manifest without referrers is not an error, and a
/// client looking for a manifest's signature has to be able to
/// distinguish "there is no signature" from "there is no manifest" without the registry
/// telling it that there is no manifest.
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

    // And a subject that does not exist is not a 404 either.
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

/// A digest that is not a digest is a client error.
///
/// The protocol says so with a `400`: an invalid request is not a
/// search without results.
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

/// The referrers of another account are not seen.
///
/// A repository of another account does not exist for this one, and its error is the
/// same as that of a repository that does not exist: a `403` or a `404`
/// that distinguishes it leaks the existence of what belongs to someone else.
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

    // The repository that **only** the first account has: the second
    // account's is another one, with the same name and other content.
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
// The real client: `oras`.
// ---------------------------------------------------------------------------

/// `oras` attaches an SBOM to a manifest and finds it again.
///
/// `oras` is the artifacts client: it attaches a signature or an SBOM
/// to a manifest without touching it, and finds it again with
/// `discover`. And it does so speaking the whole protocol: the challenge, the
/// token endpoint and the referrers API.
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

    // The manifest the SBOM is attached to.
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

    // The SBOM that gets attached.
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
            // The file is in a temporary directory, and its path is
            // absolute: `oras` checks it, and here it is on purpose.
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

    // And `discover` finds it: `oras` asks the referrers API,
    // and the response is what the registry has.
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

    // And the referrers API, asked without a client, says the same.
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
    // `oras` packs the attachment as an **image manifest** with
    // an `artifactType`: it is what the manifest says, and the descriptor
    // says it as it is. What says **what** an SBOM is the
    // `artifactType`, which is exactly what the filter uses.
    assert_eq!(
        manifests[0]
            .get("mediaType")
            .and_then(|value| value.as_str()),
        Some(OCI_MANIFEST),
        "the referrer is not served with the media type of the manifest: {body}"
    );
    // And the referrer's manifest is served with its bytes, which are those
    // that `oras` uploaded.
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

    // The filter by artifact type: only what is of that type, and the
    // header that says it was filtered.
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

    // And a filter that matches nothing is an empty list, not an error.
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
// Trust: a referrer promotes nothing.
// ---------------------------------------------------------------------------

/// An attached signature does not promote what it hangs from.
///
/// A signature's referrer is one more manifest: it is published, listed and
/// served. What it does **not** do is promote the subject, because promoting
/// is a decision of the trust engine and the presence of a file is
/// not a decision.
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

    // The manifest.
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

    // And the signature, hung from it.
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

    // And the service has to release the database so that another process
    // can read it.
    support::stop(&mut service);

    // The inspector is another process, and cannot see what this test
    // has in memory: the digests travel through the environment.
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
// What the registry does not let through.
// ---------------------------------------------------------------------------

/// A `subject` that is not a digest is not published.
///
/// The `subject`'s digest travels to the catalog, and the catalog rereads it with
/// the rule of a digest: 64 hexadecimal characters. If the push accepted a
/// half-written digest, the manifest would be published with a 201 and
/// would be left **unreadable**: the asset read fails as corrupt and the
/// manifest could not be downloaded again either by its digest or by its
/// tag. A 201 with a manifest that cannot be read is worse than
/// a 400.
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
    // The `subject` is what is wrong: a half-written digest.
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

    // And it is not left published: a manifest that cannot be read must not
    // be left half-published.
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

/// A descriptor that **says** it is the empty one but names other
/// content is not the empty one.
///
/// The empty descriptor is the two bytes `{}`, and its digest is that of those
/// two bytes. A descriptor that says "empty" and names a digest that
/// is not that of `{}` is saying one thing and another: a registry that
/// took it for the empty one would stop requiring a content that the manifest
/// names, and would publish a manifest that cannot be downloaded.
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

/// The tags of another account are not seen.
///
/// The tag list comes from the catalog, and the catalog filters by
/// account and by repository. A regression that removed that filter would not be
/// detected by any other test: the suite would stay green and the
/// tags of another account would be visible.
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

/// An `n` that is not a number is a 400, not a 405.
///
/// The parameter is an integer, and an integer that is not one is a badly
/// written request: the reference registry answers with a 400 and the
/// code `PAGINATION_NUMBER_INVALID`, and a client that looks at the code
/// sees the same as it would in any other registry. A 405 would say that
/// the registry does not know how to handle the request, and it does.
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

/// The channel inspector, in a separate process.
///
/// It looks at the catalog from another process, with its own client of the
/// deployment's Dgraph namespace, as an operator would.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo de los tests del indice OCI"]
async fn the_signed_manifest_is_still_in_quarantine() {
    let Some(db_path) = std::env::var(SEED_DB).ok() else {
        return;
    };

    // `HODEI_OCI_INDEX_SEED_DB` carries the locator of the deployment's graph
    // tenant: the inspector reads through the real Dgraph catalog store.
    let tenant = support::graph_profile::GraphTenant::from_locator(&db_path)
        .expect("the locator names a graph tenant");
    let catalog = tenant.catalog_store().await;

    // What matters: **neither** of the two references has left
    // quarantine. The signature is one more referrer, and a referrer does not promote
    // anyone: neither the manifest it hangs from, nor itself.
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

/// The digests of the manifests that the test publishes.
///
/// The inspector runs in another process and cannot see what the test
/// has done in memory: the digests go through the environment.
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
