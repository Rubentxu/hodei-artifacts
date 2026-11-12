//! The reason codes of a trust decision.
//!
//! A decision that says "the SBOM is missing" in free text is a decision
//! that no program can read, no policy can react to, and no audit can
//! compare. The reason of a decision is therefore a **code** from a closed
//! set, and the text that a person reads is derived from the code plus the
//! evidence that surrounds it.
//!
//! The set is closed on purpose. A reason code that is not in this list is
//! not a reason, and a decision that needed one would say that the reason
//! is unknown -- which is a fact worth knowing.

use serde::{Deserialize, Serialize};

/// Why a decision came out the way it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasonCode {
    // ── Authorization ──
    /// The principal is not allowed to do this.
    AuthzPrincipalNotAllowed,
    /// The principal could not be identified.
    AuthzPrincipalUnknown,
    /// The authorization could not be decided.
    AuthzIndeterminate,

    // ── Evidence that is missing ──
    /// There is no SBOM for the subject.
    TrustSbomMissing,
    /// There is no provenance for the subject.
    TrustProvenanceMissing,
    /// There is no signature for the subject.
    TrustSignatureMissing,
    /// There is no evidence of the integrity of the subject.
    TrustIntegrityMissing,
    /// A required piece of evidence is missing by name.
    EvidenceMissing,

    // ── Evidence that does not hold ──
    /// The provenance was checked and it does not hold.
    TrustProvenanceInvalid,
    /// The signature was not verified.
    TrustSignatureUnverified,
    /// The integrity of the subject was checked and it does not hold.
    TrustIntegrityInvalid,
    /// The inventory does not name every component.
    TrustInventoryIncomplete,
    /// Two pieces of evidence contradict each other.
    TrustEvidenceContradictory,
    /// The evidence is older than the policy allows.
    EvidenceStale,

    // ── Risk ──
    /// A reachable vulnerability of critical severity is open.
    RiskReachableCriticalVulnerability,
    /// The risk could not be established, and the policy does not allow
    /// promoting without knowing it.
    RiskUnknown,
    /// A blocking risk is open.
    RiskPolicyBlocking,

    // ── The decision itself ──
    /// The policy requires an approval that was not given.
    ApprovalRequired,
    /// An exception would have allowed it, and there is no active one.
    ExceptionRequired,
    /// The exception that was used.
    ExceptionApplied,
    /// The policy set could not be loaded.
    PolicyUnavailable,
    /// The trust context could not be built.
    ContextUnavailable,
}

impl ReasonCode {
    /// The code as it travels: a name that is stable and machine readable.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthzPrincipalNotAllowed => "AUTHZ_PRINCIPAL_NOT_ALLOWED",
            Self::AuthzPrincipalUnknown => "AUTHZ_PRINCIPAL_UNKNOWN",
            Self::AuthzIndeterminate => "AUTHZ_INDETERMINATE",
            Self::TrustSbomMissing => "TRUST_SBOM_MISSING",
            Self::TrustProvenanceMissing => "TRUST_PROVENANCE_MISSING",
            Self::TrustSignatureMissing => "TRUST_SIGNATURE_MISSING",
            Self::TrustIntegrityMissing => "TRUST_INTEGRITY_MISSING",
            Self::EvidenceMissing => "EVIDENCE_MISSING",
            Self::TrustProvenanceInvalid => "TRUST_PROVENANCE_INVALID",
            Self::TrustSignatureUnverified => "TRUST_SIGNATURE_UNVERIFIED",
            Self::TrustIntegrityInvalid => "TRUST_INTEGRITY_INVALID",
            Self::TrustInventoryIncomplete => "TRUST_INVENTORY_INCOMPLETE",
            Self::TrustEvidenceContradictory => "TRUST_EVIDENCE_CONTRADICTORY",
            Self::EvidenceStale => "EVIDENCE_STALE",
            Self::RiskReachableCriticalVulnerability => "RISK_REACHABLE_CRITICAL_VULNERABILITY",
            Self::RiskUnknown => "RISK_UNKNOWN",
            Self::RiskPolicyBlocking => "RISK_POLICY_BLOCKING",
            Self::ApprovalRequired => "APPROVAL_REQUIRED",
            Self::ExceptionRequired => "EXCEPTION_REQUIRED",
            Self::ExceptionApplied => "EXCEPTION_APPLIED",
            Self::PolicyUnavailable => "POLICY_UNAVAILABLE",
            Self::ContextUnavailable => "CONTEXT_UNAVAILABLE",
        }
    }

    /// Whether the reason says that something is missing.
    ///
    /// A missing thing is not a bad thing: it is an absence. The two are
    /// told apart because a policy may treat them differently, and a
    /// report that merges them tells the reader less than the facts.
    pub fn is_absence(&self) -> bool {
        matches!(
            self,
            Self::TrustSbomMissing
                | Self::TrustProvenanceMissing
                | Self::TrustSignatureMissing
                | Self::TrustIntegrityMissing
                | Self::EvidenceMissing
        )
    }

    /// Whether the reason is about the authorization and not about the
    /// trust.
    ///
    /// This is what keeps the two halves apart: a decision that was
    /// refused because the principal was not allowed says so, and it never
    /// dresses it up as a problem with the artefact.
    pub fn is_authorization(&self) -> bool {
        matches!(
            self,
            Self::AuthzPrincipalNotAllowed | Self::AuthzPrincipalUnknown | Self::AuthzIndeterminate
        )
    }
}

impl std::fmt::Display for ReasonCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_is_what_travels() {
        assert_eq!(ReasonCode::TrustSbomMissing.as_str(), "TRUST_SBOM_MISSING");
        assert_eq!(
            ReasonCode::TrustSbomMissing.to_string(),
            "TRUST_SBOM_MISSING"
        );
    }

    #[test]
    fn an_absence_is_not_a_failure() {
        assert!(ReasonCode::TrustSbomMissing.is_absence());
        assert!(!ReasonCode::TrustProvenanceInvalid.is_absence());
    }

    #[test]
    fn an_authorization_reason_is_only_about_the_authorization() {
        assert!(ReasonCode::AuthzPrincipalNotAllowed.is_authorization());
        assert!(!ReasonCode::TrustSbomMissing.is_authorization());
    }

    #[test]
    fn the_codes_round_trip_through_json() {
        let code = ReasonCode::RiskReachableCriticalVulnerability;
        let json = serde_json::to_string(&code).expect("the code is serialized");
        assert_eq!(json, "\"RISK_REACHABLE_CRITICAL_VULNERABILITY\"");
        let read: ReasonCode = serde_json::from_str(&json).expect("the code is read");
        assert_eq!(read, code);
    }
}
