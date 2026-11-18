//! La exportacion del paquete: el resultado de resolver, con su respaldo.
//!
//! Exportar no es copiar el cache: es declarar que cada ruta exportada
//! estaba **resuelta y confirmada** en el momento de cerrar el paquete. La
//! confirmacion es la misma que exige el servidor sin red: un recibo del
//! upstream y de la configuracion vigentes, que habla del contenido al que
//! apunta la ruta, y que no es una cuarentena. Lo que no cumple se va en
//! `withheld` con su motivo, para que la ausencia sea explicable al
//! importar y no un misterio.
//!
//! El paquete se escribe como la copia de recuperacion: en un directorio
//! temporal del mismo destino y con un renombrado atomico. Un corte deja,
//! como mucho, un tempororal que nadie importa; no deja nunca un paquete a
//! medias con el nombre del destino.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use hodei_catalog::{
    CatalogReader, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome, RepositoryMode,
    TrustLedger, UpstreamConfig,
};
use hodei_hub::BlobStore;

use super::BundleError;
use super::manifest::{
    AssetEntry, BlobEntry, BundleManifest, DecisionEvidence, FORMAT, MANIFEST_NAME, Origin,
    Withheld,
};

/// Lo que hizo una exportacion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportReport {
    /// Donde quedo el paquete.
    pub path: PathBuf,
    /// Las rutas exportadas.
    pub exported: usize,
    /// Las rutas retenidas, cada una con su motivo.
    pub withheld: Vec<Withheld>,
    /// Los bytes de blobs que lleva el paquete.
    pub blob_bytes: u64,
    /// La capsula OCI escrita junto al paquete, o `None` si el firmante
    /// no estaba configurado. El paquete se escribe siempre: la capsula
    /// es lo que se puede dejar de producir, nunca lo que se corrompe.
    pub capsule: Option<CapsuleReport>,
}

/// La capsula de un export: donde quedo, que digest tiene y con que
/// clave se firmo su manifiesto.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CapsuleReport {
    pub layout: std::path::PathBuf,
    pub image_digest: String,
    pub signer: String,
}

/// Exporta el repositorio proxy `repository` de `account` como paquete en
/// `to`.
///
/// Con `signer` escrito además como capsula OCI bajo `to/oci` (véase
/// [`super::capsule`]); sin signer el paquete se exporta igual y la
/// capsula brilla por su ausencia explicita.
///
/// La cuenta opera como principal de sus propias lecturas: la exportacion
/// es una operacion de operador, y el decisor que se inyecte es el que
/// dice si puede ver el repositorio.
#[allow(clippy::too_many_arguments)]
pub async fn export(
    reader: &dyn CatalogReader,
    ledger: &dyn ProvenanceLedger,
    trust: &dyn TrustLedger,
    blobs: &dyn BlobStore,
    account: &str,
    repository: &str,
    to: &Path,
    signer: Option<&crate::openpgp::MetadataSigner>,
) -> Result<ExportReport, BundleError> {
    let descriptor = reader.repository(account, repository).await?;
    if descriptor.mode != RepositoryMode::Proxy {
        return Err(BundleError::Refused(format!(
            "the repository {repository} is a {}, and only a proxy resolves content",
            descriptor.mode
        )));
    }
    let upstream = UpstreamConfig::of(&descriptor.config)
        .map_err(|error| {
            BundleError::Refused(format!("the proxy configuration is broken: {error}"))
        })?
        .ok_or_else(|| {
            BundleError::Refused(format!(
                "the repository {repository} has no upstream configured, and there is \
                 nothing resolved to export"
            ))
        })?;

    let mut entries: Vec<AssetEntry> = Vec::new();
    let mut withheld: Vec<Withheld> = Vec::new();
    let mut content: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    for asset in reader.assets_under(account, repository, "").await? {
        let receipts = ledger
            .receipts_of(account, repository, &asset.asset_path)
            .await?;
        // La misma lectura que el servidor sin red: el primer recibo que
        // habla de este contenido y no es cuarentena es el que confirma.
        let confirmation = receipts.iter().find(|receipt| {
            receipt.digest == asset.digest && receipt.outcome != ReceiptOutcome::Quarantined
        });
        // La cuarentena del recibo mas reciente manda sobre cualquier
        // confirmacion anterior, igual que en el servidor sin red: dura
        // hasta que cambia la configuracion.
        let quarantined_now = receipts.first().is_some_and(|receipt| {
            receipt.outcome == ReceiptOutcome::Quarantined
                && receipt.config_version == descriptor.config.version
        });
        let reason = match confirmation {
            Some(_) if quarantined_now => Some("the origin has this content quarantined"),
            Some(receipt) if receipt.upstream_id != upstream.id => {
                Some("the confirming receipt speaks of another upstream")
            }
            Some(receipt) if receipt.config_version != descriptor.config.version => {
                Some("the confirming receipt speaks of an older configuration")
            }
            Some(_) => None,
            None => Some("no provenance receipt confirms this content"),
        };
        if let Some(reason) = reason {
            withheld.push(Withheld {
                asset_path: asset.asset_path.clone(),
                reason: reason.to_string(),
            });
            continue;
        }
        let receipt = confirmation.expect("the absence of a reason means a confirmation");

        let bytes = match blobs.get(&asset.digest).await {
            Ok(bytes) if bytes.len() as u64 == asset.size_bytes => bytes,
            Ok(_) => {
                withheld.push(Withheld {
                    asset_path: asset.asset_path.clone(),
                    reason: "the store holds a different length than the catalog declares"
                        .to_string(),
                });
                continue;
            }
            Err(error) => {
                withheld.push(Withheld {
                    asset_path: asset.asset_path.clone(),
                    reason: format!("the content is not in the store of the origin: {error}"),
                });
                continue;
            }
        };

        let decision = trust
            .active_grant(account, repository, &asset.digest, Utc::now())
            .await?
            .map(|grant| DecisionEvidence {
                decision_id: grant.decision_id,
                actor: grant.actor,
                granted_at: grant.granted_at,
            });

        let digest = asset.digest.to_string();
        content.entry(digest.clone()).or_insert(bytes);
        entries.push(AssetEntry {
            asset_path: asset.asset_path,
            package: asset.coordinates.package_name,
            version: asset.coordinates.package_version,
            digest,
            size_bytes: asset.size_bytes,
            receipt: ProvenanceReceipt {
                account: account.to_string(),
                repository: repository.to_string(),
                ..receipt.clone()
            },
            decision,
        });
    }

    let manifest = BundleManifest {
        format: FORMAT,
        exported_at: Utc::now(),
        origin: Origin {
            account: account.to_string(),
            repository: repository.to_string(),
            upstream_id: upstream.id,
            config_version: descriptor.config.version,
        },
        assets: entries,
        blobs: content
            .iter()
            .map(|(digest, bytes)| BlobEntry {
                digest: digest.clone(),
                length: bytes.len() as u64,
            })
            .collect(),
        withheld,
    };
    // La comprobacion es la misma que haria el destino: este binario no
    // escribe un paquete que el mismo rechazaria al importar.
    manifest.check_importable()?;

    if to.exists() {
        return Err(BundleError::Refused(format!(
            "the bundle directory {} already exists; a bundle is never overwritten",
            to.display()
        )));
    }
    let parent = to
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let scratch = parent.join(".bundle.tmp");
    let _ = std::fs::remove_dir_all(&scratch);
    for (digest, bytes) in &content {
        let path = scratch.join(BundleManifest::blob_file(digest));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, bytes)?;
    }
    std::fs::write(
        scratch.join(MANIFEST_NAME),
        serde_json::to_vec_pretty(&manifest).expect("a checked manifest serializes"),
    )?;
    std::fs::rename(&scratch, to).map_err(|error| {
        BundleError::Io(format!(
            "the bundle could not be published at {}: {error}",
            to.display()
        ))
    })?;

    // La capsula se escribe **despues** de publicar el paquete: un
    // firmante que falta o una clave rota no pueden dejar a medias lo
    // que ya es verificable por si mismo. Sin firmante no hay capsula,
    // y el export sigue siendo un export.
    let capsule = match signer {
        Some(signer) => {
            let report = super::capsule::export_capsule(to, &to.join("oci"), signer)?;
            Some(CapsuleReport {
                layout: report.layout_dir,
                image_digest: report.image_digest,
                signer: signer.fingerprint_hex(),
            })
        }
        None => None,
    };

    Ok(ExportReport {
        path: to.to_path_buf(),
        exported: manifest.assets.len(),
        withheld: manifest.withheld.clone(),
        blob_bytes: manifest.blob_bytes(),
        capsule,
    })
}
