//! The state of the vulnerability feed: what Hodei has seen, when it saw
//! it, and what it means when the feed stops saying anything.
//!
//! Hodei does not invent a database: it reads what the publishers give it
//! and it keeps what it read. Three rules hold this module:
//!
//! - **A feed that is down or behind marks freshness and coverage, and it
//!   does not erase what was already known.** The absence of a page is
//!   not the absence of a vulnerability: yesterday's CVE does not stop
//!   existing because today the fetch failed.
//! - **Sync is idempotent.** The same page applied twice leaves the same
//!   state. A publisher that retries is not a new advisory, and a feed
//!   that answers the same thing twice does not double the work.
//! - **An air-gap import is verifiable.** Every document carries the
//!   digest it was exported with; a bundle whose document does not match
//!   its digest is rejected and not applied. What the state says after an
//!   import says which bundle it came from.
//!
//! Aliases are deduplicated WITHOUT dropping the advisories: two
//! publishers that call the same problem by two identifiers are two
//! statements about ONE vulnerability, and the feed keeps both statements
//! and one canonical key.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::affected::{AffectedState, DependencyKind, Finding};
use crate::osv::{OsvAdvisory, read_osv};
use crate::version::{Match, match_version};

/// One page of advisories, as a source handed it over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPage {
    /// The source that published the page.
    pub source: String,
    /// The moment the page was fetched.
    pub fetched_at: DateTime<Utc>,
    /// The advisory documents, as their publisher wrote them.
    pub documents: Vec<String>,
}

/// What one document is, keyed by the digest it was exported with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEntry {
    /// The advisory as it is in the feed.
    pub advisory_id: String,
    /// The digest of the document, as `sha256:<hex>`.
    pub digest: String,
    /// The document itself, as the publisher wrote it.
    pub document: String,
}

/// A bundle taken out of the feed and carried across an air gap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirGapBundle {
    /// Where the bundle came from.
    pub source: String,
    /// The moment it was exported.
    pub exported_at: DateTime<Utc>,
    /// The documents it carries.
    pub entries: Vec<BundleEntry>,
}

/// What the state was at the moment the page was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Advisories that were not in the state before.
    pub inserted: usize,
    /// Advisories whose document changed and was applied.
    pub updated: usize,
    /// Advisories that were already there, byte for byte.
    pub unchanged: usize,
    /// Documents that were not applied: they were not advisories, or the
    /// state already held a newer version of them.
    pub rejected: usize,
    /// Withdrawn advisories that stayed in the state because a withdrawal
    /// withdraws a statement and does not delete a vulnerability.
    pub withdrawn_kept: usize,
    /// The identifiers of the advisories whose record was inserted or
    /// updated. The incremental reevaluation of AC4 is driven by these
    /// and by nothing else: the page that changed nothing asks for
    /// nothing.
    pub changed_ids: Vec<String>,
    /// The freshness the state has after the sync.
    pub freshness: FeedStatus,
}

/// Why a page could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedFailure {
    /// The source did not answer.
    Unavailable(String),
    /// The source answered late or out of order; the page is not applied.
    Behind {
        /// The newest page the feed has seen.
        last_seen: DateTime<Utc>,
    },
}

/// How old the newest thing the feed knows is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedStatus {
    /// The feed said something recently.
    Current,
    /// The feed said something, and that was a while ago.
    Stale {
        /// Minutes since the last good page.
        lag_minutes: i64,
    },
    /// The last attempt failed, and the state is what it was when it
    /// last succeeded. The records are NOT erased.
    Down {
        /// Minutes since the last good page.
        lag_minutes: i64,
        /// Attempts that failed since then.
        failures: u32,
    },
    /// Nothing has been seen at all.
    Unknown,
}

impl FeedStatus {
    /// Whether the feed is not telling anything useful right now. A stale
    /// or down feed is a reason to mark the answers, and it is never a
    /// reason to forget them.
    pub fn is_impaired(&self) -> bool {
        !matches!(self, Self::Current)
    }
}

/// What the feed knows, counted without interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// The advisories kept, withdrawn ones included.
    pub advisories: usize,
    /// How many of them were withdrawn and kept.
    pub withdrawn: usize,
    /// The distinct vulnerabilities once the aliases are deduplicated.
    pub canonical_vulnerabilities: usize,
    /// The affected packages per ecosystem they name.
    pub by_ecosystem: BTreeMap<String, usize>,
}

/// One advisory as the feed keeps it: the document as it was published,
/// its digest, and when the feed first and last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryRecord {
    /// The advisory, read.
    pub advisory: OsvAdvisory,
    /// The document as the publisher wrote it.
    pub document: String,
    /// The digest of that document.
    pub digest: String,
    /// When the feed saw this advisory for the first time.
    pub first_seen_at: DateTime<Utc>,
    /// When the feed saw this version of it.
    pub last_seen_at: DateTime<Utc>,
}

/// The feed's state: what it has, keyed by the identifier of the
/// advisory, with the alias index beside it.
#[derive(Debug, Clone)]
pub struct RiskFeed {
    records: BTreeMap<String, AdvisoryRecord>,
    /// Every identifier an advisory answers to (its own and its aliases)
    /// mapped to the canonical advisory. The canonical one is the first
    /// the feed saw, so that a second publisher calling the same problem
    /// by another name does not move the key under anybody's feet.
    alias_index: BTreeMap<String, String>,
    last_success_at: Option<DateTime<Utc>>,
    last_failure_at: Option<DateTime<Utc>>,
    failures_since_success: u32,
    /// How long before "the feed said something" becomes "the feed is
    /// behind".
    stale_after: Duration,
}

impl RiskFeed {
    /// A feed that says it is stale after the given duration of silence.
    pub fn new(stale_after: Duration) -> Self {
        Self {
            records: BTreeMap::new(),
            alias_index: BTreeMap::new(),
            last_success_at: None,
            last_failure_at: None,
            failures_since_success: 0,
            stale_after,
        }
    }

    /// The advisories kept, withdrawn ones included.
    pub fn records(&self) -> impl Iterator<Item = &AdvisoryRecord> {
        self.records.values()
    }

    /// The advisory of an identifier, be it its own or an alias.
    pub fn by_alias(&self, alias: &str) -> Option<&OsvAdvisory> {
        self.alias_index
            .get(alias)
            .and_then(|id| self.records.get(id))
            .map(|record| &record.advisory)
    }

    /// How many advisories are kept.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether nothing is kept at all.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The freshness of the feed at the given moment.
    pub fn status(&self, now: DateTime<Utc>) -> FeedStatus {
        let Some(last_success) = self.last_success_at else {
            return FeedStatus::Unknown;
        };
        let lag = now.signed_duration_since(last_success).num_minutes();
        if self.failures_since_success > 0 {
            return FeedStatus::Down {
                lag_minutes: lag,
                failures: self.failures_since_success,
            };
        }
        if now.signed_duration_since(last_success) > self.stale_after {
            return FeedStatus::Stale { lag_minutes: lag };
        }
        FeedStatus::Current
    }

    /// The moment the feed last succeeded, when it succeeded.
    pub fn last_success_at(&self) -> Option<DateTime<Utc>> {
        self.last_success_at
    }

    /// The moment the feed last failed, when it failed.
    pub fn last_failure_at(&self) -> Option<DateTime<Utc>> {
        self.last_failure_at
    }

    /// What the feed knows, counted. The count does not change when the
    /// feed is down: the CVEs are still there.
    pub fn coverage(&self) -> Coverage {
        let mut by_ecosystem: BTreeMap<String, usize> = BTreeMap::new();
        let mut withdrawn = 0usize;
        for record in self.records.values() {
            if record.advisory.is_withdrawn() {
                withdrawn += 1;
            }
            for affected in &record.advisory.affected {
                *by_ecosystem
                    .entry(affected.package.ecosystem.clone())
                    .or_default() += 1;
            }
        }
        let distinct: BTreeSet<String> = self.alias_index.values().cloned().collect();
        Coverage {
            advisories: self.records.len(),
            withdrawn,
            canonical_vulnerabilities: distinct.len(),
            by_ecosystem,
        }
    }

    /// Applies a page the source handed over.
    ///
    /// A document already there with the same digest is unchanged; a
    /// document newer or equal in `modified` is applied; a document
    /// older than what the state holds is rejected, because a page that
    /// answers out of order does not undo what the feed already saw. A
    /// withdrawn advisory is kept and marked. The failure of a page
    /// marks the feed and changes nothing else.
    pub fn apply_page(
        &mut self,
        page: Result<FeedPage, FeedFailure>,
        now: DateTime<Utc>,
    ) -> SyncReport {
        let page = match page {
            Ok(page) => page,
            Err(_) => {
                self.failures_since_success += 1;
                self.last_failure_at = Some(now);
                let freshness = self.status(now);
                return SyncReport {
                    inserted: 0,
                    updated: 0,
                    unchanged: 0,
                    rejected: 0,
                    withdrawn_kept: 0,
                    changed_ids: Vec::new(),
                    freshness,
                };
            }
        };

        let mut report = SyncReport {
            inserted: 0,
            updated: 0,
            unchanged: 0,
            rejected: 0,
            withdrawn_kept: 0,
            changed_ids: Vec::new(),
            freshness: FeedStatus::Unknown,
        };

        for document in &page.documents {
            let Ok(advisory) = read_osv(document) else {
                report.rejected += 1;
                continue;
            };
            let digest = digest_of(document);
            let existing = self.records.get(&advisory.id);

            let action = match existing {
                None => Action::Insert,
                Some(record) if record.digest == digest => Action::Unchanged,
                Some(record) => {
                    // Older than what the state holds is a page that
                    // answers out of order; it is not applied.
                    let out_of_order = matches!(
                        (record.advisory.modified, advisory.modified),
                        (Some(seen), Some(older)) if older < seen
                    );
                    if out_of_order {
                        Action::Reject
                    } else {
                        Action::Update
                    }
                }
            };

            match action {
                Action::Unchanged => report.unchanged += 1,
                Action::Reject => report.rejected += 1,
                Action::Insert | Action::Update => {
                    if advisory.is_withdrawn() {
                        report.withdrawn_kept += 1;
                    }
                    let first_seen_at = existing.map(|record| record.first_seen_at).unwrap_or(now);
                    if matches!(action, Action::Update) {
                        report.updated += 1;
                    } else {
                        report.inserted += 1;
                    }
                    report.changed_ids.push(advisory.id.clone());
                    self.index_aliases(&advisory);
                    self.records.insert(
                        advisory.id.clone(),
                        AdvisoryRecord {
                            advisory,
                            document: document.clone(),
                            digest,
                            first_seen_at,
                            last_seen_at: now,
                        },
                    );
                }
            }
        }

        self.last_success_at = Some(now);
        self.failures_since_success = 0;
        report.freshness = self.status(now);
        report
    }

    /// Takes the state out to be carried across an air gap.
    pub fn export_bundle(&self, source: &str, now: DateTime<Utc>) -> AirGapBundle {
        let entries = self
            .records
            .values()
            .map(|record| BundleEntry {
                advisory_id: record.advisory.id.clone(),
                digest: record.digest.clone(),
                document: record.document.clone(),
            })
            .collect();
        AirGapBundle {
            source: source.to_string(),
            exported_at: now,
            entries,
        }
    }

    /// Puts a bundle in, checking every document against the digest it
    /// was exported with.
    ///
    /// A document that does not match its digest is REJECTED and not
    /// applied: an import that could not be verified is an import nobody
    /// can trust. A document that is already there, byte for byte, is
    /// unchanged: importing the same bundle twice leaves the same state.
    pub fn import_bundle(&mut self, bundle: &AirGapBundle, now: DateTime<Utc>) -> SyncReport {
        let mut report = SyncReport {
            inserted: 0,
            updated: 0,
            unchanged: 0,
            rejected: 0,
            withdrawn_kept: 0,
            changed_ids: Vec::new(),
            freshness: FeedStatus::Unknown,
        };

        for entry in &bundle.entries {
            if digest_of(&entry.document) != entry.digest {
                report.rejected += 1;
                continue;
            }
            let Ok(advisory) = read_osv(&entry.document) else {
                report.rejected += 1;
                continue;
            };

            let existing = self.records.get(&advisory.id);
            if existing.is_some_and(|record| record.digest == entry.digest) {
                report.unchanged += 1;
                continue;
            }

            if advisory.is_withdrawn() {
                report.withdrawn_kept += 1;
            }
            let first_seen_at = existing.map(|record| record.first_seen_at).unwrap_or(now);
            if existing.is_some() {
                report.updated += 1;
            } else {
                report.inserted += 1;
            }
            report.changed_ids.push(advisory.id.clone());
            self.index_aliases(&advisory);
            self.records.insert(
                advisory.id.clone(),
                AdvisoryRecord {
                    advisory,
                    document: entry.document.clone(),
                    digest: entry.digest.clone(),
                    first_seen_at,
                    last_seen_at: now,
                },
            );
        }

        self.last_success_at = Some(now);
        self.failures_since_success = 0;
        report.freshness = self.status(now);
        report
    }

    fn index_aliases(&mut self, advisory: &OsvAdvisory) {
        // The canonical key is the FIRST identifier the feed saw for this
        // problem, so that a second publisher with another name does not
        // move the key.
        let mut keys: Vec<&str> = vec![advisory.id.as_str()];
        keys.extend(advisory.aliases.iter().map(String::as_str));

        let canonical = keys
            .iter()
            .find_map(|key| self.alias_index.get(*key).cloned())
            .unwrap_or_else(|| advisory.id.clone());

        for key in keys {
            self.alias_index
                .entry(key.to_string())
                .or_insert_with(|| canonical.clone());
        }
    }
}

/// What applying one document does.
enum Action {
    Insert,
    Update,
    Unchanged,
    Reject,
}

/// The digest a document is keyed by.
fn digest_of(document: &str) -> String {
    format!(
        "sha256:{}",
        hodei_evidence::ContentDigest::of_bytes(document.as_bytes()).value
    )
}

/// What a component looks like to the reevaluation: the package and the
/// version, without the inventory behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedComponent {
    /// The name of the package.
    pub package: String,
    /// The version installed.
    pub version: Option<String>,
    /// How the artefact reaches it.
    pub dependency: DependencyKind,
}

/// One round of reevaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reevaluation {
    /// The findings of the packages the changed advisories talk about.
    pub findings: Vec<Finding>,
    /// Advisories that were matched against components.
    pub advisories_matched: usize,
    /// Components that were matched against advisories.
    pub components_matched: usize,
    /// Advisories the feed holds in total.
    pub advisories_held: usize,
    /// Components watched in total.
    pub components_watched: usize,
}

impl Reevaluation {
    /// Whether this round did less than a global scan would do.
    ///
    /// The default is the incremental one, and a round that walked the
    /// whole feed for one new advisory would be the global scan wearing a
    /// different name.
    pub fn is_incremental(&self) -> bool {
        self.advisories_matched < self.advisories_held
            || self.components_matched < self.components_watched
    }

    /// The identifiers of the advisories this round actually worked on.
    pub fn touched_advisories(&self) -> Vec<String> {
        self.findings
            .iter()
            .map(|finding| finding.advisory_id.clone())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect()
    }
}

/// Reevaluates the packages the changed advisories talk about.
///
/// Only the advisories that changed are matched, and only the components
/// those advisories name are compared: one new advisory does not open a
/// scan of the whole inventory, and the work done is said out loud so
/// that a reader can check it was not a global scan.
pub fn reassess(
    feed: &RiskFeed,
    changed_advisory_ids: &[String],
    components: &[WatchedComponent],
) -> Reevaluation {
    let mut findings = Vec::new();
    let mut advisories_matched = 0usize;
    let mut components_matched = 0usize;

    for id in changed_advisory_ids {
        let Some(record) = feed.records.get(id) else {
            continue;
        };
        // A withdrawn advisory does not open new findings: the publisher
        // withdrew the statement, and the record stays in the feed for
        // what it is. The advisory that was WITHDRAWN is not a scan
        // target; the ones that arrived or changed are.
        if record.advisory.is_withdrawn() {
            advisories_matched += 1;
            continue;
        }
        advisories_matched += 1;

        // The packages this advisory talks about.
        let packages: Vec<&str> = record
            .advisory
            .affected
            .iter()
            .map(|affected| affected.package.name.as_str())
            .collect();

        for component in components
            .iter()
            .filter(|component| packages.contains(&component.package.as_str()))
        {
            components_matched += 1;
            let state = match &component.version {
                Some(version) => {
                    let matched = record
                        .advisory
                        .affected
                        .iter()
                        .find(|affected| affected.package.name == component.package)
                        .map(|affected| match_version(affected, version))
                        .unwrap_or(Match::Unknown);
                    match matched {
                        Match::Affected => AffectedState::Affected,
                        Match::Unaffected => AffectedState::Unaffected,
                        Match::Unknown => AffectedState::Unknown,
                    }
                }
                None => AffectedState::Unknown,
            };
            findings.push(Finding {
                advisory_id: record.advisory.id.clone(),
                component: component.package.clone(),
                version: component.version.clone(),
                state,
                dependency: component.dependency,
                vex: None,
            });
        }
    }

    Reevaluation {
        findings,
        advisories_matched,
        components_matched,
        advisories_held: feed.len(),
        components_watched: components.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
            .expect("a fixed clock")
            .with_timezone(&Utc)
    }

    fn clock(minutes: i64) -> DateTime<Utc> {
        now() + Duration::minutes(minutes)
    }

    fn parse_clock(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("a fixed clock")
            .with_timezone(&Utc)
    }

    fn advisory_document(id: &str, modified: &str, withdrawn: bool) -> String {
        let withdrawn_line = if withdrawn {
            format!(", \"withdrawn\": \"{modified}\"")
        } else {
            String::new()
        };
        format!(
            r#"{{"id": "{id}", "modified": "{modified}", "affected": [
              {{"package": {{"ecosystem": "Maven", "name": "org.example:lib-b"}},
               "ranges": [{{"type": "ECOSYSTEM", "events": [{{"introduced": "0"}}, {{"fixed": "2.0.1"}}]}}]
              }}]{withdrawn_line} }}"#
        )
    }

    #[test]
    fn the_same_page_twice_leaves_the_same_state() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        let page = FeedPage {
            source: "osv".to_string(),
            fetched_at: now(),
            documents: vec![advisory_document(
                "GHSA-test-0100",
                "2026-10-01T00:00:00Z",
                false,
            )],
        };
        let first = feed.apply_page(Ok(page.clone()), now());
        assert_eq!((first.inserted, first.updated, first.unchanged), (1, 0, 0));

        let second = feed.apply_page(Ok(page), clock(5));
        assert_eq!(
            (second.inserted, second.updated, second.unchanged),
            (0, 0, 1),
            "the same page applied twice is the same state"
        );
        assert_eq!(feed.len(), 1);
    }

    #[test]
    fn an_update_is_applied_and_a_late_page_is_not() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: vec![advisory_document(
                    "GHSA-test-0107",
                    "2026-10-02T00:00:00Z",
                    false,
                )],
            }),
            now(),
        );

        // An out-of-order page with an older document is rejected and
        // does not undo the newer one.
        let late = feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: clock(1),
                documents: vec![advisory_document(
                    "GHSA-test-0107",
                    "2026-10-01T00:00:00Z",
                    false,
                )],
            }),
            clock(1),
        );
        assert_eq!((late.updated, late.rejected), (0, 1));

        // A newer document of the same advisory is applied.
        let newer = feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: clock(2),
                documents: vec![advisory_document(
                    "GHSA-test-0107",
                    "2026-10-03T00:00:00Z",
                    true,
                )],
            }),
            clock(2),
        );
        assert_eq!((newer.updated, newer.withdrawn_kept), (1, 1));
        assert!(
            feed.by_alias("GHSA-test-0107")
                .expect("it is still there")
                .is_withdrawn(),
            "the withdrawal updated the record and kept it"
        );
    }

    #[test]
    fn a_failure_marks_the_feed_and_forgets_nothing() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: vec![advisory_document(
                    "GHSA-test-0101",
                    "2026-10-01T00:00:00Z",
                    false,
                )],
            }),
            now(),
        );

        let report = feed.apply_page(
            Err(FeedFailure::Unavailable("connection refused".to_string())),
            clock(60),
        );
        assert!(report.freshness.is_impaired());
        assert_eq!(
            feed.status(clock(120)),
            FeedStatus::Down {
                lag_minutes: 120,
                failures: 1
            },
            "the feed is down, and that is said out loud"
        );

        // And the CVE the feed already knew is still known: a failed
        // page does not erase a vulnerability.
        assert_eq!(feed.len(), 1);
        assert!(feed.by_alias("GHSA-test-0101").is_some());
        assert_eq!(feed.coverage().advisories, 1);
    }

    #[test]
    fn a_silent_feed_goes_stale_by_the_clock() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: Vec::new(),
            }),
            now(),
        );
        assert_eq!(feed.status(clock(60)), FeedStatus::Current);
        assert_eq!(
            feed.status(clock(420)),
            FeedStatus::Stale { lag_minutes: 420 },
            "behind is behind, and it is marked"
        );
    }

    #[test]
    fn an_import_is_verified_by_digest_and_idempotent() {
        let mut source = RiskFeed::new(Duration::hours(6));
        source.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: vec![advisory_document(
                    "GHSA-test-0102",
                    "2026-10-01T00:00:00Z",
                    false,
                )],
            }),
            now(),
        );
        let bundle = source.export_bundle("osv", now());

        let mut air_gapped = RiskFeed::new(Duration::hours(6));
        let first = air_gapped.import_bundle(&bundle, now());
        assert_eq!(first.inserted, 1);
        assert_eq!(first.rejected, 0);

        let second = air_gapped.import_bundle(&bundle, clock(1));
        assert_eq!(
            (second.inserted, second.unchanged),
            (0, 1),
            "the same bundle twice is the same state"
        );

        // And a tampered document is refused, not applied.
        let mut tampered = bundle.clone();
        tampered.entries[0].document =
            advisory_document("GHSA-test-0102", "2026-10-02T00:00:00Z", false);
        let report = air_gapped.import_bundle(&tampered, clock(2));
        assert_eq!(report.rejected, 1, "the digest does not match");
        assert_eq!(air_gapped.len(), 1, "and the record is the trusted one");
        assert!(
            air_gapped.records().next().is_some_and(
                |record| record.advisory.modified == Some(parse_clock("2026-10-01T00:00:00Z"))
            ),
            "the tampered version never made it in"
        );
    }

    #[test]
    fn withdrawn_advisories_are_kept_and_counted() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: vec![
                    advisory_document("GHSA-test-0103", "2026-10-01T00:00:00Z", false),
                    advisory_document("GHSA-test-0104", "2026-10-01T00:00:00Z", true),
                ],
            }),
            now(),
        );
        let coverage = feed.coverage();
        assert_eq!(coverage.advisories, 2, "a withdrawal keeps the advisory");
        assert_eq!(coverage.withdrawn, 1, "and says it was withdrawn");
    }

    #[test]
    fn aliases_point_at_one_canonical_vulnerability() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        let ghsa = r#"{
          "id": "GHSA-test-0105",
          "modified": "2026-10-01T00:00:00Z",
          "aliases": ["CVE-2026-11111"],
          "affected": [{"package": {"ecosystem": "npm", "name": "left-pad"}}]
        }"#;
        let osv = r#"{
          "id": "OSV-test-0106",
          "modified": "2026-10-01T00:00:00Z",
          "aliases": ["CVE-2026-11111"],
          "affected": [{"package": {"ecosystem": "npm", "name": "left-pad"}}]
        }"#;
        feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: now(),
                documents: vec![ghsa.to_string(), osv.to_string()],
            }),
            now(),
        );

        assert_eq!(feed.len(), 2, "both publishers are kept");
        let coverage = feed.coverage();
        assert_eq!(
            coverage.canonical_vulnerabilities, 1,
            "and they are one vulnerability"
        );
        assert_eq!(
            feed.by_alias("CVE-2026-11111")
                .expect("the alias answers")
                .id,
            "GHSA-test-0105",
            "the canonical one is the first the feed saw"
        );
    }

    #[test]
    fn one_changed_advisory_reassesses_only_its_packages() {
        let mut feed = RiskFeed::new(Duration::hours(6));
        let mut documents = Vec::new();
        for index in 0..20 {
            documents.push(format!(
                r#"{{"id": "GHSA-test-{index:04}", "modified": "2026-10-01T00:00:00Z", "affected": [
                  {{"package": {{"ecosystem": "Maven", "name": "org.example:lib-{index}"}}}}
                ] }}"#
            ));
        }
        let page = FeedPage {
            source: "osv".to_string(),
            fetched_at: now(),
            documents,
        };
        feed.apply_page(Ok(page), now());

        // The new advisory arrives; only it and only its package move.
        let changed = r#"{
          "id": "GHSA-test-new",
          "modified": "2026-11-01T00:00:00Z",
          "affected": [
            {"package": {"ecosystem": "Maven", "name": "org.example:lib-b"},
             "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.1"}]}]}
          ]
        }"#;
        let report = feed.apply_page(
            Ok(FeedPage {
                source: "osv".to_string(),
                fetched_at: clock(1),
                documents: vec![changed.to_string()],
            }),
            clock(1),
        );
        assert_eq!(report.inserted, 1);

        let components: Vec<WatchedComponent> = (0..50)
            .map(|index| WatchedComponent {
                package: if index == 0 {
                    "org.example:lib-b".to_string()
                } else {
                    format!("org.example:lib-{index}")
                },
                version: Some("2.0.0".to_string()),
                dependency: DependencyKind::Direct,
            })
            .collect();

        let round = reassess(&feed, &["GHSA-test-new".to_string()], &components);
        assert_eq!(round.advisories_matched, 1);
        assert_eq!(round.components_matched, 1);
        assert_eq!(round.advisories_held, 21);
        assert_eq!(round.components_watched, 50);
        assert!(
            round.is_incremental(),
            "one new advisory is not forty-four matches of the whole feed"
        );
        assert_eq!(round.findings.len(), 1);
        assert_eq!(round.findings[0].state, AffectedState::Affected);
    }
}
