//! The OCI Distribution pull slice (HSP-100 / HAR-06), against the wire.
//!
//! `hodei-hub::oci` already reads manifests as **types**; this suite tests the
//! protocol **for real**: a real binary, a real catalog, a real byte
//! store and a real `curl`, over the `/v2` endpoints of the OCI
//! distribution.
//!
//! The slice is **pull**: `GET /v2/`, blobs and manifests by digest and by
//! tag, with type negotiation. Push is not announced yet.
//!
//! The fixtures (blobs, manifest, tag) are seeded in a **separate
//! process**, like the rest of the catalog, against the deployment's
//! Dgraph namespace.

mod support;

use std::path::Path;

use hodei_catalog::CatalogWriter;
use hodei_hub::blob::BlobStore;
use support::{
    Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// The media type of an OCI manifest.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
/// The media type of a Docker schema2 manifest.
const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
/// A media type this registry does NOT support in this slice (index).
const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// A type this registry does **not** serve: the Notary signature.
///
/// The OCI index is already served --HSP-102 published it--, so the
/// example of "a type that is not served" has to be one that is never
/// going to be served, and not the one that used to be.
const UNSERVED_MEDIA_TYPE: &str = "application/vnd.cncf.notary.signature";

/// The catalog seeder (repositories per account).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The tenant locator (base) and the CAS path the OCI fixture uses.
const SEED_DB: &str = "HODEI_OCI_SEED_DB";
const SEED_CAS: &str = "HODEI_OCI_SEED_CAS";

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
///
/// The manifest is just another blob: its digest is that of its own bytes,
/// and its descriptors point to the digests of the configuration and of the
/// layer. The bytes that are served are EXACTLY these.
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

/// The digest of a content, without the algorithm prefix.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// The OCI fixture's path, in its own process.
///
/// Like the catalog seeder: the harness passes it the database and
/// CAS paths through the environment, this one writes it and ends.
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

    // The configuration, the layer and the OCI manifest, as blobs of the
    // store and as assets of the first account's repository.
    let config = a_config();
    let layer = a_layer(1);
    let manifest = a_manifest(&the_digest(&config), &the_digest(&layer), OCI_MANIFEST);

    // The v1 tag is born with the manifest's publication: it points to
    // the coordinates of that asset. The content's identity is still
    // the manifest's digest; the tag is the mutable reference
    // that resolves it.
    // The registry has both manifests: the OCI one and the Docker schema2 one.
    // A client asks for the one it wants by its digest, and the registry serves it
    // with the media type the manifest itself declares.
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

/// Seeds the catalog and the OCI fixtures in separate processes.
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

/// The OCI path of a resource of the first account's repository.
fn the_oci_path(kind: &str, reference: &str) -> String {
    format!("/v2/{FIRST_REPOSITORY}/{kind}/{reference}")
}

/// The reference of a blob or a manifest by digest.
///
/// The digest carries its algorithm: `sha256:<hex>`. It is what the spec says
/// and what real clients send.
fn the_digest_reference(digest: &str) -> String {
    format!("sha256:{digest}")
}

/// Downloads the response to a file and returns it.
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

/// The distribution API's version header.
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

/// A blob referenced by the repository is served by its digest, with
/// its OCI headers; HEAD describes it without fetching it.
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

    // And HEAD describes it without fetching it.
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

/// The manifest is served by its digest, with its media type.
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

/// The tag resolves to the content without becoming it.
///
/// The oracle of AC#2: serving by tag returns EXACTLY the same
/// bytes and the same digest as serving by the manifest's digest. The
/// tag is the mutable reference; the content's identity is the
/// digest, and the response says so.
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

/// Type negotiation is explicit: OCI and Docker schema2 are served;
/// a client that only accepts unsupported types receives the OCI error.
#[test]
fn the_accept_negotiation_is_explicit_and_unsupported_types_fail_with_oci_errors() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Docker schema2 is also served (the negotiation names it).
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

    // And a client that ONLY accepts an unsupported type receives the OCI
    // unknown-manifest error, not a misleading 200.
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

/// Failures speak the OCI dialect: envelope with code, and without leaking
/// the existence of what belongs to others.
#[test]
fn the_errors_speak_oci_and_do_not_leak_the_other_account() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");

    // Unknown blob: 404 with the BLOB_UNKNOWN code.
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

    // Unknown manifest: 404 with MANIFEST_UNKNOWN.
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

    // Repository the account does not have: 404 NAME_UNKNOWN, without saying
    // whether it exists in another account.
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

/// Malformed paths are rejected with OCI codes, not with a 500 or
/// with a router 404 that confuses absence with error.
#[test]
fn malformed_names_and_digests_get_oci_client_errors() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // A name with traversal: 400 NAME_INVALID.
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

    // A malformed digest: 400 DIGEST_INVALID.
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

/// A blob that is in the byte store but that the repository does not
/// reference is NOT served.
///
/// AC#4: the catalog's reference is what authorizes serving. Orphaned
/// CAS bytes are not repository content.
#[tokio::test]
async fn a_blob_that_the_repository_does_not_reference_is_not_served() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // **Real** bytes in the CAS (the store verifies them on read, so
    // a hand-made file named after the digest would be of little use), but
    // without a row in the catalog of the repository that references them.
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

/// A row that only NAMES the path does not authorize serving that digest.
///
/// The CAS is a common tree: one account's bytes are there, next to
/// another's. If the pull served the bytes of the **requested** digest instead of
/// those the row **references**, a row that names a path
/// would serve the content of another blob, which may belong to another account.
///
/// It is the reverse of the orphan blob case: here the row exists and the
/// content is someone else's.
#[tokio::test]
async fn a_blob_that_the_repository_only_names_by_path_is_not_served() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let acme = support::token(FIRST_ACCOUNT, "alice");
    let other = support::token(SECOND_ACCOUNT, "bob");

    // Another account's content: it lives in the global CAS.
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

    // The requesting account only NAMES the path; the row points to other
    // content. Same size on purpose: the size check
    // must not mask the failure.
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

    // The pull by that digest does NOT return the foreign bytes.
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

/// A manifest that declares a type this registry does NOT serve receives an
/// OCI error, not a 200 with another type.
///
/// Serving it with a different type is a 200 that pretends to be what the
/// client asked for, and the client that asked for it did not receive it.
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

    // And with an `Accept` that does not limit it, the registry rejects it all the same:
    // the type the manifest declares is what decides.
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

/// Negotiation limits the type that is served.
///
/// A client that only accepts the OCI type does not receive a Docker
/// schema2 manifest with a 200: it receives the OCI error. Serving it another type would be
/// answering with something it did not ask for.
#[tokio::test]
async fn a_client_that_only_accepts_one_type_is_not_served_another() {
    let deployment = a_oci_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The Docker schema2 manifest is seeded in the fixture.
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
