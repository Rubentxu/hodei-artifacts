//! Trust grants: which digest the trusted channel may serve.
//!
//! A repository serves its content through one of two channels:
//!
//! - **`open`** (default): what is published is served to whoever Cedar lets
//!   read. It is the quarantine channel: the content exists and can be
//!   queried, but nobody has said it is trustworthy.
//! - **`trusted`**: only a digest that has an **active grant**
//!   is served, that is, a promotion decision that advanced and that nobody
//!   has revoked afterwards. Everything else responds as if it did not exist.
//!
//! The grant is **per digest**, not per name. A tag that
//! moves points to another digest, and that digest inherits nothing: yesterday's decision
//! about `sha256:X` says nothing about `sha256:Y`.
//!
//! # What a grant says about itself
//!
//! Each new grant carries the **validity** it was born with: the evaluated
//! subject, the version of the policy that decided, the hash of the evidence
//! capsule and the version of the risk that were consulted, and the moment
//! until which it is trustworthy (`valid_until`). With that a grant can
//! be audited and revalidated without asking the past again; without that
//! it is **not active**: the rows written before this contract remain in
//! the database as history, but the trusted channel does not serve them until a new
//! promotion revalidates them. Failing closed is the migration.
//!
//! # Why it is read live
//!
//! Every request to the trusted channel asks the ledger **with the clock of
//! now**: a revocation takes effect on the next request, an
//! expired validity stops serving without anyone revoking it, and any
//! ledger error is an error: it is **never** turned into a permission.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::{CatalogError, RepositoryConfig};

/// The channel through which a repository serves its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServeChannel {
    /// What is published is served, regardless of trust.
    Open,
    /// Only a digest with an active grant is served.
    Trusted,
}

impl ServeChannel {
    /// The channel a repository's configuration declares.
    ///
    /// It is read from `config_settings.trust.serve`. A repository without the
    /// `trust` key is open. A `trust` key that exists but is not
    /// understood (another type, another value) is **trusted**: an unreadable
    /// security setting fails on the closed side, not the open one.
    pub fn of(config: &RepositoryConfig) -> Self {
        let Some(trust) = config.config_settings.get("trust") else {
            return Self::Open;
        };
        match trust.get("serve").and_then(|serve| serve.as_str()) {
            Some("open") => Self::Open,
            _ => Self::Trusted,
        }
    }

    /// The stable name of the channel, as it goes in the configuration.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Trusted => "trusted",
        }
    }

    /// The channel that says its own name.
    ///
    /// The name is the one that goes in the configuration: `open` or
    /// `trusted`. A name that does not exist is not a channel: it is a
    /// text nobody can interpret, and a channel that cannot be read is
    /// the closed one.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "open" => Some(Self::Open),
            "trusted" => Some(Self::Trusted),
            _ => None,
        }
    }
}

/// The grant of a digest within a repository, by a decision.
///
/// The grant **does not duplicate** the decision: it names it with its
/// `decision_id` and keeps only the five data that are needed to
/// judge its validity today. Everything else --the reasons, the requirements, the
/// outcome-- is read from the persisted decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustGrant {
    /// The account that owns the repository.
    pub account: String,
    /// The repository.
    pub repository: String,
    /// The granted content.
    pub digest: ContentDigest,
    /// The promotion decision that granted it.
    pub decision_id: String,
    /// Who promoted.
    pub actor: String,
    /// When it was granted.
    pub granted_at: DateTime<Utc>,
    /// When it was revoked, if it was revoked.
    pub revoked_at: Option<DateTime<Utc>>,
    /// Who revoked.
    pub revoked_by: Option<String>,
    /// Why it was revoked.
    pub revoke_reason: Option<String>,
    /// The subject (HRN) the decision evaluated, if the grant carries
    /// validity. A grant prior to this contract does not carry it.
    #[serde(default)]
    pub subject: Option<String>,
    /// The version of the policy it was decided with, if the grant
    /// carries validity.
    #[serde(default)]
    pub policy_version: Option<String>,
    /// The hash of the evidence capsule consulted, if the decision had
    /// one. It identifies the evidence without duplicating it: the detail lives in the
    /// persisted decision.
    #[serde(default)]
    pub capsule_hash: Option<String>,
    /// The version of the risk registry that was consulted, if it was consulted
    /// with validity. Zero is an honest value: nobody's registry.
    #[serde(default)]
    pub risk_version: Option<u64>,
    /// Until when this grant is trustworthy, if it carries one. Without a
    /// declared validity there is no active grant: it is the closed side of
    /// the migration.
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
}

impl TrustGrant {
    /// Whether the grant is still in force **at `now`**.
    ///
    /// Three conditions and no charity: not being revoked, carrying the
    /// complete validity (the version of the policy that decided) and not having
    /// expired. A grant without validity --written before this
    /// contract or half-written-- is not active: without proof of validity it is not
    /// served, and revalidating it means promoting again.
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none()
            && self.policy_version.is_some()
            && self.valid_until.is_some_and(|until| until > now)
    }
}

/// The grant ledger.
///
/// It does not authorize: whoever decides whether a person can promote or revoke is the
/// service's gate, before getting here.
#[async_trait]
pub trait TrustLedger: Send + Sync {
    /// Writes the grants of a decision, all or none.
    ///
    /// Repeating a grant (same account, repository, digest and
    /// decision) leaves it as it was: the operation is idempotent, and a
    /// revoked grant is **not** reactivated by repeating it; to grant again
    /// a new decision is needed.
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError>;

    /// Revokes all the grants of a decision. Returns how many
    /// were active.
    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError>;

    /// Revokes all the grants of a decision across the whole account,
    /// without knowing in which repository they live.
    ///
    /// It is the revocation that re-evaluation asks for: the truth about the risk
    /// changed and the decision that granted no longer holds, but whoever re-evaluates
    /// names the subject and its account, not the repositories where that
    /// decision promoted. Returns how many were active.
    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError>;

    /// Revokes all the grants of a digest, whichever decision
    /// granted them. Returns how many were active.
    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError>;

    /// An active grant of that digest in that repository **at
    /// `at`**, if there is one.
    ///
    /// The time is a parameter, not an internal trap: whoever asks
    /// decides what now is, and so the clock can be injected in
    /// a test. The unfiltered ledger answers validity as the guard does.
    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError>;

    /// All the grants of a repository, active or revoked.
    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError>;

    /// Which of these digests have an active grant in the repository at
    /// `at`.
    ///
    /// It is the question a listing asks for a whole page, and it has to
    /// cost what the page costs, not what the repository holds. The
    /// default asks [`TrustLedger::active_grant`] one digest at a time; a
    /// ledger that can answer the page in one bounded read overrides it.
    /// The answer follows the same rule as `active_grant`: revoked,
    /// expired and unproven grants are not active.
    async fn active_digests(
        &self,
        account: &str,
        repository: &str,
        digests: &[ContentDigest],
        at: DateTime<Utc>,
    ) -> Result<Vec<ContentDigest>, CatalogError> {
        let mut active = Vec::new();
        for digest in digests {
            if self
                .active_grant(account, repository, digest, at)
                .await?
                .is_some()
            {
                active.push(digest.clone());
            }
        }
        Ok(active)
    }
}

/// A concession the validity sweep looks at: the row, with what the
/// renewal needs and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantSweepRow {
    /// The decision that granted it.
    pub decision_id: String,
    /// The account it belongs to.
    pub account: String,
    /// The repository it covers.
    pub repository: String,
    /// The algorithm of the digest it covers, as stored (`sha256`).
    pub digest_algorithm: String,
    /// The value of the digest it covers.
    pub digest: String,
    /// The subject the decision evaluated, when the grant carries the
    /// validity contract.
    pub subject: Option<String>,
    /// The version of the policy that decided it.
    pub policy_version: Option<String>,
    /// The instant it stops being valid.
    pub valid_until: Option<DateTime<Utc>>,
}

/// The question the validity sweep asks of the book: which concessions
/// are not revoked yet, across every account.
///
/// It is not part of [`TrustLedger`] because only the sweep needs it and
/// the serving path must not grow a way to enumerate the book.
#[async_trait]
pub trait GrantSweepSource: Send + Sync {
    /// The concessions nobody revoked, ordered by account, repository and
    /// digest, at most `limit` of them. The bound keeps a pass finite: the
    /// rest belongs to the next one.
    async fn unrevoked_grants(&self, limit: usize) -> Result<Vec<GrantSweepRow>, CatalogError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DigestScope;
    use chrono::Duration;
    use serde_json::json;

    fn a_grant(now: DateTime<Utc>) -> TrustGrant {
        TrustGrant {
            account: "acme".to_string(),
            repository: "raw".to_string(),
            digest: ContentDigest::of_bytes(b"the bytes"),
            decision_id: "dec-1".to_string(),
            actor: "promoter".to_string(),
            granted_at: now,
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_string()),
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: Some("sha256:cap".to_string()),
            risk_version: Some(7),
            valid_until: Some(now + Duration::hours(1)),
        }
    }

    #[test]
    fn a_grant_with_full_validity_is_active_until_it_expires() {
        let now = Utc::now();
        let grant = a_grant(now);
        assert!(grant.is_active_at(now));
        assert!(
            grant.is_active_at(now + Duration::minutes(59)),
            "inside the horizon the promise still holds"
        );
        assert!(
            !grant.is_active_at(now + Duration::hours(1)),
            "valid_until is exclusive: at the deadline the grant is over"
        );
        assert!(!grant.is_active_at(now + Duration::hours(2)));
    }

    #[test]
    fn a_revocation_ends_the_validity_it_has() {
        let now = Utc::now();
        let mut grant = a_grant(now);
        grant.revoked_at = Some(now);
        assert!(!grant.is_active_at(now));
    }

    /// Migration AC: validity is not presumed. Without a policy version
    /// or without `valid_until` there is no active grant, however clean
    /// the rest of the row is.
    #[test]
    fn a_grant_without_declared_validity_is_never_active() {
        let now = Utc::now();
        let mut no_version = a_grant(now);
        no_version.policy_version = None;
        assert!(!no_version.is_active_at(now));

        let mut no_horizon = a_grant(now);
        no_horizon.valid_until = None;
        assert!(!no_horizon.is_active_at(now));

        let mut legacy = a_grant(now);
        legacy.subject = None;
        legacy.capsule_hash = None;
        legacy.risk_version = None;
        legacy.policy_version = None;
        legacy.valid_until = None;
        assert!(
            !legacy.is_active_at(now),
            "a row from before the contract stays out of trusted until revalidated"
        );
    }

    #[test]
    fn the_validity_fields_are_optional_on_the_wire() {
        let mut legacy = serde_json::to_value(a_grant(Utc::now())).expect("a grant serializes");
        let fields = legacy.as_object_mut().expect("a grant is an object");
        for name in [
            "subject",
            "policy_version",
            "capsule_hash",
            "risk_version",
            "valid_until",
        ] {
            fields.remove(name);
        }
        let grant: TrustGrant = serde_json::from_value(legacy).expect("it parses");
        assert!(grant.policy_version.is_none());
        assert!(grant.valid_until.is_none());
        assert!(!grant.is_active_at(Utc::now()));
    }

    fn config(settings: serde_json::Value) -> RepositoryConfig {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = settings;
        config
    }

    #[test]
    fn a_repository_without_trust_settings_is_open() {
        assert_eq!(ServeChannel::of(&config(json!({}))), ServeChannel::Open);
    }

    #[test]
    fn the_trusted_channel_is_declared_by_name() {
        let trusted = config(json!({ "trust": { "serve": "trusted" } }));
        assert_eq!(ServeChannel::of(&trusted), ServeChannel::Trusted);
        let open = config(json!({ "trust": { "serve": "open" } }));
        assert_eq!(ServeChannel::of(&open), ServeChannel::Open);
    }

    #[test]
    fn a_trust_setting_nobody_can_read_closes_the_channel() {
        for settings in [
            json!({ "trust": "trusted" }),
            json!({ "trust": {} }),
            json!({ "trust": { "serve": 1 } }),
            json!({ "trust": { "serve": "everybody" } }),
            json!({ "trust": { "serve": "TRUSTED" } }),
            json!({ "trust": null }),
        ] {
            assert_eq!(
                ServeChannel::of(&config(settings.clone())),
                ServeChannel::Trusted,
                "{settings}"
            );
        }
    }

    #[test]
    fn a_channel_is_read_from_its_own_name() {
        assert_eq!(ServeChannel::parse("open"), Some(ServeChannel::Open));
        assert_eq!(ServeChannel::parse("trusted"), Some(ServeChannel::Trusted));
        for bad in ["", "OPEN", "everybody", "open "] {
            assert_eq!(ServeChannel::parse(bad), None, "{bad:?} is not a channel");
        }
    }

    #[test]
    fn a_channel_goes_on_the_wire_with_the_name_it_is_read_with() {
        assert_eq!(
            serde_json::to_string(&ServeChannel::Open).expect("json"),
            "\"open\""
        );
        assert_eq!(
            serde_json::to_string(&ServeChannel::Trusted).expect("json"),
            "\"trusted\""
        );
    }
}
