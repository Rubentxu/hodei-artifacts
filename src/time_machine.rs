//! Repository time machine (HSP-165, slice 1): explain what changed between two
//! frozen views of the same repository.
//!
//! There is no second ledger. The history is the set of freeze manifests the
//! dependency freeze (HSP-161) already writes; this module only *compares* two
//! of them. The comparison is a pure function of the two manifests, so it can
//! be repeated at any time with the same answer and it cannot write anything.
//!
//! What a difference cites are historical facts: the path, the package and
//! version, the digest and the size that a manifest recorded when it was
//! frozen. What this module never does is invent bytes: the *current*
//! availability of a digest is filled by the caller from the content store
//! (`present` / `gone`), and a digest the store no longer holds is reported as
//! `gone`, a coverage statement, not as a recoverable fact.

use serde::Serialize;

use crate::freeze::{FreezeEntry, FreezeManifest};

/// Most differences one answer lists. The totals are always exact; only the
/// listing is cut (`truncated`), so a client knows it is looking at a prefix.
pub const MAX_CHANGES: usize = 200;

/// Whether the bytes of a digest are in the content store today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The store holds the content.
    Present,
    /// The store no longer holds it (reclaimed by the collector). Coverage,
    /// not content: the time machine does not reconstruct it.
    Gone,
}

/// How one path differs between the two freezes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Only the later freeze names the path.
    Added,
    /// Only the earlier freeze names the path.
    Removed,
    /// The path names different content (a moved tag, a republished version).
    DigestChanged,
    /// The same content under another package name or version.
    CoordinatesChanged,
}

/// One difference, with the historical fact on each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// The path inside the repository.
    pub path: String,
    /// What kind of difference it is.
    pub kind: ChangeKind,
    /// What the earlier freeze recorded for the path.
    pub from: Option<FreezeEntry>,
    /// What the later freeze recorded for the path.
    pub to: Option<FreezeEntry>,
}

/// The difference between two manifests of one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// The listed differences, ordered by path, at most [`MAX_CHANGES`].
    pub changes: Vec<Change>,
    /// How many differences there are in total.
    pub total_changes: usize,
    /// Whether `changes` is a prefix of the total.
    pub truncated: bool,
    /// How many paths both freezes name identically.
    pub unchanged: usize,
}

/// Compares two manifests path by path. Both are sorted by path (the manifest
/// guarantees it), so this is one linear merge.
pub fn explain(from: &FreezeManifest, to: &FreezeManifest) -> Diff {
    let (mut i, mut j) = (0, 0);
    let mut all: Vec<Change> = Vec::new();
    let mut unchanged = 0;
    let change = |path: &str, kind, from: Option<&FreezeEntry>, to: Option<&FreezeEntry>| Change {
        path: path.to_string(),
        kind,
        from: from.cloned(),
        to: to.cloned(),
    };
    while i < from.entries.len() || j < to.entries.len() {
        match (from.entries.get(i), to.entries.get(j)) {
            (Some(a), Some(b)) if a.path == b.path => {
                if a == b {
                    unchanged += 1;
                } else if a.digest != b.digest {
                    all.push(change(&a.path, ChangeKind::DigestChanged, Some(a), Some(b)));
                } else {
                    all.push(change(
                        &a.path,
                        ChangeKind::CoordinatesChanged,
                        Some(a),
                        Some(b),
                    ));
                }
                i += 1;
                j += 1;
            }
            (Some(a), Some(b)) if a.path < b.path => {
                all.push(change(&a.path, ChangeKind::Removed, Some(a), None));
                i += 1;
            }
            (Some(_), Some(b)) => {
                all.push(change(&b.path, ChangeKind::Added, None, Some(b)));
                j += 1;
            }
            (Some(a), None) => {
                all.push(change(&a.path, ChangeKind::Removed, Some(a), None));
                i += 1;
            }
            (None, Some(b)) => {
                all.push(change(&b.path, ChangeKind::Added, None, Some(b)));
                j += 1;
            }
            (None, None) => break,
        }
    }
    let total_changes = all.len();
    all.truncate(MAX_CHANGES);
    Diff {
        truncated: total_changes > all.len(),
        changes: all,
        total_changes,
        unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hodei_catalog::{AssetRecord, PackageCoordinates};
    use hodei_evidence::ContentDigest;

    fn record(path: &str, version: &str, bytes: &[u8]) -> AssetRecord {
        AssetRecord {
            account: "acme".to_string(),
            repository: "r".to_string(),
            asset_path: path.to_string(),
            coordinates: PackageCoordinates {
                package_name: "p".to_string(),
                package_version: version.to_string(),
            },
            digest: ContentDigest::of_bytes(bytes),
            size_bytes: bytes.len() as u64,
            subject: None,
            updated_at: Utc::now(),
        }
    }

    fn manifest(records: &[AssetRecord]) -> FreezeManifest {
        FreezeManifest::build("acme", "r", "", "alice", Utc::now(), None, records).unwrap()
    }

    #[test]
    fn every_kind_of_difference_is_named_with_both_historical_facts() {
        let a = manifest(&[
            record("keep", "1", b"k"),
            record("moved", "1", b"old"),
            record("gone", "1", b"g"),
            record("renamed", "1", b"r"),
        ]);
        let b = manifest(&[
            record("keep", "1", b"k"),
            record("moved", "1", b"new"),
            record("new", "1", b"n"),
            record("renamed", "2", b"r"),
        ]);
        let diff = explain(&a, &b);
        let kinds: Vec<(&str, ChangeKind)> = diff
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("gone", ChangeKind::Removed),
                ("moved", ChangeKind::DigestChanged),
                ("new", ChangeKind::Added),
                ("renamed", ChangeKind::CoordinatesChanged),
            ]
        );
        assert_eq!(diff.unchanged, 1);
        let moved = &diff.changes[1];
        assert_ne!(
            moved.from.as_ref().unwrap().digest,
            moved.to.as_ref().unwrap().digest
        );
        assert!(!diff.truncated);
        assert_eq!(explain(&a, &a).total_changes, 0);
    }

    #[test]
    fn the_listing_is_cut_but_the_totals_are_exact() {
        let a = manifest(&[record("anchor", "1", b"a")]);
        let mut many: Vec<AssetRecord> = (0..MAX_CHANGES + 50)
            .map(|n| record(&format!("p{n:05}"), "1", format!("v{n}").as_bytes()))
            .collect();
        many.push(record("anchor", "1", b"a"));
        let diff = explain(&a, &manifest(&many));
        assert_eq!(diff.total_changes, MAX_CHANGES + 50);
        assert_eq!(diff.changes.len(), MAX_CHANGES);
        assert!(diff.truncated);
        assert_eq!(diff.unchanged, 1);
    }
}
