//! El evaluador de decisiones: la decision de verdad, con la capsula de
//! verdad.
//!
//! La simulacion decide con la MISMA capsula que decide la verdad: la
//! evidencia del sujeto, ensamblada en su capsula, y la politica
//! evaluada contra ella. Y los hechos hipoteticos se aplican **antes** de
//! ensamblar, que es lo que hace que la decision del fork sea la
//! decision de un mundo donde esos hechos pasaron.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::evidence::{EvidenceRecord, EvidenceVerification};
use hodei_evidence::ports::EvidenceStore;
use hodei_simulation::diff::TheDecision;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim};
use hodei_simulation::ports::DecisionEvaluator;
use hodei_trust_policy::TrustPolicy;
use hodei_trust_policy::context::RiskFacts;
use hodei_trust_policy::decision::evaluate;
use kernel::Hrn;

/// El evaluador que decide con la capsula del sujeto.
pub struct CapsuleDecisionEvaluator {
    reader: Arc<dyn EvidenceStore>,
    assembler: CapsuleAssembler,
    account: String,
}

impl CapsuleDecisionEvaluator {
    /// El evaluador sobre un lector de evidencia y una cuenta.
    pub fn new(reader: Arc<dyn EvidenceStore>, account: impl Into<String>) -> Self {
        Self {
            reader,
            assembler: CapsuleAssembler::new(),
            account: account.into(),
        }
    }

    /// La evidencia del sujeto, con los hechos hipoteticos aplicados.
    ///
    /// Una ventana en la que no se fia del constructor deja sin verificar
    /// lo que se observo dentro de la ventana: es lo que significa decir
    /// que el constructor estaba comprometido durante ese rato.
    async fn the_evidence(
        &self,
        subject: &Hrn,
        facts: &[Hypothetical],
    ) -> Result<Vec<EvidenceRecord>, String> {
        // El fallo de lectura NO es una capsula vacia: un error del
        // almacen llega como error, porque una capsula vacia diria "no
        // sabemos nada" y el almacen dice "no pudimos mirar". Son cosas
        // distintas, y la segunda no decide nada.
        let evidence = self
            .reader
            .evidence_of_subject(&self.account, subject)
            .await
            .map_err(|error| error.to_string())?;
        // Las ventanas que alcanzan a ESTE sujeto: una ventana que no
        // nombra al sujeto no le dice nada.
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

    /// Los hechos de riesgo que los hechos hipoteticos anaden.
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
            established: critical > 0,
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
        // Una evidencia REALMENTE vacia no es un error: el sujeto puede
        // no tener nada dicho, y la decision se toma con lo que hay --
        // que puede ser Indeterminate, y Indeterminate NO es un ALLOW.
        let capsule = self.assembler.assemble(subject, &evidence, now);
        let context = hodei_trust_policy::TrustContext::builder(&capsule)
            .with_risk(self.the_risk(subject, facts))
            .assembled_at(now)
            .build();

        let decision = evaluate(&context, policy);

        Ok(TheDecision {
            outcome: decision.outcome,
            policy: policy.clone(),
        })
    }
}

/// La misma pieza de evidencia, con otra verificacion.
///
/// La validez desconocida se CONSERVA como desconocida: una evidencia
/// que no dice cuando deja de ser cierta no se le inventa una fecha de
/// caducidad, porque esa fecha seria una verdad que nadie dijo. Y la
/// fecha de caducidad inventada caduca cuando la evidencia no caduca,
/// y hace desaparecer una afirmacion que sigue en pie.
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
    // Las dos validez se copian tal cual, incluidas las que no se
    // saben: el registro reconstruido es el mismo registro.
    if let Some(valid_from) = record.valid_from() {
        builder = builder.valid_from(valid_from);
    }
    if let Some(valid_to) = record.valid_to() {
        builder = builder.valid_to(valid_to);
    }
    builder.build()
}
