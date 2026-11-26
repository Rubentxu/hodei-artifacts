//! The manifest of a resolution bundle: what the bundle says about
//! itself.
//!
//! It is the last thing written on export and the first thing read on
//! import. Like that of a backup, its check is **pure**: it rejects the
//! whole bundle before touching anything if a hostile or broken manifest could
//! cause content to be imported without backing, overwrite someone else's path, or
//! write outside the bundle. The rule is taken from the backup
//! manifest: a bundle that this binary does not understand is rejected with a
//! message, not imported halfway.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use hodei_catalog::{ProvenanceReceipt, ReceiptOutcome};
use serde::{Deserialize, Serialize};

use super::BundleError;
use crate::recovery::manifest::hex_of;

/// The version of the bundle format.
///
/// It goes up when an old bundle can no longer be read. One with a format
/// that this binary does not understand is rejected, not imported halfway.
pub const FORMAT: u32 = 1;

/// The name of the manifest within the bundle.
pub const MANIFEST_NAME: &str = "bundle.json";

/// The blobs directory within the bundle, with the same layout as the
/// store: `<ab>/<cd>/<hex>`.
pub const BLOBS_DIRECTORY: &str = "blobs";

/// The maximum number of paths a bundle may declare.
///
/// It is the honest bound of the export read: a bundle with more
/// entries than the catalog can enumerate at once could not be
/// exported in full, and what looks complete and is not does not get imported.
pub const MAX_BUNDLE_ASSETS: usize = hodei_catalog::MAX_ASSETS_UNDER;

/// Where the bundle came from.
///
/// It does not carry the upstream URL: a configuration URL may carry
/// credentials in the user part, and a bundle does not leave home with
/// secrets. The per-archive URL lives, sanitized, in the receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The account of the exported repository.
    pub account: String,
    /// The key of the proxy repository.
    pub repository: String,
    /// The identifier of the upstream it was resolved with.
    pub upstream_id: String,
    /// The version of the configuration it was resolved with.
    pub config_version: u64,
}

/// The decision that backed a digest at export time.
///
/// It is **evidence, not permission**: on import, the destination judges trust
/// with its own grants. It is kept so that it is possible to answer later for
/// why the bundle brought what it brought.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionEvidence {
    /// The promotion that granted the digest.
    pub decision_id: String,
    /// Who promoted.
    pub actor: String,
    /// When it was granted.
    pub granted_at: DateTime<Utc>,
}

/// A resolved path of the bundle, with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    /// The path within the repository.
    pub asset_path: String,
    /// The package name exactly as the resolution published it.
    pub package: String,
    /// The version published at the path.
    pub version: String,
    /// The content of the path, as `sha256:<hex>`.
    pub digest: String,
    /// The bytes of the content.
    pub size_bytes: u64,
    /// The provenance receipt that confirmed the path at export.
    pub receipt: ProvenanceReceipt,
    /// The active decision on that digest at export, if one existed.
    #[serde(default)]
    pub decision: Option<DecisionEvidence>,
}

/// A blob of the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobEntry {
    /// The content, as `sha256:<hex>`.
    pub digest: String,
    /// The bytes it occupies.
    pub length: u64,
}

/// A path that the bundle does not carry, and why.
///
/// It is written down so that the absence is **explainable**: whoever imports
/// sees that the origin had that path and why it did not come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    /// The path withheld at the origin.
    pub asset_path: String,
    /// Why it was not exported.
    pub reason: String,
}

/// What a bundle says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// The format version.
    pub format: u32,
    /// When the bundle was closed.
    pub exported_at: DateTime<Utc>,
    /// Where it came from.
    pub origin: Origin,
    /// The resolved paths, sorted by path.
    pub assets: Vec<AssetEntry>,
    /// The blobs, sorted by digest and without repetition.
    pub blobs: Vec<BlobEntry>,
    /// The paths that the origin had and the bundle does not carry.
    pub withheld: Vec<Withheld>,
}

impl BundleManifest {
    /// The bytes of blobs that the bundle carries.
    pub fn blob_bytes(&self) -> u64 {
        self.blobs.iter().map(|blob| blob.length).sum()
    }

    /// Checks that the bundle can be imported with this binary.
    ///
    /// It does not touch the disk: it is the pure part. It rejects what, if accepted,
    /// would let a hostile bundle make content visible without backing
    /// or write outside its own rules.
    pub fn check_importable(&self) -> Result<(), BundleError> {
        if self.format != FORMAT {
            return Err(BundleError::Unsound(format!(
                "the bundle has format {} and this build understands format {FORMAT}",
                self.format
            )));
        }
        for field in [
            ("account", &self.origin.account),
            ("repository", &self.origin.repository),
            ("upstream", &self.origin.upstream_id),
        ] {
            if field.1.is_empty() {
                return Err(BundleError::Unsound(format!(
                    "the origin {} of the bundle is empty",
                    field.0
                )));
            }
        }
        if self.assets.len() > MAX_BUNDLE_ASSETS {
            return Err(BundleError::Unsound(format!(
                "the bundle declares {} assets, more than the {} a single export can honestly \
                 enumerate",
                self.assets.len(),
                MAX_BUNDLE_ASSETS
            )));
        }

        let mut digests = Vec::with_capacity(self.assets.len());
        let mut previous: Option<&str> = None;
        for entry in &self.assets {
            check_path(&entry.asset_path)?;
            if previous.is_some_and(|last| last >= entry.asset_path.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the path {} is repeated or out of order",
                    entry.asset_path
                )));
            }
            previous = Some(&entry.asset_path);

            let hex = hex_of(&entry.digest).ok_or_else(|| {
                BundleError::Unsound(format!(
                    "the digest {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    entry.digest
                ))
            })?;
            let receipt = &entry.receipt;
            if receipt.digest.value != hex || !receipt.digest.is_well_formed() {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} speaks of a different content than the entry: {}",
                    entry.asset_path, receipt.digest.value
                )));
            }
            if receipt.outcome == ReceiptOutcome::Quarantined {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} is a quarantine: those bytes were refused by the origin",
                    entry.asset_path
                )));
            }
            if receipt.account != self.origin.account
                || receipt.repository != self.origin.repository
                || receipt.asset_path != entry.asset_path
            {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} belongs to another path or repository",
                    entry.asset_path
                )));
            }
            if receipt.size_bytes != entry.size_bytes {
                return Err(BundleError::Unsound(format!(
                    "the receipt of {} disagrees about the size",
                    entry.asset_path
                )));
            }
            digests.push(entry.digest.as_str());
        }

        let mut previous: Option<&str> = None;
        let mut referenced = Vec::with_capacity(digests.len());
        for blob in &self.blobs {
            hex_of(&blob.digest).ok_or_else(|| {
                BundleError::Unsound(format!(
                    "the blob {} is not `sha256:` followed by 64 lowercase hexadecimal digits",
                    blob.digest
                ))
            })?;
            if previous.is_some_and(|last| last >= blob.digest.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the blob {} is repeated or out of order",
                    blob.digest
                )));
            }
            previous = Some(&blob.digest);
            // A blob that declares no path is not imported: there is nothing
            // to back it with.
            match digests.iter().find(|digest| **digest == blob.digest) {
                None => {
                    return Err(BundleError::Unsound(format!(
                        "the blob {} belongs to no resolved path",
                        blob.digest
                    )));
                }
                // The declared size has to match that of every
                // path that shares it: the same content cannot measure
                // two things.
                Some(_) => {
                    for entry in self
                        .assets
                        .iter()
                        .filter(|entry| entry.digest == blob.digest)
                    {
                        if entry.size_bytes != blob.length {
                            return Err(BundleError::Unsound(format!(
                                "the blob {} says {} bytes and the path {} says {}",
                                blob.digest, blob.length, entry.asset_path, entry.size_bytes
                            )));
                        }
                    }
                    referenced.push(blob.digest.as_str());
                }
            }
        }
        for digest in &digests {
            if !referenced.contains(digest) {
                return Err(BundleError::Unsound(format!(
                    "the path content {digest} has no blob in the bundle"
                )));
            }
        }

        let mut previous: Option<&str> = None;
        for withheld in &self.withheld {
            check_path(&withheld.asset_path)?;
            if withheld.reason.is_empty() {
                return Err(BundleError::Unsound(format!(
                    "the withheld path {} has no reason",
                    withheld.asset_path
                )));
            }
            if previous.is_some_and(|last| last >= withheld.asset_path.as_str()) {
                return Err(BundleError::Unsound(format!(
                    "the withheld path {} is repeated or out of order",
                    withheld.asset_path
                )));
            }
            previous = Some(&withheld.asset_path);
            if self
                .assets
                .iter()
                .any(|entry| entry.asset_path == withheld.asset_path)
            {
                return Err(BundleError::Unsound(format!(
                    "the path {} is exported and withheld at once",
                    withheld.asset_path
                )));
            }
        }
        Ok(())
    }

    /// The path of a blob's file within the bundle.
    ///
    /// Only called with digests of an already-checked manifest.
    pub fn blob_file(digest: &str) -> PathBuf {
        let hex = hex_of(digest).expect("a blob path is built only on a checked manifest");
        PathBuf::from(BLOBS_DIRECTORY)
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(hex)
    }
}

/// A path that cannot write outside its repository on import.
fn check_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty())
        || path.chars().any(|c| c.is_control())
    {
        return Err(BundleError::Unsound(format!(
            "the path {path:?} is not a repository-relative path"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::ContentDigest;

    fn digest(byte: char) -> ContentDigest {
        ContentDigest::of_bytes(byte.to_string().as_bytes())
    }

    fn receipt(path: &str, content: ContentDigest) -> ProvenanceReceipt {
        ProvenanceReceipt {
            id: format!("prv-{path}"),
            account: "acme".into(),
            repository: "mirror".into(),
            asset_path: path.into(),
            upstream_id: "central".into(),
            source_url: format!("https://repo.example.com/{path}"),
            config_version: 3,
            size_bytes: 1,
            checksum: hodei_catalog::ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: None,
            last_modified: None,
            outcome: ReceiptOutcome::Fetched,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("time"),
            digest: content,
        }
    }

    fn entry(path: &str, byte: char) -> AssetEntry {
        let content = digest(byte);
        AssetEntry {
            asset_path: path.into(),
            package: path.into(),
            version: content.value.clone(),
            digest: format!("sha256:{}", content.value),
            size_bytes: 1,
            receipt: receipt(path, content),
            decision: None,
        }
    }

    fn manifest() -> BundleManifest {
        let entries = vec![entry("a/one", 'a'), entry("b/two", 'b')];
        let mut blobs: Vec<BlobEntry> = entries
            .iter()
            .map(|entry| BlobEntry {
                digest: entry.digest.clone(),
                length: entry.size_bytes,
            })
            .collect();
        blobs.sort_by(|a, b| a.digest.cmp(&b.digest));
        BundleManifest {
            format: FORMAT,
            exported_at: DateTime::<Utc>::from_timestamp(1_700_000_100, 0).expect("time"),
            origin: Origin {
                account: "acme".into(),
                repository: "mirror".into(),
                upstream_id: "central".into(),
                config_version: 3,
            },
            assets: entries,
            blobs,
            withheld: Vec::new(),
        }
    }

    #[test]
    fn a_well_formed_bundle_is_importable() {
        let bundle = manifest();
        bundle.check_importable().expect("sound");

        // And the round trip through JSON changes nothing: the canonical manifest is what
        // gets written and what gets read.
        let text = serde_json::to_string(&bundle).expect("writes");
        let read: BundleManifest = serde_json::from_str(&text).expect("reads");
        assert_eq!(read, bundle);
        read.check_importable().expect("still sound");
    }

    #[test]
    fn a_format_this_build_does_not_understand_is_refused() {
        let mut bundle = manifest();
        bundle.format = FORMAT + 1;
        let error = bundle.check_importable().expect_err("refused");
        assert!(
            error
                .to_string()
                .contains(&format!("format {}", FORMAT + 1)),
            "{error}"
        );
    }

    #[test]
    fn hostile_paths_never_come_in() {
        for case in ["", "/abs", "a/../b", "..", "a\\b", "a//b", "a\nb"] {
            let mut bundle = manifest();
            bundle.assets[0].asset_path = case.to_string();
            bundle.assets[0].receipt.asset_path = case.to_string();
            assert!(
                bundle.check_importable().is_err(),
                "{case:?} should be refused"
            );
        }
    }

    #[test]
    fn paths_must_be_ordered_and_never_repeated() {
        let mut bundle = manifest();
        bundle.assets.reverse();
        assert!(bundle.check_importable().is_err());
        let mut bundle = manifest();
        bundle.assets[1] = bundle.assets[0].clone();
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn a_receipt_of_other_than_the_entry_is_not_provenance() {
        // The receipt speaks of other content.
        let mut bundle = manifest();
        bundle.assets[0].receipt.digest = digest('z');
        assert!(bundle.check_importable().is_err());

        // The receipt is from another account or another repository.
        let mut bundle = manifest();
        bundle.assets[0].receipt.account = "someone-else".into();
        assert!(bundle.check_importable().is_err());

        // The receipt records a quarantine: those bytes were rejected at
        // the origin and do not travel as a resolution.
        let mut bundle = manifest();
        bundle.assets[0].receipt.outcome = ReceiptOutcome::Quarantined;
        assert!(bundle.check_importable().is_err());

        // And a size that does not match its receipt.
        let mut bundle = manifest();
        bundle.assets[0].size_bytes += 1;
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn blobs_and_paths_have_to_cover_each_exactly() {
        // The blob of a path is missing.
        let mut bundle = manifest();
        bundle.blobs.remove(0);
        assert!(bundle.check_importable().is_err());

        // There is a blob that belongs to no path.
        let mut bundle = manifest();
        bundle.blobs.push(BlobEntry {
            digest: format!("sha256:{}", digest('q').value),
            length: 1,
        });
        bundle.blobs.sort_by(|a, b| a.digest.cmp(&b.digest));
        assert!(bundle.check_importable().is_err());

        // The blob states some bytes that the path does not state.
        let mut bundle = manifest();
        bundle.blobs[0].length += 1;
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn the_same_content_under_two_paths_is_one_blob_and_two_entries() {
        let mut bundle = manifest();
        let content = digest('a');
        bundle.assets[1] = AssetEntry {
            asset_path: "c/same".into(),
            package: "c/same".into(),
            version: content.value.clone(),
            digest: format!("sha256:{}", content.value),
            size_bytes: 1,
            receipt: receipt("c/same", content),
            decision: None,
        };
        bundle
            .blobs
            .retain(|blob| blob.digest == bundle.assets[0].digest);
        bundle.check_importable().expect("sound");
    }

    #[test]
    fn a_withheld_path_is_explained_and_never_also_exported() {
        let mut bundle = manifest();
        bundle.withheld.push(Withheld {
            asset_path: "d/absent".into(),
            reason: "the content was damaged in the origin".into(),
        });
        bundle.check_importable().expect("sound");

        bundle.withheld[0].reason = String::new();
        assert!(bundle.check_importable().is_err());

        let mut bundle = manifest();
        bundle.withheld.push(Withheld {
            asset_path: "a/one".into(),
            reason: "exported and withheld at once".into(),
        });
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn a_bundle_bigger_than_an_honest_export_is_refused() {
        let mut bundle = manifest();
        bundle.assets = (0..=MAX_BUNDLE_ASSETS)
            .map(|index| entry(&format!("many/{index:07}"), 'a'))
            .collect();
        bundle.blobs = vec![BlobEntry {
            digest: bundle.assets[0].digest.clone(),
            length: 1,
        }];
        assert!(bundle.check_importable().is_err());
    }

    #[test]
    fn blob_files_live_in_the_same_layout_as_the_store() {
        let hex = digest('a').value;
        assert_eq!(
            BundleManifest::blob_file(&format!("sha256:{hex}")),
            PathBuf::from("blobs")
                .join(&hex[0..2])
                .join(&hex[2..4])
                .join(hex)
        );
    }

    #[test]
    fn the_well_formed_digest_of_a_receipt_is_its_own() {
        // A malformed receipt digest (even if the input matches) backs
        // nothing.
        let mut bundle = manifest();
        bundle.assets[0].receipt.digest.value = "../../etc/passwd".into();
        bundle.assets[0].digest = "sha256:../../etc/passwd".into();
        assert!(bundle.check_importable().is_err());
    }
}
