//! The heuristic signals: what a publication looks like to the rules
//! that know the attacks.
//!
//! Each detector is a pure function over data the repository already
//! holds — the published name, the publisher identity, the declared
//! metadata, the catalog of legitimate names. A detector NEVER opens a
//! package: no archive is unpacked, no script is run, no installer is
//! invoked. When a manifest declares install hooks, the declaration is
//! READ and becomes a signal; reading a declaration is not executing
//! it.
//!
//! Every signal answers two separate questions, and the separation is
//! the point:
//!
//! - **What epistemic status does it have?** `Observation` — a fact
//!   about names and metadata, always; or `Hypothesis` — a
//!   corroborated guess, when independent signals point the same way.
//!   There is no `Fact` of malware in the vocabulary: nothing here can
//!   produce one.
//! - **What weight does it carry for the policy?** A severity that
//!   makes a signal worth reading, and a score that the threshold
//!   compares against. Severity and score say how loud the observation
//!   is; they NEVER say whether a package may be held — that is the
//!   policy's authorized rule and the reservation's job.

use serde::{Deserialize, Serialize};

use super::catalog::{Ecosystem, EcosystemCatalog, normalize_name};

/// What the platform read from the package manifest, as data.
///
/// The fields are the ones the registry metadata carries. Nothing here
/// is the body of a script: `install_hooks` names the hooks the
/// manifest DECLARES — the values are strings read from JSON, never
/// invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRecord {
    /// The ecosystem of the publication.
    pub ecosystem: Ecosystem,
    /// The published name.
    pub name: String,
    /// The version being published.
    pub version: String,
    /// The identity of the publisher account.
    pub publisher: String,
    /// The metadata of this release.
    pub metadata: super::catalog::KnownMetadata,
}

/// The epistemic status of a signal — what kind of statement it is.
///
/// The vocabulary deliberately has no variant that reads "this is
/// malware". A heuristic says what it measured; a malware conclusion
/// requires evidence a name comparison cannot produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicStatus {
    /// A fact about the data: two names are close, this publisher was
    /// never seen, the metadata changed. Always safe to state.
    Observation,
    /// A corroborated guess: independent signals point the same way,
    /// and a human must still confirm it. Not a fact, not a claim.
    Hypothesis,
}

impl EpistemicStatus {
    /// The name of the status.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Hypothesis => "hypothesis",
        }
    }
}

/// The reason codes a signal carries.
///
/// They are typed, finite and searchable — the observation is never a
/// free-text blob that cannot be counted or audited. Each code belongs
/// to one detector family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// The published name is within edit distance 1 of a legitimate
    /// name in the catalog: a transposition, substitution, insertion
    /// or deletion away.
    NameSimilarToKnownPackage,
    /// The publication lands inside a reserved namespace while the
    /// publisher is not the owner of the legitimate packages there.
    ReservedNamespaceFromUnknownPublisher,
    /// A bare name that the ecosystem's coordinates normally carry a
    /// group or scope: the shape of a dependency-confusion upload.
    PackageNameCoordinatesAnomalous,
    /// This publisher identity has never released in the catalog.
    FirstSeenPublisher,
    /// The publisher account is younger than the catalog's maturity
    /// window.
    PublisherAccountTooYoung,
    /// Hooks that run at install time were declared where the package
    /// had none before, or by a publisher with no history of them.
    InstallHooksDeclared,
    /// The declared dependency set changed wildly with respect to the
    /// last known release.
    DependencySetChanged,
    /// The source repository declaration disappeared.
    RepositoryLinkDropped,
    /// The publisher of a version is not the publisher of the versions
    /// the catalog knows.
    PublisherChangedForKnownPackage,
}

impl ReasonCode {
    /// The stable text the store and the API show.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NameSimilarToKnownPackage => "name-similar-to-known-package",
            Self::ReservedNamespaceFromUnknownPublisher => {
                "reserved-namespace-from-unknown-publisher"
            }
            Self::PackageNameCoordinatesAnomalous => "package-name-coordinates-anomalous",
            Self::FirstSeenPublisher => "first-seen-publisher",
            Self::PublisherAccountTooYoung => "publisher-account-too-young",
            Self::InstallHooksDeclared => "install-hooks-declared",
            Self::DependencySetChanged => "dependency-set-changed",
            Self::RepositoryLinkDropped => "repository-link-dropped",
            Self::PublisherChangedForKnownPackage => "publisher-changed-for-known-package",
        }
    }

    /// The detector family the code belongs to, for the metrics split.
    pub fn family(&self) -> &'static str {
        match self {
            Self::NameSimilarToKnownPackage => "name",
            Self::ReservedNamespaceFromUnknownPublisher | Self::PackageNameCoordinatesAnomalous => {
                "confusion"
            }
            Self::FirstSeenPublisher | Self::PublisherAccountTooYoung => "publisher",
            Self::InstallHooksDeclared
            | Self::DependencySetChanged
            | Self::RepositoryLinkDropped
            | Self::PublisherChangedForKnownPackage => "metadata",
        }
    }
}

/// The detector that produced a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// Name similarity against the catalog.
    Typosquat,
    /// Reserved-namespace and coordinate-shape checks.
    Confusion,
    /// Publisher identity and age.
    Publisher,
    /// Metadata anomaly against the known history.
    MetadataAnomaly,
}

/// The method and revision a signal is attributed to.
///
/// The observation is linked to its method so that a reviewer can find
/// the rule that fired, its version, and the threshold it used — an
/// observation without a method reference is an anecdote.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MethodRef {
    /// Levenshtein edit distance against the catalog, with the maximum
    /// distance the method accepted.
    TyposquatLevenshtein { max_distance: usize },
    /// Reserved-namespace judgement ported from the HSP-143 policy.
    NamespaceReservation,
    /// Coordinate-shape rule by ecosystem.
    CoordinateShape,
    /// First-seen publisher lookup in the catalog.
    PublisherHistory,
    /// Publisher account age against a window in days.
    PublisherAgeDays { window_days: i64 },
    /// Diff of the declared metadata against the last known release.
    MetadataDiff,
}

/// One signal a detector raised about one publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeuristicSignal {
    /// The reason code, typed.
    pub reason: ReasonCode,
    /// The detector family, for grouping.
    pub kind: SignalKind,
    /// The epistemic status: what kind of statement this is.
    pub status: EpistemicStatus,
    /// The weight of the signal for the score, 0..=100.
    pub weight: u8,
    /// The method and revision that produced it.
    pub method: MethodRef,
    /// The revision of the rule set that produced the signal — the
    /// weights and windows that were in force when it fired.
    pub rule_revision: String,
    /// The human-readable explanation, naming the data compared.
    pub detail: String,
}

/// The observation the module produces for a publication: every signal
/// raised, the score that aggregates them, and the classification the
/// module — not the policy — gives.
///
/// Classification is fail-closed: reaching the threshold says
/// `Hypothesis` at most, never a malware claim. What happens to the
/// package is the policy's question, and it is answered elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// The publication the signals are about.
    pub record: PackageRecord,
    /// The signals, in the order the detectors produced them.
    pub signals: Vec<HeuristicSignal>,
    /// The aggregate score: the weights summed, capped at 100.
    pub score: u16,
    /// The classification this observation deserves.
    pub status: EpistemicStatus,
    /// The reason codes of every signal raised.
    pub reasons: Vec<ReasonCode>,
}

/// The revision of the detector rule set.
///
/// It travels with every signal: an observation is linked to the
/// method that raised it AND to the revision of the weights and the
/// windows in force, so a reviewer who reads the case months later can
/// tell which rule set judged it — and can tell when the rule set
/// changed under it.
pub const DETECTOR_REVISION: &str = "hodei-risk-advisories-1";

/// The weights each signal carries. They are published in evidence.md —
/// the detector budget is the dataset's, and the numbers are part of
/// the contract.
const W_TYPOSQUAT: u8 = 40;
const W_RESERVED_CONFUSION: u8 = 35;
const W_COORDINATES: u8 = 20;
const W_FIRST_PUBLISHER: u8 = 15;
const W_YOUNG_PUBLISHER: u8 = 20;
const W_HOOKS: u8 = 30;
const W_DEPS_CHANGED: u8 = 25;
const W_REPO_DROPPED: u8 = 20;
const W_PUBLISHER_CHANGED: u8 = 30;

/// The edit distance upper bound that still counts as "close enough to
/// a legitimate name to be worth reading". One substitution, one
/// insertion, one deletion or one transposition.
pub const TYPOSQUAT_MAX_DISTANCE: usize = 1;

/// The window of publisher-account maturity in days.
pub const PUBLISHER_AGE_WINDOW_DAYS: i64 = 30;

/// The minimum number of legitimate versions a publisher must have
/// released for a name swap to fire.
const KNOWN_PACKAGE_HISTORY: usize = 1;

/// Scores at or above this are hypotheses worth a reviewer; below it,
/// the signals stay plain observations. The threshold is published with
/// the dataset metrics.
pub const HYPOTHESIS_THRESHOLD: u16 = 50;

/// The Damerau-Levenshtein distance (restricted, adjacent
/// transpositions count as one edit) between two normalized names,
/// capped so that a comparison of long names does not run away.
/// Returns `None` when the real distance exceeds `max` — which is what
/// the typosquat rule asks: "is it within one?".
///
/// The transposition matters: the classic fat-finger squat swaps two
/// neighbours (`react` to `raect`, `lodash` to `loadsh`), and plain
/// Levenshtein would count that as two edits and let the squat pass.
pub fn edit_distance_within(a: &str, b: &str, max: usize) -> Option<usize> {
    if a == b {
        return Some(0);
    }
    let la = a.chars().count();
    let lb = b.chars().count();
    if la.abs_diff(lb) > max {
        return None;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    // The classic restricted Damerau-Levenshtein dynamic program. The
    // names are short; the O(la*lb) matrix is the honest size.
    let width = lb + 1;
    let mut table = vec![0usize; (la + 1) * (lb + 1)];
    for i in 0..=la {
        table[i * width] = i;
    }
    for (j, cell) in table[..width].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=la {
        // The row minimum carries the path bound: an optimal path of
        // at most `max` edits passes through every row, so if the
        // smallest value in a row is already over `max`, the final
        // distance is over `max` too. The column 0 cell is part of the
        // row, and it is `i`.
        let mut row_min = i;
        for j in 1..=lb {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            let mut best = (table[(i - 1) * width + j] + 1) // deletion
                .min(table[i * width + j - 1] + 1) // insertion
                .min(table[(i - 1) * width + j - 1] + cost); // substitution
            if i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                best = best.min(table[(i - 2) * width + j - 2] + 1); // transposition
            }
            table[i * width + j] = best;
            row_min = row_min.min(best);
        }
        if row_min > max {
            return None;
        }
    }
    let distance = table[la * width + lb];
    (distance <= max).then_some(distance)
}

/// Runs every detector against one publication and returns the
/// observation.
///
/// The function is pure over the record and the catalog: the same
/// inputs give the same signals, in the same order, with the same
/// weights. No file is opened, no command is run, no network is asked.
pub fn evaluate(record: &PackageRecord, catalog: &EcosystemCatalog) -> Observation {
    let mut signals = Vec::new();

    // 1. Typosquat: within edit distance 1 of a DIFFERENT legitimate
    //    name. Identical names are not typosquats, they are the package
    //    itself.
    let normalized = normalize_name(record.ecosystem, &record.name);
    let closest = catalog
        .known_names(record.ecosystem)
        .into_iter()
        .filter(|known| known != &normalized)
        .filter_map(|known| {
            edit_distance_within(&known, &normalized, TYPOSQUAT_MAX_DISTANCE).map(|d| (known, d))
        })
        .min_by_key(|(_, distance)| *distance);
    if let Some((known, distance)) = closest {
        signals.push(HeuristicSignal {
            reason: ReasonCode::NameSimilarToKnownPackage,
            kind: SignalKind::Typosquat,
            status: EpistemicStatus::Observation,
            weight: W_TYPOSQUAT,
            method: MethodRef::TyposquatLevenshtein {
                max_distance: TYPOSQUAT_MAX_DISTANCE,
            },
            rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!(
                "the published name `{}` is {} edit(s) away from the legitimate `{}`",
                record.name, distance, known
            ),
        });
    }

    // 2. Dependency confusion: a publication inside a reserved
    //    namespace from a publisher the catalog does not know, and the
    //    coordinate-shape rule (a bare Maven name).
    if catalog.is_reserved(record.ecosystem, &record.name)
        && !catalog.seen_publisher(&record.publisher)
    {
        signals.push(HeuristicSignal {
            reason: ReasonCode::ReservedNamespaceFromUnknownPublisher,
            kind: SignalKind::Confusion,
            status: EpistemicStatus::Observation,
            weight: W_RESERVED_CONFUSION,
            method: MethodRef::NamespaceReservation,
            rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!(
                "`{}` lands inside a reserved namespace and `{}` has not released here before",
                record.name, record.publisher
            ),
        });
    }
    if record.ecosystem.bare_name_is_anomalous() && !record.name.contains(':') {
        signals.push(HeuristicSignal {
            reason: ReasonCode::PackageNameCoordinatesAnomalous,
            kind: SignalKind::Confusion,
            status: EpistemicStatus::Observation,
            weight: W_COORDINATES,
            method: MethodRef::CoordinateShape,
            rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!("Maven coordinates without a groupId: `{}`", record.name),
        });
    }

    // 3. Publisher identity: first-seen and account age.
    if !catalog.seen_publisher(&record.publisher) {
        signals.push(HeuristicSignal {
            reason: ReasonCode::FirstSeenPublisher,
            kind: SignalKind::Publisher,
            status: EpistemicStatus::Observation,
            weight: W_FIRST_PUBLISHER,
            method: MethodRef::PublisherHistory,
            rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!(
                "`{}` is a publisher the catalog has never seen",
                record.publisher
            ),
        });
    }
    if record.metadata.publisher_age_days < PUBLISHER_AGE_WINDOW_DAYS {
        signals.push(HeuristicSignal {
            reason: ReasonCode::PublisherAccountTooYoung,
            kind: SignalKind::Publisher,
            status: EpistemicStatus::Observation,
            weight: W_YOUNG_PUBLISHER,
            method: MethodRef::PublisherAgeDays {
                window_days: PUBLISHER_AGE_WINDOW_DAYS,
            },
            rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!(
                "the publisher account is {} day(s) old, under the {}-day window",
                record.metadata.publisher_age_days, PUBLISHER_AGE_WINDOW_DAYS
            ),
        });
    }

    // 4. Metadata anomaly against the last known release of the name,
    //    or against the publisher when the name is new.
    if let Some(known) = catalog.known(record.ecosystem, &record.name) {
        if !record.metadata.install_hooks.is_empty() && known.last_metadata.install_hooks.is_empty()
        {
            signals.push(HeuristicSignal {
                reason: ReasonCode::InstallHooksDeclared,
                kind: SignalKind::MetadataAnomaly,
                status: EpistemicStatus::Observation,
                weight: W_HOOKS,
                method: MethodRef::MetadataDiff,
                rule_revision: DETECTOR_REVISION.to_string(),
                detail: format!(
                    "the release declares install hooks ({}) where the known release declared none",
                    record.metadata.install_hooks.join(", ")
                ),
            });
        }
        let changed = known
            .last_metadata
            .dependencies
            .iter()
            .filter(|dependency| !record.metadata.dependencies.contains(dependency))
            .count()
            + record
                .metadata
                .dependencies
                .iter()
                .filter(|dependency| !known.last_metadata.dependencies.contains(dependency))
                .count();
        let baseline = known
            .last_metadata
            .dependencies
            .len()
            .max(record.metadata.dependencies.len());
        if baseline > 0 && changed * 2 > baseline {
            signals.push(HeuristicSignal {
                reason: ReasonCode::DependencySetChanged,
                kind: SignalKind::MetadataAnomaly,
                status: EpistemicStatus::Observation,
                weight: W_DEPS_CHANGED,
                method: MethodRef::MetadataDiff,
                rule_revision: DETECTOR_REVISION.to_string(),
            detail: format!(
                    "the declared dependency set changed {changed} entries over a baseline of {baseline}"
                ),
            });
        }
        if known.last_metadata.has_repository && !record.metadata.has_repository {
            signals.push(HeuristicSignal {
                reason: ReasonCode::RepositoryLinkDropped,
                kind: SignalKind::MetadataAnomaly,
                status: EpistemicStatus::Observation,
                weight: W_REPO_DROPPED,
                method: MethodRef::MetadataDiff,
                rule_revision: DETECTOR_REVISION.to_string(),
                detail: "the source repository declaration disappeared".to_string(),
            });
        }
        if known.versions.len() >= KNOWN_PACKAGE_HISTORY && known.publisher != record.publisher {
            signals.push(HeuristicSignal {
                reason: ReasonCode::PublisherChangedForKnownPackage,
                kind: SignalKind::MetadataAnomaly,
                status: EpistemicStatus::Observation,
                weight: W_PUBLISHER_CHANGED,
                method: MethodRef::PublisherHistory,
                rule_revision: DETECTOR_REVISION.to_string(),
                detail: format!(
                    "the package was released by `{}` before and this release comes from `{}`",
                    known.publisher, record.publisher
                ),
            });
        }
    }

    let score = signals
        .iter()
        .map(|signal| u16::from(signal.weight))
        .sum::<u16>()
        .min(100);

    // The corroboration that turns observations into a hypothesis is
    // two INDEPENDENT families at meaningful weight — one loud detector
    // alone is still a plain observation.
    let families = signals
        .iter()
        .filter(|signal| signal.weight >= W_COORDINATES)
        .map(|signal| signal.reason.family())
        .collect::<std::collections::BTreeSet<_>>();
    let status = if score >= HYPOTHESIS_THRESHOLD && families.len() >= 2 {
        EpistemicStatus::Hypothesis
    } else {
        EpistemicStatus::Observation
    };

    Observation {
        record: record.clone(),
        reasons: signals.iter().map(|signal| signal.reason).collect(),
        signals,
        score,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisories::catalog::{KnownMetadata, KnownPackage};

    fn metadata() -> KnownMetadata {
        KnownMetadata {
            publisher_age_days: 365,
            ..KnownMetadata::default()
        }
    }

    fn record(ecosystem: Ecosystem, name: &str, publisher: &str) -> PackageRecord {
        PackageRecord {
            ecosystem,
            name: name.to_string(),
            version: "1.0.0".to_string(),
            publisher: publisher.to_string(),
            metadata: metadata(),
        }
    }

    #[test]
    fn the_edit_distance_closes_the_typo_and_opens_the_rest() {
        assert_eq!(edit_distance_within("react", "raect", 1), Some(1));
        assert_eq!(edit_distance_within("lodash", "loadsh", 1), Some(1));
        // "loadash" is "lodash" with one insertion (the `a` after the
        // `o`): distance 1. The transposition and the insertion are the
        // two operations the squatter uses most.
        assert_eq!(edit_distance_within("lodash", "loadash", 1), Some(1));
        // A substitution AND a deletion are two edits, and two edits
        // are not one.
        assert_eq!(
            edit_distance_within("lodash", "l0dsh", 1),
            None,
            "two edits is not one"
        );
        assert_eq!(edit_distance_within("lodash", "l0dsh", 2), Some(2));
        assert_eq!(edit_distance_within("axios", "axios", 1), Some(0));
    }

    #[test]
    fn a_typosquat_against_a_known_name_is_an_observation_not_a_claim() {
        let catalog = EcosystemCatalog::new(
            vec![KnownPackage {
                name: "react".to_string(),
                publisher: "facebook".to_string(),
                versions: vec!["18.0.0".to_string()],
                last_metadata: metadata(),
            }],
            Vec::new(),
        );
        let observation = evaluate(&record(Ecosystem::Npm, "raect", "someone-else"), &catalog);
        assert!(
            observation
                .reasons
                .contains(&ReasonCode::NameSimilarToKnownPackage)
        );
        // Single family -> observation; the policy decides what happens.
        assert_eq!(observation.status, EpistemicStatus::Observation);
        assert_eq!(
            observation.signals[0].method,
            MethodRef::TyposquatLevenshtein {
                max_distance: TYPOSQUAT_MAX_DISTANCE
            }
        );
    }

    #[test]
    fn a_reserved_namespace_from_a_new_publisher_is_a_hypothesis() {
        let catalog = EcosystemCatalog::new(
            vec![KnownPackage {
                name: "@acme/utils".to_string(),
                publisher: "acme-ci".to_string(),
                versions: vec!["1.0.0".to_string()],
                last_metadata: metadata(),
            }],
            vec!["@acme".to_string()],
        );
        let observation = evaluate(&record(Ecosystem::Npm, "@acme/utils", "intruder"), &catalog);
        assert_eq!(
            observation.status,
            EpistemicStatus::Hypothesis,
            "confusion + publisher, two families"
        );
        assert!(observation.score >= HYPOTHESIS_THRESHOLD);
        // And even then, the classification stops at Hypothesis.
        assert_ne!(observation.status, EpistemicStatus::Observation);
    }

    #[test]
    fn a_benign_release_of_a_known_package_raises_only_noise() {
        let catalog = EcosystemCatalog::new(
            vec![KnownPackage {
                name: "requests".to_string(),
                publisher: "psf-bot".to_string(),
                versions: vec!["2.31.0".to_string()],
                last_metadata: metadata(),
            }],
            Vec::new(),
        );
        let observation = evaluate(&record(Ecosystem::Pypi, "requests", "psf-bot"), &catalog);
        assert!(observation.reasons.is_empty());
        assert_eq!(observation.score, 0);
        assert_eq!(observation.status, EpistemicStatus::Observation);
    }

    #[test]
    fn install_hooks_are_read_and_never_invoked() {
        let mut meta = metadata();
        meta.install_hooks = vec!["preinstall".to_string()];
        let catalog = EcosystemCatalog::new(
            vec![KnownPackage {
                name: "acme-pkg".to_string(),
                publisher: "acme-dev".to_string(),
                versions: vec!["1.0.0".to_string()],
                last_metadata: metadata(),
            }],
            Vec::new(),
        );
        let mut rec = record(Ecosystem::Npm, "acme-pkg", "acme-dev");
        rec.metadata = meta;
        let observation = evaluate(&rec, &catalog);
        assert!(
            observation
                .reasons
                .contains(&ReasonCode::InstallHooksDeclared)
        );
        // Reading the declaration is the whole detection; there is no
        // hook execution anywhere in the signal path (see the
        // no-execution contract test).
    }

    #[test]
    fn publisher_age_fires_inside_the_window() {
        let catalog = EcosystemCatalog::new(Vec::new(), Vec::new());
        let mut rec = record(Ecosystem::Npm, "brand-new-pkg", "acme-dev");
        rec.metadata.publisher_age_days = 2;
        let observation = evaluate(&rec, &catalog);
        assert!(
            observation
                .reasons
                .contains(&ReasonCode::PublisherAccountTooYoung)
        );
    }
}
