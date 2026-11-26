//! The bundle import: resolving without going out to the network.
//!
//! Importing is the inverse and asymmetric operation. Asymmetric because the
//! bundle's origin is **provenance, not authority**: what authorizes serving
//! a content at the destination is the destination's configuration, and for
//! that reason the receipt written here is signed with the current upstream and
//! configuration version, with the `imported` result, while the original URL
//! and decision are kept only as evidence inside the
//! manifest.
//!
//! Visibility is earned the other way around from how it is told: **nothing becomes
//! publishable until the whole bundle has been verified**. First the
//! blobs are checked and stored against the manifest's digest, digit by digit;
//! only afterwards, and in an order that leaves no orphan paths, are the
//! receipts written and the paths published. A lying manifest or
//! a corrupt file aborts the import without making anything visible.

use std::path::Path;

use hodei_catalog::{
    CatalogReader, CatalogWriter, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    RepositoryMode, UpstreamConfig,
};
use hodei_hub::BlobStore;

use super::manifest::{BundleManifest, MANIFEST_NAME};
use super::{BundleError, content_digest};

/// What an import did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReport {
    /// The paths published by this import.
    pub imported: usize,
    /// The paths that were already there, with the same content.
    pub already_present: usize,
    /// The bytes of blobs that reached the destination's store.
    pub blob_bytes: u64,
}

/// How to import a bundle into a destination that is not the origin's.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mapping {
    /// Allow importing into an account or a repository different from the
    /// origin. Without this, the cross import is rejected: whoever imports
    /// has to state explicitly that this path belongs to them.
    pub allow: bool,
    /// Allow a path in the bundle to replace another one at the destination.
    /// Without this, a path collision aborts the import.
    pub overwrite: bool,
}

/// Reads and checks the manifest of a bundle, without touching the catalog.
pub fn read_manifest(directory: &Path) -> Result<BundleManifest, BundleError> {
    let raw = std::fs::read(directory.join(MANIFEST_NAME)).map_err(|error| {
        BundleError::Read(format!(
            "the bundle manifest at {} cannot be read: {error}",
            directory.display()
        ))
    })?;
    let manifest: BundleManifest = serde_json::from_slice(&raw)?;
    manifest.check_importable()?;
    Ok(manifest)
}

/// Imports the bundle from `directory` into the proxy repository `repository`
/// of `account`.
#[allow(clippy::too_many_arguments)]
pub async fn import(
    reader: &dyn CatalogReader,
    writer: &dyn CatalogWriter,
    ledger: &dyn ProvenanceLedger,
    blobs: &dyn BlobStore,
    account: &str,
    repository: &str,
    directory: &Path,
    mapping: Mapping,
    actor: &str,
    correlation_id: &str,
) -> Result<ImportReport, BundleError> {
    let manifest = read_manifest(directory)?;
    let descriptor = reader
        .repository(account, repository)
        .await
        .map_err(|error| {
            BundleError::Refused(format!(
                "the destination repository {account}/{repository} does not answer: {error}; \
             create the proxy repository before importing"
            ))
        })?;
    if descriptor.mode != RepositoryMode::Proxy {
        return Err(BundleError::Refused(format!(
            "the destination is a {}, and a resolution bundle only feeds a proxy",
            descriptor.mode
        )));
    }
    let upstream = UpstreamConfig::of(&descriptor.config)
        .map_err(|error| {
            BundleError::Refused(format!("the destination configuration is broken: {error}"))
        })?
        .ok_or_else(|| {
            BundleError::Refused("the destination proxy has no upstream configured".to_string())
        })?;
    if upstream.id != manifest.origin.upstream_id {
        return Err(BundleError::Refused(format!(
            "the destination resolves against {} and the bundle was resolved against {}; \
             provenance is not authority — point the proxy at the same upstream or refuse \
             the package",
            upstream.id, manifest.origin.upstream_id
        )));
    }
    if !mapping.allow
        && (manifest.origin.account != account || manifest.origin.repository != repository)
    {
        return Err(BundleError::Refused(format!(
            "the bundle comes from {}/{}, another owner than {}/{}, and importing across \
             owners needs the explicit mapping flag",
            manifest.origin.account, manifest.origin.repository, account, repository
        )));
    }

    // 1. Verify and store the blobs. Nothing in the catalog is touched here:
    //    a rotten bundle aborts before any path becomes visible.
    let mut blob_bytes = 0;
    for blob in &manifest.blobs {
        let digest = content_digest(&blob.digest).expect("a checked manifest has real digests");
        let path = directory.join(BundleManifest::blob_file(&blob.digest));
        let bytes = std::fs::read(&path).map_err(|error| {
            BundleError::Unsound(format!(
                "the blob {} of the package is not at {}: {error}",
                blob.digest,
                path.display()
            ))
        })?;
        let computed = hodei_evidence::ContentDigest::of_bytes(&bytes);
        if computed != digest {
            return Err(BundleError::Unsound(format!(
                "the file {} is not the content the manifest declares: its digest is {computed}",
                path.display()
            )));
        }
        if bytes.len() as u64 != blob.length {
            return Err(BundleError::Unsound(format!(
                "the blob {} declares {} bytes and the file has {}",
                blob.digest,
                blob.length,
                bytes.len()
            )));
        }
        blobs.put(&bytes).await.map_err(|error| {
            BundleError::Io(format!(
                "the blob {} cannot be stored: {error}",
                blob.digest
            ))
        })?;
        blob_bytes += blob.length;
    }

    // 2. Make the paths visible: first the receipt that backs them,
    //    then the publication. A cut between the two leaves backing without a
    //    path, never a path without backing.
    let mut imported = 0;
    let mut already_present = 0;
    for entry in &manifest.assets {
        let digest = content_digest(&entry.digest).expect("a checked manifest has real digests");
        ledger
            .record(&ProvenanceReceipt {
                account: account.to_string(),
                repository: repository.to_string(),
                upstream_id: upstream.id.clone(),
                config_version: descriptor.config.version,
                outcome: ReceiptOutcome::Imported,
                actor: actor.to_string(),
                correlation_id: correlation_id.to_string(),
                ..entry.receipt.clone()
            })
            .await?;

        let existing = reader.asset(account, repository, &entry.asset_path).await;
        if let Ok(asset) = &existing
            && asset.digest == digest
        {
            already_present += 1;
            continue;
        }
        if existing.is_ok() && !mapping.overwrite {
            return Err(BundleError::Refused(format!(
                "the path {} points to other content in the destination; importing over it \
                 needs the explicit overwrite flag",
                entry.asset_path
            )));
        }
        writer
            .publish(
                account,
                hodei_catalog::Publication {
                    repository: repository.to_string(),
                    asset_path: entry.asset_path.clone(),
                    coordinates: hodei_catalog::PackageCoordinates {
                        package_name: entry.package.clone(),
                        package_version: entry.version.clone(),
                    },
                    digest,
                    size_bytes: entry.size_bytes,
                    tags: Vec::new(),
                    actor: actor.to_string(),
                    correlation_id: correlation_id.to_string(),
                    overwrite: mapping.overwrite,
                    precondition: None,
                    subject: None,
                },
            )
            .await?;
        imported += 1;
    }

    Ok(ImportReport {
        imported,
        already_present,
        blob_bytes,
    })
}
