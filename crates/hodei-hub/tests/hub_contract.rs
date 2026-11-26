//! The contract of the trusted artifact hub.
//!
//! Six things are promised and six things are checked:
//!
//! - SPIKE-010 decides whether Hodei speaks OCI/Maven/npm or integrates with
//!   external registries, and its conclusion is applied to the design.
//! - The CAS store keeps content by its digest, and the same
//!   content is not stored twice.
//! - Quarantine separates what is not promoted from the trust channel, and leaving
//!   it requires a decision.
//! - The supported OCI subset is declared and tested, with its
//!   limitation written down.
//! - Promotion to the hub is enforced by the trust decision, and without a
//!   decision there is no promotion.
//! - The compatibility UAT demonstrates the interaction with the registry
//!   declared as supported.

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobStore, FileBlobStore, InMemoryBlobStore};
use hodei_hub::hub::{ArtifactHub, Availability, PromotionError, TrustVerdict};
use hodei_hub::oci::{
    MANIFEST_MEDIA_TYPE, ManifestKind, OciError, OciManifest, SUPPORTED_MANIFESTS, UNSUPPORTED,
    the_digest_of, the_kind_of,
};
use hodei_trust_policy::decision::Outcome;

// ───────────────────────────── AC1 ─────────────────────────────

/// SPIKE-010 decides, and its conclusion is applied to the design.
#[test]
fn the_conclusion_of_spike_010_is_written_down_and_applied() {
    let source = include_str!("../src/lib.rs");

    // The conclusion: native ownership gives the enforcement point.
    assert!(
        source.contains("Compite como punto de imposicion"),
        "the conclusion says where the native property wins"
    );
    // And the non-goals are explicit.
    assert!(
        source.contains("Maven y npm no"),
        "and the non-goals are explicit"
    );

    // And it is APPLIED: the code has no Maven or npm at all.
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.contains("maven"), "no hay maven en las dependencias");
    assert!(!cargo.contains("npm"), "no hay npm en las dependencias");
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The CAS store keeps content by its digest, and the same content
/// is not stored twice.
#[tokio::test]
async fn the_content_is_stored_by_its_digest_and_only_once() {
    let store = InMemoryBlobStore::empty();
    let bytes = b"the bytes of the artefact".to_vec();

    let first = store.put(&bytes).await.expect("the content is stored");
    let second = store
        .put(&bytes)
        .await
        .expect("the content is stored again");

    assert_eq!(first, second, "el mismo contenido tiene el mismo digest");
    assert!(store.exists(&first).await.expect("the store answers"));

    // And what comes out by the digest is EXACTLY what went in.
    let read = store.get(&first).await.expect("the content is read");
    assert_eq!(read, bytes, "los bytes son los mismos");
    assert_eq!(
        ContentDigest::of_bytes(&read),
        first,
        "y el digest de lo leido es el pedido"
    );

    // And a digest that is not there, is not there.
    let missing = ContentDigest::of_bytes(b"other content");
    assert!(store.get(&missing).await.is_err(), "lo que no esta no sale");
}

/// And on disk, the same: the same content does not take up space twice.
#[tokio::test]
async fn the_file_store_stores_the_content_once() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let bytes = b"the bytes of the artefact".to_vec();

    let digest = store.put(&bytes).await.expect("the content is stored");
    let path = store.the_path_of(&digest);
    assert!(path.exists(), "el blob esta en su sitio");

    // And storing it again does not create a second blob.
    let before = std::fs::read_dir(tmp.path())
        .expect("el directorio se lee")
        .count();
    store
        .put(&bytes)
        .await
        .expect("the content is stored again");
    let after = std::fs::read_dir(tmp.path())
        .expect("el directorio se lee")
        .count();
    assert_eq!(before, after, "no hay un segundo blob");
}

// ───────────────────────────── AC3 ─────────────────────────────

/// Quarantine separates what is not promoted from the trust channel, and leaving
/// it requires a decision.
#[tokio::test]
async fn the_quarantine_is_the_default_and_leaving_it_needs_a_decision() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));
    let bytes = b"the bytes of the artefact".to_vec();

    let ingested = hub.ingest(&bytes).await.expect("el artefacto se admite");
    assert_eq!(
        ingested.availability,
        Availability::Quarantined,
        "un artefacto sin decision esta en cuarentena"
    );
    assert_eq!(hub.quarantine().len(), 1);
    assert!(
        hub.trusted().is_empty(),
        "y el canal de confianza esta vacio"
    );

    // Without a decision, there is no promotion.
    let error = hub
        .promote(&ingested.digest, None, "alice", Utc::now())
        .await
        .expect_err("sin decision no hay promocion");
    assert_eq!(error, PromotionError::NoDecision);
    assert!(hub.trusted().is_empty(), "y el canal sigue vacio");

    // With the decision that denies, neither.
    let denying = TrustVerdict::denying("dec-1", "1.0.0", vec!["TRUST_SBOM_MISSING".to_string()]);
    let error = hub
        .promote(&ingested.digest, Some(&denying), "alice", Utc::now())
        .await
        .expect_err("una decision que deniega no promociona");
    match error {
        PromotionError::Denied {
            decision_id,
            reasons,
        } => {
            assert_eq!(decision_id, "dec-1");
            assert_eq!(reasons, vec!["TRUST_SBOM_MISSING".to_string()]);
        }
        other => panic!("el error deberia ser el de la decision: {other:?}"),
    }
    assert!(hub.trusted().is_empty());

    // And with the one that allows, it goes out to the trust channel.
    let allowing = TrustVerdict::allowing("dec-2", "1.0.0");
    let promoted = hub
        .promote(&ingested.digest, Some(&allowing), "alice", Utc::now())
        .await
        .expect("la decision permite la promocion");
    assert_eq!(promoted.availability, Availability::Trusted);
    assert_eq!(hub.trusted().len(), 1);
    assert!(hub.quarantine().is_empty(), "y sale de la cuarentena");

    // And the bytes have NOT changed: promotion changes the channel, not the
    // content.
    assert_eq!(
        promoted.digest, ingested.digest,
        "el digest es el mismo: la promocion no muta los bytes"
    );
    let bytes_after = hub
        .bytes_of(&ingested.digest)
        .await
        .expect("los bytes se leen");
    assert_eq!(bytes_after, bytes, "y los bytes son los mismos");
}

/// And the promotion trace says who, with which decision and why.
#[tokio::test]
async fn the_promotion_is_traced() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));
    let ingested = hub
        .ingest(b"the bytes")
        .await
        .expect("el artefacto se admite");

    let denying = TrustVerdict::denying("dec-1", "2.0.0", vec!["TRUST_POLICY_DENIED".to_string()]);
    let _ = hub
        .promote(&ingested.digest, Some(&denying), "alice", Utc::now())
        .await;

    let audits = hub.audits();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].decision_id, "dec-1");
    assert_eq!(audits[0].policy_version, "2.0.0");
    assert_eq!(audits[0].by, "alice");
    assert_eq!(audits[0].reasons, vec!["TRUST_POLICY_DENIED".to_string()]);
}

// ───────────────────────────── AC4 ─────────────────────────────

/// The supported OCI subset is declared and tested, with its
/// limitation written down.
#[test]
fn the_oci_subset_is_declared_and_its_limitation_written() {
    // An image manifest: supported.
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }]
    });
    let read = OciManifest::read(manifest.to_string().as_bytes()).expect("el manifiesto se lee");
    assert_eq!(read.schema_version, 2);
    assert_eq!(read.the_digests().len(), 2, "la configuracion y la capa");

    // And what is not an image manifest is reported with its error: this
    // reader reads image manifests, not indexes.
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        "layers": []
    });
    let error = OciManifest::read(index.to_string().as_bytes())
        .expect_err("un indice no es un manifiesto de imagen");
    assert!(
        matches!(error, OciError::UnsupportedMediaType(_)),
        "y el error lo dice: {error}"
    );
    assert!(
        error.to_string().contains("not supported"),
        "con su limitacion escrita: {error}"
    );

    // The index, on the other hand, is a manifest type the registry
    // speaks: it is published and served as what it is, with its kind.
    assert!(
        SUPPORTED_MANIFESTS.contains(&"application/vnd.oci.image.index.v1+json"),
        "el indice multi-plataforma es un tipo de manifiesto soportado"
    );
    assert_eq!(
        the_kind_of("application/vnd.oci.image.index.v1+json"),
        Some(ManifestKind::Index),
        "y su clase es la de un indice"
    );
    assert_eq!(
        the_kind_of("application/vnd.oci.artifact.manifest.v1+json"),
        Some(ManifestKind::Artifact),
        "el manifiesto de artefacto es un artefacto"
    );
    assert_eq!(
        the_kind_of(MANIFEST_MEDIA_TYPE),
        Some(ManifestKind::Image),
        "y el manifiesto de imagen sigue siendo una imagen"
    );
    assert!(
        the_kind_of("application/vnd.example.nonsense").is_none(),
        "un tipo que no se habla no tiene clase"
    );

    // And the list of what is unsupported is in the code, not in a document.
    assert!(
        !UNSUPPORTED.contains(&"application/vnd.oci.image.index.v1+json"),
        "el indice ya no es una limitacion"
    );
    assert!(
        UNSUPPORTED.contains(&"application/vnd.cncf.notary.signature"),
        "la limitacion que queda esta declarada en el codigo"
    );
}

/// And a manifest's digest has to be well formed.
#[test]
fn a_malformed_digest_is_not_a_digest() {
    assert!(
        the_digest_of("sha256:aaaa").is_none(),
        "un digest corto no es un digest"
    );
    assert!(
        the_digest_of("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .is_some(),
        "uno bien formado si"
    );
}

// ───────────────────────────── AC5 ─────────────────────────────

/// Promotion to the hub is enforced by the trust decision, and without a
/// decision there is no promotion.
#[tokio::test]
async fn the_promotion_is_forced_by_the_decision() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));
    let ingested = hub
        .ingest(b"the bytes")
        .await
        .expect("el artefacto se admite");

    // The artifact is not trustworthy until said otherwise.
    assert_eq!(
        hub.artifact(&ingested.digest)
            .expect("el artefacto esta")
            .availability,
        Availability::Quarantined
    );

    // And a decision that is not ALLOW does not promote.
    let warning = TrustVerdict {
        decision_id: "dec-1".to_string(),
        outcome: Outcome::Warn,
        reasons: vec![],
        policy_version: "1.0.0".to_string(),
    };
    let error = hub
        .promote(&ingested.digest, Some(&warning), "alice", Utc::now())
        .await
        .expect_err("solo un ALLOW promociona");
    assert!(matches!(error, PromotionError::Denied { .. }), "{error:?}");
    assert!(hub.trusted().is_empty());
}

// ───────────────────────────── AC6 ─────────────────────────────

/// The compatibility UAT demonstrates the interaction with the registry
/// declared as supported.
#[tokio::test]
async fn the_external_registry_coexists_without_migration() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));

    // An artifact that lives in an external registry: Hodei references it, cites
    // it and knows about it, without migrating its bytes.
    let external = ContentDigest::of_bytes(b"the bytes in the external registry");
    let referenced = hub.reference(external.clone(), "registry.example.com/app:1.0.0");

    assert_eq!(
        referenced.availability,
        Availability::ReferenceOnly,
        "Hodei no afirma tener los bytes que no tiene"
    );
    assert_eq!(
        referenced.external_reference.as_deref(),
        Some("registry.example.com/app:1.0.0")
    );

    // And the hub does not have them: there is no migration.
    assert!(hub.bytes_of(&external).await.is_err(), "y no los inventa");

    // And an artifact that Hodei DOES have, with its digest, in the channel.
    let own = hub
        .ingest(b"the bytes of our own artefact")
        .await
        .expect("el artefacto se admite");
    let allowing = TrustVerdict::allowing("dec-1", "1.0.0");
    let promoted = hub
        .promote(&own.digest, Some(&allowing), "alice", Utc::now())
        .await
        .expect("la promocion");
    assert_eq!(promoted.digest, own.digest);
    assert_eq!(hub.trusted().len(), 1);
    assert_eq!(hub.quarantine().len(), 0);
}
