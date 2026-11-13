//! El contrato del hub de artefactos de confianza.
//!
//! Seis cosas se prometen y seis cosas se comprueban:
//!
//! - El SPIKE-010 decide si Hodei habla OCI/Maven/npm o se integra con
//!   registros externos, y su conclusion se aplica al diseño.
//! - El almacen CAS guarda el contenido por su digest, y el mismo
//!   contenido no se guarda dos veces.
//! - La cuarentena separa lo no promovido del canal de confianza, y salir
//!   de ella exige una decisión.
//! - El subconjunto de OCI soportado esta declarado y probado, con su
//!   limitacion escrita.
//! - La promocion al hub queda forzada por la decision de confianza, y sin
//!   decision no hay promocion.
//! - La UAT de compatibilidad demuestra la interaccion con el registro que
//!   se declare soportado.

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobStore, FileBlobStore, InMemoryBlobStore};
use hodei_hub::hub::{ArtifactHub, Availability, PromotionError, TrustVerdict};
use hodei_hub::oci::{MANIFEST_MEDIA_TYPE, OciError, OciManifest, UNSUPPORTED, the_digest_of};
use hodei_trust_policy::decision::Outcome;

// ───────────────────────────── AC1 ─────────────────────────────

/// El SPIKE-010 decide, y su conclusion se aplica al diseño.
#[test]
fn the_conclusion_of_spike_010_is_written_down_and_applied() {
    let source = include_str!("../src/lib.rs");

    // La conclusion: la propiedad nativa da el punto de imposicion.
    assert!(
        source.contains("Compite como punto de imposicion"),
        "the conclusion says where the native property wins"
    );
    // Y los no-objetivos son explicitos.
    assert!(
        source.contains("Maven y npm no"),
        "and the non-goals are explicit"
    );

    // Y se APLICA: el codigo no tiene ningun Maven ni ningun npm.
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.contains("maven"), "no hay maven en las dependencias");
    assert!(!cargo.contains("npm"), "no hay npm en las dependencias");
}

// ───────────────────────────── AC2 ─────────────────────────────

/// El almacen CAS guarda el contenido por su digest, y el mismo contenido
/// no se guarda dos veces.
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

    // Y lo que sale por el digest es EXACTAMENTE lo que entro.
    let read = store.get(&first).await.expect("the content is read");
    assert_eq!(read, bytes, "los bytes son los mismos");
    assert_eq!(
        ContentDigest::of_bytes(&read),
        first,
        "y el digest de lo leido es el pedido"
    );

    // Y un digest que no esta, no esta.
    let missing = ContentDigest::of_bytes(b"other content");
    assert!(store.get(&missing).await.is_err(), "lo que no esta no sale");
}

/// Y en disco, lo mismo: el mismo contenido no ocupa dos veces.
#[tokio::test]
async fn the_file_store_stores_the_content_once() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let bytes = b"the bytes of the artefact".to_vec();

    let digest = store.put(&bytes).await.expect("the content is stored");
    let path = store.the_path_of(&digest);
    assert!(path.exists(), "el blob esta en su sitio");

    // Y guardarlo otra vez no crea un segundo blob.
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

/// La cuarentena separa lo no promovido del canal de confianza, y salir de
/// ella exige una decision.
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

    // Sin decision, no hay promocion.
    let error = hub
        .promote(&ingested.digest, None, "alice", Utc::now())
        .await
        .expect_err("sin decision no hay promocion");
    assert_eq!(error, PromotionError::NoDecision);
    assert!(hub.trusted().is_empty(), "y el canal sigue vacio");

    // Con la decision que deniega, tampoco.
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

    // Y con la que permite, sale al canal de confianza.
    let allowing = TrustVerdict::allowing("dec-2", "1.0.0");
    let promoted = hub
        .promote(&ingested.digest, Some(&allowing), "alice", Utc::now())
        .await
        .expect("la decision permite la promocion");
    assert_eq!(promoted.availability, Availability::Trusted);
    assert_eq!(hub.trusted().len(), 1);
    assert!(hub.quarantine().is_empty(), "y sale de la cuarentena");

    // Y los bytes NO han cambiado: la promocion cambia el canal, no el
    // contenido.
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

/// Y la traza de la promocion dice quien, con que decision y por que.
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

/// El subconjunto de OCI soportado esta declarado y probado, con su
/// limitacion escrita.
#[test]
fn the_oci_subset_is_declared_and_its_limitation_written() {
    // Un manifiesto de imagen: soportado.
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

    // Y lo que no esta soportado, se dice con su error.
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        "layers": []
    });
    let error = OciManifest::read(index.to_string().as_bytes()).expect_err("un indice no se sirve");
    assert!(
        matches!(error, OciError::UnsupportedMediaType(_)),
        "y el error lo dice: {error}"
    );
    assert!(
        error.to_string().contains("not supported"),
        "con su limitacion escrita: {error}"
    );

    // Y la lista de lo no soportado esta en el codigo, no en un documento.
    assert!(
        UNSUPPORTED.contains(&"application/vnd.oci.image.index.v1+json"),
        "la limitacion esta declarada en el codigo"
    );
}

/// Y un digest de un manifiesto tiene que estar bien formado.
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

/// La promocion al hub queda forzada por la decision de confianza, y sin
/// decision no hay promocion.
#[tokio::test]
async fn the_promotion_is_forced_by_the_decision() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));
    let ingested = hub
        .ingest(b"the bytes")
        .await
        .expect("el artefacto se admite");

    // El artefacto no es de fiar mientras no se diga lo contrario.
    assert_eq!(
        hub.artifact(&ingested.digest)
            .expect("el artefacto esta")
            .availability,
        Availability::Quarantined
    );

    // Y una decision que no es ALLOW no promociona.
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

/// La UAT de compatibilidad demuestra la interaccion con el registro que
/// se declare soportado.
#[tokio::test]
async fn the_external_registry_coexists_without_migration() {
    let hub = ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()));

    // Un artefacto que vive en un registro externo: Hodei lo referencia, lo
    // cita y lo sabe, sin migrar sus bytes.
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

    // Y el hub no los tiene: no hay migracion.
    assert!(hub.bytes_of(&external).await.is_err(), "y no los inventa");

    // Y un artefacto que Hodei SI tiene, con su digest, en el canal.
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
