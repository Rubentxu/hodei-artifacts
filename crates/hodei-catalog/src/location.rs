//! The location journal of a blob (HSP-325).
//!
//! The content store holds the bytes; this journal says **where the bytes are
//! read from**. Every digest has one row with a monotonic fence: whoever changes
//! the row holds a lease on it, and a writer whose lease expired and was taken
//! over (a larger fence) can no longer advance it.
//!
//! The phases of a digest and where a read goes:
//!
//! - [`Phase::Origin`]: the origin serves (a digest without a row is here);
//! - [`Phase::Switched`]: the destination serves, the origin copy is intact;
//! - [`Phase::Retired`]: the destination serves, the origin copy was moved
//!   aside (never deleted), so the step is still reversible.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use std::time::Duration;

use crate::CatalogError;

/// Where the reads of a digest go, and what state the origin copy is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The origin serves.
    Origin,
    /// The destination serves; the origin copy is intact.
    Switched,
    /// The destination serves; the origin copy was moved aside.
    Retired,
}

impl Phase {
    /// The name that goes in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Origin => "origin",
            Self::Switched => "switched",
            Self::Retired => "retired",
        }
    }

    /// Parses the name kept in the database.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "origin" => Some(Self::Origin),
            "switched" => Some(Self::Switched),
            "retired" => Some(Self::Retired),
            _ => None,
        }
    }

    /// Whether the destination is the first place a read goes to.
    pub fn reads_destination(self) -> bool {
        !matches!(self, Self::Origin)
    }

    /// Whether the journal allows going from `self` to `to`.
    pub fn may_become(self, to: Phase) -> bool {
        matches!(
            (self, to),
            (Phase::Origin, Phase::Switched)
                | (Phase::Switched, Phase::Retired)
                | (Phase::Switched, Phase::Origin)
                | (Phase::Retired, Phase::Origin)
        )
    }
}

/// The row of a digest as the journal carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationEntry {
    /// The content.
    pub digest: ContentDigest,
    /// Where reads go.
    pub phase: Phase,
    /// The monotonic fence: it only grows, one step per take-over.
    pub fence: u64,
    /// When the current lease ends.
    pub lease_until: DateTime<Utc>,
}

/// The lease of one digest: the right to advance its row while it lasts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationLease {
    /// The content.
    pub digest: ContentDigest,
    /// The fence of this claim.
    pub fence: u64,
    /// The token of this claim.
    pub token: String,
    /// When it ends.
    pub lease_until: DateTime<Utc>,
}

/// The journal of locations.
#[async_trait]
pub trait LocationJournal: Send + Sync {
    /// The row of a digest, if it has one (no row means the origin serves).
    async fn location_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<LocationEntry>, CatalogError>;

    /// Takes or renews the lease of a digest.
    ///
    /// No row or an expired lease: a NEW fence and token (the previous holder
    /// is fenced out). The same owner on a live lease renews it with the same
    /// token. Another owner on a live lease is a `Conflict`. The verdict comes
    /// from a re-read after the commit.
    async fn acquire_location(
        &self,
        digest: &ContentDigest,
        owner: &str,
        lease_for: Duration,
        now: DateTime<Utc>,
    ) -> Result<LocationLease, CatalogError>;

    /// Moves the row from `from` to `to` if, and only if, `lease` is still the
    /// live claim and the row is still in `from`. Otherwise
    /// `PreconditionFailed` and the row is not touched.
    async fn advance_location(
        &self,
        lease: &LocationLease,
        from: Phase,
        to: Phase,
    ) -> Result<LocationEntry, CatalogError>;

    /// Gives the claim back (the lease ends now). Guarded by the token.
    async fn release_location(&self, lease: &LocationLease) -> Result<(), CatalogError>;

    /// The `(account, repository)` pairs whose rows name this digest.
    async fn digest_owners(
        &self,
        digest: &ContentDigest,
    ) -> Result<Vec<(String, String)>, CatalogError>;
}
