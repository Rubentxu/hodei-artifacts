//! The investigation report: what the model said, put in a form
//! that can be consumed without mistaking plausible text for
//! an objective check.
//!
//! The finding classes are DISTINCT and remain so in the report:
//! what the **deterministic verifier** checked against the current
//! evidence is a `VerifiedFact`; what the model said with a citation that
//! resolves is, at most, an `EvidenceBackedClaim`; and an inference,
//! an uncertainty or a recommendation are each their own class,
//! with their origin and their limits.
//!
//! # What is deterministic here
//!
//! The verifier does NOT read text with fuzzy judgement: it extracts from the
//! statement the canonical relations it asserts -- two HRNs and a
//! relation word between them -- and compares them against the read-set's STAMPS,
//! which are the relations the world really has.
//! A statement that asserts a relation the world does not have is a
//! CONTRADICTION, and a contradiction never produces a VerifiedFact,
//! even if its citation is authentic.

use serde::{Deserialize, Serialize};

use crate::claim::{AIClaim, ClaimKind};
use crate::read_set::{ReadKind, ReadSet};
use crate::scope::Coverage;
use kernel::Hrn;

/// The version of the report contract.
pub const REPORT_SCHEMA_VERSION: &str = "1";

/// The class of finding. The classes are not mixed: a model text
/// with a resolvable citation is NOT a verified fact, and an inference is NOT
/// a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// The deterministic verifier checked it against the current
    /// evidence. ONLY the verifier grants it.
    VerifiedFact,
    /// The model said it and its citation resolves. Nobody has checked the
    /// statement.
    EvidenceBackedClaim,
    /// An inference by the model: declared as such, with its limits.
    Inference,
    /// A declared uncertainty: what is not known is said not to be
    /// known.
    Uncertainty,
    /// A recommendation: NOBODY executed it, and it does not change the trust
    /// state.
    Recommendation,
}

/// The verification state of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// The deterministic verifier checked it.
    Verified,
    /// The citation resolves, but nobody has checked the statement.
    Unverified,
    /// The world's evidence CONTRADICTS the statement.
    Contradicted,
    /// It could not be checked: what would be needed is not there.
    Unverifiable,
}

/// A finding of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The class of the finding.
    pub kind: FindingKind,
    /// The statement, as stated by whoever said it.
    pub statement: String,
    /// The evidence that is cited.
    pub citations: Vec<String>,
    /// The verification state.
    pub status: VerificationStatus,
    /// What the state rests on: the origin and the limits of the finding.
    pub basis: String,
}

/// The report of an investigation.
///
/// It is serializable and VERSIONED: a consumer knows which contract
/// it is reading. It includes the finding types, the cited evidence,
/// the read-set with its versions and the coverage -- and it does NOT invent
/// authoritative decisions: the report describes, it does not decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestigationReport {
    /// The version of the contract.
    pub schema_version: String,
    /// The subject that was investigated.
    pub subject: String,
    /// The findings, in the order of the claims that produced them.
    pub findings: Vec<Finding>,
    /// How much of what exists was handed to the model.
    pub coverage: Coverage,
    /// The manifest of the reads, with their versions.
    pub read_set: ReadSet,
}

/// The deterministic verifier of statements.
///
/// It extracts from the statement the canonical relations it asserts -- two HRNs
/// and a relation word between them -- and compares them against the
/// read-set's stamps. It is a pure function: the same stamps and the
/// same statement give the same verdict, always.
pub fn verify_statement(
    statement: &str,
    citations: &[String],
    read_set: &ReadSet,
) -> VerificationStatus {
    // The citations have to be evidence from the manifest: a citation that
    // is not in the read-set cannot be checked.
    let citations_resolve = citations.iter().all(|citation| {
        read_set
            .stamps
            .iter()
            .any(|stamp| stamp.kind == ReadKind::Evidence && stamp.resource == *citation)
    });
    if !citations_resolve {
        return VerificationStatus::Unverifiable;
    }

    // The canonical relations the statement asserts.
    let asserted = the_asserted_relations(statement);
    if asserted.is_empty() {
        // A known predicate cannot be extracted: the citation resolves,
        // but nobody has checked the statement.
        return VerificationStatus::Unverified;
    }

    let mut statuses = Vec::new();
    for (relation, from, to) in asserted {
        let identity = format!("{relation}|{from}|{to}");
        let known = read_set
            .stamps
            .iter()
            .any(|stamp| stamp.kind == ReadKind::Relation && stamp.resource == identity);
        if known {
            statuses.push(VerificationStatus::Verified);
        } else {
            // The statement asserts a relation the world does not have:
            // it is a contradiction, and a contradiction is never a
            // verified fact, even if its citation is authentic.
            statuses.push(VerificationStatus::Contradicted);
        }
    }
    if statuses.contains(&VerificationStatus::Contradicted) {
        VerificationStatus::Contradicted
    } else {
        VerificationStatus::Verified
    }
}

/// The canonical relations a statement asserts: two HRNs and a
/// relation word between them. Extraction is by fixed
/// patterns, not by fuzzy judgement.
fn the_asserted_relations(statement: &str) -> Vec<(String, String, String)> {
    let hrns = the_hrn_substrings(statement);
    let lowered = statement.to_lowercase();
    // The two writings of each relation: the one the **graph** writes
    // (`depends_on`) and the one a **model** writes (`depends on`). Both
    // are the same relation, and the manifest keeps the identity with the
    // first: without both, a relation the world truly has
    // --"deployed_as", "affected_by"-- came out as unchecked, which is
    // a way of lying by omission.
    let words = [
        ("depends on", "depends_on"),
        ("depends", "depends_on"),
        ("depends_on", "depends_on"),
        ("contains", "contains"),
        ("affected by", "affected_by"),
        ("affected_by", "affected_by"),
        ("deployed as", "deployed_as"),
        ("deployed_as", "deployed_as"),
    ];

    let mut asserted = Vec::new();
    // Word boundaries, not byte ones: a statement may carry
    // accents --a model writes it-- and a cut in the middle of a character
    // is a panic, not a relation.
    for (word_index, _) in lowered.char_indices() {
        for (phrase, relation) in words {
            if lowered[word_index..].starts_with(phrase) {
                // The two HRNs surrounding the word, in the order in which
                // they appear.
                let before = &lowered[..word_index];
                let after = &lowered[word_index + phrase.len()..];
                let from = the_last_hrn_before(before, &hrns);
                let to = the_first_hrn_after(after, &hrns);
                if let (Some(from), Some(to)) = (from, to) {
                    let asserted_relation = (relation.to_string(), from, to);
                    // The same relation counted twice --"depends" and
                    // "depends_on" look alike-- is ONE relation.
                    if !asserted.contains(&asserted_relation) {
                        asserted.push(asserted_relation);
                    }
                }
            }
        }
    }
    asserted
}

/// The HRNs that appear in a text, as canonical strings.
fn the_hrn_substrings(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if &bytes[index..index + 4] == b"hrn:" {
            let mut end = index + 4;
            while end < bytes.len() {
                let byte = bytes[end];
                if byte.is_ascii_alphanumeric()
                    || byte == b':'
                    || byte == b'.'
                    || byte == b'-'
                    || byte == b'_'
                    || byte == b'/'
                {
                    end += 1;
                } else {
                    break;
                }
            }
            found.push(text[index..end].to_string());
            index = end;
        } else {
            index += 1;
        }
    }
    found
}

fn the_last_hrn_before(before: &str, hrns: &[String]) -> Option<String> {
    // The text is lowercase: the HRNs are compared in lowercase
    // too, because "Artifact" and "artifact" are the same HRN here.
    let lowered = before.to_lowercase();
    let mut best: Option<String> = None;
    for hrn in hrns {
        if lowered.contains(&hrn.to_lowercase()) {
            best = Some(hrn.clone());
        }
    }
    best
}

fn the_first_hrn_after(after: &str, hrns: &[String]) -> Option<String> {
    let lowered = after.to_lowercase();
    hrns.iter()
        .find(|hrn| lowered.contains(&hrn.to_lowercase()))
        .cloned()
}

/// The class of finding that corresponds to a claim, given its verification
/// state.
fn the_finding_kind(kind: ClaimKind, status: VerificationStatus) -> FindingKind {
    match kind {
        ClaimKind::Fact => {
            if status == VerificationStatus::Verified {
                FindingKind::VerifiedFact
            } else {
                // Model text with a resolvable citation is NOT a verified
                // fact, and a contradiction even less so.
                FindingKind::EvidenceBackedClaim
            }
        }
        ClaimKind::Inference => FindingKind::Inference,
        ClaimKind::Unknown => FindingKind::Uncertainty,
        ClaimKind::Recommendation => FindingKind::Recommendation,
    }
}

/// What the finding's state rests on: the origin and the limits.
fn the_basis(kind: ClaimKind, status: VerificationStatus) -> String {
    match status {
        VerificationStatus::Verified => {
            "the deterministic verifier checked the asserted relations against the read-set \
             stamps"
                .to_string()
        }
        VerificationStatus::Unverified => match kind {
            ClaimKind::Fact => {
                "the citation resolves, but the statement asserts no relation the verifier \
                 knows how to check"
                    .to_string()
            }
            _ => "declared by the model; not a fact to verify".to_string(),
        },
        VerificationStatus::Contradicted => {
            "the statement asserts relations the world does not have: the evidence cited is \
             genuine and it contradicts the model"
                .to_string()
        }
        VerificationStatus::Unverifiable => {
            "the citations do not resolve in the read-set: nothing to check against".to_string()
        }
    }
}

/// Builds the report of an investigation.
///
/// The ALREADY VALIDATED claims enter here: the report describes what the
/// model said and what the verifier could check, and does NOT invent
/// authoritative decisions. The same claims, the same read-set and the
/// same coverage give the SAME report.
pub fn build_report(
    subject: &Hrn,
    claims: &[AIClaim],
    read_set: &ReadSet,
    coverage: Coverage,
) -> InvestigationReport {
    let findings = claims
        .iter()
        .map(|claim| {
            let status = verify_statement(&claim.statement, &claim.citations, read_set);
            Finding {
                kind: the_finding_kind(claim.kind, status),
                statement: claim.statement.clone(),
                citations: claim.citations.clone(),
                status,
                basis: the_basis(claim.kind, status),
            }
        })
        .collect();

    InvestigationReport {
        schema_version: REPORT_SCHEMA_VERSION.to_string(),
        subject: subject.to_string(),
        findings,
        coverage,
        read_set: read_set.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::{AIClaim, ClaimKind};

    fn claim(statement: &str, kind: ClaimKind, citations: &[&str]) -> AIClaim {
        AIClaim::new(
            statement,
            kind,
            citations.iter().map(|one| one.to_string()).collect(),
        )
    }

    fn the_world() -> ReadSet {
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", 12);
        read_set.read(crate::read_set::ReadStamp::evidence("evd-1", "sha256:aaa"));
        read_set.read(crate::read_set::ReadStamp::relation(
            "depends_on|hrn:hodei:artifacts:eu-west-1:acme:Artifact/app",
            "evd-1",
        ));
        // The relation the world has: app depends on lib-v1.
        read_set.read(crate::read_set::ReadStamp::relation(
            "depends_on|hrn:hodei:artifacts:eu-west-1:acme:Artifact/app|hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1",
            "evd-1",
        ));
        read_set
    }

    /// AC1: the statement the world CONFIRMS, with a citation that resolves,
    /// is a VerifiedFact -- and ONLY the verifier grants it.
    #[test]
    fn a_statement_the_world_confirms_is_a_verified_fact() {
        let world = the_world();
        let claim = claim(
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app depends on \
             hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1",
            ClaimKind::Fact,
            &["evd-1"],
        );
        let report = build_report(&subject(), &[claim], &world, Coverage::Complete);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].kind, FindingKind::VerifiedFact);
        assert_eq!(report.findings[0].status, VerificationStatus::Verified);
    }

    /// AC1: an AUTHENTIC citation that contradicts the model's text does NOT
    /// produce a VerifiedFact: it is a contradicted EvidenceBackedClaim.
    #[test]
    fn a_genuine_citation_that_contradicts_the_text_is_not_a_verified_fact() {
        let world = the_world();
        // The model says lib-v2; the world has lib-v1. The citation is
        // authentic and resolves.
        let claim = claim(
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app depends on \
             hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2",
            ClaimKind::Fact,
            &["evd-1"],
        );
        let report = build_report(&subject(), &[claim], &world, Coverage::Complete);
        assert_eq!(report.findings[0].kind, FindingKind::EvidenceBackedClaim);
        assert_eq!(report.findings[0].status, VerificationStatus::Contradicted);
        assert!(
            report.findings[0].basis.contains("contradicts"),
            "el limite se dice: {}",
            report.findings[0].basis
        );
    }

    /// A fact without verifiable support (a citation that does not resolve) fails
    /// closed: Unverifiable, never Verified.
    #[test]
    fn an_unresolvable_citation_is_unverifiable_not_verified() {
        let world = the_world();
        let claim = claim("the artefact is fine", ClaimKind::Fact, &["evd-unknown"]);
        let report = build_report(&subject(), &[claim], &world, Coverage::Complete);
        assert_eq!(report.findings[0].status, VerificationStatus::Unverifiable);
        assert_eq!(report.findings[0].kind, FindingKind::EvidenceBackedClaim);
    }

    /// The classes are NOT mixed: an inference is an inference, an
    /// uncertainty is an uncertainty, a recommendation is a
    /// recommendation -- with their origin and their limits.
    #[test]
    fn the_classes_stay_distinct_with_their_limits() {
        let world = the_world();
        let claims = vec![
            claim(
                "the deployment is therefore exposed",
                ClaimKind::Inference,
                &["evd-1"],
            ),
            claim("we do not know the reachability", ClaimKind::Unknown, &[]),
            claim("upgrade the component", ClaimKind::Recommendation, &[]),
        ];
        let report = build_report(&subject(), &claims, &world, Coverage::Complete);
        assert_eq!(report.findings[0].kind, FindingKind::Inference);
        assert_eq!(report.findings[1].kind, FindingKind::Uncertainty);
        assert_eq!(report.findings[2].kind, FindingKind::Recommendation);
        for finding in &report.findings {
            assert!(
                !finding.basis.is_empty(),
                "cada finding dice su origen y sus limites"
            );
        }
    }

    /// AC2 + AC3: the report is serializable, versioned and carries the
    /// read-set with its versions.
    #[test]
    fn the_report_is_versioned_and_serializable() {
        let world = the_world();
        let claim = claim(
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app depends on \
             hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1",
            ClaimKind::Fact,
            &["evd-1"],
        );
        let report = build_report(&subject(), &[claim], &world, Coverage::Complete);
        assert_eq!(report.schema_version, REPORT_SCHEMA_VERSION);
        assert_eq!(report.subject, subject().to_string());
        assert_eq!(report.coverage, Coverage::Complete);
        assert_eq!(report.read_set.fingerprint(), world.fingerprint());
        let json = serde_json::to_string(&report).expect("el informe serializa");
        assert!(json.contains("\"schema_version\":\"1\""), "{json}");
        assert!(
            serde_json::from_str::<InvestigationReport>(&json).is_ok(),
            "el informe deserializa con su version"
        );
    }

    /// The replay: the same claims and the same read-set give the SAME
    /// report, with no clocks or accidental ordering.
    #[test]
    fn the_same_claims_and_world_give_the_same_report() {
        let world = the_world();
        let claims = vec![claim(
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app depends on \
             hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1",
            ClaimKind::Fact,
            &["evd-1"],
        )];
        let first = build_report(&subject(), &claims, &world, Coverage::Complete);
        let second = build_report(&subject(), &claims, &world, Coverage::Complete);
        assert_eq!(first, second);
    }

    fn subject() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app").expect("un HRN")
    }
}
