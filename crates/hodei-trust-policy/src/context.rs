//! The context that a trust policy is evaluated against.
//!
//! A trust policy does not go and ask. It receives a **context that was
//! assembled on purpose**: the capsule of the subject, the facts of the
//! risk, the state of the exceptions and the identity of the request. The
//! policy reads the context and decides, and it never touches a database,
//! a scanner or a graph.
//!
//! That is what makes the decision reproducible. The same context and the
//! same policy give the same decision, and the context says which version
//! of the builder assembled it, so a decision made six months ago can be
//! read with the rules that made it.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::capsule::{DimensionName, DimensionState, TrustCapsule};
use serde::{Deserialize, Serialize};

/// The version of the builder of the context.
///
/// It is part of the decision trace, because a context built by another
/// version is another context, and a decision that does not say which one
/// built it cannot be replayed.
pub const TRUST_CONTEXT_VERSION: &str = "1.0.0";

/// The environment a promotion targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetEnvironment {
    /// A staging environment.
    Staging,
    /// A production environment.
    Production,
}

impl TargetEnvironment {
    /// The name of the environment.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

/// How exposed a deployment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Exposure {
    /// Not reachable from outside.
    Internal,
    /// Reachable from inside the organisation.
    Network,
    /// Reachable from the internet.
    Internet,
    /// Not known.
    Unknown,
}

/// The facts of the risk, computed before the policy is evaluated.
///
/// The reachability and the severity are **inputs** and not something the
/// policy calculates: a policy that queried a graph would be a policy
/// whose decision depends on when it was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskFacts {
    /// The vulnerabilities of critical severity that are known.
    pub critical_known: u32,
    /// The ones of critical severity that are reachable.
    pub critical_reachable: u32,
    /// How exposed the deployment is.
    pub exposure: Exposure,
    /// The statements that were accepted without fixing the finding.
    pub vex_accepted: u32,
    /// Whether the risk could be established at all.
    pub established: bool,
}

impl Default for RiskFacts {
    fn default() -> Self {
        Self {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Unknown,
            vex_accepted: 0,
            established: false,
        }
    }
}

/// An exception that a decision may lean on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionFact {
    /// The identifier of the exception.
    pub id: String,
    /// The name the policy gives it.
    ///
    /// The policy names the exception it accepts, and the exception
    /// carries that name. A requirement is only waived by the exception
    /// that was named for it, and not by any exception that happens to be
    /// active.
    pub name: String,
    /// When it stops being usable.
    pub expires_at: DateTime<Utc>,
    /// Whether it applies to this subject.
    pub applies_to_subject: bool,
}

impl ExceptionFact {
    /// Whether the exception is usable at a moment.
    ///
    /// An exception that expired is not usable, and it cannot be reused:
    /// the decision that leans on it has to say that it was active at the
    /// moment it was made, and that moment is now.
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.applies_to_subject && self.expires_at > now
    }
}

/// The subject a decision is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectFact {
    /// The HRN of the subject.
    pub hrn: String,
    /// The digest of the subject.
    pub digest: Option<String>,
}

/// The context a trust policy is evaluated against.
///
/// It is a value and not a handle: once it is built, it does not change,
/// and two contexts with the same content are the same context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustContext {
    /// The version of the builder that assembled it.
    pub version: String,
    /// The subject.
    pub subject: SubjectFact,
    /// The environment the promotion targets.
    pub target: TargetEnvironment,
    /// The state of each dimension of the capsule.
    pub dimensions: Vec<DimensionFact>,
    /// The hash of the capsule the dimensions came from.
    pub capsule_hash: Option<String>,
    /// The facts of the risk.
    pub risk: RiskFacts,
    /// The exceptions that could apply.
    pub exceptions: Vec<ExceptionFact>,
    /// The identifiers of the evidence the dimensions came from.
    pub evidence_ids: Vec<String>,
    /// The moment the context was assembled.
    pub assembled_at: DateTime<Utc>,
}

/// The state of one dimension, in the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionFact {
    /// The dimension.
    pub name: DimensionName,
    /// Its state.
    pub state: DimensionState,
}

impl TrustContext {
    /// Assembles the context from a capsule and the facts around it.
    ///
    /// The dimensions come from the capsule, so the context cannot say
    /// something the capsule does not say.
    pub fn builder(capsule: &TrustCapsule) -> TrustContextBuilder {
        TrustContextBuilder {
            version: TRUST_CONTEXT_VERSION.to_string(),
            subject: SubjectFact {
                hrn: capsule.subject.to_string(),
                digest: None,
            },
            target: TargetEnvironment::Production,
            dimensions: capsule
                .profile
                .dimensions
                .iter()
                .map(|dimension| DimensionFact {
                    name: dimension.name.clone(),
                    state: dimension.state,
                })
                .collect(),
            capsule_hash: Some(capsule.content_hash.to_string()),
            risk: RiskFacts::default(),
            exceptions: Vec::new(),
            evidence_ids: capsule
                .profile
                .dimensions
                .iter()
                .flat_map(|dimension| dimension.evidence.iter())
                .map(|link| link.evidence_id.to_string())
                .collect(),
            assembled_at: Utc::now(),
        }
    }

    /// The state of a dimension, if the context carries it.
    pub fn state_of(&self, name: &DimensionName) -> Option<DimensionState> {
        self.dimensions
            .iter()
            .find(|dimension| &dimension.name == name)
            .map(|dimension| dimension.state)
    }

    /// The exceptions that are usable at a moment.
    pub fn active_exceptions(&self, now: DateTime<Utc>) -> Vec<&ExceptionFact> {
        self.exceptions
            .iter()
            .filter(|exception| exception.is_active_at(now))
            .collect()
    }
}

/// The builder of a trust context.
///
/// It is explicit and it is versioned: the version it carries is the
/// version of the rules that assembled it, and it is written into the
/// context so that a decision can name it.
#[derive(Debug, Clone)]
pub struct TrustContextBuilder {
    version: String,
    subject: SubjectFact,
    target: TargetEnvironment,
    dimensions: Vec<DimensionFact>,
    capsule_hash: Option<String>,
    risk: RiskFacts,
    exceptions: Vec<ExceptionFact>,
    evidence_ids: Vec<String>,
    assembled_at: DateTime<Utc>,
}

impl TrustContextBuilder {
    /// The environment the promotion targets.
    pub fn targeting(mut self, target: TargetEnvironment) -> Self {
        self.target = target;
        self
    }

    /// The digest of the subject.
    pub fn with_digest(mut self, digest: impl Into<String>) -> Self {
        self.subject.digest = Some(digest.into());
        self
    }

    /// The facts of the risk.
    pub fn with_risk(mut self, risk: RiskFacts) -> Self {
        self.risk = risk;
        self
    }

    /// The exceptions that could apply.
    pub fn with_exceptions(mut self, exceptions: Vec<ExceptionFact>) -> Self {
        self.exceptions = exceptions;
        self
    }

    /// The moment the context is assembled at.
    pub fn assembled_at(mut self, moment: DateTime<Utc>) -> Self {
        self.assembled_at = moment;
        self
    }

    /// The context.
    pub fn build(self) -> TrustContext {
        TrustContext {
            version: self.version,
            subject: self.subject,
            target: self.target,
            dimensions: self.dimensions,
            capsule_hash: self.capsule_hash,
            risk: self.risk,
            exceptions: self.exceptions,
            evidence_ids: self.evidence_ids,
            assembled_at: self.assembled_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exception_that_expired_is_not_active() {
        let now = Utc::now();
        let expired = ExceptionFact {
            id: "ex-1".to_string(),
            name: "sbom-waiver".to_string(),
            expires_at: now - chrono::Duration::hours(1),
            applies_to_subject: true,
        };
        assert!(!expired.is_active_at(now));

        let live = ExceptionFact {
            expires_at: now + chrono::Duration::hours(1),
            ..expired.clone()
        };
        assert!(live.is_active_at(now));

        let other = ExceptionFact {
            applies_to_subject: false,
            ..live
        };
        assert!(!other.is_active_at(now));
    }

    #[test]
    fn a_missing_dimension_is_not_a_state() {
        let context = TrustContext {
            version: TRUST_CONTEXT_VERSION.to_string(),
            subject: SubjectFact {
                hrn: "hrn:hodei:artifacts:eu-west-1:acme:Artifact/web".to_string(),
                digest: None,
            },
            target: TargetEnvironment::Production,
            dimensions: Vec::new(),
            capsule_hash: None,
            risk: RiskFacts::default(),
            exceptions: Vec::new(),
            evidence_ids: Vec::new(),
            assembled_at: Utc::now(),
        };
        assert_eq!(context.state_of(&DimensionName::Integrity), None);
    }
}
