//! Rollback and freeze protection for the signed index of a mirror (HSP-167).
//!
//! A signed `Release` proves *who* wrote the index, not *when*: an attacker
//! (or a broken mirror) that replays an older, still validly signed `Release`
//! pins clients to vulnerable packages, and an upstream that silently stops
//! publishing keeps an old signature "valid" forever. This module is the
//! TUF-style countermeasure, kept to what the apt proxy can honestly check:
//!
//! - the signature is verified **before** anything here runs, against the key
//!   the operator pinned in the repository configuration
//!   (`apt.trusted_key_asc`), never against a key the upstream announces;
//! - the `Date` of the signed `Release` is the monotonic version: the highest
//!   accepted `Date` per (account, repository, distribution) is persisted,
//!   and an older `Date` is a rollback; the same `Date` with different
//!   content is a conflict (a signed fork), and both are refused;
//! - a `Release` past its `Valid-Until`, or older than the operator's
//!   `apt.max_metadata_age_secs`, is a freeze and is refused, as is a `Date`
//!   from the future (it would lock out every genuine successor);
//! - every failure of the ledger fails closed: when the mark cannot be read
//!   or advanced the index is not served.
//!
//! No crypto lives here: the signature check is the OpenPGP one of the apt
//! surface. The decision ([`judge`]) is pure; only [`admit`] touches the
//! ledger port, and the persisted form is the Dgraph ledger of
//! [`super::dgraph_metadata_ledger`]. The ledger is append-only: each advance
//! is a new row keyed by its sequence number, so two racing writers cannot
//! both claim the same step, and the history of accepted `Date`s is kept.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

/// The most a `Date` may lie ahead of the clock before it is refused.
pub const MAX_CLOCK_SKEW_SECS: i64 = 300;

/// How many times [`admit`] re-reads after losing a race to another writer.
const MAX_ATTEMPTS: usize = 4;

/// What the guard tracks: one signed index lineage.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetadataKey {
    /// The owning account.
    pub account: String,
    /// The proxy repository.
    pub repository: String,
    /// The distribution (`bookworm`, ...).
    pub distribution: String,
}

impl MetadataKey {
    /// The scoped subject the ledger indexes by.
    ///
    /// Each part is escaped (`%` and `/`), so no two different triples can
    /// ever share a subject.
    pub fn subject(&self) -> String {
        let escape = |part: &str| part.replace('%', "%25").replace('/', "%2F");
        format!(
            "{}/{}/{}",
            escape(&self.account),
            escape(&self.repository),
            escape(&self.distribution)
        )
    }
}

/// The highest accepted state of a lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// The `Date` of the accepted `Release`.
    pub date: DateTime<Utc>,
    /// The digest of its content (`sha256:<hex>` of the verified body).
    pub digest: String,
}

/// The head of a lineage ledger.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Head {
    /// The sequence of the last row (0 when the lineage was never seen).
    pub seq: u64,
    /// The baseline; `None` for a new lineage or right after an explicit reset.
    pub baseline: Option<Baseline>,
    /// When the baseline was accepted.
    pub accepted_at: Option<DateTime<Utc>>,
}

/// One step appended to a lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A verified `Release` newer than the baseline.
    Accept(Baseline),
    /// An operator re-baseline: the next verified `Release` is accepted
    /// whatever its `Date`. It is a recorded, audited act, never implicit.
    Reset {
        /// Who asked for it.
        actor: String,
    },
}

/// Why the ledger could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the metadata ledger is unavailable: {0}")]
pub struct LedgerError(pub String);

/// The durable, append-only record of accepted metadata.
#[async_trait]
pub trait MirrorMetadataLedger: Send + Sync {
    /// The head of the lineage.
    async fn head(&self, key: &MetadataKey) -> Result<Head, LedgerError>;

    /// Appends `step` as sequence `seq`. `Ok(false)` means another writer
    /// already took that sequence: nothing was written and the caller
    /// re-reads and re-decides.
    async fn append(
        &self,
        key: &MetadataKey,
        seq: u64,
        step: &Step,
        at: DateTime<Utc>,
    ) -> Result<bool, LedgerError>;
}

/// The facts of a signature-verified `Release` the decision needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFacts {
    /// `Date`, when present.
    pub date: Option<DateTime<Utc>>,
    /// `Valid-Until`, when present.
    pub valid_until: Option<DateTime<Utc>>,
    /// Digest of the verified content, `sha256:<hex>`.
    pub digest: String,
}

/// The operator's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Policy {
    /// The oldest `Date` tolerated; `None` for no age bound.
    pub max_age: Option<Duration>,
}

/// Why a signed `Release` is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// No `Date`: monotonicity cannot be established.
    #[error("the signed Release carries no Date: its freshness cannot be established")]
    MissingDate,
    /// A `Date` beyond the clock skew.
    #[error("the signed Release is dated in the future")]
    FromTheFuture,
    /// Past `Valid-Until`.
    #[error("the signed Release has expired (Valid-Until)")]
    Expired,
    /// Older than the operator's bound: upstream looks frozen.
    #[error("the signed Release is older than the allowed metadata age: upstream looks frozen")]
    Stale,
    /// Older than what was already accepted.
    #[error("the signed Release is older than the one already accepted: rollback refused")]
    Rollback {
        /// The accepted `Date`.
        accepted: DateTime<Utc>,
        /// The offered `Date`.
        offered: DateTime<Utc>,
    },
    /// Same `Date`, other content.
    #[error("two different signed Releases carry the same Date: refused")]
    Fork,
    /// The ledger could not decide.
    #[error("the metadata ledger could not decide")]
    Unavailable,
}

impl Policy {
    /// A policy bounded to `max_age_secs` (`None` for no bound). The bound is
    /// clamped to [`MAX_AGE_CAP_SECS`], so an absurd configured value can
    /// neither overflow nor panic: it just means "very old is still fine".
    pub fn bounded(max_age_secs: Option<u64>) -> Self {
        Self {
            max_age: max_age_secs.map(|secs| {
                Duration::seconds(i64::try_from(secs.min(MAX_AGE_CAP_SECS)).unwrap_or(0))
            }),
        }
    }
}

/// The largest age bound honoured: about 100 years.
pub const MAX_AGE_CAP_SECS: u64 = 100 * 365 * 86_400;

/// The digest of the verified body, normalized (trailing blanks and trailing
/// empty lines removed) so the clearsigned and the detached form of the same
/// `Release` yield the same digest and one never looks like a fork of the other.
pub fn body_digest(body: &str) -> String {
    use sha2::Digest as _;
    let mut text = String::new();
    for line in body.trim_end().lines() {
        text.push_str(line.trim_end());
        text.push('\n');
    }
    let hash = sha2::Sha256::digest(text.as_bytes());
    format!(
        "sha256:{}",
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

impl Refusal {
    /// A stable machine code for the audit trail and the logs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingDate => "E_METADATA_NO_DATE",
            Self::FromTheFuture => "E_METADATA_FUTURE",
            Self::Expired => "E_METADATA_EXPIRED",
            Self::Stale => "E_METADATA_FROZEN",
            Self::Rollback { .. } => "E_METADATA_ROLLBACK",
            Self::Fork => "E_METADATA_FORK",
            Self::Unavailable => "E_METADATA_LEDGER",
        }
    }
}

/// What a passing `Release` did to the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admitted {
    /// It is the accepted one again.
    Unchanged,
    /// It is newer and became the baseline.
    Advanced,
}

/// The pure decision. `Ok(true)` means advance the baseline.
pub fn judge(
    head: &Head,
    facts: &ReleaseFacts,
    policy: &Policy,
    now: DateTime<Utc>,
) -> Result<bool, Refusal> {
    let date = facts.date.ok_or(Refusal::MissingDate)?;
    if date > now + Duration::seconds(MAX_CLOCK_SKEW_SECS) {
        return Err(Refusal::FromTheFuture);
    }
    if facts.valid_until.is_some_and(|until| until < now) {
        return Err(Refusal::Expired);
    }
    if policy.max_age.is_some_and(|max| now - date > max) {
        return Err(Refusal::Stale);
    }
    match &head.baseline {
        None => Ok(true),
        Some(baseline) if date < baseline.date => Err(Refusal::Rollback {
            accepted: baseline.date,
            offered: date,
        }),
        Some(baseline) if date == baseline.date => {
            if baseline.digest == facts.digest {
                Ok(false)
            } else {
                Err(Refusal::Fork)
            }
        }
        Some(_) => Ok(true),
    }
}

/// Judges `facts` against the ledger and advances it when they are newer.
pub async fn admit(
    ledger: &dyn MirrorMetadataLedger,
    key: &MetadataKey,
    facts: &ReleaseFacts,
    policy: &Policy,
    now: DateTime<Utc>,
) -> Result<Admitted, Refusal> {
    for _ in 0..MAX_ATTEMPTS {
        let head = ledger.head(key).await.map_err(|error| {
            tracing::error!(%error, "the metadata ledger cannot be read");
            Refusal::Unavailable
        })?;
        if !judge(&head, facts, policy, now)? {
            return Ok(Admitted::Unchanged);
        }
        // `judge` only advances for a present `Date`.
        let date = facts.date.ok_or(Refusal::MissingDate)?;
        let step = Step::Accept(Baseline {
            date,
            digest: facts.digest.clone(),
        });
        let written = ledger
            .append(key, head.seq + 1, &step, now)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the metadata ledger cannot be advanced");
                Refusal::Unavailable
            })?;
        if written {
            return Ok(Admitted::Advanced);
        }
    }
    Err(Refusal::Unavailable)
}

/// Re-baselines a lineage (the explicit operator act). Returns the sequence.
pub async fn reset(
    ledger: &dyn MirrorMetadataLedger,
    key: &MetadataKey,
    actor: &str,
    now: DateTime<Utc>,
) -> Result<u64, LedgerError> {
    for _ in 0..MAX_ATTEMPTS {
        let head = ledger.head(key).await?;
        let seq = head.seq + 1;
        let step = Step::Reset {
            actor: actor.to_string(),
        };
        if ledger.append(key, seq, &step, now).await? {
            return Ok(seq);
        }
    }
    Err(LedgerError("the lineage kept racing other writers".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 10, 0, 0).unwrap()
    }
    fn facts(day: u32, digest: &str) -> ReleaseFacts {
        ReleaseFacts {
            date: Some(at(day)),
            valid_until: None,
            digest: digest.into(),
        }
    }
    fn head(day: u32, digest: &str) -> Head {
        Head {
            seq: 3,
            baseline: Some(Baseline {
                date: at(day),
                digest: digest.into(),
            }),
            accepted_at: None,
        }
    }

    #[test]
    fn a_new_lineage_accepts_and_advances() {
        assert_eq!(
            judge(
                &Head::default(),
                &facts(10, "a"),
                &Policy::default(),
                at(11)
            ),
            Ok(true)
        );
    }

    #[test]
    fn newer_advances_same_is_unchanged_older_is_rollback() {
        let p = Policy::default();
        assert_eq!(judge(&head(10, "a"), &facts(12, "b"), &p, at(13)), Ok(true));
        assert_eq!(
            judge(&head(10, "a"), &facts(10, "a"), &p, at(13)),
            Ok(false)
        );
        assert!(matches!(
            judge(&head(10, "a"), &facts(9, "b"), &p, at(13)),
            Err(Refusal::Rollback { .. })
        ));
        assert_eq!(
            judge(&head(10, "a"), &facts(10, "b"), &p, at(13)),
            Err(Refusal::Fork)
        );
    }

    #[test]
    fn freeze_signals_are_refused() {
        let mut expired = facts(10, "a");
        expired.valid_until = Some(at(11));
        assert_eq!(
            judge(&Head::default(), &expired, &Policy::default(), at(12)),
            Err(Refusal::Expired)
        );
        let policy = Policy {
            max_age: Some(Duration::days(2)),
        };
        assert_eq!(
            judge(&Head::default(), &facts(10, "a"), &policy, at(13)),
            Err(Refusal::Stale)
        );
        assert_eq!(
            judge(&Head::default(), &facts(10, "a"), &policy, at(11)),
            Ok(true)
        );
    }

    #[test]
    fn an_absurd_age_bound_neither_overflows_nor_panics() {
        let policy = Policy::bounded(Some(u64::MAX));
        assert_eq!(
            judge(&Head::default(), &facts(10, "a"), &policy, at(11)),
            Ok(true)
        );
        assert_eq!(Policy::bounded(None).max_age, None);
    }

    #[test]
    fn subjects_of_different_triples_never_collide() {
        let key = |a: &str, r: &str, d: &str| MetadataKey {
            account: a.into(),
            repository: r.into(),
            distribution: d.into(),
        };
        assert_ne!(
            key("a/b", "c", "d").subject(),
            key("a", "b/c", "d").subject()
        );
        assert_ne!(
            key("a%2Fb", "c", "d").subject(),
            key("a/b", "c", "d").subject()
        );
    }

    #[test]
    fn the_digest_ignores_trailing_blanks() {
        assert_eq!(body_digest("A: 1\nB: 2 \n\n"), body_digest("A: 1\nB: 2\n"));
        assert_ne!(body_digest("A: 1\n"), body_digest("A: 2\n"));
    }

    #[test]
    fn missing_or_future_date_is_refused() {
        let mut none = facts(10, "a");
        none.date = None;
        assert_eq!(
            judge(&Head::default(), &none, &Policy::default(), at(11)),
            Err(Refusal::MissingDate)
        );
        assert_eq!(
            judge(
                &Head::default(),
                &facts(20, "a"),
                &Policy::default(),
                at(11)
            ),
            Err(Refusal::FromTheFuture)
        );
    }
}
