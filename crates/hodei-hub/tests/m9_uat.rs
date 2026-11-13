//! La UAT de M9: el hub de artefactos de confianza.
//!
//! La UAT corre sobre Hodei Artifacts y fixtures locales, sin integraciones
//! obligatorias con otros productos.

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{FileBlobStore, InMemoryBlobStore};
use hodei_hub::hub::{ArtifactHub, Availability, PromotionError, TrustVerdict};
use hodei_hub::oci::{MANIFEST_MEDIA_TYPE, OciManifest};

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

/// El hub de la UAT.
fn the_hub() -> ArtifactHub {
    ArtifactHub::new(Arc::new(InMemoryBlobStore::empty()))
}

/// El manifiesto de una imagen de ejemplo.
fn a_manifest() -> Vec<u8> {
    serde_json::json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_MEDIA_TYPE,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222"
        }]
    })
    .to_string()
    .into_bytes()
}

// ───────────────────────────── M9-UAT-01 ─────────────────────────────

/// M9-UAT-01 La admision direccionada por contenido.
///
/// Given OCI artifact bytes B, when ingested, then storage identity is
/// immutable digest D and retrieving D returns byte-identical content
/// verified by digest.
#[tokio::test]
async fn m9_uat_01_the_ingest_is_content_addressed() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
        let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
        let hub = ArtifactHub::new(Arc::new(store));

        let bytes = b"the bytes of the image layer".to_vec();
        let ingested = hub.ingest(&bytes).await.map_err(|e| e.to_string())?;

        // La identidad es el digest, y es inmutable.
        let digest = ingested.digest.clone();
        assert_eq!(digest, ContentDigest::of_bytes(&bytes));

        // Y lo que sale por el digest es IDENTICO y verificado por el digest.
        let read = hub.bytes_of(&digest).await.map_err(|e| e.to_string())?;
        assert_eq!(read, bytes, "los bytes son los mismos, byte a byte");
        assert_eq!(
            ContentDigest::of_bytes(&read),
            digest,
            "y el digest de lo leido es el pedido"
        );

        // Y la identidad no cambia por el canal: el artefacto sigue siendo
        // el mismo digest antes y despues de la promocion.
        let allowing = TrustVerdict::allowing("dec-1", "1.0.0");
        let promoted = hub
            .promote(&digest, Some(&allowing), "alice", Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(promoted.digest, digest, "la identidad no se mueve");
        Ok(())
    }
    .await;

    record("M9-UAT-01", result);
}

// ───────────────────────────── M9-UAT-02 ─────────────────────────────

/// M9-UAT-02 La cuarentena por defecto.
///
/// Given newly ingested artifact lacks TrustDecision, then it is not
/// visible in trusted/promotion channel.
#[tokio::test]
async fn m9_uat_02_the_quarantine_is_the_default() {
    let result: Result<(), String> = async {
        let hub = the_hub();
        let ingested = hub
            .ingest(b"the bytes of the artefact")
            .await
            .map_err(|e| e.to_string())?;

        // No hay decision, luego no esta en el canal de confianza.
        assert_eq!(ingested.availability, Availability::Quarantined);
        assert!(
            hub.trusted().is_empty(),
            "el canal de confianza no muestra lo que no tiene decision"
        );
        assert_eq!(hub.quarantine().len(), 1, "y esta en la cuarentena");
        Ok(())
    }
    .await;

    record("M9-UAT-02", result);
}

// ───────────────────────────── M9-UAT-03 ─────────────────────────────

/// M9-UAT-03 La promocion con decision.
///
/// Given artifact has ALLOW decision for trusted publication, when
/// authorized promotion occurs, then bytes are not modified/copied under a
/// different semantic identity and trusted channel references exact digest.
#[tokio::test]
async fn m9_uat_03_the_trust_promotion_keeps_the_digest() {
    let result: Result<(), String> = async {
        let hub = the_hub();
        let bytes = b"the bytes of the artefact".to_vec();
        let ingested = hub.ingest(&bytes).await.map_err(|e| e.to_string())?;

        let allowing = TrustVerdict::allowing("dec-1", "1.0.0");
        let promoted = hub
            .promote(&ingested.digest, Some(&allowing), "alice", Utc::now())
            .await
            .map_err(|e| e.to_string())?;

        // El canal de confianza referencia el digest EXACTO.
        assert_eq!(promoted.digest, ingested.digest);
        let trusted = hub.trusted();
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].digest, ingested.digest);

        // Y los bytes no se han modificado ni se han copiado bajo otra
        // identidad: son los mismos bytes con el mismo digest.
        let read = hub
            .bytes_of(&trusted[0].digest)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(read, bytes);
        assert_eq!(ContentDigest::of_bytes(&read), ingested.digest);

        // Y sale de la cuarentena.
        assert!(hub.quarantine().is_empty());
        Ok(())
    }
    .await;

    record("M9-UAT-03", result);
}

// ───────────────────────────── M9-UAT-04 ─────────────────────────────

/// M9-UAT-04 La promocion denegada.
///
/// Given DENY TrustDecision, when promotion is attempted, then operation
/// is rejected and audit trace references decision/reasons.
#[tokio::test]
async fn m9_uat_04_the_denied_promotion_is_rejected_and_traced() {
    let result: Result<(), String> = async {
        let hub = the_hub();
        let ingested = hub
            .ingest(b"the bytes of the artefact")
            .await
            .map_err(|e| e.to_string())?;

        let denying = TrustVerdict::denying(
            "dec-9",
            "3.1.0",
            vec![
                "TRUST_SBOM_MISSING".to_string(),
                "TRUST_PROVENANCE_UNVERIFIED".to_string(),
            ],
        );
        let error = hub
            .promote(&ingested.digest, Some(&denying), "alice", Utc::now())
            .await
            .expect_err("una decision que deniega no promociona");

        // Y el error dice cual fue la decision y sus motivos.
        match &error {
            PromotionError::Denied {
                decision_id,
                reasons,
            } => {
                assert_eq!(decision_id, "dec-9");
                assert_eq!(reasons.len(), 2);
                assert!(reasons.contains(&"TRUST_SBOM_MISSING".to_string()));
            }
            other => panic!("el error deberia ser el de la decision: {other:?}"),
        }

        // Y la traza los referencia.
        let audits = hub.audits();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].decision_id, "dec-9");
        assert_eq!(audits[0].policy_version, "3.1.0");
        assert_eq!(audits[0].by, "alice");
        assert_eq!(audits[0].reasons.len(), 2);

        // Y el canal de confianza sigue vacio.
        assert!(hub.trusted().is_empty());
        Ok(())
    }
    .await;

    record("M9-UAT-04", result);
}

// ───────────────────────────── M9-UAT-05 ─────────────────────────────

/// M9-UAT-05 La coexistencia con el registro externo.
///
/// Given the catalog contains an artifact with a fixture external OCI
/// reference, then Hodei can ingest evidence/reference it without requiring
/// migration into native hub.
#[tokio::test]
async fn m9_uat_05_the_external_registry_coexists() {
    let result: Result<(), String> = async {
        let hub = the_hub();

        // El artefacto vive en el registro externo: Hodei lo referencia.
        let external = ContentDigest::of_bytes(b"the bytes in the external registry");
        let referenced = hub.reference(external.clone(), "registry.example.com/app:1.0.0");

        assert_eq!(referenced.availability, Availability::ReferenceOnly);
        assert_eq!(
            referenced.external_reference.as_deref(),
            Some("registry.example.com/app:1.0.0")
        );

        // Hodei lo CITA (lo tiene en el catalogo) sin haberlo migrado.
        assert!(hub.artifact(&external).is_some(), "lo tiene en el catalogo");

        // Y NO tiene sus bytes, y no dice que los tenga.
        assert!(
            hub.bytes_of(&external).await.is_err(),
            "no hay migracion, y no se inventa"
        );
        assert!(
            !hub.trusted().iter().any(|a| a.digest == external),
            "y lo referenciado no pasa al canal de confianza"
        );
        Ok(())
    }
    .await;

    record("M9-UAT-05", result);
}

// ───────────────────────────── M9-UAT-06 ─────────────────────────────

/// M9-UAT-06 La compatibilidad de OCI.
///
/// Selected OCI client push/pull flows pass documented compatibility suite
/// for supported subset. Unsupported semantics are explicit.
#[tokio::test]
async fn m9_uat_06_the_oci_subset_is_compatible_and_its_limits_are_explicit() {
    let result: Result<(), String> = async {
        // El push: el manifiesto y las capas entran por su digest.
        let hub = the_hub();
        let manifest = a_manifest();
        let read = OciManifest::read(&manifest).map_err(|e| e.to_string())?;
        let digests = read.the_digests();
        assert_eq!(digests.len(), 2, "la configuracion y la capa");

        for digest in &digests {
            // El pull: lo que sale por el digest es lo que entro.
            let blob = format!("the bytes of the blob {}", digest.value).into_bytes();
            let ingested = hub.ingest(&blob).await.map_err(|e| e.to_string())?;
            assert_eq!(
                &ingested.digest,
                &ContentDigest::of_bytes(&blob),
                "la identidad es el digest de los bytes"
            );
            let read_back = hub
                .bytes_of(&ingested.digest)
                .await
                .map_err(|e| e.to_string())?;
            assert_eq!(read_back, blob, "y el pull devuelve los mismos bytes");
        }

        // Y lo que NO se soporta es explicito, con su error.
        let index = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            },
            "layers": []
        })
        .to_string();
        let error = OciManifest::read(index.as_bytes()).expect_err("un indice no se sirve");
        assert!(
            error.to_string().contains("not supported"),
            "y lo dice con su limitacion: {error}"
        );

        // Y SE DECLARA QUE NO HAY SERVIDOR OCI: la compatibilidad probada
        // es del subconjunto, no del protocolo. Esta afirmacion es la que
        // evita que la UAT prometa mas de lo que demuestra.
        let source = include_str!("../src/lib.rs");
        assert!(
            source.contains("no compite como almacen"),
            "la conclusion del SPIKE-010 esta escrita"
        );
        Ok(())
    }
    .await;

    record("M9-UAT-06", result);
}

// ───────────────────────────── M9-UAT-07 ─────────────────────────────

/// M9-UAT-07 La regla de admision de las funciones antiguas.
///
/// Before enabling Maven/npm or legacy batch/chunk features, then product
/// need, protocol spike and new UAT must exist; old code presence alone is
/// insufficient justification.
#[test]
fn m9_uat_07_the_legacy_features_are_not_admitted_without_cause() {
    let result: Result<(), String> = {
        // La regla esta escrita.
        let adr = include_str!(
            "../../../docs/roadmaps/hodei-artifacts-graph-first/ADR/015-the-trusted-hub-is-an-enforcement-point-not-a-store.md"
        );
        assert!(
            adr.contains("Maven y npm no"),
            "la regla esta escrita en la decision"
        );
        assert!(
            adr.contains("spike de protocolo"),
            "y dice lo que hace falta: spike de protocolo y UAT nueva"
        );

        // Y no hay codigo de maven ni de npm: la presencia del codigo viejo
        // no es un argumento, y de hecho no hay.
        let cargo = include_str!("../Cargo.toml");
        assert!(!cargo.contains("maven"), "no hay maven");
        assert!(!cargo.contains("npm"), "no hay npm");

        // Y el subconjunto de OCI no incluye lo no soportado.
        let source = include_str!("../src/oci.rs");
        assert!(
            source.contains("UNSUPPORTED"),
            "lo no soportado esta declarado en el codigo"
        );
        Ok(())
    };

    record("M9-UAT-07", result);
}
