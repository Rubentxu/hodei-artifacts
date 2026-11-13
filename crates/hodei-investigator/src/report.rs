//! El informe de la investigacion: lo que el modelo dijo, dicho de forma
//! que se pueda consumir sin confundir un texto plausible con una
//! comprobacion objetiva.
//!
//! Las clases de finding son DISTINTAS y lo siguen siendo en el informe:
//! lo que el **verificador determinista** comprobo contra la evidencia
//! vigente es un `VerifiedFact`; lo que el modelo dijo con una cita que
//! resuelve es, como mucho, un `EvidenceBackedClaim`; y una inferencia,
//! una incertidumbre o una recomendacion son cada una su propia clase,
//! con su origen y sus limites.
//!
//! # Que es determinista aqui
//!
//! El verificador NO lee texto con criterio difuso: extrae del
//! enunciado las relaciones canónicas que afirma -- dos HRNs y una
//! palabra de relacion entre ellos -- y las compara contra los SELLOS
//! del read-set, que son las relaciones que el mundo realmente tiene.
//! Un enunciado que afirma una relacion que el mundo no tiene es una
//! CONTRADICCION, y una contradiccion nunca produce un VerifiedFact,
//! aunque su cita sea autentica.

use serde::{Deserialize, Serialize};

use crate::claim::{AIClaim, ClaimKind};
use crate::read_set::{ReadKind, ReadSet};
use crate::scope::Coverage;
use kernel::Hrn;

/// La version del contrato del informe.
pub const REPORT_SCHEMA_VERSION: &str = "1";

/// La clase de finding. Las clases no se mezclan: un texto del modelo
/// con cita resoluble NO es un hecho verificado, y una inferencia NO es
/// un hecho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// El verificador determinista lo comprobo contra la evidencia
    /// vigente. SOLO el verificador lo otorga.
    VerifiedFact,
    /// El modelo lo dijo y su cita resuelve. Nadie ha comprobado el
    /// enunciado.
    EvidenceBackedClaim,
    /// Una inferencia del modelo: se declara como tal, con sus limites.
    Inference,
    /// Una incertidumbre declarada: lo que no se sabe se dice que no se
    /// sabe.
    Uncertainty,
    /// Una recomendacion: NADIE la ejecuto, y no cambia el estado de
    /// confianza.
    Recommendation,
}

/// El estado de verificacion de un finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// El verificador determinista lo comprobo.
    Verified,
    /// La cita resuelve, pero nadie ha comprobado el enunciado.
    Unverified,
    /// La evidencia del mundo CONTRADICE el enunciado.
    Contradicted,
    /// No se pudo comprobar: lo que haria falta no esta.
    Unverifiable,
}

/// Un finding del informe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// La clase del finding.
    pub kind: FindingKind,
    /// El enunciado, tal como lo dijo quien lo dijo.
    pub statement: String,
    /// Las evidencias que se citan.
    pub citations: Vec<String>,
    /// El estado de verificacion.
    pub status: VerificationStatus,
    /// En que se apoya el estado: el origen y los limites del finding.
    pub basis: String,
}

/// El informe de una investigacion.
///
/// Es serializable y VERSIONADO: un consumidor sabe con que contrato
/// esta leyendo. Incluye los tipos de finding, las evidencias citadas,
/// el read-set con sus versiones y la cobertura -- y NO inventa
/// decisiones autoritativas: el informe describe, no decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestigationReport {
    /// La version del contrato.
    pub schema_version: String,
    /// El sujeto sobre el que se investigo.
    pub subject: String,
    /// Los findings, en el orden de las afirmaciones que los produjeron.
    pub findings: Vec<Finding>,
    /// Cuanto de lo que hay se entrego al modelo.
    pub coverage: Coverage,
    /// El manifiesto de las lecturas, con sus versiones.
    pub read_set: ReadSet,
}

/// El verificador determinista de enunciados.
///
/// Extrae del enunciado las relaciones canónicas que afirma -- dos HRNs
/// y una palabra de relacion entre ellos -- y las compara contra los
/// sellos del read-set. Es una funcion pura: los mismos sellos y el
/// mismo enunciado dan el mismo veredicto, siempre.
pub fn verify_statement(statement: &str, citations: &[String], read_set: &ReadSet) -> VerificationStatus {
    // Las citas tienen que ser evidencias del manifiesto: una cita que
    // no esta en el read-set no se puede comprobar.
    let citations_resolve = citations.iter().all(|citation| {
        read_set.stamps.iter().any(|stamp| {
            stamp.kind == ReadKind::Evidence && stamp.resource == *citation
        })
    });
    if !citations_resolve {
        return VerificationStatus::Unverifiable;
    }

    // Las relaciones canonicas que el enunciado afirma.
    let asserted = the_asserted_relations(statement);
    if asserted.is_empty() {
        // No se puede extraer un predicado conocido: la cita resuelve,
        // pero nadie ha comprobado el enunciado.
        return VerificationStatus::Unverified;
    }

    let mut statuses = Vec::new();
    for (relation, from, to) in asserted {
        let identity = format!("{relation}|{from}|{to}");
        let known = read_set.stamps.iter().any(|stamp| {
            stamp.kind == ReadKind::Relation && stamp.resource == identity
        });
        if known {
            statuses.push(VerificationStatus::Verified);
        } else {
            // El enunciado afirma una relacion que el mundo no tiene:
            // es una contradiccion, y una contradiccion nunca es un
            // hecho verificado, aunque su cita sea autentica.
            statuses.push(VerificationStatus::Contradicted);
        }
    }
    if statuses.contains(&VerificationStatus::Contradicted) {
        VerificationStatus::Contradicted
    } else {
        VerificationStatus::Verified
    }
}

/// Las relaciones canonicas que un enunciado afirma: dos HRNs y una
/// palabra de relacion entre ellos. La extraccion es por patrones
/// fijos, no por criterio difuso.
fn the_asserted_relations(statement: &str) -> Vec<(String, String, String)> {
    let hrns = the_hrn_substrings(statement);
    let lowered = statement.to_lowercase();
    let words = [
        ("depends on", "depends_on"),
        ("depends", "depends_on"),
        ("contains", "contains"),
        ("affected by", "affected_by"),
        ("deployed as", "deployed_as"),
    ];

    let mut asserted = Vec::new();
    for word_index in 0..lowered.len() {
        for (phrase, relation) in words {
            if lowered[word_index..].starts_with(phrase) {
                // Los dos HRNs que rodean la palabra, en el orden en que
                // aparecen.
                let before = &lowered[..word_index];
                let after = &lowered[word_index + phrase.len()..];
                let from = the_last_hrn_before(before, &hrns);
                let to = the_first_hrn_after(after, &hrns);
                if let (Some(from), Some(to)) = (from, to) {
                    asserted.push((relation.to_string(), from, to));
                }
            }
        }
    }
    asserted
}

/// Los HRNs que aparecen en un texto, como cadenas canonicas.
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
    // El texto esta en minusculas: los HRNs se comparan en minusculas
    // tambien, porque "Artifact" y "artifact" son el mismo HRN aqui.
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

/// La clase de finding que corresponde a una afirmacion, dado su estado
/// de verificacion.
fn the_finding_kind(kind: ClaimKind, status: VerificationStatus) -> FindingKind {
    match kind {
        ClaimKind::Fact => {
            if status == VerificationStatus::Verified {
                FindingKind::VerifiedFact
            } else {
                // El texto del modelo con cita resoluble NO es un hecho
                // verificado, y una contradiccion mucho menos.
                FindingKind::EvidenceBackedClaim
            }
        }
        ClaimKind::Inference => FindingKind::Inference,
        ClaimKind::Unknown => FindingKind::Uncertainty,
        ClaimKind::Recommendation => FindingKind::Recommendation,
    }
}

/// En que se apoya el estado del finding: el origen y los limites.
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
            "the citations do not resolve in the read-set: nothing to check against"
                .to_string()
        }
    }
}

/// Construye el informe de una investigacion.
///
/// Las afirmaciones YA VALIDADAS entran aqui: el informe describe lo que
/// dijo el modelo y lo que el verificador pudo comprobar, y NO inventa
/// decisiones autoritativas. Los mismos claims, el mismo read-set y la
/// misma cobertura dan el MISMO informe.
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
        AIClaim::new(statement, kind, citations.iter().map(|one| one.to_string()).collect())
    }

    fn the_world() -> ReadSet {
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", 12);
        read_set.read(crate::read_set::ReadStamp::evidence("evd-1", "sha256:aaa"));
        read_set.read(crate::read_set::ReadStamp::relation(
            "depends_on|hrn:hodei:artifacts:eu-west-1:acme:Artifact/app",
            "evd-1",
        ));
        // La relacion que el mundo tiene: app depende de lib-v1.
        read_set.read(crate::read_set::ReadStamp::relation(
            "depends_on|hrn:hodei:artifacts:eu-west-1:acme:Artifact/app|hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1",
            "evd-1",
        ));
        read_set
    }

    /// AC1: el enunciado que el mundo CONFIRMA, con cita que resuelve,
    /// es un VerifiedFact -- y SOLO el verificador lo otorga.
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

    /// AC1: una cita AUTENTICA que contradice el texto del modelo NO
    /// produce un VerifiedFact: es un EvidenceBackedClaim contradicho.
    #[test]
    fn a_genuine_citation_that_contradicts_the_text_is_not_a_verified_fact() {
        let world = the_world();
        // El modelo dice lib-v2; el mundo tiene lib-v1. La cita es
        // autentica y resuelve.
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

    /// Un hecho sin soporte verificable (cita que no resuelve) falla
    /// cerrado: Unverifiable, nunca Verified.
    #[test]
    fn an_unresolvable_citation_is_unverifiable_not_verified() {
        let world = the_world();
        let claim = claim("the artefact is fine", ClaimKind::Fact, &["evd-unknown"]);
        let report = build_report(&subject(), &[claim], &world, Coverage::Complete);
        assert_eq!(report.findings[0].status, VerificationStatus::Unverifiable);
        assert_eq!(report.findings[0].kind, FindingKind::EvidenceBackedClaim);
    }

    /// Las clases NO se mezclan: una inferencia es inferencia, una
    /// incertidumbre es incertidumbre, una recomendacion es
    /// recomendacion -- con su origen y sus limites.
    #[test]
    fn the_classes_stay_distinct_with_their_limits() {
        let world = the_world();
        let claims = vec![
            claim("the deployment is therefore exposed", ClaimKind::Inference, &["evd-1"]),
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

    /// AC2 + AC3: el informe es serializable, versionado y lleva el
    /// read-set con sus versiones.
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

    /// El replay: los mismos claims y el mismo read-set dan el MISMO
    /// informe, sin relojes ni orden accidental.
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
