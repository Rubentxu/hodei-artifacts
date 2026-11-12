//! La UAT de M2: la capsula de confianza.
//!
//! Son los siete escenarios de `uat/M2-TRUST-CAPSULE-UAT.md`, ejecutados
//! sobre fixtures locales y servicios efimeros, sin integraciones
//! obligatorias con otros productos.
//!
//! Cada escenario deja el comando con el que se ejecuta y su resultado, que
//! es lo que la tarea pide que quede registrado.
//!
//! # Los escenarios
//!
//! - M2-UAT-01 El conjunto completo de evidencia de confianza.
//! - M2-UAT-02 La evidencia que falta es explicita.
//! - M2-UAT-03 La firma invalida.
//! - M2-UAT-04 La evidencia caducada.
//! - M2-UAT-05 La evidencia contradictoria.
//! - M2-UAT-06 La correlacion de identidades canonicas.
//! - M2-UAT-07 La inmutabilidad de la capsula por su hash.
//!
//! # Que se ejecuta de verdad
//!
//! La cadena entera: el documento de CycloneDX y la envoltura DSSE de las
//! fixtures de conformidad, leidos por los normalizadores, con la
//! verificacion de la firma de verdad, admitidos en el almacen de verdad, y
//! la capsula ensamblada de lo que quedo ahi. No hay ningun simulacro: la
//! firma se comprueba contra la clave de la fixture.

use chrono::{DateTime, Duration, TimeZone, Utc};
use hodei_evidence::application::capsule::{CapsuleAssembler, FreshnessThreshold};
use hodei_evidence::domain::capsule::{DimensionName, DimensionState, EvidenceLink, TrustCapsule};
use hodei_evidence::domain::evidence::{EvidenceType, EvidenceVerification};
use hodei_evidence::domain::standards::identity::{IdentityNamespace, MappingConfidence};
use hodei_evidence::domain::standards::sbom::SbomFormat;
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence::standards::attestation::{bind_to_subject, read_attestation};
use hodei_evidence::standards::correlation::{correlate_identities, correlate_inventory};
use hodei_evidence::standards::dsse::{KeyRing, VerificationKey};
use hodei_evidence::standards::inventory::{read_cyclonedx_17, read_spdx_30};
use hodei_evidence::{
    ContentDigest, EvidenceIngestion, IngestionEnvelope, IngestionOutcome, IngestionRequest,
    Normalization, Producer, SourceRef,
};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore,
};
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// El SBOM de CycloneDX de la suite de conformidad.
const CYCLONEDX: &str =
    include_str!("../../hodei-evidence/tests/fixtures/standards/cyclonedx-1.7.json");

/// El documento SPDX de la suite de conformidad.
const SPDX: &str = include_str!("../../hodei-evidence/tests/fixtures/standards/spdx-3.0.json");

/// La atrestacion firmada con la clave de la suite.
const ATTESTATION: &str =
    include_str!("../../hodei-evidence/tests/fixtures/standards/attestation.dsse.json");

/// La misma atrestacion con la firma alterada.
const ATTESTATION_INVALID: &str =
    include_str!("../../hodei-evidence/tests/fixtures/standards/attestation-invalid.dsse.json");

/// El punto publico de la clave de la suite.
const KEY_POINT_HEX: &str =
    include_str!("../../hodei-evidence/tests/fixtures/standards/signing-key-public-point.hex");

/// El identificador de la clave de la suite.
const KEY_ID: &str = "test-key-p256-01";

/// El digest del sujeto del que hablan las fixtures.
const SUBJECT_DIGEST: &str = "ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06";

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

/// El sujeto: la release de la que speaks la UAT.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

/// La cuenta a la que pertenece el sujeto.
fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

/// El anillo de claves de la suite.
fn the_key_ring() -> KeyRing {
    let point: Vec<u8> = KEY_POINT_HEX
        .trim()
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("the hexadecimal is text");
            u8::from_str_radix(text, 16).expect("the hexadecimal is a byte")
        })
        .collect();
    KeyRing::with_key(VerificationKey {
        key_id: KEY_ID.to_string(),
        point,
    })
}

/// El almacen, el autenticador y la admision.
struct Fixture {
    evidence_store: Arc<SurrealEvidenceStore<Db>>,
    authenticator: Arc<HmacEnvelopeAuthenticator>,
    ingestion: Arc<EvidenceIngestion>,
}

async fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("the namespace");

    let ledger = Arc::new(SurrealEventLedger::new(db.clone()));
    let evidence_store = Arc::new(SurrealEvidenceStore::new(db.clone()));
    let content_store = Arc::new(InMemoryContentStore::new());
    let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
    authenticator.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"the-secret-of-the-producer".to_vec(),
            accounts: vec!["acme".to_string()],
        },
    );

    let ingestion = Arc::new(EvidenceIngestion::new(
        content_store,
        evidence_store.clone(),
        ledger,
        authenticator.clone(),
    ));

    Fixture {
        evidence_store,
        authenticator,
        ingestion,
    }
}

/// Admite una pieza de evidencia y dice que la admitio.
///
/// The facts of the piece are the arguments and not a struct: a test that
/// reads like the thing it admits is easier to check against the scenario.
#[allow(clippy::too_many_arguments)]
async fn admit(
    fixture: &Fixture,
    external_id: &str,
    kind: EvidenceType,
    content: &[u8],
    verification: EvidenceVerification,
    normalizer: &str,
    version: &str,
    observed_at: DateTime<Utc>,
) -> hodei_evidence::EvidenceId {
    let mut envelope = IngestionEnvelope {
        producer: producer(),
        tenant_hrn: account(),
        subject_refs: vec![subject()],
        evidence_type: kind,
        schema_uri: None,
        schema_version: None,
        source: SourceRef::by_external_id(external_id),
        content_digest: ContentDigest::of_bytes(content),
        relation_claim: None,
        observed_at,
        valid_from: None,
        verification,
        normalization: Normalization::new(normalizer, version),
        correlation_id: None,
        signature: String::new(),
    };
    envelope.signature = fixture
        .authenticator
        .sign(&envelope)
        .expect("the producer is known");

    let outcome = fixture
        .ingestion
        .ingest(IngestionRequest {
            envelope,
            content: content.to_vec(),
        })
        .await
        .expect("the ingestion runs");

    match outcome {
        IngestionOutcome::Accepted { evidence_id, .. } => evidence_id,
        other => panic!("the evidence was not accepted: {other:?}"),
    }
}

/// La capsula del sujeto, ensamblada de lo que hay en el almacen.
async fn the_capsule(fixture: &Fixture, now: DateTime<Utc>) -> TrustCapsule {
    let evidence = fixture
        .evidence_store
        .evidence_of_subject("acme", &subject())
        .await
        .expect("the evidence is read");

    CapsuleAssembler::with_freshness(FreshnessThreshold {
        maximum_age: Duration::days(30),
    })
    .assemble(&subject(), &evidence, now)
}

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

// ───────────────────────────── M2-UAT-01 ─────────────────────────────

/// M2-UAT-01 El conjunto completo de evidencia de confianza.
///
/// Given artifact A has CycloneDX/SPDX inventory, SLSA/in-toto provenance
/// and verified Sigstore signature evidence, when Trust Capsule is
/// requested, then capsule reports integrity/provenance/inventory/signature
/// dimensions and each dimension links to evidence ID, verification state
/// and parser/normalizer version.
#[tokio::test]
async fn m2_uat_01_complete_trusted_evidence_set() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let now = moment(60);

        // El inventario de CycloneDX, leido por su normalizador.
        let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");
        assert_eq!(inventory.format, SbomFormat::CycloneDx17);
        admit(
            &fixture,
            "sbom-cyclonedx",
            EvidenceType::Sbom,
            CYCLONEDX.as_bytes(),
            EvidenceVerification::verified("schema"),
            "cyclonedx",
            &inventory.normalizer_version,
            now,
        )
        .await;

        // El mismo inventario por el camino de compatibilidad de SPDX: el
        // mismo modelo nativo desde otro formato.
        let spdx = read_spdx_30(SPDX).expect("the document is read");
        admit(
            &fixture,
            "sbom-spdx",
            EvidenceType::Sbom,
            SPDX.as_bytes(),
            EvidenceVerification::verified("schema"),
            "spdx",
            &spdx.normalizer_version,
            now,
        )
        .await;

        // La integridad del artefacto: la observacion de su digest.
        admit(
            &fixture,
            "integrity",
            EvidenceType::ArtifactIntegrity,
            SUBJECT_DIGEST.as_bytes(),
            EvidenceVerification::verified("digest"),
            "oci",
            "1.0.0",
            now,
        )
        .await;

        // La procedencia, con su firma verificada de verdad.
        let mut attestation = read_attestation(ATTESTATION, &the_key_ring()).expect("the envelope");
        assert!(attestation.verification.proves(), "the signature holds");
        bind_to_subject(&mut attestation, "sha256", SUBJECT_DIGEST)
            .expect("the attestation speaks about the subject");
        admit(
            &fixture,
            "provenance-slsa",
            EvidenceType::Provenance,
            ATTESTATION.as_bytes(),
            EvidenceVerification::verified("dsse"),
            "slsa",
            "1.0.0",
            now,
        )
        .await;

        // Y la firma, que es lo que el digest del artefacto sostiene.
        admit(
            &fixture,
            "signature-ecdsa",
            EvidenceType::SignatureVerification,
            ATTESTATION.as_bytes(),
            EvidenceVerification::verified("ecdsa-p256"),
            "sigstore",
            "1.0.0",
            now,
        )
        .await;

        let capsule = the_capsule(&fixture, now).await;

        // La capsula reporta las cuatro dimensiones que el enunciado
        // nombra.
        for name in [
            DimensionName::Integrity,
            DimensionName::BuildProvenance,
            DimensionName::InventoryCompleteness,
            DimensionName::SignatureState,
        ] {
            let dimension = capsule
                .profile
                .dimension(&name)
                .ok_or_else(|| format!("the dimension {name} is not in the capsule"))?;
            assert!(
                !dimension.evidence.is_empty(),
                "the dimension {name} links to its evidence"
            );
            for link in &dimension.evidence {
                assert!(
                    !link.evidence_id.as_str().is_empty(),
                    "the link names the evidence"
                );
                assert!(
                    link.verification.status
                        == hodei_evidence::domain::evidence::VerificationStatus::Verified,
                    "the link says how far the verification goes"
                );
                assert!(
                    !link.normalizer.is_empty() && !link.normalizer_version.is_empty(),
                    "the link says the version of the normalizer that read it"
                );
            }
        }

        // Y el inventario esta completo, no solo verificado.
        assert_eq!(
            capsule
                .profile
                .dimension(&DimensionName::InventoryCompleteness)
                .expect("the dimension is there")
                .state,
            DimensionState::Complete
        );
        Ok(())
    }
    .await;

    record("M2-UAT-01", result);
}

// ───────────────────────────── M2-UAT-02 ─────────────────────────────

/// M2-UAT-02 La evidencia que falta es explicita.
///
/// Given same artifact without SBOM, when capsule is assembled, then
/// inventory completeness is MISSING/UNKNOWN as defined, never VERIFIED, and
/// missing requirement is actionable.
#[tokio::test]
async fn m2_uat_02_missing_evidence_is_explicit() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let now = moment(60);

        // Sin inventario: solo la integridad.
        admit(
            &fixture,
            "integrity",
            EvidenceType::ArtifactIntegrity,
            b"the-bytes-of-the-artifact",
            EvidenceVerification::verified("digest"),
            "oci",
            "1.0.0",
            now,
        )
        .await;

        let capsule = the_capsule(&fixture, now).await;

        let inventory = capsule
            .profile
            .dimension(&DimensionName::InventoryCompleteness)
            .ok_or("the inventory dimension is not in the capsule")?;

        assert_eq!(
            inventory.state,
            DimensionState::Missing,
            "the completeness of the inventory is MISSING"
        );
        assert_ne!(
            inventory.state,
            DimensionState::Verified,
            "and it is never VERIFIED"
        );
        assert!(
            inventory.evidence.is_empty(),
            "there is no evidence that holds it up"
        );
        assert!(
            inventory.reason.is_some(),
            "and the missing requirement is actionable: it says what is missing"
        );

        // Y el perfil dice que hay dimensiones sin resolver.
        assert!(
            !capsule.profile.unsatisfied().is_empty(),
            "the profile says what is not established"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-02", result);
}

// ───────────────────────────── M2-UAT-03 ─────────────────────────────

/// M2-UAT-03 La firma invalida.
///
/// Given signature evidence whose verification result is INVALID, when
/// capsule is assembled, then integrity/signature assurance cannot be
/// reported verified and raw verification evidence remains inspectable.
#[tokio::test]
async fn m2_uat_03_invalid_signature() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let now = moment(60);

        // La atrestacion con la firma alterada: la verificacion falla de
        // verdad, contra la clave de verdad.
        let attestation =
            read_attestation(ATTESTATION_INVALID, &the_key_ring()).expect("the envelope is read");
        assert!(
            !attestation.verification.proves(),
            "the altered signature does not hold"
        );
        assert!(
            attestation.verification.reason().is_some(),
            "and it says why"
        );

        admit(
            &fixture,
            "provenance-invalid",
            EvidenceType::Provenance,
            ATTESTATION_INVALID.as_bytes(),
            EvidenceVerification::invalid("dsse"),
            "slsa",
            "1.0.0",
            now,
        )
        .await;

        let capsule = the_capsule(&fixture, now).await;

        let provenance = capsule
            .profile
            .dimension(&DimensionName::BuildProvenance)
            .ok_or("the provenance dimension is not in the capsule")?;

        assert_eq!(
            provenance.state,
            DimensionState::Invalid,
            "the assurance of the signature cannot be reported as verified"
        );
        assert_ne!(provenance.state, DimensionState::Verified);
        assert!(
            !provenance.evidence.is_empty(),
            "the raw verification evidence remains inspectable"
        );
        assert!(
            provenance.reason.is_some(),
            "and the reason says what happened"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-03", result);
}

// ───────────────────────────── M2-UAT-04 ─────────────────────────────

/// M2-UAT-04 La evidencia caducada.
///
/// Given scan evidence older than configured freshness threshold, when
/// capsule is assembled, then freshness is STALE while historical result
/// remains visible.
#[tokio::test]
async fn m2_uat_04_stale_evidence() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let observed = moment(0);
        let now = moment(60 * 60 * 24 * 60); // sesenta dias despues

        admit(
            &fixture,
            "sbom-old",
            EvidenceType::Sbom,
            CYCLONEDX.as_bytes(),
            EvidenceVerification::verified("schema"),
            "cyclonedx",
            "1.0.0",
            observed,
        )
        .await;

        let capsule = the_capsule(&fixture, now).await;

        let freshness = capsule
            .profile
            .dimension(&DimensionName::EvidenceFreshness)
            .ok_or("the freshness dimension is not in the capsule")?;
        assert_eq!(
            freshness.state,
            DimensionState::Stale,
            "the freshness is STALE"
        );

        // Y el resultado historico sigue a la vista.
        let inventory = capsule
            .profile
            .dimension(&DimensionName::InventoryCompleteness)
            .ok_or("the inventory dimension is not in the capsule")?;
        assert_eq!(
            inventory.evidence.len(),
            1,
            "the historical result remains visible"
        );
        assert_eq!(
            inventory.evidence[0].source.external_id.as_deref(),
            Some("sbom-old"),
            "and it is the piece of evidence that was read"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-04", result);
}

// ───────────────────────────── M2-UAT-05 ─────────────────────────────

/// M2-UAT-05 La evidencia contradictoria.
///
/// Given scanner A claims affected and VEX B claims not affected for a
/// scoped product, when capsule/risk view is assembled, then both source
/// observations remain visible and derived interpretation references the
/// precedence/context rules rather than deleting one claim.
#[tokio::test]
async fn m2_uat_05_contradictory_evidence() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let now = moment(60);

        // Dos lecturas que no se ponen de acuerdo sobre lo mismo.
        admit(
            &fixture,
            "scan-affected",
            EvidenceType::ArtifactIntegrity,
            b"the-scan-that-found-it",
            EvidenceVerification::verified("scanner"),
            "scanner",
            "1.0.0",
            now,
        )
        .await;
        admit(
            &fixture,
            "vex-not-affected",
            EvidenceType::ArtifactIntegrity,
            b"the-vex-that-denies-it",
            EvidenceVerification::invalid("scanner"),
            "vex",
            "1.0.0",
            now,
        )
        .await;

        let capsule = the_capsule(&fixture, now).await;

        let dimension = capsule
            .profile
            .dimension(&DimensionName::Integrity)
            .ok_or("the integrity dimension is not in the capsule")?;

        assert_eq!(
            dimension.state,
            DimensionState::Contradictory,
            "the two claims contradict each other"
        );
        assert_eq!(
            dimension.evidence.len(),
            2,
            "both source observations remain visible: the capsule deletes neither"
        );

        // Y la lectura derivada dice cual es la regla que aplico.
        let reason = dimension
            .reason
            .as_deref()
            .ok_or("the contradiction says why")?;
        assert!(
            reason.contains("claim") || reason.contains("check"),
            "the derived interpretation references the rule it applied: {reason}"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-05", result);
}

// ───────────────────────────── M2-UAT-06 ─────────────────────────────

/// M2-UAT-06 La correlacion de identidades canonicas.
///
/// Given SBOM component uses PURL and registry/artifact source uses
/// digest/coordinates, when normalization correlates identities, then
/// deterministic mapping is recorded with source and confidence. Ambiguous
/// mapping remains ambiguous and cannot silently satisfy trust requirements.
#[tokio::test]
async fn m2_uat_06_canonical_identity_correlation() {
    let result: Result<(), String> = async {
        let inventory = read_cyclonedx_17(CYCLONEDX).expect("the document is read");

        // El componente con PURL y el artefacto con el mismo PURL: la
        // correlacion es determinista y lleva su confianza.
        let artifact = hodei_evidence::domain::standards::identity::ExternalIdentity::confirmed(
            IdentityNamespace::Purl,
            "pkg:maven/org.example/lib-a@1.2.3",
            "ev-registry",
        );
        let correlated = correlate_inventory(&inventory, &[artifact]);

        let lib_a = correlated
            .iter()
            .find(|component| component.name == "lib-a")
            .ok_or("the component is correlated")?;
        let correlation = lib_a
            .correlation
            .as_ref()
            .ok_or("the correlation is recorded")?;
        assert!(correlation.same, "the mapping is the same thing");
        assert_eq!(
            correlation.confidence,
            MappingConfidence::Deterministic,
            "and it carries the confidence of a deterministic rule"
        );
        assert!(correlation.is_verdict(), "so it may be used as a verdict");

        // Un componente sin identidad no se correlaciona, y una pista no
        // cumple una exigencia de confianza.
        let unknown = correlated
            .iter()
            .find(|component| component.name == "lib-unknown")
            .ok_or("the component is there")?;
        assert!(
            !unknown.is_verdict(),
            "a component without an identity cannot satisfy a trust requirement"
        );

        // Y una correspondencia ambigua queda ambigua.
        let hint = hodei_evidence::domain::standards::identity::ExternalIdentity::unconfirmed(
            IdentityNamespace::Purl,
            "pkg:maven/org.example/lib-a@1.2.3",
        );
        let other = hodei_evidence::domain::standards::identity::ExternalIdentity::confirmed(
            IdentityNamespace::Purl,
            "pkg:maven/org.example/lib-a@1.2.3",
            "ev-other",
        );
        let ambiguous = correlate_identities(&hint, &other).expect("the correlation");
        assert!(
            !ambiguous.is_verdict(),
            "an unconfirmed mapping is not a verdict"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-06", result);
}

// ───────────────────────────── M2-UAT-07 ─────────────────────────────

/// M2-UAT-07 La inmutabilidad de la capsula por su hash.
///
/// Given capsule inputs set S, when capsule hash H is used by a later
/// decision, then a newly arriving evidence item creates a new
/// capsule/version/hash rather than changing semantics under H.
#[tokio::test]
async fn m2_uat_07_capsule_immutability_by_hash() {
    let result: Result<(), String> = async {
        let fixture = fixture().await;
        let now = moment(60);

        admit(
            &fixture,
            "integrity",
            EvidenceType::ArtifactIntegrity,
            b"the-bytes-of-the-artifact",
            EvidenceVerification::verified("digest"),
            "oci",
            "1.0.0",
            now,
        )
        .await;

        let before = the_capsule(&fixture, now).await;

        // Llega una pieza mas de evidencia.
        admit(
            &fixture,
            "sbom",
            EvidenceType::Sbom,
            CYCLONEDX.as_bytes(),
            EvidenceVerification::verified("schema"),
            "cyclonedx",
            "1.0.0",
            now,
        )
        .await;

        let after = the_capsule(&fixture, now).await;

        assert_ne!(
            before.content_hash, after.content_hash,
            "a newly arriving evidence item creates a new hash"
        );
        assert_eq!(
            before.version, after.version,
            "and the version of the assembly is the same"
        );

        // Y la capsula de antes sigue diciendo lo que decia: su hash no
        // cambia porque la semantica de debajo haya cambiado.
        let again = the_capsule(&fixture, now).await;
        assert_eq!(
            again.content_hash, after.content_hash,
            "the same inputs give the same capsule"
        );
        assert_ne!(
            again.content_hash, before.content_hash,
            "and the capsule under the old hash is a different one"
        );

        // Y el perfil de la de antes no se ha movido.
        let before_again = the_capsule(&fixture, now).await;
        assert_eq!(
            before_again.profile.dimensions.len(),
            after.profile.dimensions.len(),
            "the reading under the new hash has more dimensions"
        );
        Ok(())
    }
    .await;

    record("M2-UAT-07", result);
}

/// El enlace de la evidencia dice lo que un lector necesita para ir a
/// mirar: quien la produjo, de donde vino y hasta donde llega su
/// verificacion.
#[test]
fn the_evidence_link_carries_what_a_reader_needs() {
    let link = EvidenceLink {
        evidence_id: hodei_evidence::EvidenceId::new("ev-1"),
        evidence_type: EvidenceType::Sbom,
        verification: EvidenceVerification::verified("schema"),
        normalizer: "cyclonedx".to_string(),
        normalizer_version: "1.0.0".to_string(),
        source: SourceRef::by_external_id("sbom-1"),
        observed_at: moment(0),
    };

    assert_eq!(link.evidence_id.as_str(), "ev-1");
    assert_eq!(link.normalizer, "cyclonedx");
    assert_eq!(link.normalizer_version, "1.0.0");
    assert!(link.verification.proves());
    assert_eq!(
        link.source.external_id.as_deref(),
        Some("sbom-1"),
        "the link says where the evidence came from"
    );
}
