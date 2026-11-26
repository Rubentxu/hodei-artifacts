//! The collection plan: the verdict of each blob, without effects.
//!
//! It is a pure function: it receives what is in the store, the catalog
//! roots, the retentions, the manifest edges and the
//! grace period, and returns the verdict of each blob **with its
//! cause**. It neither reads nor deletes anything, and so it is tested without a store and without a
//! database. What can be *seen* in a dry-run is exactly what the
//! sweep can *do*: both come from here.
//!
//! # The verdicts
//!
//! - `Retained`: something keeps it alive. A catalog root, a live
//!   manifest that names it, or a current retention.
//! - `InGrace`: nobody references it, but it is too recent to
//!   touch. It is the barrier against an upload or a promotion that
//!   has not yet reached the catalog.
//! - `Reclaimable`: orphaned and older than the grace period.
//!
//! That is the precedence: a retention beats grace, and grace
//! beats reclamation.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{ContentHold, ContentRoot, HoldScope};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::StoredBlob;
use serde::Serialize;

/// The causes listed per blob. A blob shared by a thousand
/// repositories need not produce a thousand-line report.
pub const MAX_REASONS: usize = 8;

/// Why a blob is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// A catalog row references it.
    Root {
        /// Which row: asset, reference, version or subject.
        root: &'static str,
        /// The account.
        account: String,
        /// The repository.
        repository: String,
        /// The path or the version.
        subject: String,
    },
    /// A live manifest names it: it is its config, a layer or a
    /// child manifest.
    ReferencedBy {
        /// The manifest that names it.
        manifest: String,
    },
    /// A current retention covers it.
    Hold {
        /// The retention.
        id: String,
        /// Why it was placed.
        reason: String,
    },
}

/// What the plan says about a blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// It is kept.
    Retained {
        /// The causes, bounded by [`MAX_REASONS`].
        reasons: Vec<Reason>,
        /// How many more causes there are that are not listed.
        omitted: usize,
    },
    /// Orphaned, but within the grace period.
    InGrace {
        /// How long it has been in the store, in seconds.
        age_secs: u64,
        /// How long until it becomes reclaimable, in seconds.
        remaining_secs: u64,
    },
    /// Orphaned and outside the grace period.
    Reclaimable {
        /// How long it has been in the store, in seconds.
        age_secs: u64,
    },
    /// It could not be determined whether it is alive: the census did not cover its scope, the
    /// read of a root failed, or the graph said "let's paginate and not
    /// finish". **Never reclaimable** (HSP-192, AC3): when in doubt
    /// nothing is deleted, and the verdict says so in the type so that no
    /// pass can interpret it as orphaned.
    Unknown {
        /// Why it is not known, said quietly but said.
        reason: String,
    },
}

/// The verdict of a blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The content, as `sha256:<hex>`.
    pub digest: String,
    /// The bytes it occupies.
    pub length: u64,
    /// What the plan says.
    #[serde(flatten)]
    pub verdict: Verdict,
}

/// The summary of a plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// How many blobs are in the store.
    pub total: u64,
    /// How many are kept.
    pub retained: u64,
    /// How many are in grace.
    pub in_grace: u64,
    /// How many are reclaimable.
    pub reclaimable: u64,
    /// The bytes that would be freed.
    pub reclaimable_bytes: u64,
    /// How many remain in unknown state: they are **never** deleted.
    pub unknown: u64,
}

/// The complete plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The grace period it was computed with, in seconds.
    pub grace_secs: u64,
    /// The totals.
    pub summary: Summary,
    /// The verdict of each blob, sorted by digest.
    pub entries: Vec<Entry>,
}

impl Plan {
    /// The reclaimable blobs, with their length.
    pub fn reclaimable(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.verdict, Verdict::Reclaimable { .. }))
    }
}

/// What the plan needs to know.
pub struct Input<'a> {
    /// The blobs in the store.
    pub blobs: &'a [StoredBlob],
    /// The catalog roots.
    pub roots: &'a [ContentRoot],
    /// The retentions, current or not.
    pub holds: &'a [ContentHold],
    /// The manifest edges: from a manifest to what it names.
    pub edges: &'a HashMap<ContentDigest, Vec<ContentDigest>>,
    /// The grace period.
    pub grace: Duration,
    /// The instant against which retentions expire.
    pub now: DateTime<Utc>,
    /// Whether `roots` is the **complete** census of roots. With a paginated
    /// census (HSP-192, AC3) each page only answers for its batch: what
    /// looks orphaned here may be alive in a page that has not yet
    /// been read. With `false`, nothing is declared reclaimable out of
    /// ignorance: the verdict degrades to `Unknown`, and the coordinated
    /// sweep requires the complete census before deleting.
    pub roots_are_complete: bool,
}

/// Computes the plan.
pub fn plan(input: &Input<'_>) -> Plan {
    // Who owns each live content: the account/repository pairs
    // of the roots that reach it, directly or transitively. A
    // repository or account retention is resolved against this.
    let mut owners: HashMap<&ContentDigest, BTreeSet<(&str, &str)>> = HashMap::new();
    let mut direct: HashMap<&ContentDigest, Vec<&ContentRoot>> = HashMap::new();
    let mut work: Vec<&ContentDigest> = Vec::new();
    for root in input.roots {
        direct.entry(&root.digest).or_default().push(root);
        owners
            .entry(&root.digest)
            .or_default()
            .insert((root.account.as_str(), root.repository.as_str()));
        work.push(&root.digest);
    }

    // Reachability: a live manifest keeps alive what it names,
    // and does so with the same owners. It iterates until nothing changes:
    // a digest is visited again only if it gains an owner.
    let mut parents: HashMap<&ContentDigest, Vec<&ContentDigest>> = HashMap::new();
    while let Some(parent) = work.pop() {
        let Some(children) = input.edges.get(parent) else {
            continue;
        };
        let parent_owners = owners.get(parent).cloned().unwrap_or_default();
        for child in children {
            let named_by = parents.entry(child).or_default();
            if !named_by.contains(&parent) {
                named_by.push(parent);
            }
            let slot = owners.entry(child).or_default();
            let before = slot.len();
            slot.extend(parent_owners.iter().copied());
            if slot.len() != before || before == 0 {
                work.push(child);
            }
        }
    }

    let active: Vec<&ContentHold> = input
        .holds
        .iter()
        .filter(|hold| hold.is_active(input.now))
        .collect();

    let mut entries: Vec<Entry> = input
        .blobs
        .iter()
        .map(|blob| {
            let mut reasons: Vec<Reason> = Vec::new();

            if let Some(roots) = direct.get(&blob.digest) {
                reasons.extend(roots.iter().map(|root| Reason::Root {
                    root: root.kind.as_str(),
                    account: root.account.clone(),
                    repository: root.repository.clone(),
                    subject: root.subject.clone(),
                }));
            }
            if let Some(manifests) = parents.get(&blob.digest) {
                let mut named: Vec<String> = manifests.iter().map(|m| m.to_string()).collect();
                named.sort();
                reasons.extend(
                    named
                        .into_iter()
                        .map(|manifest| Reason::ReferencedBy { manifest }),
                );
            }
            let blob_owners = owners.get(&blob.digest);
            for hold in &active {
                let reaches = match &hold.scope {
                    HoldScope::Digest { digest } => digest == &blob.digest,
                    HoldScope::Repository {
                        account,
                        repository,
                    } => blob_owners.is_some_and(|set| {
                        set.iter().any(|(a, r)| a == account && r == repository)
                    }),
                    HoldScope::Account { account } => {
                        blob_owners.is_some_and(|set| set.iter().any(|(a, _)| a == account))
                    }
                };
                if reaches {
                    reasons.push(Reason::Hold {
                        id: hold.id.clone(),
                        reason: hold.reason.clone(),
                    });
                }
            }

            let verdict = if !reasons.is_empty() {
                let omitted = reasons.len().saturating_sub(MAX_REASONS);
                reasons.truncate(MAX_REASONS);
                Verdict::Retained { reasons, omitted }
            } else if !input.roots_are_complete {
                Verdict::Unknown {
                    reason: "the root census is partial: nothing unseen can be called orphan"
                        .to_string(),
                }
            } else if blob.age < input.grace {
                Verdict::InGrace {
                    age_secs: blob.age.as_secs(),
                    remaining_secs: (input.grace - blob.age).as_secs().max(1),
                }
            } else {
                Verdict::Reclaimable {
                    age_secs: blob.age.as_secs(),
                }
            };
            Entry {
                digest: blob.digest.to_string(),
                length: blob.length,
                verdict,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.digest.cmp(&b.digest));

    let mut summary = Summary {
        total: entries.len() as u64,
        ..Summary::default()
    };
    for entry in &entries {
        match &entry.verdict {
            Verdict::Retained { .. } => summary.retained += 1,
            Verdict::InGrace { .. } => summary.in_grace += 1,
            Verdict::Reclaimable { .. } => {
                summary.reclaimable += 1;
                summary.reclaimable_bytes += entry.length;
            }
            Verdict::Unknown { .. } => summary.unknown += 1,
        }
    }

    Plan {
        grace_secs: input.grace.as_secs(),
        summary,
        entries,
    }
}

/// Why a manifest could not be read as a graph.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeError {
    /// It is not JSON.
    #[error("the manifest is not valid JSON")]
    NotJson,
    /// It is neither an image nor an index.
    #[error("the manifest is neither an image nor an index")]
    Unrecognized,
    /// A descriptor does not name a well-formed `sha256:<hex>` digest.
    #[error("a descriptor does not name a well formed digest")]
    BadDigest,
}

/// What a manifest names.
///
/// For an image or an artifact: the config and the layers. For an
/// index: the child manifests. The `subject` is **not** an edge: a
/// referrer does not keep its subject alive, it is the subject that makes it
/// reachable. It is a **strict** read: a manifest that is not
/// understood is an error, because a half-built graph would leave unprotected
/// what that manifest names.
pub fn manifest_edges(bytes: &[u8]) -> Result<Vec<ContentDigest>, EdgeError> {
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| EdgeError::NotJson)?;
    let object = manifest.as_object().ok_or(EdgeError::Unrecognized)?;

    let mut edges = Vec::new();
    let mut recognized = false;

    if let Some(children) = object.get("manifests") {
        recognized = true;
        for child in children.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(child)?);
        }
    }
    if let Some(config) = object.get("config") {
        recognized = true;
        edges.push(descriptor_digest(config)?);
    }
    if let Some(layers) = object.get("layers") {
        recognized = true;
        for layer in layers.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(layer)?);
        }
    }
    if let Some(blobs) = object.get("blobs") {
        // The artifact manifest of the previous protocol
        // version names its content here.
        recognized = true;
        for blob in blobs.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(blob)?);
        }
    }

    if !recognized {
        return Err(EdgeError::Unrecognized);
    }
    let unique: HashSet<ContentDigest> = edges.into_iter().collect();
    let mut edges: Vec<ContentDigest> = unique.into_iter().collect();
    edges.sort_by_key(ToString::to_string);
    Ok(edges)
}

fn descriptor_digest(descriptor: &serde_json::Value) -> Result<ContentDigest, EdgeError> {
    let text = descriptor
        .get("digest")
        .and_then(|digest| digest.as_str())
        .ok_or(EdgeError::BadDigest)?;
    let (algorithm, value) = text.split_once(':').ok_or(EdgeError::BadDigest)?;
    if algorithm != "sha256" {
        return Err(EdgeError::BadDigest);
    }
    let digest = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        value: value.to_ascii_lowercase(),
    };
    if digest.is_well_formed() {
        Ok(digest)
    } else {
        Err(EdgeError::BadDigest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_catalog::RootKind;

    const GRACE: Duration = Duration::from_secs(3600);

    fn digest(n: u8) -> ContentDigest {
        ContentDigest::sha256(format!("{n:064x}"))
    }

    fn blob(n: u8, age_secs: u64) -> StoredBlob {
        StoredBlob {
            digest: digest(n),
            length: 10,
            age: Duration::from_secs(age_secs),
        }
    }

    fn root(kind: RootKind, repository: &str, n: u8) -> ContentRoot {
        ContentRoot {
            kind,
            account: "acme".to_string(),
            repository: repository.to_string(),
            subject: "blobs/x".to_string(),
            digest: digest(n),
        }
    }

    fn hold(id: &str, scope: HoldScope, expires: Option<DateTime<Utc>>) -> ContentHold {
        ContentHold {
            id: id.to_string(),
            scope,
            reason: "legal".to_string(),
            actor: "ops".to_string(),
            created_at: Utc::now(),
            expires_at: expires,
        }
    }

    fn run(
        blobs: &[StoredBlob],
        roots: &[ContentRoot],
        holds: &[ContentHold],
        edges: &HashMap<ContentDigest, Vec<ContentDigest>>,
    ) -> Plan {
        run_with(blobs, roots, holds, edges, true)
    }

    /// The same plan, declaring whether the root census is complete or
    /// only a partial paginated one (HSP-192, AC3).
    fn run_with(
        blobs: &[StoredBlob],
        roots: &[ContentRoot],
        holds: &[ContentHold],
        edges: &HashMap<ContentDigest, Vec<ContentDigest>>,
        roots_are_complete: bool,
    ) -> Plan {
        plan(&Input {
            blobs,
            roots,
            holds,
            edges,
            grace: GRACE,
            now: Utc::now(),
            roots_are_complete,
        })
    }

    fn verdict_of(plan: &Plan, n: u8) -> &Verdict {
        let wanted = digest(n).to_string();
        &plan
            .entries
            .iter()
            .find(|entry| entry.digest == wanted)
            .expect("the entry")
            .verdict
    }

    #[test]
    fn an_old_orphan_is_reclaimable_and_a_young_one_is_in_grace() {
        let plan = run(&[blob(1, 7200), blob(2, 60)], &[], &[], &HashMap::new());
        assert!(matches!(verdict_of(&plan, 1), Verdict::Reclaimable { .. }));
        assert!(matches!(
            verdict_of(&plan, 2),
            Verdict::InGrace {
                remaining_secs: 3540,
                ..
            }
        ));
        assert_eq!(plan.summary.reclaimable, 1);
        assert_eq!(plan.summary.reclaimable_bytes, 10);
        assert_eq!(plan.summary.in_grace, 1);
    }

    #[test]
    fn a_partial_root_census_never_calls_anything_reclaimable() {
        // HSP-192, AC3: a paginated census that has not finished cannot
        // declare orphans. An old blob with no root on this page remains
        // Unknown until the pagination completes.
        let plan = run_with(&[blob(1, 99_999)], &[], &[], &HashMap::new(), false);
        assert!(matches!(verdict_of(&plan, 1), Verdict::Unknown { .. }));
        assert_eq!(plan.summary.reclaimable, 0);
        assert_eq!(plan.summary.unknown, 1);
    }

    #[test]
    fn a_partial_census_still_honors_roots_and_holds() {
        // What WAS seen alive, lives; what was not seen is unknown.
        // Degrading to Unknown does not erase a real retention.
        let roots = [root(RootKind::Asset, "app", 1)];
        let holds = [hold("h", HoldScope::Digest { digest: digest(2) }, None)];
        let plan = run_with(
            &[blob(1, 99_999), blob(2, 99_999)],
            &roots,
            &holds,
            &HashMap::new(),
            false,
        );
        assert!(matches!(verdict_of(&plan, 1), Verdict::Retained { .. }));
        assert!(matches!(verdict_of(&plan, 2), Verdict::Retained { .. }));
        let plan = run_with(&[blob(3, 99_999)], &roots, &[], &HashMap::new(), false);
        assert!(matches!(verdict_of(&plan, 3), Verdict::Unknown { .. }));
    }

    #[test]
    fn a_root_retains_even_an_old_blob() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let plan = run(&[blob(1, 99_999)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => {
                assert!(matches!(reasons[0], Reason::Root { root: "asset", .. }));
            }
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn a_blob_shared_by_two_repositories_names_both() {
        let roots = [
            root(RootKind::Asset, "app", 1),
            root(RootKind::Asset, "lib", 1),
        ];
        let plan = run(&[blob(1, 99_999)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => assert_eq!(reasons.len(), 2),
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn a_live_manifest_retains_its_config_and_layers() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2), digest(3)]);
        let plan = run(
            &[
                blob(1, 99_999),
                blob(2, 99_999),
                blob(3, 99_999),
                blob(4, 99_999),
            ],
            &roots,
            &[],
            &edges,
        );
        for layer in [2, 3] {
            match verdict_of(&plan, layer) {
                Verdict::Retained { reasons, .. } => {
                    assert!(matches!(reasons[0], Reason::ReferencedBy { .. }));
                }
                other => panic!("layer {layer}: expected retained, got {other:?}"),
            }
        }
        assert!(matches!(verdict_of(&plan, 4), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn retention_is_transitive_through_an_index() {
        // index (1) -> manifest (2) -> layer (3)
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        edges.insert(digest(2), vec![digest(3)]);
        let plan = run(&[blob(3, 99_999)], &roots, &[], &edges);
        assert!(matches!(verdict_of(&plan, 3), Verdict::Retained { .. }));
    }

    #[test]
    fn a_manifest_that_is_not_alive_does_not_retain_what_it_names() {
        // Manifest 1 is not a root: its layers are not kept because of it.
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        let plan = run(&[blob(1, 99_999), blob(2, 99_999)], &[], &[], &edges);
        assert!(matches!(verdict_of(&plan, 2), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn a_cycle_in_the_edges_terminates() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        edges.insert(digest(2), vec![digest(1)]);
        let plan = run(&[blob(2, 99_999)], &roots, &[], &edges);
        assert!(matches!(verdict_of(&plan, 2), Verdict::Retained { .. }));
    }

    #[test]
    fn a_hold_beats_the_grace_and_the_age() {
        let holds = [hold("h1", HoldScope::Digest { digest: digest(1) }, None)];
        let plan = run(
            &[blob(1, 99_999), blob(2, 60)],
            &[],
            &holds,
            &HashMap::new(),
        );
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => {
                assert!(matches!(&reasons[0], Reason::Hold { id, .. } if id == "h1"));
            }
            other => panic!("expected retained, got {other:?}"),
        }
        assert!(matches!(verdict_of(&plan, 2), Verdict::InGrace { .. }));
    }

    #[test]
    fn an_expired_hold_retains_nothing() {
        let expired = Utc::now() - chrono::Duration::seconds(5);
        let holds = [hold(
            "h1",
            HoldScope::Digest { digest: digest(1) },
            Some(expired),
        )];
        let plan = run(&[blob(1, 99_999)], &[], &holds, &HashMap::new());
        assert!(matches!(verdict_of(&plan, 1), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn a_repository_hold_reaches_what_the_repository_keeps_alive() {
        // Manifest 1 is in `app`; its layer 2 is from the same repo.
        // Layer 3 is from `lib` and does not reach it.
        let roots = [
            root(RootKind::Asset, "app", 1),
            root(RootKind::Asset, "lib", 3),
        ];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        let holds = [hold(
            "h1",
            HoldScope::Repository {
                account: "acme".to_string(),
                repository: "app".to_string(),
            },
            None,
        )];
        let plan = run(
            &[blob(1, 5), blob(2, 5), blob(3, 5)],
            &roots,
            &holds,
            &edges,
        );
        let held = |n: u8| match verdict_of(&plan, n) {
            Verdict::Retained { reasons, .. } => {
                reasons.iter().any(|r| matches!(r, Reason::Hold { .. }))
            }
            _ => false,
        };
        assert!(held(1));
        assert!(held(2), "the layer is reached through the manifest");
        assert!(!held(3), "another repository is not reached");
    }

    #[test]
    fn an_account_hold_reaches_every_repository_of_the_account() {
        let mut other = root(RootKind::Asset, "app", 2);
        other.account = "globex".to_string();
        let roots = [root(RootKind::Asset, "app", 1), other];
        let holds = [hold(
            "h1",
            HoldScope::Account {
                account: "acme".to_string(),
            },
            None,
        )];
        let plan = run(&[blob(1, 5), blob(2, 5)], &roots, &holds, &HashMap::new());
        let held = |n: u8| match verdict_of(&plan, n) {
            Verdict::Retained { reasons, .. } => {
                reasons.iter().any(|r| matches!(r, Reason::Hold { .. }))
            }
            _ => false,
        };
        assert!(held(1));
        assert!(!held(2));
    }

    #[test]
    fn the_reasons_are_bounded_and_the_rest_is_counted() {
        let roots: Vec<ContentRoot> = (0..20)
            .map(|i| root(RootKind::Asset, &format!("repo-{i}"), 1))
            .collect();
        let plan = run(&[blob(1, 5)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, omitted } => {
                assert_eq!(reasons.len(), MAX_REASONS);
                assert_eq!(*omitted, 12);
            }
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn the_plan_is_deterministic_and_sorted() {
        let blobs = [blob(3, 99_999), blob(1, 99_999), blob(2, 99_999)];
        let first = run(&blobs, &[], &[], &HashMap::new());
        let second = run(&blobs, &[], &[], &HashMap::new());
        assert_eq!(first, second);
        let digests: Vec<&String> = first.entries.iter().map(|e| &e.digest).collect();
        let mut sorted = digests.clone();
        sorted.sort();
        assert_eq!(digests, sorted);
    }

    #[test]
    fn the_edges_of_an_image_are_its_config_and_layers() {
        let manifest = format!(
            r#"{{"config":{{"digest":"sha256:{:064x}"}},"layers":[{{"digest":"sha256:{:064x}"}},{{"digest":"sha256:{:064x}"}}]}}"#,
            1, 2, 2,
        );
        let edges = manifest_edges(manifest.as_bytes()).expect("edges");
        assert_eq!(edges, vec![digest(1), digest(2)]);
    }

    #[test]
    fn the_edges_of_an_index_are_its_manifests() {
        let manifest = format!(r#"{{"manifests":[{{"digest":"sha256:{:064x}"}}]}}"#, 7,);
        assert_eq!(
            manifest_edges(manifest.as_bytes()).expect("edges"),
            vec![digest(7)]
        );
    }

    #[test]
    fn the_subject_is_not_an_edge() {
        let manifest = format!(
            r#"{{"config":{{"digest":"sha256:{:064x}"}},"layers":[],"subject":{{"digest":"sha256:{:064x}"}}}}"#,
            1, 9,
        );
        assert_eq!(
            manifest_edges(manifest.as_bytes()).expect("edges"),
            vec![digest(1)]
        );
    }

    #[test]
    fn a_manifest_that_is_not_understood_is_an_error_and_never_an_empty_graph() {
        assert_eq!(manifest_edges(b"not json"), Err(EdgeError::NotJson));
        assert_eq!(manifest_edges(b"{}"), Err(EdgeError::Unrecognized));
        assert_eq!(manifest_edges(b"[]"), Err(EdgeError::Unrecognized));
        assert_eq!(
            manifest_edges(br#"{"layers":[{"digest":"md5:abc"}]}"#),
            Err(EdgeError::BadDigest)
        );
        assert_eq!(
            manifest_edges(br#"{"layers":[{}]}"#),
            Err(EdgeError::BadDigest)
        );
    }
}
