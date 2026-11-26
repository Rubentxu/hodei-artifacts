//! The decision evaluator: the real decision, with the truth
//! capsule.
//!
//! The simulation decides with the SAME capsule that decides the truth: the
//! subject's evidence, assembled into its capsule, and the policy
//! evaluated against it. And the hypothetical facts are applied **before**
//! assembling, which is what makes the fork's decision the
//! decision of a world where those facts happened.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::evidence::{EvidenceRecord, EvidenceVerification};
use hodei_evidence::domain::read_set::{ReadSet, ReadStamp};
use hodei_evidence::ports::EvidenceStore;
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim};
use hodei_simulation::ports::DecisionEvaluator;
use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::context::RiskFacts;
use hodei_trust_policy::decision::evaluate;
use kernel::Hrn;

/// The evaluator that decides with the subject's capsule.
pub struct CapsuleDecisionEvaluator {
    reader: Arc<dyn EvidenceStore>,
    assembler: CapsuleAssembler,
    account: String,
}

impl CapsuleDecisionEvaluator {
    /// The evaluator over an evidence reader and an account.
    pub fn new(reader: Arc<dyn EvidenceStore>, account: impl Into<String>) -> Self {
        Self {
            reader,
            assembler: CapsuleAssembler::new(),
            account: account.into(),
        }
    }

    /// The subject's evidence, with the hypothetical facts applied.
    ///
    /// A window in which the builder is not trusted leaves unverified
    /// what was observed within the window: that is what it means to say
    /// that the builder was compromised during that time.
    async fn the_evidence(
        &self,
        subject: &Hrn,
        facts: &[Hypothetical],
    ) -> Result<Vec<EvidenceRecord>, String> {
        // A read failure is NOT an empty capsule: a store error
        // arrives as an error, because an empty capsule would say "we
        // know nothing" and the store says "we could not look". They are
        // different things, and the second decides nothing.
        let evidence = self
            .reader
            .evidence_of_subject(&self.account, subject)
            .await
            .map_err(|error| error.to_string())?;
        // The windows that reach THIS subject: a window that does not
        // name the subject says nothing about it.
        let windows: Vec<(DateTime<Utc>, DateTime<Utc>)> = facts
            .iter()
            .filter_map(|fact| match &fact.claim {
                HypotheticalClaim::UntrustedWindow { from, to, subjects }
                    if subjects.contains(subject) =>
                {
                    Some((*from, *to))
                }
                _ => None,
            })
            .collect();

        if windows.is_empty() {
            return Ok(evidence);
        }

        let evidence: Vec<EvidenceRecord> = evidence
            .into_iter()
            .map(|record| {
                let observed_at = record.observed_at();
                if windows
                    .iter()
                    .any(|(from, to)| observed_at >= *from && observed_at <= *to)
                {
                    the_record_with(record, EvidenceVerification::unverified())
                } else {
                    record
                }
            })
            .collect();
        Ok(evidence)
    }

    /// The risk facts that the hypothetical facts add.
    fn the_risk(&self, subject: &Hrn, facts: &[Hypothetical]) -> RiskFacts {
        let critical: usize = facts
            .iter()
            .filter(|fact| {
                matches!(
                    &fact.claim,
                    HypotheticalClaim::Risk { subject: who, critical, reachable, .. }
                        if who == subject && *critical && *reachable
                )
            })
            .count();
        RiskFacts {
            critical_known: critical as u32,
            critical_reachable: critical as u32,
            // The hypothetical feed was consulted for this subject and
            // answered: whatever it did not name is an established zero,
            // never UNKNOWN. Only the real reader can fail to establish.
            established: true,
            ..RiskFacts::default()
        }
    }
}

#[async_trait]
impl DecisionEvaluator for CapsuleDecisionEvaluator {
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
        now: DateTime<Utc>,
    ) -> Result<TheDecision, String> {
        let evidence = self
            .the_evidence(subject, facts)
            .await
            .map_err(|reason| format!("the evidence of {subject} could not be read: {reason}"))?;
        // Truly EMPTY evidence is not an error: the subject may
        // have nothing said, and the decision is taken with what there is --
        // which may be Indeterminate, and Indeterminate is NOT an ALLOW.
        let capsule = self.assembler.assemble(subject, &evidence, now);
        let context = hodei_trust_policy::TrustContext::builder(&capsule)
            .with_risk(self.the_risk(subject, facts))
            .assembled_at(now)
            .build();

        let decision = evaluate(&context, policy);

        Ok(TheDecision {
            outcome: decision.outcome,
            policy: policy.clone(),
            read_set: the_read_set_of(&evidence),
        })
    }

    async fn the_reads_of(&self, subject: &Hrn) -> Result<ReadSet, String> {
        let evidence = self
            .reader
            .evidence_of_subject(&self.account, subject)
            .await
            .map_err(|error| error.to_string())?;
        Ok(the_read_set_of(&evidence))
    }
}

/// The manifest of what was read: a piece of evidence by its id and
/// the digest of its content.
///
/// The digest is what allows the origin to be checked: if the evidence
/// changes content, the digest changes, and the digest is what is
/// compared at the end of the simulation.
fn the_read_set_of(evidence: &[EvidenceRecord]) -> ReadSet {
    let mut read_set = ReadSet::empty();
    for record in evidence {
        read_set.read(ReadStamp::evidence(
            record.id().to_string(),
            record.content().digest.to_string(),
        ));
    }
    read_set
}

/// The same piece of evidence, with another verification.
///
/// Unknown validity is KEPT as unknown: evidence
/// that does not say when it stops being true is not given an invented
/// expiry date, because that date would be a truth that nobody stated. And the
/// invented expiry date expires when the evidence does not expire,
/// and makes an assertion that still stands disappear.
fn the_record_with(record: EvidenceRecord, verification: EvidenceVerification) -> EvidenceRecord {
    let mut builder = EvidenceRecord::builder(record.id().clone(), record.tenant_hrn().clone())
        .subjects(record.subject_refs().to_vec())
        .evidence_type(record.evidence_type().clone())
        .schema(
            record.schema_uri().unwrap_or_default().to_string(),
            record.schema_version().unwrap_or_default().to_string(),
        )
        .producer(record.producer().clone())
        .source(record.source().clone())
        .content(record.content().clone())
        .observed_at(record.observed_at())
        .recorded_at(record.recorded_at())
        .verification(verification)
        .normalization(record.normalization().clone())
        .correlation_id(record.correlation_id().unwrap_or_default().to_string());
    // Both validities are copied as they are, including those that are not
    // known: the rebuilt record is the same record.
    if let Some(valid_from) = record.valid_from() {
        builder = builder.valid_from(valid_from);
    }
    if let Some(valid_to) = record.valid_to() {
        builder = builder.valid_to(valid_to);
    }
    builder.build()
}
