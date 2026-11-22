//! El pasaporte de confianza como extension de la capsula (HSP-160).
//!
//! Lo que aqui se comprueba no es que el pasaporte *declare* una
//! verificacion, sino que la *contenga de verdad*: los veredictos del
//! VSA salen de `cosign_verify` y del ladder SLSA ejecutados sobre los
//! bytes firmados, no de una autoafirmacion. Cada condicion de
//! aceptacion tiene un test que puede fallar:
//!
//! - AC1 un VSA con veredictos reales: una clave extrana produce un
//!   `FAILED` honesto, nunca un `PASSED` lavado; un lector que trae
//!   expectativas que el material no confirma se rechaza con
//!   `verdict-mismatch`, no se reinterpreta.
//! - AC3 una importacion aislada conserva el origen y los instantes del
//!   registro, y la frescura revocatoria en offline es
//!   `UnknownOffline` — nunca "no hay revocacion, luego hoy es valido".
//! - AC4 dos exportaciones de las mismas entradas reproducen la
//!   identidad del manifiesto byte a byte; las firmas quedan declaradas
//!   fuera de esa identidad.
//!
//! La condicion AC2 (herramientas externas: `oras`, `cosign`, `gpg`)
//! vive en su propio target, `hsp160_trust_passport_external.rs`, porque
//! necesita binarios reales del sistema y se ejecuta en un carril aparte.

use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, Utc};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt,
    ReceiptOutcome, TrustGrant, TrustLedger,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::standards::attestation::DsseEnvelope;
use hodei_evidence::standards::{
    FreshnessNow, RootRevision, SlsaBuildLevel, TrustRoots, VerificationKey,
};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

use hodei_artifacts_api::bundles::passport::{
    PASSPORT_ARTIFACT_TYPE, PassportProblemCode, PassportReading, SIGSTORE_BUNDLE_MEDIA_TYPE,
    SignedDocument, VSA_MEDIA_TYPE, export_passport, verify_passport_offline,
};
use hodei_artifacts_api::bundles::{
    self, BundleError, Mapping, TrustState, passport::PassportInputs, passport::PassportPolicy,
    passport::ReaderExpectations,
};
use hodei_artifacts_api::openpgp::MetadataSigner;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";
const ISSUER: &str = "registry.example";

// El andamiaje del sitio es el del contrato del paquete (hsp118): una
// capsula que no se exporte desde un paquete real no prueba nada.
include!("support/hsp118_site.rs");
// El andamiaje de material de procedencia firmada (HSP-160).
include!("support/hsp160_material.rs");

/// Un paquete con las tres formas que puede tener una ruta: confirmada
/// con decision, confirmada sin decision y retenida. El mismo andamiaje
/// del contrato de la capsula (hsp126): el pasaporte se exporta sobre un
/// paquete que ya paso por esas tres vias.
async fn three_way_site() -> (Site, ContentDigest) {
    let site = Site::new().await;
    site.create_proxy("acme", "mirror", "central").await;
    let decided = site
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/a.bin",
        &decided,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    site.store
        .grant(&[TrustGrant {
            account: "acme".to_string(),
            repository: "mirror".to_string(),
            digest: decided.clone(),
            decision_id: "dec-1".to_string(),
            actor: "hrn:hodei:iam:acme:User/carol".to_string(),
            granted_at: Utc::now() - Duration::days(1),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(Utc::now() + Duration::hours(1)),
        }])
        .await
        .expect("the grant exists");
    let bare = site
        .resolve("acme", "mirror", "tools/b.bin", b"tool b")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/b.bin",
        &bare,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    // c.bin no tiene recibo: se retiene y la capsula lo dice.
    site.resolve("acme", "mirror", "tools/c.bin", b"tool c")
        .await;
    (site, decided)
}

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-160 {tag} <passport@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

/// La hex de un digest `sha256:<hex>`, para abrir su blob en el layout.
fn blob_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").expect("a sha256 digest")
}

/// El digest que el indice de la capsula anuncia primero: su imagen.
fn capsule_digest(capsule_dir: &Path) -> String {
    let index = read_json(&capsule_dir.join("index.json"));
    index["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// Lo que una prueba necesita despues de exportar un pasaporte: el sitio
/// vivo (su `TempDir` sostiene los ficheros), las raices, la politica y
/// el firmante. `layout` es el directorio OCI del pasaporte recien
/// escrito.
struct PassportCase {
    _site: Site,
    roots: TrustRoots,
    policy: PassportPolicy,
    signer: MetadataSigner,
    layout: std::path::PathBuf,
}

/// Exporta un pasaporte sobre un paquete real, con procedencia firmada
/// por la raiz alfa anclada a `registry.example`.
async fn export_good(name: &str) -> PassportCase {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer(name);
    let bundle_dir = export_with(&site, name, Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let revision = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(revision.clone());
    let policy = hsp160_policy();
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&alpha, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![revision],
        verified_at: hsp160_verified_at(),
    };
    let layout = bundle_dir.join(format!("{name}.passport"));
    export_passport(&inputs, &layout, &signer).expect("the passport exports");
    PassportCase {
        _site: site,
        roots,
        policy,
        signer,
        layout,
    }
}

fn read_with(
    case: &PassportCase,
    issuer: Option<String>,
    online_freshness: Option<Duration>,
) -> Result<PassportReading, BundleError> {
    verify_passport_offline(
        &case.layout,
        &case.signer,
        &ReaderExpectations {
            roots: &case.roots,
            policy: &case.policy,
            issuer,
            online_freshness,
        },
        hsp160_verified_at(),
    )
}

/// Lectura aislada offline con las expectativas correctas del caso bueno.
fn read_ok(case: &PassportCase) -> PassportReading {
    read_with(case, Some(ISSUER.to_string()), None)
        .expect("a passport built over real pinned provenance reads")
}

/// El codigo de rechazo con el que el pasaporte contesto. `PassportProblem`
/// escribe el codigo en forma `Debug` entre corchetes; los rechazos
/// estructurales del layout que `tag_missing_and_tampered` etiqueta
/// llevan la forma kebab de la serde (`[missing-evidence]`,
/// `[tampered-blob]`), asi que se prueban las dos.
fn code_of(error: &BundleError) -> PassportProblemCode {
    let text = error.to_string();
    [
        PassportProblemCode::MissingEvidence,
        PassportProblemCode::TamperedBlob,
        PassportProblemCode::PolicyMismatch,
        PassportProblemCode::VerdictMismatch,
        PassportProblemCode::SignerRevoked,
        PassportProblemCode::SubjectMismatch,
        PassportProblemCode::Malformed,
        PassportProblemCode::SignatureInvalid,
    ]
    .into_iter()
    .find(|code| {
        text.contains(&format!("{code:?}"))
            || text.contains(
                &serde_json::to_value(code)
                    .expect("a code serializes")
                    .as_str()
                    .expect("a kebab code")
                    .to_string(),
            )
    })
    .unwrap_or_else(|| panic!("the error names no passport code: {text}"))
}

#[tokio::test]
async fn ac1_the_vsa_carries_verdicts_the_material_actually_produces() {
    let case = export_good("good").await;
    let reading = read_ok(&case);

    // AC1: el resultado del VSA es el nivel que la escalera realmente
    // afirma sobre estos bytes, no una etiqueta puesta a mano. Con el
    // builder confiado, el emisor anclado y los dos establecimientos de
    // politica declarados, el material da L3.
    assert_eq!(
        reading.vsa.predicate.verification_result,
        "SLSA_BUILD_LEVEL_3"
    );
    assert_eq!(reading.recorded.asserted_level(), Some(SlsaBuildLevel::L3));
    assert!(
        reading.recorded.gaps().is_empty(),
        "un pasaporte L3 no tiene huecos: {:?}",
        reading.recorded.gaps()
    );
    // La puerta anti-autoafirmacion: lo registrado y lo recalculado son
    // iguales requisito a requisito (la lectura solo paso por eso).
    assert_eq!(reading.recorded.findings, reading.recomputed.findings);
    assert!(
        reading.cryptographic_holds,
        "la firma de la raiz alfa sostiene"
    );
    assert_eq!(reading.root, Some(("alpha".to_string(), 1)));
}

#[tokio::test]
async fn ac1_a_foreign_key_lands_as_an_honest_failed_never_a_washed_pass() {
    // La procedencia la firma una raiz extrana (beta) que las raices del
    // export no conocen. El export NO la lava a pasado: ejecuta
    // `cosign_verify`, ve la clave desconocida y escribe un FAILED.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("foreign");
    let bundle_dir = export_with(&site, "foreign", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let revision = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(revision.clone());
    let policy = hsp160_policy();
    let beta = SignedRoot::generate("hsp160-root-beta-v1", "beta", "other-registry.example");

    let layout = bundle_dir.join("foreign.passport");
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&beta, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![revision],
        verified_at: hsp160_verified_at(),
    };
    let export =
        export_passport(&inputs, &layout, &signer).expect("a foreign key still exports as FAILED");
    assert!(!export.cryptographic_holds, "una clave extrana no sostiene");

    let reading = verify_passport_offline(
        &layout,
        &signer,
        &ReaderExpectations {
            roots: &roots,
            policy: &policy,
            issuer: Some(ISSUER.to_string()),
            online_freshness: None,
        },
        hsp160_verified_at(),
    )
    .expect("el FAILED se lee igual de honesto que un PASSED");
    // El registro dice FAILED y el material se recalcula FAILED: la
    // verificacion real manda sobre cualquier etiqueta puesta a mano.
    assert_eq!(reading.vsa.predicate.verification_result, "FAILED");
    assert_eq!(reading.recorded, reading.recomputed);
    assert!(
        !reading.recorded.unmet_at(SlsaBuildLevel::L1).is_empty(),
        "un FAILED tiene al menos un requisito de suelo no cumplido"
    );
    assert!(!reading.cryptographic_holds);
    // La escalera no afirma ningun nivel sobre una clave desconocida.
    assert_eq!(reading.recorded.asserted_level(), None);
}

#[tokio::test]
async fn ac1_a_reader_with_wrong_expectations_is_refused_not_reinterpreted() {
    // El export anclo el emisor registry.example y salio L3. Un lector
    // que no ancla ningun emisor re-ejecuta la cadena sin pin: la
    // escalera marca el pin como `NotEvaluated` (hueco) y baja a L1. La
    // puerta anti-autoafirmacion ve que el registro dice 3 y el material
    // 1, y rechaza con verdict-mismatch en vez de re-sustituir el hueco.
    let case = export_good("mismatch").await;
    let error = read_with(&case, None, None)
        .expect_err("un lector sin anclaje no puede releer un registro anclado");
    assert_eq!(code_of(&error), PassportProblemCode::VerdictMismatch);
    let text = error.to_string();
    assert!(
        text.contains("SLSA_BUILD_LEVEL_3") && text.contains("SLSA_BUILD_LEVEL_1"),
        "el rechazo compara lo registrado con lo recalculado: {text}"
    );
}

#[tokio::test]
async fn ac1_a_foreign_key_cannot_be_laundered_into_the_record() {
    // El guard del export: si la clave que firmo es una raiz publicada
    // en el ring pero no esta entre las revisiones que el pasaporte
    // declararia, el export se rechaza — un registro no puede documentar
    // unas raices y avalar con otras.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("launder");
    let bundle_dir = export_with(&site, "launder", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    // Beta comparte emisor para que la cadena llegue a Verified (un
    // pin roto no activa el guard de la firma); solo cambia su linaje.
    let beta = SignedRoot::generate("hsp160-root-beta-v1", "beta", ISSUER);
    let b1 = beta.revision(1, hsp160_verified_at() - Duration::days(30));
    // El ring del export conoce alfa y beta; el pasaporte solo publicaria alfa.
    let mut roots = alpha.trust_roots(r1.clone());
    roots.activate(b1, "eve", "a lineage the record would not publish");
    let policy = hsp160_policy();

    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&beta, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![r1],
        verified_at: hsp160_verified_at(),
    };
    let error = export_passport(&inputs, &bundle_dir.join("launder.passport"), &signer)
        .expect_err("la clave firmante no puede quedarse fuera del record");
    assert!(
        error.to_string().contains("not among the revisions"),
        "el rechazo nombra la revision no publicada: {error}"
    );
}

#[tokio::test]
async fn ac1_a_self_written_vsa_cannot_become_a_pass() {
    // Autoafirmacion: reescribir la capa del VSA para que proclame otro
    // veredicto no engana al lector. El blob del VSA es contenido-
    // direccionado: cambiar sus bytes rompe el digest que el manifiesto
    // anuncia, y `read_declared_blobs` lo rechaza antes de leerlo.
    let case = export_good("selfwritten").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let vsa_digest = hsp160_layer_digest(&passport, VSA_MEDIA_TYPE);

    // Forjar: mismo shape, resultado y hallazgos cambiados.
    let mut forged: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &vsa_digest)).unwrap();
    forged["predicate"]["verificationResult"] = json!("SLSA_BUILD_LEVEL_0");
    hsp160_write_blob(
        &case.layout,
        &vsa_digest,
        &serde_json::to_vec_pretty(&forged).expect("forge serializes"),
    );

    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("un VSA manipulado no se lee como bueno");
    assert_eq!(
        code_of(&error),
        PassportProblemCode::TamperedBlob,
        "el digest del VSA ya no es el que el manifiesto promete: {error}"
    );
}

#[tokio::test]
async fn ac1_a_tampered_envelope_is_refused_at_its_digest() {
    // Manipular el envelope (la capa del sigstore bundle) rompe el digest
    // que el VSA nombra en `inputAttestations`: verdict-mismatch, porque
    // el registro evaluo otros bytes. El envelope NO esta anclado en el
    // manifiesto, asi que la capa se lee; es el digest del VSA el que
    // delata que los bytes ya no son los verificados.
    let case = export_good("envelope-tamper").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let bundle_digest = hsp160_layer_digest(&passport, SIGSTORE_BUNDLE_MEDIA_TYPE);
    let mut bundle: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &bundle_digest)).unwrap();
    // Cambiar la firma dentro del bundle sin tocar el digest del VSA.
    bundle["dsseEnvelope"]["signatures"][0]["sig"] = json!(BASE64.encode([0u8; 8]));
    hsp160_write_blob(
        &case.layout,
        &bundle_digest,
        &serde_json::to_vec_pretty(&bundle).expect("bundle serializes"),
    );

    // La manipulacion cambia los bytes del blob del bundle; el descriptor
    // del manifiesto sigue anunciando el digest viejo -> el blob miente.
    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("un envelope manipulado no pasa");
    assert!(
        code_of(&error) == PassportProblemCode::TamperedBlob
            || code_of(&error) == PassportProblemCode::VerdictMismatch,
        "o el blob del bundle miente o el digest del VSA lo delata: {error}"
    );
}

#[tokio::test]
async fn ac3_isolated_import_keeps_origin_and_temporal_validity() {
    let case = export_good("origin").await;
    let reading = read_ok(&case);

    // AC3: el origen del paquete sobrevive a una lectura sin servicio.
    assert_eq!(reading.manifest.origin.account, "acme");
    assert_eq!(reading.manifest.origin.repository, "mirror");
    assert_eq!(reading.trust_state.origin, reading.manifest.origin);
    // La validez temporal del registro es la que se congelo al exportar.
    assert_eq!(reading.verified_at, hsp160_verified_at());
    // La capsula de confianza conserva la decision y la ausencia original.
    let decided = reading
        .trust_state
        .entries
        .iter()
        .find(|e| e.asset_path.ends_with("tools/a.bin"))
        .expect("the decided path");
    assert_eq!(
        decided.decision.as_ref().expect("a decision").decision_id,
        "dec-1",
        "la decision original se conserva"
    );
    assert_eq!(reading.trust_state.entries.len(), 2);
    assert_eq!(
        reading.trust_state.withheld.len(),
        1,
        "la ausencia sigue contada"
    );
    // El TrustState del pasaporte es derivable del manifiesto que lleva.
    assert_eq!(reading.trust_state, TrustState::of(&reading.manifest));
}

#[tokio::test]
async fn ac3_offline_never_claims_fresh_revocation_state() {
    let case = export_good("fresh").await;
    // Lectura offline: aunque el instante de lectura sea el del registro,
    // la frescura revocatoria NO es Fresh — es UnknownOffline.
    let reading = read_ok(&case);
    assert_eq!(reading.freshness, FreshnessNow::UnknownOffline);
    assert!(reading.cryptographic_holds, "los bytes si sostienen");
    assert!(
        !reading.proves_current(),
        "offline no se afirma frescura: UnknownOffline no es una luz verde"
    );
    // El limite esta declarado dentro del propio VSA, no solo en la lectura.
    assert_eq!(
        reading
            .vsa
            .predicate
            .hodei_offline_limits
            .revocation_freshness,
        "unknown-offline-today"
    );
    assert!(
        reading
            .vsa
            .predicate
            .hodei_offline_limits
            .no_transparency_log
    );
    assert_eq!(
        reading.vsa.predicate.hodei_offline_limits.issuer_kind,
        "hodei-own-verification"
    );
}

#[tokio::test]
async fn ac3_a_revoked_signing_key_is_refused_although_the_signature_holds() {
    // El lector roto la raiz y revoco la revision que firmo: la firma
    // sigue siendo valida sobre los bytes, pero la confianza la retiro.
    // La lectura la rechaza con signer-revoked, no como fallo de firma.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("revoked");
    let bundle_dir = export_with(&site, "revoked", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(60));
    let roots = alpha.trust_roots(r1.clone());
    let policy = hsp160_policy();
    let layout = bundle_dir.join("revoked.passport");
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&alpha, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![r1],
        verified_at: hsp160_verified_at(),
    };
    export_passport(&inputs, &layout, &signer).expect("mientras la clave vive, se exporta");

    // Revocar la revision 1 rotando a una 2 y marcando la 1 revocada
    // antes del instante de lectura.
    let mut roots = roots;
    let second = RootRevision {
        lineage: "alpha".to_string(),
        revision: 2,
        key: VerificationKey {
            key_id: "hsp160-root-alpha-v2".to_string(),
            point: alpha.point.clone(),
        },
        issuer: Some(ISSUER.to_string()),
        activated_at: hsp160_verified_at(),
        retired_at: None,
        revoked_at: None,
    };
    roots
        .rotate(
            "alpha",
            second,
            true,
            hsp160_verified_at() - Duration::days(1),
            "eve",
            "the key leaked",
        )
        .expect("the rotation revokes revision 1 before the reading");

    let error = verify_passport_offline(
        &layout,
        &signer,
        &ReaderExpectations {
            roots: &roots,
            policy: &policy,
            issuer: Some(ISSUER.to_string()),
            online_freshness: None,
        },
        hsp160_verified_at(),
    )
    .expect_err("una clave revocada no avala el pasaporte");
    assert_eq!(code_of(&error), PassportProblemCode::SignerRevoked);
    assert!(
        error.to_string().contains("revoked"),
        "el rechazo nombra la revocacion, no un fallo de firma: {error}"
    );
}

#[tokio::test]
async fn ac4_two_exports_of_the_same_inputs_reproduce_the_manifest_identity() {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("repro");
    let bundle_dir = export_with(&site, "repro", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(r1.clone());
    let published = vec![r1];
    let policy = hsp160_policy();

    // La procedencia es UN input canonico: se firma una sola vez. Dos
    // firmas ECDSA del mismo mensaje no son el mismo envelope (el nonce
    // es aleatorio), y la identidad reproducible se jura sobre las
    // mismas entradas, no sobre firmas rehechas.
    let provenance = hsp160_provenance(&alpha, &digest);
    let build_inputs = || PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: provenance.clone(),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: published.clone(),
        verified_at: hsp160_verified_at(),
    };
    let a = export_passport(&build_inputs(), &bundle_dir.join("a.passport"), &signer).expect("a");
    let b = export_passport(&build_inputs(), &bundle_dir.join("b.passport"), &signer).expect("b");

    // AC4: la identidad del manifiesto es identica byte a byte.
    println!("AC4 passport identity a: {}", a.image_digest);
    println!("AC4 passport identity b: {}", b.image_digest);
    println!("AC4 vsa digest: {}", a.vsa_digest);
    assert_eq!(
        a.image_digest, b.image_digest,
        "misma identidad reproducible"
    );
    assert_eq!(
        a.signed_manifest, b.signed_manifest,
        "los mismos bytes firmados"
    );
    assert_eq!(a.vsa_digest, b.vsa_digest);
    assert_eq!(a.asserted_level, Some(SlsaBuildLevel::L3));

    // Las firmas y las marcas de tiempo quedan declaradas fuera de la
    // identidad, no silenciosamente excluidas.
    let manifest: serde_json::Value = serde_json::from_slice(&a.signed_manifest).unwrap();
    assert_eq!(
        manifest["annotations"]["hodei.passport.signatures"],
        json!("detached-openpgp-referrer-excluded-from-identity")
    );
    assert_eq!(
        manifest["annotations"]["hodei.passport.timestamps"],
        json!("input-declared-in-layers; no clock in manifest")
    );

    // Y la re-lectura aislada de la primera copia confirma la misma
    // identidad del manifiesto que el export anancio.
    let case_a = PassportCase {
        _site: site,
        roots,
        policy,
        signer,
        layout: a.layout_dir.clone(),
    };
    let reading = read_ok(&case_a);
    assert_eq!(reading.passport_image_digest, a.image_digest);
    assert!(reading.cryptographic_holds);
    assert_eq!(reading.freshness, FreshnessNow::UnknownOffline);
}

#[tokio::test]
async fn ac4_missing_evidence_is_named_as_absent_not_as_invalid() {
    // Quitar un blob del layout es evidencia que falta (missing-evidence),
    // distinto de evidencia que miente (tampered-blob). Un consumidor sin
    // servicio debe poder decir cual ausencia tuvo.
    let case = export_good("missing").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let sbom_digest = hsp160_layer_digest(&passport, "application/vnd.cyclonedx+json");
    hsp160_remove_blob(&case.layout, &sbom_digest);

    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("un pasaporte al que le falta un blob no se lee");
    assert!(
        error.to_string().contains("missing-evidence"),
        "la ausencia se nombra ausencia, no invalidacion: {error}"
    );
}

#[tokio::test]
async fn the_envelope_the_record_carries_is_the_envelope_the_signature_said() {
    // Invariante: la capa del sigstore bundle lleva el envelope DSSE que
    // firmo la raiz alfa sobre el digest de capsula, y se re-verifica sin
    // el servicio.
    let case = export_good("envelope").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let bundle_digest = hsp160_layer_digest(&passport, SIGSTORE_BUNDLE_MEDIA_TYPE);
    let bundle: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &bundle_digest)).unwrap();
    assert_eq!(
        bundle["mediaType"],
        json!("application/vnd.dev.sigstore.bundle.v0.3+json")
    );
    let envelope: DsseEnvelope =
        serde_json::from_value(bundle["dsseEnvelope"].clone()).expect("a DSSE envelope inside");
    assert_eq!(envelope.payload_type, "application/vnd.in-toto+json");
    assert_eq!(envelope.signatures[0].keyid, "hsp160-root-alpha-v1");

    let reading = read_ok(&case);
    assert_eq!(reading.root.expect("a known root").0, "alpha");
    assert_eq!(reading.signer_fingerprint, case.signer.fingerprint_hex());
}
