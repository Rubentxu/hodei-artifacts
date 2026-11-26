//! Publish-time admission of evidence (HSP-153, ported by HSP-321 tramo i).
//!
//! A hosted repository may **declare** admission in its `config_settings`:
//!
//! ```json
//! "admission": {
//!   "required": ["integrity", "scan"],
//!   "policy_version": "1",
//!   "provider": { "url": "https://scan.internal/report", "id": "scanner", "version": "7" },
//!   "budget": { "timeout_ms": 5000, "max_response_bytes": 1048576, "max_attempts": 3 }
//! }
//! ```
//!
//! After the bytes are in the content store (so the digest is known) and
//! **before** the catalog publication, [`gate`] decides whether the digest may
//! be published. This is not a scanner and it never executes the content: the
//! evidence comes from an isolated, versioned provider, is normalised into
//! transient evidence records, and the **existing** Trust Gate
//! ([`hodei_trust_policy::evaluate`]) over the capsule decides.
//!
//! # Rules
//!
//! - No declaration, no gate: the normal path is unchanged (opt-in).
//! - The provider's report has a closed schema (`deny_unknown_fields`): a
//!   provider that tries to choose the outcome or write canonical facts has its
//!   whole report refused. It must also name the digest it was asked about.
//! - The provider is reached through the service egress policy (the
//!   administrator's allow-list, DNS pinning, no redirects), with a bounded
//!   timeout, bounded body and bounded attempts. A destination that is not in
//!   the allow-list is never contacted.
//! - UNKNOWN is never PASS: a provider that is down, a partial coverage, an
//!   unknown scan verdict, an unreadable declaration or a requirement this
//!   pipeline cannot measure are all `Pending`, and `Pending` **fails closed**
//!   (nothing is published).
//!
//! The verdict only decides; the catalog is not touched on a refusal, so a
//! refused publication leaves no asset, no package version and no reference.

use std::time::Duration;

use hodei_catalog::RepositoryDescriptor;
use hodei_evidence::ContentDigest;
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState, TrustCapsule};
use hodei_evidence::domain::content::ContentRef;
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_trust_policy::{
    OnFailure, Outcome, Requirement, RequirementKind, RiskFacts, TrustContext, TrustDecision,
    TrustPolicy, evaluate,
};
use kernel::Hrn;
use serde::Deserialize;
use url::Url;

use crate::app_state::AppState;
use crate::proxy::fetch::FetchError;
use crate::resolution::{AdmissionRefusal, ResolutionError, the_content_store};

/// The version of the rules; stamped on every refusal reason set.
pub const ADMISSION_PIPELINE_VERSION: &str = "1";

/// Hard ceilings: a repository may narrow its budget, never widen it.
const MAX_TIMEOUT_MS: u64 = 30_000;
const MAX_RESPONSE_BYTES: u64 = 4 << 20;
const MAX_ATTEMPTS: u32 = 5;

/// What the pipeline decided for a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The Trust Gate allowed it: publish.
    Admitted,
    /// The evidence is there and the gate said no: nothing is published.
    Quarantined(Vec<String>),
    /// Tampering is proven (integrity does not hold): nothing is published.
    Denied(Vec<String>),
    /// It could not be known (provider down, partial, unreadable): fail closed.
    Pending(Vec<String>),
}

/// The budget of one evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    /// Total time for one provider attempt.
    pub timeout_ms: u64,
    /// Bytes of provider response accepted.
    pub max_response_bytes: u64,
    /// Provider attempts for one evaluation.
    pub max_attempts: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            max_response_bytes: 1 << 20,
            max_attempts: 3,
        }
    }
}

/// The provider a repository declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Where the report is requested (`POST`).
    pub url: Url,
    /// Who it is.
    pub id: String,
    /// Which version of it, stamped on the decision.
    pub version: String,
}

/// The declaration of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionConfig {
    /// The evidence the repository requires: `integrity`, `scan`.
    pub required: Vec<String>,
    /// The version of the repository's admission policy.
    pub policy_version: String,
    /// The external provider of evidence, if any.
    pub provider: Option<ProviderConfig>,
    /// The evaluation budget.
    pub budget: Budget,
}

impl AdmissionConfig {
    /// Reads the declaration. `Ok(None)` is a repository without admission.
    ///
    /// A declaration that is present but cannot be read is an `Err`, never
    /// `None`: failing open on a badly written policy would be a bypass.
    pub fn of(repository: &RepositoryDescriptor) -> Result<Option<Self>, String> {
        Self::from_settings(&repository.config.config_settings)
    }

    /// Reads the declaration from a repository's `config_settings`.
    pub fn from_settings(settings: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(admission) = settings.get("admission") else {
            return Ok(None);
        };
        let object = admission
            .as_object()
            .ok_or("the admission declaration is not an object")?;
        let required = match object.get("required") {
            Some(list) => list
                .as_array()
                .ok_or("admission.required is not a list")?
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or("admission.required holds a non-string")
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let policy_version = object
            .get("policy_version")
            .and_then(|version| version.as_str())
            .unwrap_or("0")
            .to_string();
        let provider = match object.get("provider") {
            None => None,
            Some(provider) => {
                let field = |name: &str| {
                    provider
                        .get(name)
                        .and_then(|value| value.as_str())
                        .ok_or(format!("admission.provider.{name} is missing"))
                };
                let url = Url::parse(field("url")?)
                    .map_err(|_| "admission.provider.url is not a URL".to_string())?;
                Some(ProviderConfig {
                    url,
                    id: field("id")?.to_string(),
                    version: field("version")?.to_string(),
                })
            }
        };
        let defaults = Budget::default();
        let budget = match object.get("budget") {
            None => defaults,
            Some(budget) => {
                let number = |name: &str, default: u64, ceiling: u64| -> u64 {
                    budget
                        .get(name)
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(default)
                        .min(ceiling)
                };
                Budget {
                    timeout_ms: number("timeout_ms", defaults.timeout_ms, MAX_TIMEOUT_MS).max(1),
                    max_response_bytes: number(
                        "max_response_bytes",
                        defaults.max_response_bytes,
                        MAX_RESPONSE_BYTES,
                    ),
                    max_attempts: number("max_attempts", u64::from(defaults.max_attempts), 5)
                        .clamp(1, u64::from(MAX_ATTEMPTS)) as u32,
                }
            }
        };
        Ok(Some(Self {
            required,
            policy_version,
            provider,
            budget,
        }))
    }

    /// The capsule dimension that carries each supported requirement.
    fn dimension(required: &str) -> Option<DimensionName> {
        match required {
            "integrity" => Some(DimensionName::Integrity),
            "scan" => Some(DimensionName::VulnerabilityRisk),
            _ => None,
        }
    }

    /// The Trust Gate policy: each required evidence is a dimension that must
    /// be `Verified`. No free-form rules live here: the engine decides.
    fn policy(&self) -> TrustPolicy {
        let requirements = self
            .required
            .iter()
            .map(|kind| {
                // A requirement this pipeline cannot measure is a dimension
                // that can never be favourable: the digest stays unadmitted
                // and the reason says so.
                let dimension = Self::dimension(kind)
                    .unwrap_or_else(|| DimensionName::Other(format!("unsupported:{kind}")));
                Requirement {
                    kind: RequirementKind::Dimension {
                        dimension,
                        accepts: vec![DimensionState::Verified],
                    },
                    on_failure: OnFailure::Deny,
                    overridable_by: None,
                }
            })
            .collect();
        TrustPolicy::new("admission", self.policy_version.clone(), requirements)
    }

    fn needs_provider(&self) -> bool {
        self.required.iter().any(|kind| kind == "scan")
    }
}

/// What a provider says. The schema is **closed**: any other field (an
/// attempt to choose the outcome, to write canonical facts) refuses the whole
/// report.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderReport {
    /// The digest the report speaks about; it must be the one asked.
    digest: String,
    /// `complete` or `partial`.
    coverage: String,
    /// `pass`, `fail`, or anything else (UNKNOWN).
    #[serde(default)]
    scan: Option<String>,
}

/// Decides the digest, or refuses the publication.
///
/// `Ok(())` means: publish. Every other outcome is a [`ResolutionError`] that
/// the caller returns **before** touching the catalog.
pub async fn gate(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
    digest: &ContentDigest,
) -> Result<(), ResolutionError> {
    let config = match AdmissionConfig::of(repository) {
        Ok(None) => return Ok(()),
        Ok(Some(config)) => config,
        Err(reason) => return refuse(Verdict::Pending(vec![reason])),
    };
    refuse(evaluate_digest(state, account, repository, digest, &config).await)
}

/// `Ok` only for an admitted digest; every other verdict is a refusal.
fn refuse(verdict: Verdict) -> Result<(), ResolutionError> {
    let (state, reasons) = match verdict {
        Verdict::Admitted => return Ok(()),
        Verdict::Quarantined(reasons) => ("quarantine", reasons),
        Verdict::Denied(reasons) => ("denied", reasons),
        Verdict::Pending(reasons) => ("pending", reasons),
    };
    Err(ResolutionError::Admission(AdmissionRefusal {
        state,
        reasons: reasons.join("; "),
    }))
}

/// The evaluation, without the HTTP mapping.
pub async fn evaluate_digest(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
    digest: &ContentDigest,
    config: &AdmissionConfig,
) -> Verdict {
    let mut flags: Vec<String> = Vec::new();
    let subject = crate::trust::hrn::artifact_hrn(
        repository.hrn.region(),
        account,
        &repository.repo_key,
        digest,
    );
    let tenant = Hrn::with_region(
        "hodei".to_string(),
        "evidence".to_string(),
        "global".to_string(),
        account.to_string(),
        "tenant".to_string(),
        "admission".to_string(),
    );
    let now = chrono::Utc::now();
    let mut evidence: Vec<EvidenceRecord> = Vec::new();

    // 1. Integrity: the bytes that were just written verify against the digest.
    if config.required.iter().any(|kind| kind == "integrity") {
        match the_content_store(state) {
            Ok(store) => match store.verify(digest).await {
                Ok(()) => evidence.push(record(&tenant, &subject, "integrity", true, now)),
                Err(hodei_hub::blob::BlobError::Unavailable(_)) => {
                    flags.push("the content store could not verify integrity".to_string());
                }
                Err(_) => {
                    flags.push("the stored bytes do not verify against the digest".to_string());
                    evidence.push(record(&tenant, &subject, "integrity", false, now));
                }
            },
            Err(_) => flags.push("there is no content store to verify integrity".to_string()),
        }
    }

    // 2. The provider's evidence, under budget.
    let mut coverage = String::new();
    if config.needs_provider() {
        match ask_provider(state, account, repository, digest, config).await {
            Ok(report) => {
                coverage = report.coverage.clone();
                match report.coverage.as_str() {
                    "complete" => {}
                    "partial" => flags.push("the coverage is partial: UNKNOWN is not PASS".into()),
                    other => flags.push(format!("the coverage {other:?} is not a coverage")),
                }
                match report.scan.as_deref() {
                    Some("pass") => evidence.push(record(&tenant, &subject, "scan", true, now)),
                    Some("fail") => evidence.push(record(&tenant, &subject, "scan", false, now)),
                    _ => flags.push("the scan verdict is UNKNOWN, not PASS".to_string()),
                }
            }
            Err(reasons) => flags.extend(reasons),
        }
    }

    // 3. The capsule and the existing Trust Gate decide.
    let capsule = CapsuleAssembler::new().assemble(&subject, &evidence, now);
    let context = TrustContext::builder(&capsule)
        .with_digest(digest.to_string())
        .with_risk(RiskFacts::default())
        .assembled_at(now)
        .build();
    let decision = evaluate(&context, &config.policy());
    let mut reasons: Vec<String> = decision
        .requirements
        .iter()
        .filter(|requirement| !requirement.satisfied)
        .map(|requirement| {
            format!(
                "{}:{}",
                requirement.requirement,
                requirement
                    .reason
                    .map(|reason| reason.as_str())
                    .unwrap_or("UNSATISFIED")
            )
        })
        .collect();
    reasons.extend(flags);
    translate(&decision, &capsule, &coverage, reasons)
}

/// The Trust Gate outcome as an admission verdict.
///
/// - `Allow`/`Warn` -> admitted, unless coverage is partial (UNKNOWN is not PASS).
/// - `Deny` with proven tampering (integrity invalid) -> denied.
/// - `Deny` with every unsatisfied requirement an absence -> pending.
/// - any other `Deny` -> quarantined: the evidence is there and says no.
/// - `Indeterminate`/`RequireApproval` -> pending.
fn translate(
    decision: &TrustDecision,
    capsule: &TrustCapsule,
    coverage: &str,
    reasons: Vec<String>,
) -> Verdict {
    match decision.outcome {
        Outcome::Allow | Outcome::Warn => {
            if coverage == "partial" {
                Verdict::Pending(reasons)
            } else {
                Verdict::Admitted
            }
        }
        Outcome::Deny => {
            let tampered = capsule.dimensions().iter().any(|dimension| {
                dimension.name == DimensionName::Integrity
                    && matches!(
                        dimension.state,
                        DimensionState::Invalid | DimensionState::Contradictory
                    )
            });
            if tampered {
                return Verdict::Denied(reasons);
            }
            let unsatisfied: Vec<_> = decision
                .requirements
                .iter()
                .filter(|requirement| !requirement.satisfied)
                .collect();
            if !unsatisfied.is_empty()
                && unsatisfied
                    .iter()
                    .all(|requirement| requirement.reason.is_some_and(|reason| reason.is_absence()))
            {
                Verdict::Pending(reasons)
            } else {
                Verdict::Quarantined(reasons)
            }
        }
        Outcome::Indeterminate | Outcome::RequireApproval => Verdict::Pending(reasons),
    }
}

/// The only request to the provider, with all its limits. Transient failures
/// are retried up to the budget; a refusal of egress or an out-of-schema
/// report is final.
async fn ask_provider(
    state: &AppState,
    account: &str,
    repository: &RepositoryDescriptor,
    digest: &ContentDigest,
    config: &AdmissionConfig,
) -> Result<ProviderReport, Vec<String>> {
    let Some(provider) = &config.provider else {
        return Err(vec!["the repository declares no provider".to_string()]);
    };
    let Some(plane) = state.proxy_plane.as_ref() else {
        return Err(vec![
            "the service has no egress to reach the provider".to_string(),
        ]);
    };
    let request = serde_json::json!({
        "digest": digest.to_string(),
        "repository": repository.repo_key,
        "account": account,
    });
    let timeout = Duration::from_millis(config.budget.timeout_ms);
    let mut last = String::new();
    for _ in 0..config.budget.max_attempts {
        match plane
            .fetcher
            .post_json(
                &provider.url,
                &request,
                timeout,
                config.budget.max_response_bytes,
            )
            .await
        {
            Ok(bytes) => {
                let report = serde_json::from_slice::<ProviderReport>(&bytes).map_err(|error| {
                    vec![format!(
                        "the provider report is out of schema ({error}): a provider cannot choose the outcome nor write canonical facts"
                    )]
                })?;
                if report.digest != digest.to_string() {
                    return Err(vec![
                        "the provider report does not speak about this digest".to_string(),
                    ]);
                }
                return Ok(report);
            }
            // Never retried: the destination is not allowed, or the body is over budget.
            Err(error @ (FetchError::Egress(_) | FetchError::Offline | FetchError::TooLarge)) => {
                return Err(vec![format!(
                    "the provider {}@{} is not usable: {error}",
                    provider.id, provider.version
                )]);
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    Err(vec![format!(
        "the provider {}@{} did not give a report after {} attempts ({last})",
        provider.id, provider.version, config.budget.max_attempts
    )])
}

/// A transient evidence record: it lives as long as the evaluation. This path
/// does not write the evidence graph; it only normalises and measures.
fn record(
    tenant: &Hrn,
    subject: &Hrn,
    kind: &str,
    holds: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> EvidenceRecord {
    let (evidence_type, method) = match kind {
        "integrity" => (EvidenceType::ArtifactIntegrity, "digest-verification"),
        _ => (
            EvidenceType::Other("scan_result".to_string()),
            "binary-scan",
        ),
    };
    let content = format!("{kind}:{holds}");
    let digest = ContentDigest::of_bytes(content.as_bytes());
    let id = EvidenceId::new(format!(
        "adm-{}",
        &ContentDigest::of_bytes(format!("{subject}{method}{}", digest.value).as_bytes()).value
            [..24]
    ));
    let verification = if holds {
        EvidenceVerification::verified(method)
    } else {
        EvidenceVerification::invalid(method)
    };
    EvidenceRecord::builder(id, tenant.clone())
        .subjects(vec![subject.clone()])
        .evidence_type(evidence_type)
        .producer(Producer::new("admission-pipeline", "hodei"))
        .source(SourceRef::none())
        .content(ContentRef::by_digest(digest))
        .observed_at(now)
        .recorded_at(now)
        .verification(verification)
        .normalization(Normalization::new(
            "hodei-evidence/standards",
            ADMISSION_PIPELINE_VERSION,
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hodei_catalog::{DigestScope, RepositoryConfig, RepositoryFormat, RepositoryMode};

    fn repository(settings: serde_json::Value) -> RepositoryDescriptor {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = settings;
        RepositoryDescriptor {
            hrn: hodei_catalog::repository_hrn("hodei", "artifacts", "eu", "acme", "lab"),
            account: "acme".to_string(),
            repo_key: "lab".to_string(),
            format: RepositoryFormat::Raw,
            mode: RepositoryMode::Hosted,
            config,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn no_declaration_means_no_gate_and_a_broken_one_is_an_error_not_a_bypass() {
        assert_eq!(
            AdmissionConfig::of(&repository(serde_json::json!({}))),
            Ok(None)
        );
        for broken in [
            serde_json::json!({"admission": "yes"}),
            serde_json::json!({"admission": {"required": "scan"}}),
            serde_json::json!({"admission": {"required": [1]}}),
            serde_json::json!({"admission": {"provider": {"url": "nope", "id": "a", "version": "1"}}}),
            serde_json::json!({"admission": {"provider": {"id": "a", "version": "1"}}}),
        ] {
            assert!(
                AdmissionConfig::of(&repository(
                    serde_json::json!({ "admission": broken["admission"] })
                ))
                .is_err(),
                "{broken}"
            );
        }
    }

    #[test]
    fn a_repository_can_narrow_its_budget_but_never_widen_it() {
        let config = AdmissionConfig::of(&repository(serde_json::json!({"admission": {
            "required": ["scan"],
            "budget": {"timeout_ms": 999999999, "max_response_bytes": 999999999999u64, "max_attempts": 99}
        }})))
        .unwrap()
        .unwrap();
        assert_eq!(config.budget.timeout_ms, MAX_TIMEOUT_MS);
        assert_eq!(config.budget.max_response_bytes, MAX_RESPONSE_BYTES);
        assert_eq!(config.budget.max_attempts, MAX_ATTEMPTS);
    }

    #[test]
    fn the_provider_report_has_a_closed_schema() {
        let choosing =
            br#"{"digest":"sha256:a","coverage":"complete","scan":"pass","outcome":"allow"}"#;
        assert!(serde_json::from_slice::<ProviderReport>(choosing).is_err());
        let honest = br#"{"digest":"sha256:a","coverage":"complete","scan":"pass"}"#;
        assert!(serde_json::from_slice::<ProviderReport>(honest).is_ok());
    }

    /// The Trust Gate outcome is translated, not re-decided here: tampering is
    /// `Denied`, evidence that says no is `Quarantined`, a missing piece is
    /// `Pending`, and only all-verified is `Admitted`.
    #[test]
    fn the_gate_outcome_becomes_denied_quarantined_pending_or_admitted() {
        let config = AdmissionConfig::from_settings(&serde_json::json!({"admission": {
            "required": ["integrity", "scan"]}}))
        .unwrap()
        .unwrap();
        let verdict = |integrity: Option<bool>, scan: Option<bool>| {
            let repository = repository(serde_json::json!({}));
            let digest = ContentDigest::of_bytes(b"x");
            let subject =
                crate::trust::hrn::artifact_hrn("eu", "acme", &repository.repo_key, &digest);
            let tenant = Hrn::with_region(
                "hodei".into(),
                "evidence".into(),
                "global".into(),
                "acme".into(),
                "tenant".into(),
                "admission".into(),
            );
            let now = Utc::now();
            let mut evidence = Vec::new();
            if let Some(holds) = integrity {
                evidence.push(record(&tenant, &subject, "integrity", holds, now));
            }
            if let Some(holds) = scan {
                evidence.push(record(&tenant, &subject, "scan", holds, now));
            }
            let capsule = CapsuleAssembler::new().assemble(&subject, &evidence, now);
            let context = TrustContext::builder(&capsule)
                .with_digest(digest.to_string())
                .with_risk(RiskFacts::default())
                .assembled_at(now)
                .build();
            let decision = evaluate(&context, &config.policy());
            translate(&decision, &capsule, "complete", vec![])
        };
        assert_eq!(verdict(Some(true), Some(true)), Verdict::Admitted);
        assert!(matches!(
            verdict(Some(false), Some(true)),
            Verdict::Denied(_)
        ));
        assert!(matches!(
            verdict(Some(true), Some(false)),
            Verdict::Quarantined(_)
        ));
        assert!(matches!(verdict(Some(true), None), Verdict::Pending(_)));
        assert!(matches!(verdict(None, None), Verdict::Pending(_)));
    }
}
