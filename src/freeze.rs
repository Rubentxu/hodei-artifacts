//! Dependency freeze (HSP-161): an immutable, reproducible view of a repository.
//!
//! A freeze is a **manifest** of `(path, package, version) -> digest`, named by
//! the digest of its own canonical bytes. It is not a lockfile and it is not a
//! copy: the bytes stay where they already are, in the content store, under
//! their digest. What the freeze adds is two things:
//!
//! - **a view that cannot move**: reads of a freeze are answered from the
//!   manifest alone, never from the live catalog, so moving a tag or publishing
//!   a newer version in the origin repository changes nothing the freeze serves;
//! - **a pin**: one garbage-collection hold per digest (and one for the manifest
//!   itself), placed in the same ledger the collector already reads. There is
//!   no second retention mechanism. Releasing the freeze removes only its own
//!   holds; whether a blob can then be reclaimed is decided by the general
//!   census, which still sees every other root and hold.
//!
//! The manifest is persisted as a blob of the content store. A blob is not
//! evidence that a freeze exists (anyone who can upload a file can upload JSON
//! that parses as a manifest), so the **registry** of freezes is the hold
//! `freeze-<id>-manifest`, which only this module writes: no manifest without
//! that hold is ever served.

use chrono::{DateTime, Duration, Utc};
use hodei_catalog::{AssetRecord, ContentHold, HoldScope};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

/// The version of the manifest format.
pub const FREEZE_VERSION: u32 = 1;

/// Longest time a freeze may pin on its own. Beyond it, a freeze has to be
/// created without expiry (held until released) so the choice is explicit.
pub const MAX_TTL_SECS: i64 = 10 * 365 * 24 * 3600;

/// The stable code a client gets when the freeze does not contain what it asked
/// for. Absence is a verdict, never a prompt to resolve again.
pub const CODE_MISSING_DEPENDENCY: &str = "E_MISSING_DEPENDENCY";

/// The prefix of every hold a freeze places.
const HOLD_PREFIX: &str = "freeze-";

/// One resolved path of the freeze.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreezeEntry {
    /// The path inside the origin repository.
    pub path: String,
    /// The package name the catalog records for the path.
    pub package: String,
    /// The version the catalog records for the path.
    pub version: String,
    /// The content, as `sha256:<hex>`.
    pub digest: String,
    /// The bytes of the content.
    pub size_bytes: u64,
}

/// The immutable manifest of a freeze.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreezeManifest {
    /// The format version.
    pub version: u32,
    /// The account that owns the origin repository.
    pub account: String,
    /// The origin repository.
    pub repository: String,
    /// The path prefix the freeze was taken over (empty for everything).
    pub prefix: String,
    /// Who froze it.
    pub created_by: String,
    /// When it was frozen.
    pub created_at: DateTime<Utc>,
    /// Until when it pins its blobs. `None` is until somebody releases it.
    pub expires_at: Option<DateTime<Utc>>,
    /// The resolved paths, ordered by path and without repetitions.
    pub entries: Vec<FreezeEntry>,
}

/// Why a freeze cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FreezeError {
    /// There is nothing to freeze.
    #[error("the repository has nothing under that prefix to freeze")]
    Empty,
    /// The requested lifetime is not usable.
    #[error("the lifetime of a freeze must be positive and at most {MAX_TTL_SECS} seconds")]
    BadTtl,
    /// A stored manifest cannot be trusted.
    #[error("the manifest is not well formed")]
    Malformed,
}

impl FreezeManifest {
    /// Builds the manifest from the catalog records the caller already
    /// filtered for what the repository may serve.
    pub fn build(
        account: &str,
        repository: &str,
        prefix: &str,
        created_by: &str,
        now: DateTime<Utc>,
        ttl: Option<Duration>,
        records: &[AssetRecord],
    ) -> Result<Self, FreezeError> {
        if let Some(ttl) = ttl
            && (ttl <= Duration::zero() || ttl.num_seconds() > MAX_TTL_SECS)
        {
            return Err(FreezeError::BadTtl);
        }
        let mut entries: Vec<FreezeEntry> = records
            .iter()
            .map(|record| FreezeEntry {
                path: record.asset_path.clone(),
                package: record.coordinates.package_name.clone(),
                version: record.coordinates.package_version.clone(),
                digest: format!("{}:{}", record.digest.algorithm, record.digest.value),
                size_bytes: record.size_bytes,
            })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);
        if entries.is_empty() {
            return Err(FreezeError::Empty);
        }
        Ok(Self {
            version: FREEZE_VERSION,
            account: account.to_string(),
            repository: repository.to_string(),
            prefix: prefix.to_string(),
            created_by: created_by.to_string(),
            created_at: now,
            expires_at: ttl.map(|ttl| now + ttl),
            entries,
        })
    }

    /// The canonical bytes: field order is fixed by the type and entries are
    /// sorted, so the same manifest digests the same on any node.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a manifest always serializes")
    }

    /// The identity of the freeze: the hex digest of its canonical bytes.
    pub fn id(&self) -> String {
        ContentDigest::of_bytes(&self.bytes()).value
    }

    /// Parses stored bytes and checks they are exactly what `id` names.
    ///
    /// A manifest whose bytes do not digest to its id, or whose version this
    /// build does not write, is refused: a freeze is only served if it is the
    /// freeze that was named.
    pub fn from_verified_bytes(id: &str, bytes: &[u8]) -> Result<Self, FreezeError> {
        if ContentDigest::of_bytes(bytes).value != id {
            return Err(FreezeError::Malformed);
        }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|_| FreezeError::Malformed)?;
        if manifest.version != FREEZE_VERSION || manifest.bytes() != bytes {
            return Err(FreezeError::Malformed);
        }
        Ok(manifest)
    }

    /// The entry of an exact path, if the freeze holds it.
    pub fn entry(&self, path: &str) -> Option<&FreezeEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }

    /// Whether the freeze still pins at `now`.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }

    /// The digests the freeze pins, ordered and without repetitions.
    pub fn digests(&self) -> Vec<&str> {
        let mut digests: Vec<&str> = self
            .entries
            .iter()
            .map(|entry| entry.digest.as_str())
            .collect();
        digests.sort_unstable();
        digests.dedup();
        digests
    }

    /// The holds that make the freeze real: one per digest, plus the registry
    /// hold of the manifest blob. All of them expire with the freeze.
    pub fn holds(&self) -> Vec<ContentHold> {
        let id = self.id();
        let hold = |hold_id: String, hex: &str, what: &str| ContentHold {
            id: hold_id,
            scope: HoldScope::Digest {
                digest: ContentDigest::sha256(hex),
            },
            reason: format!("dependency freeze {id}: {what}"),
            actor: self.created_by.clone(),
            created_at: self.created_at,
            expires_at: self.expires_at,
        };
        let mut holds = vec![hold(manifest_hold_id(&id), &id, "manifest")];
        for digest in self.digests() {
            if let Some(hex) = digest.strip_prefix("sha256:") {
                holds.push(hold(entry_hold_id(&id, hex), hex, "pinned content"));
            }
        }
        holds
    }
}

/// The registry hold of a freeze: its existence is what makes a manifest blob a
/// freeze.
pub fn manifest_hold_id(id: &str) -> String {
    format!("{HOLD_PREFIX}{id}-manifest")
}

fn entry_hold_id(id: &str, hex: &str) -> String {
    format!("{HOLD_PREFIX}{id}-{hex}")
}

/// The prefix shared by every hold of one freeze, and by no other owner's.
pub fn hold_prefix(id: &str) -> String {
    format!("{HOLD_PREFIX}{id}-")
}

/// Whether text is a well formed freeze id (64 lowercase hex digits).
pub fn is_well_formed_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_catalog::PackageCoordinates;

    fn record(path: &str, bytes: &[u8]) -> AssetRecord {
        AssetRecord {
            account: "acme".to_string(),
            repository: "r".to_string(),
            asset_path: path.to_string(),
            coordinates: PackageCoordinates {
                package_name: "left-pad".to_string(),
                package_version: "1.0.0".to_string(),
            },
            digest: ContentDigest::of_bytes(bytes),
            size_bytes: bytes.len() as u64,
            subject: None,
            updated_at: Utc::now(),
        }
    }

    fn manifest(
        records: &[AssetRecord],
        ttl: Option<Duration>,
    ) -> Result<FreezeManifest, FreezeError> {
        FreezeManifest::build("acme", "r", "", "alice", Utc::now(), ttl, records)
    }

    #[test]
    fn the_id_is_the_digest_of_the_bytes_and_a_round_trip_verifies() {
        let m = manifest(&[record("b", b"2"), record("a", b"1")], None).unwrap();
        assert_eq!(m.entries[0].path, "a", "entries are ordered");
        let id = m.id();
        assert!(is_well_formed_id(&id));
        assert_eq!(
            FreezeManifest::from_verified_bytes(&id, &m.bytes()).unwrap(),
            m
        );
    }

    #[test]
    fn bytes_that_do_not_digest_to_the_id_are_refused() {
        let m = manifest(&[record("a", b"1")], None).unwrap();
        let mut forged = m.clone();
        forged.entries[0].digest = format!("sha256:{}", "0".repeat(64));
        assert_eq!(
            FreezeManifest::from_verified_bytes(&m.id(), &forged.bytes()),
            Err(FreezeError::Malformed)
        );
    }

    #[test]
    fn an_empty_freeze_and_a_bad_lifetime_are_refused() {
        assert_eq!(manifest(&[], None), Err(FreezeError::Empty));
        let one = [record("a", b"1")];
        assert_eq!(
            manifest(&one, Some(Duration::zero())),
            Err(FreezeError::BadTtl)
        );
        assert_eq!(
            manifest(&one, Some(Duration::seconds(MAX_TTL_SECS + 1))),
            Err(FreezeError::BadTtl)
        );
    }

    #[test]
    fn every_hold_belongs_to_its_freeze_and_shared_digests_are_pinned_once() {
        let m = manifest(
            &[
                record("a", b"same"),
                record("b", b"same"),
                record("c", b"x"),
            ],
            None,
        )
        .unwrap();
        let id = m.id();
        let holds = m.holds();
        assert_eq!(holds.len(), 3, "manifest + two distinct digests");
        assert!(holds.iter().all(|h| h.id.starts_with(&hold_prefix(&id))));
        assert!(holds.iter().any(|h| h.id == manifest_hold_id(&id)));
        assert!(!"freeze-other-aa".starts_with(&hold_prefix(&id)));
    }

    #[test]
    fn expiry_is_judged_against_the_clock() {
        let m = manifest(&[record("a", b"1")], Some(Duration::seconds(60))).unwrap();
        assert!(m.is_active(m.created_at));
        assert!(!m.is_active(m.created_at + Duration::seconds(61)));
        assert!(
            manifest(&[record("a", b"1")], None)
                .unwrap()
                .is_active(Utc::now() + Duration::days(9999))
        );
    }
}
