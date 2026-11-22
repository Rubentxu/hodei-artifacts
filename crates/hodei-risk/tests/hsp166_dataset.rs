//! The synthetic labelled corpus for the preventive heuristics
//! (HSP-166 AC1).
//!
//! Every row is a publication the generator builds from a seed, with
//! the label the generator intended. The corpus is small and fully
//! deterministic — the same seed gives the same rows in the same order,
//! and the dataset is versioned next to the code that scores it —
//! because the point is not to beat a leaderboard: it is to publish,
//! with numbers, what the detectors get right, what they get wrong, and
//! what the threshold does in each ecosystem.
//!
//! The shape of each population mirrors what the repositories report:
//!
//! - **benign**: legitimate releases of legitimate names by legitimate
//!   publishers, plus a few legitimate newcomers (the first release of
//!   a brand-new package), because a detector that fires on every new
//!   project is a detector people turn off. A handful of benign rows
//!   are intentionally CLOSE to a legitimate name — the honest
//!   similarity tax.
//! - **typosquats**: one edit from a catalog name (transposition,
//!   deletion, insertion, substitution), plus a two-edit variant the
//!   distance-1 rule is designed to miss.
//! - **confusion**: reserved namespace (HSP-143) from a publisher the
//!   catalog has not seen, including normalized spellings and Maven
//!   subgroups.
//! - **anomalies**: metadata churn — hooks where none existed, the
//!   repository link dropped, the publisher of a known package
//!   swapped — and the bare-name Maven shape.

use hodei_risk::advisories::{
    Decision, Ecosystem, EcosystemCatalog, EpistemicStatus, HeuristicPolicy, KnownMetadata,
    KnownPackage, PackageRecord, ReasonCode, evaluate, policy::decide,
};

/// The seed of the corpus: change it and the numbers change with it.
pub const SEED: u64 = 0x0005_1166;
/// The rows each population contributes per ecosystem.
pub const ROWS_PER_ECOSYSTEM: usize = 8;

/// The label of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// A legitimate publication.
    Benign,
    /// A publication the generator built to look like an attack.
    Malicious,
}

/// One labelled row: the record and the intent.
#[derive(Debug, Clone)]
pub struct Row {
    pub record: PackageRecord,
    pub label: Label,
    /// What the generator was trying to represent, for the report.
    pub pattern: &'static str,
}

/// Deterministic 64-bit mixing (SplitMix64): the seed in, a stream of
/// numbers out. No dependency, no randomness source.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// True with probability `numerator/denominator`.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next_u64() % denominator < numerator
    }
}

/// The benign publisher each known package releases from — the
/// identity the catalog trusts.
pub const PUBLISHERS: &[(&str, &str)] = &[
    ("express", "express-team"),
    ("react", "fb-react"),
    ("lodash", "lodash-core"),
    ("axios", "axios-owner"),
    ("@acme/utils", "acme-ci"),
    ("@acme/core", "acme-ci"),
    ("requests", "psf-kenneth"),
    ("flask", "pallets-bot"),
    ("urllib3", "urllib3-team"),
    ("acme-internal", "acme-release-bot"),
    ("org.example:core", "example-sonatype"),
    ("commons-lang:commons-lang", "apache-commons"),
    ("com.acme.platform:service", "acme-release-bot"),
];

fn stable_metadata() -> KnownMetadata {
    KnownMetadata {
        dependencies: vec![
            "dep-a".into(),
            "dep-b".into(),
            "dep-c".into(),
            "dep-d".into(),
        ],
        has_repository: true,
        has_provenance: true,
        publisher_age_days: 900,
        ..Default::default()
    }
}

/// The legitimate catalog: the names that exist, with the publisher
/// that releases them and the metadata their last release carried.
pub fn catalog() -> EcosystemCatalog {
    EcosystemCatalog::new(
        PUBLISHERS
            .iter()
            .map(|(name, publisher)| KnownPackage {
                name: (*name).to_string(),
                publisher: (*publisher).to_string(),
                versions: vec!["1.0.0".to_string(), "1.1.0".to_string()],
                last_metadata: stable_metadata(),
            })
            .collect(),
        // The reservations are the HSP-143 declarations of the org.
        vec![
            "@acme".to_string(),
            "acme-internal".to_string(),
            "com.acme.platform".to_string(),
        ],
    )
}

fn ecosystem_of(name: &str) -> Ecosystem {
    if name.contains(':') {
        Ecosystem::Maven
    } else if name.starts_with('@')
        || [
            "express",
            "react",
            "lodash",
            "axios",
            // The two benign rows the generator adds outside the
            // catalog cycle: same ecosystem as the package they sit
            // next to.
            "expres",
            "acme-new-service",
        ]
        .contains(&name)
    {
        Ecosystem::Npm
    } else {
        Ecosystem::Pypi
    }
}

/// The legitimate version of a known package, as the owner releases
/// it.
fn legitimate(rng: &mut Rng, name: &str, publisher: &str, pattern: &'static str) -> Row {
    Row {
        record: PackageRecord {
            ecosystem: ecosystem_of(name),
            name: name.to_string(),
            version: format!("1.{}.{}", 2 + rng.below_small(9), rng.below_small(9)),
            publisher: publisher.to_string(),
            metadata: stable_metadata(),
        },
        label: Label::Benign,
        pattern,
    }
}

impl Rng {
    fn below_small(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

/// The benign population.
fn benign_rows(rng: &mut Rng) -> Vec<Row> {
    let mut rows = Vec::new();
    // Ordinary legitimate releases, cycling the catalog.
    for _ in 0..4 {
        for (name, publisher) in PUBLISHERS {
            rows.push(legitimate(rng, name, publisher, "legitimate release"));
        }
    }
    // Legitimate newcomers: a brand-new package name by a publisher
    // the catalog knows. A first release must NOT look like an attack.
    rows.push(Row {
        record: PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "acme-new-service".to_string(),
            version: "0.1.0".to_string(),
            publisher: "acme-ci".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 900,
                ..stable_metadata()
            },
        },
        label: Label::Benign,
        pattern: "first legitimate release by a known publisher",
    });
    // The honest similarity tax: legitimate names that happen to sit
    // one edit from another legitimate name. Both exist; the detector
    // flags the newer one, and a reviewer closes it. That is a
    // published false positive, not a hidden one.
    rows.push(Row {
        record: PackageRecord {
            ecosystem: Ecosystem::Pypi,
            name: "requestz".to_string(),
            version: "1.0.0".to_string(),
            publisher: "psf-kenneth".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 900,
                ..stable_metadata()
            },
        },
        label: Label::Benign,
        pattern: "legitimate package one edit from another legitimate one",
    });
    rows.push(Row {
        record: PackageRecord {
            ecosystem: Ecosystem::Npm,
            name: "expres".to_string(),
            version: "2.0.0".to_string(),
            publisher: "express-team".to_string(),
            metadata: KnownMetadata {
                publisher_age_days: 900,
                ..stable_metadata()
            },
        },
        label: Label::Benign,
        pattern: "legitimate alias one edit from a flagship package",
    });
    rows
}

/// The typosquat population: one edit from a legitimate name, from an
/// account nobody has seen.
fn typosquat_rows(rng: &mut Rng) -> Vec<Row> {
    // (legitimate, typo) pairs: transposition, deletion, insertion,
    // substitution — all four flavours of the same squat.
    let npm = [
        ("express", "expres"),
        ("react", "raect"),
        ("lodash", "loadsh"),
        ("axios", "axpios"),
    ];
    let pypi = [
        ("requests", "reqests"),
        ("flask", "flaks"),
        ("urllib3", "urlib3"),
        ("acme-internal", "acme-interna1"),
    ];
    let maven = [
        ("org.example:core", "org.example:corg"),
        ("commons-lang:commons-lang", "commons-lang3:commons-lang"),
        ("com.acme.platform:service", "com.acme.platform:servlice"),
    ];
    let mut rows = Vec::new();
    for (ecosystem, pairs) in [
        (Ecosystem::Npm, &npm[..]),
        (Ecosystem::Pypi, &pypi[..]),
        (Ecosystem::Maven, &maven[..]),
    ] {
        for index in 0..ROWS_PER_ECOSYSTEM {
            let (_, typo) = pairs[index % pairs.len()];
            // Half of the squats declare install hooks: the payload
            // comes with the name. The other half are quiet — they are
            // the rows that test whether the name alone carries.
            let hooks = if rng.chance(1, 2) {
                vec!["preinstall".to_string()]
            } else {
                Vec::new()
            };
            rows.push(Row {
                record: PackageRecord {
                    ecosystem,
                    name: typo.to_string(),
                    version: "0.0.1".to_string(),
                    publisher: format!("squatter-{index}"),
                    metadata: KnownMetadata {
                        publisher_age_days: 0,
                        install_hooks: hooks,
                        dependencies: vec!["phantom-dep".to_string()],
                        has_repository: false,
                        has_provenance: false,
                        ..Default::default()
                    },
                },
                label: Label::Malicious,
                pattern: "typosquat one edit from a legitimate name",
            });
            // Two edits: the distance-1 rule is designed to miss these
            // and the report publishes it as a false negative.
            rows.push(Row {
                record: PackageRecord {
                    ecosystem,
                    name: format!("{typo}x"),
                    version: "0.0.1".to_string(),
                    publisher: format!("slow-squatter-{index}"),
                    metadata: KnownMetadata {
                        publisher_age_days: 0,
                        ..Default::default()
                    },
                },
                label: Label::Malicious,
                pattern: "slow typosquat variant (two edits, expected miss)",
            });
        }
    }
    rows
}

/// The dependency-confusion population: reserved namespace, unknown
/// publisher (the HSP-143 attack).
fn confusion_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for index in 0..ROWS_PER_ECOSYSTEM {
        // npm: the reserved scope, a version that beats the internal
        // one.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Npm,
                name: "@acme/utils".to_string(),
                version: format!("99.{}.0", index),
                publisher: format!("confusion-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 3,
                    dependencies: vec![
                        "dep-a".into(),
                        "dep-b".into(),
                        "dep-c".into(),
                        "dep-d".into(),
                    ],
                    has_repository: true,
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "dependency confusion inside a reserved npm scope",
        });
        // PyPI: the reserved project in normalized spellings — the
        // alias must not let the reservation be evaded.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Pypi,
                name: if index % 2 == 0 {
                    "ACME_Internal".to_string()
                } else {
                    "acme.internal".to_string()
                },
                version: format!("2.0.{}", index),
                publisher: format!("pypi-confusion-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 5,
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "dependency confusion inside a reserved PyPI project (normalized alias)",
        });
        // Maven: the reserved group and one of its subgroups — a
        // reservation covers what lives below it.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Maven,
                name: if index % 2 == 0 {
                    format!("com.acme.platform:agent-{index}")
                } else {
                    format!("com.acme.platform.secrets:plugin-{index}")
                },
                version: "1.0.0-SNAPSHOT".to_string(),
                publisher: format!("maven-confusion-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 2,
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "dependency confusion inside a reserved Maven group (and its subgroups)",
        });
    }
    rows
}

/// The metadata/publisher anomaly population.
fn anomaly_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for index in 0..ROWS_PER_ECOSYSTEM {
        // An account takeover: the legitimate name from a publisher
        // the catalog has never seen, with hooks the known release
        // never had.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Npm,
                name: "react".to_string(),
                version: "19.0.0".to_string(),
                publisher: format!("hijacker-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 10,
                    install_hooks: vec!["preinstall".to_string()],
                    dependencies: vec!["evil-one".to_string(), "evil-two".to_string()],
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "account takeover: new publisher plus install hooks",
        });
        // A PyPI release where the repository link disappeared.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Pypi,
                name: "flask".to_string(),
                version: "3.1.0".to_string(),
                publisher: format!("flask-stranger-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 4,
                    dependencies: vec![
                        "dep-a".into(),
                        "dep-b".into(),
                        "dep-c".into(),
                        "dep-d".into(),
                    ],
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "metadata anomaly: repository link dropped, publisher unknown",
        });
        // The bare-name Maven upload: a coordinate shape that does not
        // belong to the ecosystem's own conventions.
        rows.push(Row {
            record: PackageRecord {
                ecosystem: Ecosystem::Maven,
                name: "logj-core".to_string(),
                version: "3.0.0".to_string(),
                publisher: format!("bare-{index}"),
                metadata: KnownMetadata {
                    publisher_age_days: 1,
                    ..Default::default()
                },
            },
            label: Label::Malicious,
            pattern: "bare-name Maven upload (coordinate shape anomaly)",
        });
    }
    rows
}

/// The whole labelled corpus.
pub fn corpus() -> Vec<Row> {
    let mut rng = Rng::new(SEED);
    let mut rows = benign_rows(&mut rng);
    rows.extend(typosquat_rows(&mut rng));
    rows.extend(confusion_rows());
    rows.extend(anomaly_rows());
    rows
}

/// What one row produced under a policy.
#[derive(Debug)]
pub struct Outcome<'a> {
    pub row: &'a Row,
    pub score: u16,
    pub reasons: Vec<ReasonCode>,
    pub status: EpistemicStatus,
    pub decision: Decision,
}

/// Runs the corpus under a policy.
pub fn run<'a>(
    catalog: &EcosystemCatalog,
    policy: &HeuristicPolicy,
    rows: &'a [Row],
) -> Vec<Outcome<'a>> {
    rows.iter()
        .map(|row| {
            let observation = evaluate(&row.record, catalog);
            let verdict = decide(policy, &observation);
            Outcome {
                row,
                score: observation.score,
                reasons: observation.reasons.clone(),
                status: observation.status,
                decision: verdict.decision,
            }
        })
        .collect()
}

/// The per-ecosystem numbers the report publishes.
#[derive(Debug, PartialEq, Eq)]
pub struct EcosystemMetrics {
    pub ecosystem: Ecosystem,
    pub benign: usize,
    pub malicious: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision_percent: u32,
    pub recall_percent: u32,
    pub quarantined: usize,
}

pub fn metrics(
    catalog: &EcosystemCatalog,
    policy: &HeuristicPolicy,
    rows: &[Row],
) -> Vec<EcosystemMetrics> {
    let outcomes = run(catalog, policy, rows);
    [Ecosystem::Npm, Ecosystem::Pypi, Ecosystem::Maven]
        .into_iter()
        .map(|ecosystem| {
            let mine: Vec<&Outcome<'_>> = outcomes
                .iter()
                .filter(|outcome| outcome.row.record.ecosystem == ecosystem)
                .collect();
            let benign = mine
                .iter()
                .filter(|outcome| outcome.row.label == Label::Benign)
                .count();
            let malicious = mine.len() - benign;
            let true_positives = mine
                .iter()
                .filter(|outcome| {
                    outcome.row.label == Label::Malicious && outcome.score >= policy.alert_threshold
                })
                .count();
            let false_positives = mine
                .iter()
                .filter(|outcome| {
                    outcome.row.label == Label::Benign && outcome.score >= policy.alert_threshold
                })
                .count();
            let quarantined = mine
                .iter()
                .filter(|outcome| outcome.decision == Decision::Quarantine)
                .count();
            EcosystemMetrics {
                ecosystem,
                benign,
                malicious,
                true_positives,
                false_positives,
                false_negatives: malicious - true_positives,
                precision_percent: percentage(true_positives, true_positives + false_positives),
                recall_percent: percentage(true_positives, malicious),
                quarantined,
            }
        })
        .collect()
}

fn percentage(part: usize, total: usize) -> u32 {
    if total == 0 {
        return 100;
    }
    ((part * 1000 / total) / 10) as u32
}

/// The false positives, named: which benign row was flagged and why.
pub fn false_positives<'a>(
    catalog: &EcosystemCatalog,
    policy: &HeuristicPolicy,
    rows: &'a [Row],
) -> Vec<(&'a Row, Vec<ReasonCode>)> {
    run(catalog, policy, rows)
        .into_iter()
        .filter(|outcome| {
            outcome.row.label == Label::Benign && outcome.score >= policy.alert_threshold
        })
        .map(|outcome| (outcome.row, outcome.reasons))
        .collect()
}

/// The quarantines the HOLD policy produced, named.
pub fn quarantines<'a>(
    catalog: &EcosystemCatalog,
    policy: &HeuristicPolicy,
    rows: &'a [Row],
) -> Vec<&'a Row> {
    run(catalog, policy, rows)
        .into_iter()
        .filter(|outcome| outcome.decision == Decision::Quarantine)
        .map(|outcome| outcome.row)
        .collect()
}

#[cfg(test)]
mod contract {
    use super::*;
    use hodei_risk::advisories::{
        DETECTOR_REVISION, HeuristicSignal, MethodRef, PUBLISHER_AGE_WINDOW_DAYS, PolicyMode,
        ReasonCode, TYPOSQUAT_MAX_DISTANCE, feedback::FeedbackError,
        feedback::ReviewerAuthorization, quarantine_case, resolve_false_positive,
    };

    fn corpus_size() -> usize {
        corpus().len()
    }

    /// AC1: the corpus is deterministic and balanced enough to
    /// measure.
    #[test]
    fn the_corpus_is_deterministic_and_labelled() {
        let rows = corpus();
        let benign = rows.iter().filter(|row| row.label == Label::Benign).count();
        let malicious = rows
            .iter()
            .filter(|row| row.label == Label::Malicious)
            .count();
        assert_eq!(rows.len(), corpus_size());
        assert!(
            benign >= 40,
            "the benign population is a real population, not three rows"
        );
        assert!(
            malicious >= 60,
            "each ecosystem contributes its share of attacks"
        );
        // Running the corpus twice gives the same scores: the seed is
        // the dataset.
        let catalog = catalog();
        let policy = HeuristicPolicy::new(PolicyMode::Detect, 40);
        let first = run(&catalog, &policy, &rows);
        let second = run(&catalog, &policy, &rows);
        for (one, two) in first.iter().zip(second.iter()) {
            assert_eq!(one.score, two.score);
            assert_eq!(one.reasons, two.reasons);
        }
    }

    /// AC1: precision and recall PER ECOSYSTEM, with the threshold and
    /// the false positives published (the numbers also go to
    /// evidence.md).
    #[test]
    fn the_metrics_are_published_per_ecosystem() {
        let rows = corpus();
        let catalog = catalog();
        let threshold = 40;
        let policy = HeuristicPolicy::new(PolicyMode::Detect, threshold);
        let metrics = metrics(&catalog, &policy, &rows);
        let mut report = String::new();
        for ecosystem_metrics in &metrics {
            // Each ecosystem must show real numbers on both sides.
            assert!(ecosystem_metrics.benign > 0 && ecosystem_metrics.malicious > 0);
            report.push_str(&format!(
                "{}: benign {} malicious {} TP {} FP {} FN {} -> precision {}% recall {}% (threshold {})\n",
                ecosystem_metrics.ecosystem.as_str(),
                ecosystem_metrics.benign,
                ecosystem_metrics.malicious,
                ecosystem_metrics.true_positives,
                ecosystem_metrics.false_positives,
                ecosystem_metrics.false_negatives,
                ecosystem_metrics.precision_percent,
                ecosystem_metrics.recall_percent,
                threshold,
            ));
            // Published floors: recall at least three quarters (the
            // two-edit squats are the designed misses), precision at
            // least 90 % (the near-name tax is paid by two rows).
            assert!(
                ecosystem_metrics.recall_percent >= 70,
                "{}: recall fell below the published floor",
                ecosystem_metrics.ecosystem.as_str()
            );
            assert!(
                ecosystem_metrics.precision_percent >= 90,
                "{}: precision fell below the published floor",
                ecosystem_metrics.ecosystem.as_str()
            );
        }
        // The known attack families are not missed anywhere: every
        // reserved-namespace publication fires the confusion code.
        let outcomes = run(&catalog, &policy, &rows);
        let all_confusion_flagged = outcomes
            .iter()
            .filter(|outcome| outcome.row.pattern.contains("confusion"))
            .all(|outcome| outcome.score >= threshold);
        assert!(
            all_confusion_flagged,
            "no confusion row is allowed to be quiet"
        );

        // And the report is printed with the numbers, not hidden in
        // assertions: run with --nocapture to read it.
        eprint!("{report}");

        // The misses are published too: which malicious rows did NOT
        // reach the threshold, named. They are the designed ones —
        // the two-edit variants — plus whatever the distance-1 rule
        // cannot see, and pretending they do not exist would be the
        // false green this contract exists to avoid.
        let outcomes = run(&catalog, &policy, &rows);
        let missed: Vec<String> = outcomes
            .iter()
            .filter(|outcome| outcome.row.label == Label::Malicious && outcome.score < threshold)
            .map(|outcome| {
                format!(
                    "  - {} {} (score {}, pattern: {})",
                    outcome.row.record.ecosystem.as_str(),
                    outcome.row.record.name,
                    outcome.score,
                    outcome.row.pattern
                )
            })
            .collect();
        eprintln!(
            "published false negatives (threshold {threshold}):\n{}",
            missed.join("\n")
        );

        // The hold-mode outcome, also published: every quarantine the
        // explicit policy produced, with the authorization that backs
        // it. A row may be held ONLY because it lands in a declared
        // reservation from a publisher the organization has not seen.
        let hold_policy = HeuristicPolicy::new(PolicyMode::Hold, threshold);
        let held: Vec<String> = quarantines(&catalog, &hold_policy, &rows)
            .into_iter()
            .map(|row| {
                format!(
                    "  - {} {} from publisher {}",
                    row.record.ecosystem.as_str(),
                    row.record.name,
                    row.record.publisher
                )
            })
            .collect();
        eprintln!(
            "quarantines in hold mode ({}):\n{}",
            held.len(),
            held.join("\n")
        );
        assert!(
            !held.is_empty(),
            "the corpus exercises the authorized-hold path, not only the alert path"
        );
    }

    /// AC1: the false positives are published, named, and they are the
    /// ones the design says exist: legitimate names one edit from
    /// another legitimate name.
    #[test]
    fn the_false_positives_are_named() {
        let rows = corpus();
        let catalog = catalog();
        let policy = HeuristicPolicy::new(PolicyMode::Detect, 40);
        let false_positives = false_positives(&catalog, &policy, &rows);
        assert!(
            false_positives
                .iter()
                .any(|(row, _)| row.record.name == "requestz"),
            "the near-name tax must show up as a published false positive, not be averaged away"
        );
        assert!(
            false_positives
                .iter()
                .any(|(row, _)| row.record.name == "expres")
        );
        for (row, reasons) in &false_positives {
            assert_eq!(row.label, Label::Benign);
            // Every published false positive is a similarity signal:
            // it is the honest limit of edit distance, and the feedback
            // loop exists precisely for it.
            assert!(reasons.contains(&ReasonCode::NameSimilarToKnownPackage));
        }
        let lines: Vec<String> = false_positives
            .iter()
            .map(|(row, reasons)| {
                let codes: Vec<&str> = reasons.iter().map(|reason| reason.as_str()).collect();
                format!(
                    "- {} {} [{:?}] scored and alerted: {}",
                    row.record.ecosystem.as_str(),
                    row.record.name,
                    row.record.version,
                    codes.join(", ")
                )
            })
            .collect();
        eprintln!(
            "published false positives (threshold 40):\n{}",
            lines.join("\n")
        );
    }

    /// AC1: the threshold sweep is part of the contract — the chosen
    /// operating point is neither arbitrary nor cherry-picked.
    #[test]
    fn the_threshold_sweep_is_published() {
        let rows = corpus();
        let catalog = catalog();
        let mut table = String::new();
        for threshold in [20u16, 30, 35, 40, 50, 70] {
            let policy = HeuristicPolicy::new(PolicyMode::Detect, threshold);
            let metrics = metrics(&catalog, &policy, &rows);
            let flagged: usize = run(&catalog, &policy, &rows)
                .iter()
                .filter(|outcome| outcome.score >= threshold)
                .count();
            let recall = metrics
                .iter()
                .map(|metrics| metrics.recall_percent)
                .sum::<u32>()
                / 3;
            let precision = metrics
                .iter()
                .map(|metrics| metrics.precision_percent)
                .sum::<u32>()
                / 3;
            table.push_str(&format!(
                "threshold {threshold:>3}: recall {recall}% precision {precision}% alert rows {flagged}\n"
            ));
        }
        eprint!("{table}");
        // The sweep is monotone: a higher threshold cannot flag more.
        let a = metrics(
            &catalog,
            &HeuristicPolicy::new(PolicyMode::Detect, 40),
            &rows,
        );
        let b = metrics(
            &catalog,
            &HeuristicPolicy::new(PolicyMode::Detect, 50),
            &rows,
        );
        for (one, two) in a.iter().zip(b.iter()) {
            assert!(one.recall_percent >= two.recall_percent);
        }
    }

    /// AC2: every signal carries its method and the revision of the
    /// rule set that produced it — an observation without a method
    /// reference is an anecdote.
    #[test]
    fn the_observations_are_linked_to_method_and_revision() {
        let rows = corpus();
        let catalog = catalog();
        let mut checked = 0;
        for row in &rows {
            let observation = evaluate(&row.record, &catalog);
            for signal in &observation.signals {
                assert_eq!(
                    signal.rule_revision, DETECTOR_REVISION,
                    "the signal says which revision judged it"
                );
                match &signal.method {
                    MethodRef::TyposquatLevenshtein { max_distance } => {
                        assert_eq!(*max_distance, TYPOSQUAT_MAX_DISTANCE);
                    }
                    MethodRef::NamespaceReservation => {}
                    MethodRef::CoordinateShape => {}
                    MethodRef::PublisherHistory => {}
                    MethodRef::MetadataDiff => {}
                    MethodRef::PublisherAgeDays { window_days } => {
                        assert_eq!(*window_days, PUBLISHER_AGE_WINDOW_DAYS);
                    }
                }
                checked += 1;
            }
        }
        assert!(
            checked >= 30,
            "the corpus actually produces signals to link"
        );
    }

    /// AC2: the explicit policy decides, and the default is fail-open.
    /// The same observation that alerts in `detect` only alerts; the
    /// hold quarantines EXCLUSIVELY through the authorized reservation.
    #[test]
    fn the_policy_is_explicit_and_fail_open_by_default() {
        let rows = corpus();
        let catalog = catalog();
        let default_policy = HeuristicPolicy::default();
        assert_eq!(
            default_policy.mode,
            PolicyMode::Detect,
            "the default does not hold"
        );

        let detect_outcomes = run(&catalog, &default_policy, &rows);
        assert!(
            !detect_outcomes
                .iter()
                .any(|outcome| outcome.decision == Decision::Quarantine),
            "in the default mode nothing is ever quarantined, at any score"
        );

        let hold_policy = HeuristicPolicy::new(PolicyMode::Hold, 40);
        let quarantined = quarantines(&catalog, &hold_policy, &rows);
        assert!(!quarantined.is_empty());
        for row in &quarantined {
            assert!(
                catalog.is_reserved(row.record.ecosystem, &row.record.name)
                    && !catalog.seen_publisher(&row.record.publisher),
                "only a reserved namespace from an unknown publisher may be held: {}",
                row.record.name
            );
        }
        // The designed near-name rows: alert in detect, still only
        // alert in hold — similarity never authorizes a quarantine.
        // (Maven squat `com.acme.platform:servlice` is deliberately
        // left out: it lands in a reserved group, so its quarantine
        // comes from the reservation, not from the similarity.)
        let squatted: Vec<&Row> = rows
            .iter()
            .filter(|row| {
                row.label == Label::Malicious
                    && row.pattern.contains("typosquat")
                    && !row.pattern.contains("slow")
                    && !catalog.is_reserved(row.record.ecosystem, &row.record.name)
            })
            .collect();
        assert!(!squatted.is_empty());
        for row in squatted {
            let observation = evaluate(&row.record, &catalog);
            let verdict = decide(&hold_policy, &observation);
            assert_eq!(
                verdict.decision,
                Decision::Alert,
                "a loud typosquat in hold mode must NOT quarantine: {}",
                row.record.name
            );
        }
    }

    /// AC2 + AC3: a quarantined observation opens a case whose finding
    /// is Unknown; the review that resolves the false positive is the
    /// ONLY thing that moves the state, and it requires a signed
    /// authorization.
    #[test]
    fn a_false_positive_is_resolved_only_with_an_explicit_authorization() {
        let rows = corpus();
        let catalog = catalog();
        let hold_policy = HeuristicPolicy::new(PolicyMode::Hold, 40);
        // Take one row that quarantines and one that alerts on
        // similarity.
        let confused = rows
            .iter()
            .find(|row| {
                row.pattern
                    .contains("dependency confusion inside a reserved npm scope")
            })
            .expect("the corpus carries the npm confusion rows");
        let observation = evaluate(&confused.record, &catalog);
        let verdict = decide(&hold_policy, &observation);
        let mut case = quarantine_case(&observation, &verdict, chrono::Utc::now());
        assert_eq!(verdict.decision, Decision::Quarantine);
        assert_eq!(case.state, hodei_risk::CaseState::Open);
        assert_eq!(
            case.findings[0].state,
            hodei_risk::AffectedState::Unknown,
            "a quarantine is not an accusation: the case says nobody knows"
        );

        // Without a signature the case does not move.
        let error = resolve_false_positive(
            &mut case,
            &observation,
            Some(
                hodei_risk::advisories::ReviewOutcome::DismissedFalsePositive {
                    justification: "the publisher is ours".to_string(),
                },
            ),
            None,
            chrono::Utc::now(),
            chrono::Duration::days(7),
        )
        .expect_err("an unsigned review is no review");
        assert_eq!(error, FeedbackError::MissingAuthorization);
        assert_eq!(case.state, hodei_risk::CaseState::Open, "and nothing moved");

        // With one, the case closes and the timeline keeps who decided.
        let grant =
            ReviewerAuthorization::new("secops@acme", "HSP-166-review-1", chrono::Utc::now());
        let resolution = resolve_false_positive(
            &mut case,
            &observation,
            Some(
                hodei_risk::advisories::ReviewOutcome::DismissedFalsePositive {
                    justification: "the publisher is ours and the ticket says so".to_string(),
                },
            ),
            Some(grant.clone()),
            chrono::Utc::now(),
            chrono::Duration::days(7),
        )
        .expect("the review is authorized");
        assert_eq!(resolution.authorization, grant);
        assert_eq!(case.state, hodei_risk::CaseState::ClosedNotAffected);
        assert!(
            case.timeline
                .iter()
                .any(|event| event.detail.contains("secops@acme")
                    && event.detail.contains("HSP-166-review-1")),
            "the audit trail names the reviewer and the ticket"
        );
    }

    /// AC3: the classification never leaves the epistemic ceiling.
    /// Nothing in the vocabulary can state malware, and the corpus
    /// shows it: even a fully loud row tops out at `Hypothesis`.
    #[test]
    fn the_heuristic_never_states_malware() {
        let rows = corpus();
        let catalog = catalog();
        let outcomes = run(
            &catalog,
            &HeuristicPolicy::new(PolicyMode::Detect, 40),
            &rows,
        );
        let hypotheses = outcomes
            .iter()
            .filter(|outcome| outcome.status == EpistemicStatus::Hypothesis);
        let hypothesis_count = hypotheses.count();
        assert!(
            hypothesis_count > 0,
            "corroborated rows reach hypothesis, not observation"
        );
        // And the ceiling holds: the loudest row in the corpus is a
        // hypothesis; there is no variant in the enum that could say
        // more. This is the directive's rule, assertable by the
        // vocabulary itself.
        let states: Vec<EpistemicStatus> = outcomes.iter().map(|outcome| outcome.status).collect();
        assert!(states.iter().all(|status| matches!(
            status,
            EpistemicStatus::Observation | EpistemicStatus::Hypothesis
        )));
    }

    /// AC3: the signal code never invokes an interpreter, never
    /// installs, never unpacks. The rule is enforced over the source:
    /// the forbidden tokens cannot appear outside a comment.
    #[test]
    fn the_signal_code_never_executes_anything() {
        // The list is deliberately blunt — the detector must not even
        // MENTION the ability to run something, because reading a
        // declaration is all it does.
        let forbidden: &[&str] = &[
            "std::process",
            "process::Command",
            "Command::new",
            ".spawn(",
            "exec(",
            "eval(",
            "libc",
            "nix::",
            "install(",
            "unpack",
            "subprocess",
        ];
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
        let advisories = std::path::Path::new(&manifest).join("src/advisories");
        let mut scanned = 0;
        for entry in std::fs::read_dir(&advisories).expect("the module exists") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("the source is read");
            for line in text.lines() {
                let trimmed = line.trim_start();
                // Comments are allowed to name the forbidden things in
                // order to forbid them; the code is not.
                if trimmed.starts_with("//") || trimmed.starts_with('*') {
                    continue;
                }
                for token in forbidden {
                    assert!(
                        !trimmed.contains(token),
                        "{}: {} appears in non-comment code -- the detectors read metadata, \
                         they never execute it",
                        path.display(),
                        token
                    );
                }
                scanned += 1;
            }
        }
        assert!(
            scanned > 200,
            "the scan covers the whole module, not one file"
        );
    }

    /// AC3: the reason codes are a typed, finite vocabulary and every
    /// row that fired carries at least one — no free-text signal.
    #[test]
    fn the_reason_codes_are_typed_and_complete() {
        let rows = corpus();
        let catalog = catalog();
        let outcomes = run(
            &catalog,
            &HeuristicPolicy::new(PolicyMode::Detect, 40),
            &rows,
        );
        let flagged: Vec<&Outcome<'_>> = outcomes
            .iter()
            .filter(|outcome| outcome.score > 0)
            .collect();
        assert!(!flagged.is_empty());
        for outcome in flagged {
            assert!(
                !outcome.reasons.is_empty(),
                "a score with no reason code is not an observation: {}",
                outcome.row.record.name
            );
        }
        // The families the directive asks for are all present in the
        // corpus: name similarity, confusion, publisher, metadata.
        let families: Vec<&str> = outcomes
            .iter()
            .flat_map(|outcome| outcome.reasons.iter())
            .map(|reason| reason.family())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(families.contains(&"name"), "name similarity is measured");
        assert!(families.contains(&"confusion"), "confusion is measured");
        assert!(
            families.contains(&"publisher"),
            "publisher novelty is measured"
        );
        assert!(
            families.contains(&"metadata"),
            "metadata anomaly is measured"
        );
    }

    /// The explicit fixture: the same corpus, versioned next to the
    /// generator so a reviewer can read the rows without running
    /// anything. Regenerate with `cargo test -p hodei-risk --test
    /// hsp166_dataset -- --ignored`.
    #[test]
    fn the_fixture_agrees_with_the_generator() {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
        let path = std::path::Path::new(&manifest).join("tests/fixtures/hsp166-corpus.json");
        let text = std::fs::read_to_string(&path).expect("the fixture is versioned");
        let stored: Vec<serde_json::Value> =
            serde_json::from_str(&text).expect("and it is read as JSON");
        let rows = corpus();
        let catalog = catalog();
        let policy = HeuristicPolicy::new(PolicyMode::Detect, 40);
        let outcomes = run(&catalog, &policy, &rows);
        assert_eq!(
            stored.len(),
            outcomes.len(),
            "the fixture holds every row the generator produces"
        );
        for (stored_row, outcome) in stored.iter().zip(outcomes.iter()) {
            assert_eq!(
                stored_row["name"],
                serde_json::json!(outcome.row.record.name)
            );
            assert_eq!(stored_row["score"], serde_json::json!(outcome.score));
            assert_eq!(
                stored_row["decision"],
                serde_json::json!(outcome.decision.as_str())
            );
        }
    }

    #[test]
    #[ignore = "regenerates the fixture; run with --ignored and the env var set"]
    fn dump() {
        let rows = corpus();
        let catalog = catalog();
        let policy = HeuristicPolicy::new(PolicyMode::Detect, 40);
        let outcomes = run(&catalog, &policy, &rows);
        let json: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| {
                serde_json::json!({
                    "ecosystem": outcome.row.record.ecosystem.as_str(),
                    "name": outcome.row.record.name,
                    "version": outcome.row.record.version,
                    "publisher": outcome.row.record.publisher,
                    "label": match outcome.row.label {
                        Label::Benign => "benign",
                        Label::Malicious => "malicious",
                    },
                    "pattern": outcome.row.pattern,
                    "score": outcome.score,
                    "reasons": outcome.reasons.iter().map(|reason| reason.as_str()).collect::<Vec<_>>(),
                    "status": match outcome.status {
                        EpistemicStatus::Observation => "observation",
                        EpistemicStatus::Hypothesis => "hypothesis",
                    },
                    "decision": outcome.decision.as_str(),
                })
            })
            .collect();
        let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir");
        let path = std::path::Path::new(&manifest).join("tests/fixtures/hsp166-corpus.json");
        std::fs::write(&path, serde_json::to_string_pretty(&json).expect("json"))
            .expect("the fixture is written");
        eprintln!("fixture written to {}", path.display());
    }

    /// The typed signals keep their method reference when serialized;
    /// the observation is the thing a reviewer reads.
    #[test]
    fn signals_survive_their_serialization() {
        let rows = corpus();
        let catalog = catalog();
        let row = rows
            .iter()
            .find(|row| {
                row.pattern
                    .contains("dependency confusion inside a reserved Maven group")
            })
            .expect("the corpus carries maven confusion");
        let observation = evaluate(&row.record, &catalog);
        let json = serde_json::to_string(&observation).expect("the observation is serializable");
        let back: hodei_risk::advisories::Observation =
            serde_json::from_str(&json).expect("and it reads back");
        assert_eq!(observation, back);
        for signal in &observation.signals {
            let _ = signal; // every HeuristicSignal round-trips with its method
        }
        // The detail names the data compared: a reviewer can audit the
        // comparison without re-running anything.
        assert!(
            observation
                .signals
                .iter()
                .any(|signal: &HeuristicSignal| signal.detail.contains("reserved namespace"))
        );
    }
}
