//! The resolution bundle: what a proxy carries with it to a site with no network.
//!
//! A bundle is a **directory** with a manifest and the blobs it
//! resolves, just like a recovery backup. It is not a copy of the
//! system: it is the result of resolving a repository against its upstream,
//! with the provenance and the decisions that backed it at the moment of
//! export, so that another registry can import it and serve that content without
//! going out to the network.
//!
//! The guarantees are decided here and verified there:
//!
//! - A bundle is **verified in full before anything becomes visible**: no
//!   path becomes publishable if a single blob is missing or lies.
//! - The origin's upstream is **provenance, not authority**: on import,
//!   receipts are recorded under the destination's configuration, which is
//!   the one that can be confirmed, and the original URL is kept only as
//!   evidence.
//! - A bundle never carries credentials: the manifest does not store
//!   configuration URLs, and receipts are born without secrets.

pub mod capsule;
pub mod capsule_dgraph;
pub mod export;
pub mod import;
pub mod manifest;

pub use capsule::{CapsuleExport, CapsuleVerification, TrustState, export_capsule, verify_capsule};
pub use export::{ExportReport, export};
pub use import::{ImportReport, Mapping, import, read_manifest};
pub use manifest::{
    AssetEntry, BLOBS_DIRECTORY, BlobEntry, BundleManifest, DecisionEvidence, FORMAT,
    MANIFEST_NAME, Origin, Withheld,
};

use thiserror::Error;

/// How a bundle operation ended.
#[derive(Debug, Error)]
pub enum BundleError {
    /// The bundle, or what is asked of it, is not consistent.
    #[error("the bundle is not sound: {0}")]
    Unsound(String),
    /// The destination's configuration does not allow importing this bundle.
    #[error("the import is refused: {0}")]
    Refused(String),
    /// The disk or the store failed.
    #[error("storage failed: {0}")]
    Io(String),
    /// The catalog or the provenance ledger failed.
    #[error("the catalog failed: {0}")]
    Catalog(String),
    /// A manifest or a receipt is not understood.
    #[error("the bundle cannot be read: {0}")]
    Read(String),
}

impl From<std::io::Error> for BundleError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(error: serde_json::Error) -> Self {
        Self::Read(error.to_string())
    }
}

impl From<hodei_catalog::CatalogError> for BundleError {
    fn from(error: hodei_catalog::CatalogError) -> Self {
        Self::Catalog(error.to_string())
    }
}

/// The operator command reports the bundle with the recovery error:
/// export and import are operations of the same binary.
impl From<BundleError> for crate::recovery::RecoveryError {
    fn from(error: BundleError) -> Self {
        Self::Bundle(error.to_string())
    }
}

/// The digest of an already-checked manifest text.
///
/// `check_importable` requires the `sha256:<hex>` form before anyone
/// calls this; outside that path it returns `None` instead of improvising.
pub fn content_digest(text: &str) -> Option<hodei_evidence::ContentDigest> {
    let hex = crate::recovery::manifest::hex_of(text)?;
    Some(hodei_evidence::ContentDigest::sha256(hex))
}
