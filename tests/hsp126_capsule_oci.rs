//! La capsula de release como paquete OCI: lo que el bundle dice, el
//! layout lo repite byte a byte; lo que el bundle calla, la capsula lo
//! mantiene callado. Y la firma detached protege el manifiesto de la
//! capsula contra quien la presente como otra.
//!
//! Las garantias de HSP-126 se comprueban aqui sin red: las capas son
//! verificables con el patron de contenido de OCI, la capsula de
//! confianza no inventa decisiones (`null` dentro, `null` fuera) y la
//! firma es sobre los bytes exactos que el indice anuncia.

use chrono::{Duration, Utc};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    TrustGrant, TrustLedger,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;
use std::path::Path;

use hodei_artifacts_api::bundles::capsule::{INDEX_MEDIA_TYPE, LAYOUT_VERSION};
use hodei_artifacts_api::bundles::{
    self, Mapping, TrustState, manifest::MANIFEST_NAME, verify_capsule,
};
use hodei_artifacts_api::openpgp::MetadataSigner;

// The graph profile of the harness: a REAL Dgraph namespace per site.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";

// El andamiaje es el del contrato del paquete (hsp118): una capsula que
// no se exporte desde un paquete real no prueba nada.
include!("support/hsp118_site.rs");

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-126 {tag} <capsule@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

fn blob_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").expect("a sha256 digest")
}

/// El digest del manifiesto de imagen que el indice anuncia primero.
fn image_digest(index: &serde_json::Value) -> String {
    index["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// Un paquete con las tres formas que puede tener una ruta: confirmada
/// con decision, confirmada sin decision y retenida.
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
    site.trust
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
            valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
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

#[tokio::test]
async fn the_capsule_is_the_bundle_wearing_the_oci_standard() {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("layout");
    let directory = export_with(&site, "out", Some(&signer)).await;
    let layout = directory.join("oci");

    // Las dos puertas del layout, con los nombres del estandar.
    assert_eq!(
        read_json(&layout.join("oci-layout"))["imageLayoutVersion"],
        LAYOUT_VERSION
    );
    let index = read_json(&layout.join("index.json"));
    assert_eq!(index["mediaType"], INDEX_MEDIA_TYPE);
    assert_eq!(index["manifests"].as_array().expect("a list").len(), 2);

    // La capa del release son los bytes EXACTOS del manifiesto del
    // bundle: la capsula no re-serializa el paquete.
    let bundle_bytes = std::fs::read(directory.join(MANIFEST_NAME)).expect("the bundle");
    let image = read_json(
        &layout
            .join("blobs/sha256")
            .join(blob_hex(&image_digest(&index))),
    );
    let release = image["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["mediaType"] == "application/vnd.hodei.release.v1+json")
        .expect("the release layer");
    let release_bytes = std::fs::read(
        layout
            .join("blobs/sha256")
            .join(blob_hex(release["digest"].as_str().unwrap())),
    )
    .expect("the release blob");
    assert_eq!(release_bytes, bundle_bytes);

    // Las capas de contenido digieren igual que en el paquete.
    let manifest = bundles::read_manifest(&directory).expect("the bundle");
    let trust_layer = image["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["mediaType"] == "application/vnd.hodei.trust-state.v1+json")
        .expect("the trust layer");
    let capsule: TrustState = serde_json::from_value(read_json(
        &layout
            .join("blobs/sha256")
            .join(blob_hex(trust_layer["digest"].as_str().unwrap())),
    ))
    .expect("the capsule reads");
    assert_eq!(capsule.entries.len(), 2, "la ruta retenida no es entrada");
    assert_eq!(capsule.withheld.len(), 1, "y la ausencia si se cuenta");
    let digests: Vec<&str> = manifest.assets.iter().map(|a| a.digest.as_str()).collect();
    for entry in &capsule.entries {
        assert!(
            digests.contains(&entry.digest.as_str()),
            "every capsule digest is a bundle digest"
        );
    }
    let verified = verify_capsule(&layout, &signer).expect("the layout verifies");
    let _ = &verified;
    assert_eq!(verified.capsule.entries.len(), 2);
}

#[tokio::test]
async fn a_null_decision_exports_as_null_and_imports_as_null() {
    let (site, decided) = three_way_site().await;
    let signer = scratch_signer("nulls");
    let directory = export_with(&site, "out", Some(&signer)).await;

    let verified = verify_capsule(&directory.join("oci"), &signer).expect("the capsule verifies");
    let decided_entry = verified
        .capsule
        .entries
        .iter()
        .find(|e| e.asset_path.ends_with("tools/a.bin"))
        .expect("the decided path");
    assert_eq!(
        decided_entry
            .decision
            .as_ref()
            .expect("a carried decision")
            .decision_id,
        "dec-1",
        "la decision que tiene el paquete es la que declara la capsula"
    );
    assert_eq!(decided_entry.digest, decided.to_string());
    let bare_entry = verified
        .capsule
        .entries
        .iter()
        .find(|e| e.asset_path.ends_with("tools/b.bin"))
        .expect("the bare path");
    assert!(
        bare_entry.decision.is_none(),
        "sin decision no hay veredicto"
    );

    // El JSON dice `null`, no omite el campo: quien lee la capsula ve la
    // misma ausencia explicita que dejo el exportador.
    let layout = directory.join("oci");
    let index = read_json(&layout.join("index.json"));
    let image = read_json(
        &layout
            .join("blobs/sha256")
            .join(blob_hex(&image_digest(&index))),
    );
    let trust = image["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["mediaType"] == "application/vnd.hodei.trust-state.v1+json")
        .expect("the trust layer");
    let raw = std::fs::read_to_string(
        layout
            .join("blobs/sha256")
            .join(blob_hex(trust["digest"].as_str().unwrap())),
    )
    .expect("the trust layer");
    assert!(
        raw.contains("\"decision\": null"),
        "la ausencia se escribe como ausencia"
    );

    // Y lo que la capsula cuenta es lo que el destino importa: dos
    // rutas, ninguna decision inventada.
    let target = Site::new().await;
    target.create_proxy("acme", "mirror", "central").await;
    let report = import_into(&target, &directory, "acme", "mirror")
        .await
        .expect("the destination accepts the package");
    assert_eq!(
        report.imported, 2,
        "las mismas dos rutas que declara la capsula"
    );
}

#[tokio::test]
async fn a_signed_capsule_refuses_to_pass_for_another_one() {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("tamper");
    let directory = export_with(&site, "out", Some(&signer)).await;
    let layout = directory.join("oci");

    // Envenenar la capa de confianza con los bytes del manifiesto del
    // bundle: el descriptor anuncia un digest y los bytes son otros.
    let index = read_json(&layout.join("index.json"));
    let image = read_json(
        &layout
            .join("blobs/sha256")
            .join(blob_hex(&image_digest(&index))),
    );
    let trust = image["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["mediaType"] == "application/vnd.hodei.trust-state.v1+json")
        .expect("the trust layer");
    let trust_path = layout
        .join("blobs/sha256")
        .join(blob_hex(trust["digest"].as_str().unwrap()));
    let bundle_bytes = std::fs::read(directory.join(MANIFEST_NAME)).expect("the bundle");
    std::fs::write(&trust_path, &bundle_bytes).expect("the layer is poisoned");
    let error = verify_capsule(&layout, &signer).expect_err("a poisoned capsule does not verify");
    let refusal = error.to_string();
    assert!(
        refusal.contains("not the content its digest names") || refusal.contains("announces"),
        "the refusal says what broke: {refusal}"
    );

    // Con la capsula intacta pero otra clave, la firma no vale: se
    // verifica contra quien la cerro, no contra quien la presenta.
    let (site2, _) = three_way_site().await;
    let other = scratch_signer("other-key");
    let directory2 = export_with(&site2, "out", Some(&other)).await;
    let error = verify_capsule(&directory2.join("oci"), &signer)
        .expect_err("another hub's key is not this hub's key");
    assert!(
        error.to_string().contains("not the key we verify with")
            || error.to_string().contains("does not hold"),
        "{error}"
    );
}

#[tokio::test]
async fn a_revocation_after_export_never_rewrites_the_capsule() {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("revoked");
    let directory = export_with(&site, "out", Some(&signer)).await;
    let layout = directory.join("oci");

    // La capsula de antes, tal cual: los bytes del release son el ancla.
    let before = std::fs::read(directory.join(MANIFEST_NAME)).expect("the bundle");
    let verified = verify_capsule(&layout, &signer).expect("the capsule verifies");
    assert_eq!(
        verified
            .capsule
            .entries
            .iter()
            .find(|e| e.asset_path.ends_with("tools/a.bin"))
            .expect("the decided path")
            .decision
            .as_ref()
            .expect("the decision at export time")
            .decision_id,
        "dec-1"
    );

    // El origen revoca la decision despues del export. La capsula no
    // se entera porque no puede: es un recorte firmad del pasado, y su
    // firma es precisamente sobre esos bytes inmutables.
    site.trust
        .revoke_decision(
            "acme",
            "mirror",
            "dec-1",
            "hrn:hodei:iam:acme:User/carol",
            "policy change",
            Utc::now(),
        )
        .await
        .expect("the grant is revoked at the source");

    let verified = verify_capsule(&layout, &signer).expect("the capsule still verifies");
    assert_eq!(
        std::fs::read(directory.join(MANIFEST_NAME)).expect("the bundle"),
        before,
        "la revocacion no reescribe el paquete ni su capsula"
    );
    assert_eq!(verified.capsule.entries.len(), 2);
}

#[tokio::test]
async fn an_export_without_a_signer_ships_no_capsule() {
    let (site, _) = three_way_site().await;
    let directory = export_with(&site, "out", None).await;
    assert!(
        !directory.join("oci").exists(),
        "sin clave no hay capsula que vender"
    );
    let error = verify_capsule(&directory.join("oci"), &scratch_signer("absent"))
        .expect_err("there is nothing to verify");
    assert!(matches!(error, bundles::BundleError::Io(_)), "{error}");
}
