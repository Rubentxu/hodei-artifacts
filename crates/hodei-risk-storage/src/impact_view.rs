//! The view of the impact, materialized in the graph.
//!
//! The authoritative answer of "what does this vulnerability reach" is
//! the walk of the graph: it is always right, and it costs a traversal
//! every time. This view is the **projection** of that answer: the
//! subjects, their depths and the routes of support that reach them,
//! said ONCE and kept up to date with the deltas of the relations --
//! what changed is invalidated ONLY where it changed, and what is up to date is
//! read without walking anything.
//!
//! # The watermark
//!
//! The view says UP TO WHICH version of the deltas it is complete. A
//! reader that needs the truth of the moment compares that version with
//! the ledger's, and the lag is EXPOSED, never hidden: an old
//! result presented as current is false security.

use hodei_evidence::domain::delta::RelationDelta;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The table the materialized view lives in.
pub const IMPACT_TABLE: &str = "impact_view";

/// The table the watermarks of the view live in.
pub const IMPACT_META_TABLE: &str = "impact_meta";

/// A row of the view: ONE subject of ONE advisory, with its depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactRow {
    /// The account the view belongs to.
    pub tenant: String,
    /// The advisory the view is about.
    pub advisory: String,
    /// The subject the advisory reaches.
    pub subject: String,
    /// What the subject is: artifact | component.
    pub kind: String,
    /// How far the dependency is: the depth of the walk.
    pub depth: usize,
    /// When the row was written.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// How fresh an answer is. Freshness is EXPLAINED, never disguised:
/// an up-to-date view is Current, and an answer computed now by the
/// authoritative walk states which lagging watermark it
/// replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Freshness {
    /// The view was complete up to the version of the ledger.
    Current,
    /// The answer was computed NOW by the authoritative walk: the view
    /// was behind or was not there.
    Authoritative { view_watermark: u64 },
}

/// One subject of the answer, with the path that explains it: the
/// relations and the evidence that brought it in. Whoever asks can
/// check the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectPath {
    /// The subject, as an HRN string.
    pub subject: String,
    /// How far the dependency is: zero for the component the
    /// vulnerability is in.
    pub depth: usize,
    /// The identities of the relations that brought this subject in.
    pub relations: Vec<String>,
    /// The evidence that supports each of those relations.
    pub evidences: Vec<String>,
}

/// The answer of the impact, typed and agent-readable. It is READ-ONLY
/// by construction: there is no method that mutates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactAnswer {
    /// The account the answer belongs to.
    pub tenant: String,
    /// The advisory asked about.
    pub advisory: String,
    /// The subjects, with the path that explains each one.
    pub subjects: Vec<SubjectPath>,
    /// The deployments that run the affected artifacts.
    pub deployments: Vec<String>,
    /// The version the answer is complete up to.
    pub version: u64,
    /// Where the answer came from, and how fresh it is.
    pub freshness: Freshness,
}

/// Why serving an answer failed. An account mismatch is NOT an
/// empty answer: it is a closed door.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpactReadError {
    /// The advisory belongs to another account than the one served.
    TenantMismatch { served: String, requested: String },
    /// The advisory is not a canonical HRN.
    NotCanonical(String),
    /// The store failed underneath.
    Unavailable(String),
}

impl std::fmt::Display for ImpactReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TenantMismatch { served, requested } => write!(
                formatter,
                "the advisory belongs to account {requested} and this reader serves {served}"
            ),
            Self::NotCanonical(value) => write!(formatter, "{value} is not a canonical HRN"),
            Self::Unavailable(reason) => write!(formatter, "the store is unavailable: {reason}"),
        }
    }
}

impl std::error::Error for ImpactReadError {}

/// The READ-ONLY port of the impact. The consumer asks, and the
/// answer says where it comes from and up to which version it is complete; the
/// consumer CANNOT mutate anything through this door.
#[async_trait::async_trait]
pub trait ImpactReader: Send + Sync {
    /// The answer for one advisory of one account.
    async fn serve_impact(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactAnswer, ImpactReadError>;
}

/// The route of one advisory: the relations its walk used, with the
/// evidence that supports each. It is the provenance of the view: whoever
/// asks can check the path, and whoever invalidates knows which
/// changes touch this advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImpactRoute {
    /// The account the route belongs to.
    pub tenant: String,
    /// The advisory the route belongs to.
    pub advisory: String,
    /// The identities of the relations the walk used.
    pub relations: Vec<String>,
    /// The evidences that support those relations.
    pub evidences: Vec<String>,
    /// The nodes the walk traversed. A NEW edge is not in any
    /// previous relation: what invalidates a new edge is that its
    /// endpoints pass through the route.
    #[serde(default)]
    pub nodes: Vec<String>,
    /// The path that explains each subject, as it was walked.
    #[serde(default)]
    pub paths: Vec<SubjectPath>,
    /// The deployments that run the affected artifacts.
    #[serde(default)]
    pub deployments: Vec<String>,
    /// The version of the deltas the route was computed at.
    pub watermark: u64,
    /// When the route was written.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// What the view answers for one advisory, with the version it is
/// complete up to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactSnapshot {
    /// The advisory.
    pub advisory: String,
    /// The subjects the advisory reaches, with their depths.
    pub subjects: Vec<(String, usize)>,
    /// The version the view is complete up to.
    pub watermark: u64,
}

/// The WRITE side of the impact view, as the reactive cycle drives it:
/// materialize one advisory, ask what a delta touches, apply a delta.
///
/// The cycle depends on this port and not on an engine. Both adapters
/// keep the same contract: every operation is idempotent, a delta is
/// applied only to the advisories whose route it touches, and a failure
/// is an error, never a silent no-op.
#[async_trait::async_trait]
pub trait ImpactMaterializer: Send + Sync {
    /// Materializes the impact of one advisory from the authoritative
    /// walk, at the version the ledger has reached.
    async fn rebuild_advisory(
        &self,
        tenant: &Hrn,
        advisory: &Hrn,
    ) -> Result<ImpactSnapshot, String>;

    /// The advisories whose routes the delta invalidates.
    async fn invalidated_by(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String>;

    /// Recomputes ONLY the advisories the delta touches and answers with
    /// them.
    async fn apply_delta(&self, delta: &RelationDelta) -> Result<Vec<Hrn>, String>;
}
