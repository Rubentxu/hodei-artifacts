//! The freshness of a delivered investigation.
//!
//! A report says what it read, and says it with its manifest: the seals of
//! what it read and the version of the world in which it read it. That report is an
//! assertion with a date, and the question is whether it is still true.
//!
//! The answer has THREE values, not two: `Current`, `Stale` and `Unknown`.
//! A report may still be current, may have expired, and there may be
//! no way to know. The third is NOT the first: a store that does not
//! answer does not say that the world did not move, it says that nobody knows. A
//! checker that answered "current" when in doubt would fabricate a
//! guarantee it does not have.
//!
//! # What expires and what does not
//!
//! What the report **supports** expires: the support of the relation it
//! read, and the evidence with the digest it read. What happened in
//! the world and the report did not read does NOT expire: a new relation of another subject does not
//! invalidate a report that did not mention it, and a support that was added to
//! a relation the report read still leaves standing what the
//! report said about it.
//!
//! # Where the decision is
//!
//! There is a PURE function here and nothing else: `judge` compares a manifest
//! with a world and states a verdict. The world (the current versions,
//! the current digests, the current changes) is brought by
//! storage, and storage does not decide: this function decides.
//! That is why the same manifest and the same world give the same verdict,
//! always.

use serde::{Deserialize, Serialize};

use crate::read_set::{ReadKind, ReadSet};

/// The source the graph comes from: the version that the manifest says
/// it read.
pub const GRAPH_SOURCE: &str = "graph";

/// The source the evidence comes from: the digests that the manifest
/// says it read.
pub const EVIDENCE_SOURCE: &str = "evidence";

/// What happened to a reference of the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// The reference was added.
    Added,
    /// The reference was withdrawn: what supported it is no longer there.
    Removed,
    /// The reference is no longer there: there is nothing where what was read was.
    Gone,
    /// The reference changed: what supported it is no longer the same.
    Changed,
}

/// A reference of the world that moved.
///
/// It carries **the support that moved**, because the relation identity is not
/// enough: a relation may have several supports, and what stops being
/// supported is ONE support, not the whole relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedReference {
    /// The source that moved.
    pub source: String,
    /// What kind of reference it is.
    pub kind: ReadKind,
    /// The identity of the reference.
    pub resource: String,
    /// The support that moved: the evidence that supported it.
    pub evidence: String,
    /// What happened to it.
    pub what: ChangeKind,
}

/// The current world, stated as what storage was able to answer.
///
/// What is not here is what could not be queried: a source that
/// does not answer is stated with `unreachable`, and what is stated with
/// `unreachable` is NOT counted as "the same as before".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentWorld {
    /// The generation of each source now.
    pub generations: Vec<(String, u64)>,
    /// The current digest of each piece of evidence.
    pub digests: Vec<(String, String)>,
    /// What moved since the generation that the manifest says it
    /// read.
    pub changes: Vec<ChangedReference>,
    /// The sources that could not be queried.
    pub unreachable: Vec<String>,
}

impl CurrentWorld {
    /// The world of a source that could not be queried.
    pub fn unreachable(source: impl Into<String>) -> Self {
        Self {
            unreachable: vec![source.into()],
            ..Self::default()
        }
    }
}

/// Whether a delivered investigation still rests on the world.
///
/// `Stale` says **what** changed, because a report that expires without saying
/// why is not actionable: whoever asked for it must know what to re-read.
/// a leer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// The world did not move under what the report says.
    Current,
    /// The world moved, and what moved touches what the report
    /// supports.
    Stale {
        /// The references that changed.
        changed: Vec<ChangedReference>,
    },
    /// It could not be known: a source could not be queried.
    Unknown {
        /// Why it could not be known.
        reason: String,
    },
}

impl Freshness {
    /// Whether the report is still current.
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }

    /// Whether the report expired.
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }

    /// Whether it could not be known.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }
}

/// Says whether the investigation that the manifest supports is still current.
///
/// It is a **pure** function: the same manifest and the same world give the
/// same verdict, always. And the verdict has three values: what is known
/// to have expired, what is known to be still current, and what is not known.
///
/// A proven `Stale` beats an `Unknown`: if it is known that the world
/// moved under the report, the report is obsolete, and that the rest
/// cannot additionally be checked does not save it.
pub fn judge(report: &ReadSet, world: &CurrentWorld) -> Freshness {
    let mut changed: Vec<ChangedReference> = Vec::new();
    let mut unknown: Option<String> = None;

    // A manifest that does not say which world it spoke of cannot be checked.
    if report.generations.is_empty() {
        unknown = Some("the report does not say which version of the world it read".to_string());
    }

    // The manifest's sources: each says in which version it was read, and
    // each is contrasted with the current version.
    for (source, expected) in &report.generations {
        if world
            .unreachable
            .iter()
            .any(|unreachable| unreachable == source)
        {
            unknown.get_or_insert_with(|| format!("the source {source} could not be consulted"));
            continue;
        }
        let Some((_, current)) = world.generations.iter().find(|(name, _)| name == source) else {
            unknown.get_or_insert_with(|| format!("the source {source} could not be consulted"));
            continue;
        };
        if current == expected {
            // The source did not move: what was read is still what
            // is there.
            continue;
        }
        // The source moved. What moved may have nothing to do
        // with what the report says: what the report did NOT read cannot
        // expire what it did read.
        for change in world
            .changes
            .iter()
            .filter(|change| &change.source == source)
        {
            let rests_on_it = report.stamps.iter().any(|stamp| {
                stamp.kind == change.kind
                    && stamp.resource == change.resource
                    && stamp.verified_by == change.evidence
            });
            if rests_on_it && !changed.contains(change) {
                changed.push(change.clone());
            }
        }
    }

    // The evidence: the current digest must be the one from back then. What
    // is no longer there can no longer be read, and a report that rests on something
    // that is not there is obsolete.
    let evidence_stamps: Vec<_> = report
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .collect();
    if !evidence_stamps.is_empty() {
        if world
            .unreachable
            .iter()
            .any(|unreachable| unreachable == EVIDENCE_SOURCE)
        {
            unknown.get_or_insert_with(|| {
                format!("the source {EVIDENCE_SOURCE} could not be consulted")
            });
        } else {
            for stamp in evidence_stamps {
                let moved = match world.digests.iter().find(|(id, _)| *id == stamp.resource) {
                    Some((_, digest)) if *digest == stamp.verified_by => None,
                    Some(_) => Some(ChangeKind::Changed),
                    None => Some(ChangeKind::Gone),
                };
                if let Some(what) = moved {
                    let reference = ChangedReference {
                        source: EVIDENCE_SOURCE.to_string(),
                        kind: ReadKind::Evidence,
                        resource: stamp.resource.clone(),
                        evidence: stamp.resource.clone(),
                        what,
                    };
                    if !changed.contains(&reference) {
                        changed.push(reference);
                    }
                }
            }
        }
    }

    if !changed.is_empty() {
        Freshness::Stale { changed }
    } else if let Some(reason) = unknown {
        Freshness::Unknown { reason }
    } else {
        Freshness::Current
    }
}
