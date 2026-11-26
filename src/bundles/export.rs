//! The bundle export: the result of resolving, with its backing.
//!
//! Exporting is not copying the cache: it is declaring that each exported path
//! was **resolved and confirmed** at the moment the bundle was closed. The
//! confirmation is the same one the offline server requires: a receipt from
//! the current upstream and configuration, which speaks of the content the
//! path points to, and which is not a quarantine. Whatever does not comply goes into
//! `withheld` with its reason, so that the absence is explainable on
//! import and not a mystery.
//!
//! The bundle is written like the recovery backup: in a temporary
//! directory next to the destination and with an atomic rename. A cut leaves,
//! at most, a temporary file that nobody imports; it never leaves a
//! half-written bundle under the destination's name.

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

/// What an export did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportReport {
    /// Where the bundle ended up.
    pub path: PathBuf,
    /// The exported paths.
    pub exported: usize,
    /// The withheld paths, each with its reason.
    pub withheld: Vec<Withheld>,
    /// The bytes of blobs that the bundle carries.
    pub blob_bytes: u64,
    /// The OCI capsule written next to the bundle, or `None` if the signer
    /// was not configured. The bundle is always written: the capsule
    /// is what can be left unproduced, never what gets corrupted.
    pub capsule: Option<CapsuleReport>,
}

/// The capsule of an export: where it ended up, what digest it has and with what
/// key its manifest was signed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CapsuleReport {
    pub layout: std::path::PathBuf,
    pub image_digest: String,
    pub signer: String,
}

/// Exports the proxy repository `repository` of `account` as a bundle at
/// `to`.
///
/// With a `signer` it is additionally written as an OCI capsule under `to/oci` (see
/// [`super::capsule`]); without a signer the bundle is exported all the same and the
/// capsule is conspicuous by its explicit absence.
///
/// The account operates as the principal of its own reads: the export
/// is an operator operation, and the decider that gets injected is the one that
/// says whether the repository can be seen.
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
        // The same read as the offline server: the first receipt that
        // speaks of this content and is not a quarantine is the one that confirms.
        let confirmation = receipts.iter().find(|receipt| {
            receipt.digest == asset.digest && receipt.outcome != ReceiptOutcome::Quarantined
        });
        // The quarantine of the most recent receipt overrides any earlier
        // confirmation, just as on the offline server: it lasts
        // until the configuration changes.
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
    // The check is the same one the destination would make: this binary does not
    // write a bundle that it would itself reject on import.
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

    // The capsule is written **after** publishing the bundle: a
    // missing signer or a broken key cannot leave half-done what
    // is already verifiable by itself. Without a signer there is no capsule,
    // and the export is still an export.
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
