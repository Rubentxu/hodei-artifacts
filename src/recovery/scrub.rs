//! The scrub: checks that the catalog and the store tell each other the truth.
//!
//! Garbage collection already knows which blobs exist and which roots
//! reference them; the scrub covers what it does not do: **reading the bytes**.
//! It detects three ills, and all three are reported with the cause and with what
//! can be done, without touching anything:
//!
//! - **corrupt**: the blob's bytes no longer have the digest that its name
//!   says;
//! - **broken reference**: a catalog row points to a content that
//!   is not in the store;
//! - **orphan**: a blob that nobody references (it is reclaimed by garbage
//!   collection, not by the scrub).
//!
//! The scrub **does not repair on its own**. Repairing a blob is giving it back
//! the correct bytes, and the only correct bytes are those of a backup whose
//! hash matches. Inventing bytes, or
//! deleting a reference so that the report comes out clean, would be hiding
//! the damage.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_catalog::{ContentLedger, ContentRoot};
use hodei_hub::{BlobError, BlobStore, StoredBlob};
use serde::Serialize;

use crate::gc::plan::{Plan, Verdict};
use crate::gc::{GcError, GcService};

use super::RecoveryError;

/// A catalog row that references a content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct RootRef {
    /// Which row: asset, reference, version or subject.
    pub root: &'static str,
    /// The account.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The path or the version.
    pub subject: String,
}

/// What can be done with a finding.
///
/// It is a suggestion for the person operating, not an action that the scrub
/// executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    /// Give the bytes back from a backup whose hash matches
    /// (`hodei-recovery repair --from <backup>`).
    RestoreFromBackup,
    /// There is no backup that will do: republish or resync the
    /// content from its origin, or withdraw the affected version.
    RepublishOrRetire,
    /// Nobody references it: garbage collection picks it up.
    ReclaimWithGc,
}

/// A scrub finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// The bytes do not have the digest of their name.
    Corrupt {
        /// The content that was expected, `sha256:<hex>`.
        digest: String,
        /// The content that the existing bytes have.
        found: String,
        /// The bytes it occupies.
        length: u64,
        /// Which rows reference it: if empty, nobody uses it.
        referenced_by: Vec<RootRef>,
        /// What can be done.
        repair: RepairAction,
    },
    /// A catalog row points to a content that is not there.
    BrokenReference {
        /// The missing content, `sha256:<hex>`.
        digest: String,
        /// Which rows reference it.
        referenced_by: Vec<RootRef>,
        /// What can be done.
        repair: RepairAction,
    },
    /// A blob that nobody references.
    Orphan {
        /// The content, `sha256:<hex>`.
        digest: String,
        /// The bytes it occupies.
        length: u64,
        /// Whether it is still within the grace period: a recently
        /// uploaded content that has not been published yet is a legitimate orphan.
        in_grace: bool,
        /// What can be done.
        repair: RepairAction,
    },
}

impl Finding {
    /// The content it refers to.
    pub fn digest(&self) -> &str {
        match self {
            Self::Corrupt { digest, .. }
            | Self::BrokenReference { digest, .. }
            | Self::Orphan { digest, .. } => digest,
        }
    }

    /// Whether the finding is data damage (corrupt or broken reference),
    /// and not an orphan, which is only space.
    pub fn is_damage(&self) -> bool {
        !matches!(self, Self::Orphan { .. })
    }
}

/// The totals of a scrub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScrubSummary {
    /// Blobs with changed bytes.
    pub corrupt: u64,
    /// References to content that is not there.
    pub broken_references: u64,
    /// Blobs that nobody references and are no longer in grace.
    pub orphans: u64,
    /// Blobs that nobody references and are still in grace.
    pub orphans_in_grace: u64,
}

/// The report of a scrub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrubReport {
    /// How many blobs were read and checked.
    pub blobs_checked: u64,
    /// How many bytes were read.
    pub bytes_checked: u64,
    /// How many catalog rows were checked.
    pub roots_checked: u64,
    /// What was found, sorted by digest.
    pub findings: Vec<Finding>,
    /// The totals.
    pub summary: ScrubSummary,
    /// How long it took, in milliseconds.
    pub elapsed_ms: u64,
}

impl ScrubReport {
    /// Whether there is no damage: orphans do not count, they are space.
    pub fn is_sound(&self) -> bool {
        self.summary.corrupt == 0 && self.summary.broken_references == 0
    }

    /// Whether there is no finding at all, not even orphans.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// What the read of a blob said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The bytes have the digest that the name says.
    Sound,
    /// The bytes have another digest.
    Corrupt {
        /// The digest of the existing bytes.
        found: String,
    },
    /// The blob was no longer there when it came to be read.
    Gone,
}

/// Everything the report needs to know, already read.
pub struct Evidence<'a> {
    /// The store's blobs.
    pub blobs: &'a [StoredBlob],
    /// What the read of each one said, by `sha256:<hex>`.
    pub checks: &'a BTreeMap<String, Check>,
    /// The catalog's roots.
    pub roots: &'a [ContentRoot],
    /// The garbage collection plan: the orphans come from it.
    pub plan: &'a Plan,
    /// The contents that were checked again and are **not** there: only these
    /// can be a broken reference. A content published between the listing and
    /// the reading of the roots is not here and is not accused.
    pub confirmed_missing: &'a BTreeSet<String>,
}

fn root_ref(root: &ContentRoot) -> RootRef {
    RootRef {
        root: root.kind.as_str(),
        account: root.account.clone(),
        repository: root.repository.clone(),
        subject: root.subject.clone(),
    }
}

/// Computes the report. It is a pure function: it neither reads nor writes anything.
pub fn assess(evidence: &Evidence<'_>, elapsed: Duration) -> ScrubReport {
    // Who references each content.
    let mut referenced: BTreeMap<String, BTreeSet<RootRef>> = BTreeMap::new();
    for root in evidence.roots {
        referenced
            .entry(root.digest.to_string())
            .or_default()
            .insert(root_ref(root));
    }

    let mut findings: Vec<Finding> = Vec::new();
    let mut summary = ScrubSummary::default();
    let mut corrupt: BTreeSet<&str> = BTreeSet::new();

    for blob in evidence.blobs {
        let digest = blob.digest.to_string();
        if let Some(Check::Corrupt { found }) = evidence.checks.get(&digest) {
            let owners: Vec<RootRef> = referenced
                .get(&digest)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            let repair = if owners.is_empty() {
                RepairAction::ReclaimWithGc
            } else {
                RepairAction::RestoreFromBackup
            };
            summary.corrupt += 1;
            findings.push(Finding::Corrupt {
                digest,
                found: found.clone(),
                length: blob.length,
                referenced_by: owners,
                repair,
            });
        }
    }
    for finding in &findings {
        corrupt.insert(finding.digest());
    }
    let corrupt: BTreeSet<String> = corrupt.into_iter().map(str::to_string).collect();

    for (digest, owners) in &referenced {
        if evidence.confirmed_missing.contains(digest) {
            summary.broken_references += 1;
            findings.push(Finding::BrokenReference {
                digest: digest.clone(),
                referenced_by: owners.iter().cloned().collect(),
                repair: RepairAction::RestoreFromBackup,
            });
        }
    }

    for entry in &evidence.plan.entries {
        let in_grace = match entry.verdict {
            Verdict::InGrace { .. } => true,
            Verdict::Reclaimable { .. } => false,
            Verdict::Retained { .. } => continue,
            // The scrub's census is complete: an unknown here can only
            // be a degradation of the pagination, and that is never an
            // orphan.
            Verdict::Unknown { .. } => continue,
        };
        // A damaged orphan is already in the list as corrupt.
        if corrupt.contains(&entry.digest) {
            continue;
        }
        if in_grace {
            summary.orphans_in_grace += 1;
        } else {
            summary.orphans += 1;
        }
        findings.push(Finding::Orphan {
            digest: entry.digest.clone(),
            length: entry.length,
            in_grace,
            repair: RepairAction::ReclaimWithGc,
        });
    }

    findings.sort_by(|a, b| a.digest().cmp(b.digest()));

    ScrubReport {
        blobs_checked: evidence
            .checks
            .values()
            .filter(|check| !matches!(check, Check::Gone))
            .count() as u64,
        bytes_checked: evidence
            .blobs
            .iter()
            .filter(|blob| {
                !matches!(
                    evidence.checks.get(&blob.digest.to_string()),
                    Some(Check::Gone)
                )
            })
            .map(|blob| blob.length)
            .sum(),
        roots_checked: evidence.roots.len() as u64,
        findings,
        summary,
        elapsed_ms: elapsed.as_millis() as u64,
    }
}

/// The scrub service: reads the store and the catalog and builds the report.
pub struct Scrubber {
    store: Arc<dyn BlobStore>,
    gc: GcService,
    /// A scrub reads the whole store: there is only one at a time in the service.
    running: tokio::sync::Mutex<()>,
}

impl Scrubber {
    /// Creates the service. Fails if the store does not know how to list its blobs.
    pub fn new(
        ledger: Arc<dyn ContentLedger>,
        store: Arc<dyn BlobStore>,
        grace: Duration,
    ) -> Result<Self, RecoveryError> {
        let gc = GcService::new(ledger.clone(), store.clone(), grace)
            .map_err(|error| RecoveryError::Store(error.to_string()))?;
        Ok(Self {
            store,
            gc,
            running: tokio::sync::Mutex::new(()),
        })
    }

    /// Like [`Self::run`], but returns `None` if a scrub is already in progress.
    ///
    /// It is what the service uses: reading the whole store twice at once
    /// adds nothing and doubles the disk load.
    pub async fn try_run(&self) -> Result<Option<ScrubReport>, RecoveryError> {
        let Ok(_guard) = self.running.try_lock() else {
            return Ok(None);
        };
        self.run().await.map(Some)
    }

    /// Performs the scrub. It only reads: it writes, moves and deletes nothing.
    pub async fn run(&self) -> Result<ScrubReport, RecoveryError> {
        let started = Instant::now();
        // The census lists the blobs first and reads the roots afterwards: that way what
        // gets published in the meantime can only be over-protected, never
        // under-protected.
        let (plan, blobs) = self
            .gc
            .census_with(crate::gc::service::ManifestReads::Lenient)
            .await
            .map_err(|error| match error {
                GcError::Unknown(message) | GcError::Store(message) => {
                    RecoveryError::Store(message)
                }
                other => RecoveryError::Store(other.to_string()),
            })?;
        let roots = self
            .gc
            .census_roots()
            .await
            .map_err(|error| RecoveryError::Store(error.to_string()))?;

        let mut checks: BTreeMap<String, Check> = BTreeMap::new();
        for blob in &blobs {
            let check = match self.store.verify(&blob.digest).await {
                Ok(()) => Check::Sound,
                Err(BlobError::Corrupted { found, .. }) => Check::Corrupt {
                    found: found.to_string(),
                },
                Err(BlobError::NotFound) => Check::Gone,
                Err(error) => return Err(RecoveryError::Store(error.to_string())),
            };
            checks.insert(blob.digest.to_string(), check);
        }

        // A root whose content is not in the listing may be a broken
        // reference **or** something published after the listing: only what
        // is still missing when asked again is accused.
        let listed: BTreeSet<String> = blobs.iter().map(|blob| blob.digest.to_string()).collect();
        let mut confirmed_missing: BTreeSet<String> = BTreeSet::new();
        for root in &roots {
            let digest = root.digest.to_string();
            if listed.contains(&digest) || confirmed_missing.contains(&digest) {
                continue;
            }
            let present = self
                .store
                .exists(&root.digest)
                .await
                .map_err(|error| RecoveryError::Store(error.to_string()))?;
            if !present {
                confirmed_missing.insert(digest);
            }
        }

        Ok(assess(
            &Evidence {
                blobs: &blobs,
                checks: &checks,
                roots: &roots,
                plan: &plan,
                confirmed_missing: &confirmed_missing,
            },
            started.elapsed(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::plan::{Entry, Summary};
    use hodei_catalog::RootKind;
    use hodei_evidence::ContentDigest;

    fn digest(seed: char) -> ContentDigest {
        ContentDigest::sha256(seed.to_string().repeat(64))
    }

    fn blob(seed: char, length: u64) -> StoredBlob {
        StoredBlob {
            digest: digest(seed),
            length,
            age: Duration::from_secs(3600),
        }
    }

    fn root(seed: char, subject: &str) -> ContentRoot {
        ContentRoot {
            kind: RootKind::Asset,
            account: "acme".into(),
            repository: "images".into(),
            subject: subject.into(),
            digest: digest(seed),
        }
    }

    fn plan_of(entries: Vec<Entry>) -> Plan {
        Plan {
            grace_secs: 60,
            summary: Summary::default(),
            entries,
        }
    }

    fn orphan(seed: char, in_grace: bool) -> Entry {
        Entry {
            digest: digest(seed).to_string(),
            length: 5,
            verdict: if in_grace {
                Verdict::InGrace {
                    age_secs: 1,
                    remaining_secs: 59,
                }
            } else {
                Verdict::Reclaimable { age_secs: 3600 }
            },
        }
    }

    fn report(
        blobs: &[StoredBlob],
        checks: &[(char, Check)],
        roots: &[ContentRoot],
        entries: Vec<Entry>,
        missing: &[char],
    ) -> ScrubReport {
        let checks: BTreeMap<String, Check> = checks
            .iter()
            .map(|(seed, check)| (digest(*seed).to_string(), check.clone()))
            .collect();
        let missing: BTreeSet<String> = missing.iter().map(|s| digest(*s).to_string()).collect();
        assess(
            &Evidence {
                blobs,
                checks: &checks,
                roots,
                plan: &plan_of(entries),
                confirmed_missing: &missing,
            },
            Duration::from_millis(7),
        )
    }

    #[test]
    fn a_healthy_store_has_no_findings() {
        let result = report(
            &[blob('a', 3)],
            &[('a', Check::Sound)],
            &[root('a', "blobs/a")],
            vec![],
            &[],
        );
        assert!(result.is_clean() && result.is_sound());
        assert_eq!(result.blobs_checked, 1);
        assert_eq!(result.bytes_checked, 3);
        assert_eq!(result.roots_checked, 1);
    }

    #[test]
    fn a_corrupt_blob_names_what_it_has_and_who_uses_it() {
        let result = report(
            &[blob('a', 3)],
            &[(
                'a',
                Check::Corrupt {
                    found: digest('f').to_string(),
                },
            )],
            &[root('a', "blobs/a"), root('a', "manifests/a")],
            vec![],
            &[],
        );
        assert!(!result.is_sound());
        assert_eq!(result.summary.corrupt, 1);
        let Finding::Corrupt {
            found,
            referenced_by,
            repair,
            ..
        } = &result.findings[0]
        else {
            panic!("a corrupt finding was expected");
        };
        assert_eq!(found, &digest('f').to_string());
        assert_eq!(referenced_by.len(), 2);
        assert_eq!(*repair, RepairAction::RestoreFromBackup);
    }

    #[test]
    fn a_corrupt_blob_nobody_uses_is_left_to_the_collector() {
        let result = report(
            &[blob('a', 3)],
            &[(
                'a',
                Check::Corrupt {
                    found: digest('f').to_string(),
                },
            )],
            &[],
            vec![orphan('a', false)],
            &[],
        );
        // It appears only once: as corrupt, not also as an orphan.
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.summary.orphans, 0);
        let Finding::Corrupt { repair, .. } = &result.findings[0] else {
            panic!("a corrupt finding was expected");
        };
        assert_eq!(*repair, RepairAction::ReclaimWithGc);
    }

    #[test]
    fn a_row_that_points_at_nothing_is_a_broken_reference_with_every_owner() {
        let result = report(
            &[],
            &[],
            &[root('b', "blobs/b"), root('b', "manifests/b")],
            vec![],
            &['b'],
        );
        assert_eq!(result.summary.broken_references, 1);
        let Finding::BrokenReference { referenced_by, .. } = &result.findings[0] else {
            panic!("a broken reference was expected");
        };
        assert_eq!(referenced_by.len(), 2);
        assert!(!result.is_sound());
    }

    #[test]
    fn content_that_was_published_after_the_listing_is_not_accused() {
        // The root exists, the blob was not listed, but on asking again
        // it was there: it is not in `confirmed_missing` and is not a finding.
        let result = report(&[], &[], &[root('c', "blobs/c")], vec![], &[]);
        assert!(result.is_clean());
    }

    #[test]
    fn an_orphan_is_space_not_damage_and_the_grace_is_kept_apart() {
        let result = report(
            &[blob('d', 4), blob('e', 6)],
            &[('d', Check::Sound), ('e', Check::Sound)],
            &[],
            vec![orphan('d', false), orphan('e', true)],
            &[],
        );
        assert!(result.is_sound());
        assert!(!result.is_clean());
        assert_eq!(result.summary.orphans, 1);
        assert_eq!(result.summary.orphans_in_grace, 1);
        assert!(result.findings.iter().all(|finding| !finding.is_damage()));
    }

    #[test]
    fn a_blob_that_vanished_while_reading_is_not_counted_as_checked() {
        let result = report(&[blob('a', 3)], &[('a', Check::Gone)], &[], vec![], &[]);
        assert_eq!(result.blobs_checked, 0);
        assert_eq!(result.bytes_checked, 0);
        assert!(result.is_clean());
    }

    #[test]
    fn the_findings_are_ordered_by_digest_and_serialize_with_their_kind() {
        let result = report(
            &[blob('b', 1), blob('a', 1)],
            &[
                (
                    'a',
                    Check::Corrupt {
                        found: digest('f').to_string(),
                    },
                ),
                ('b', Check::Sound),
            ],
            &[root('a', "blobs/a"), root('c', "blobs/c")],
            vec![orphan('b', false)],
            &['c'],
        );
        let digests: Vec<&str> = result.findings.iter().map(Finding::digest).collect();
        let mut sorted = digests.clone();
        sorted.sort();
        assert_eq!(digests, sorted);
        let json = serde_json::to_value(&result).unwrap();
        let kinds: Vec<&str> = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| finding["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["corrupt", "orphan", "broken_reference"]);
    }
}
