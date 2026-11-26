//! The view of the impact, materialized in the graph.
//!
//! The authoritative answer of "what does this vulnerability reach" is
//! the walk of the graph: it is always right, and it costs a traversal
//! every time. This view is the **projection** of that answer: the
//! subjects, their depths and the routes of support that reach them,
//! said ONCE and kept up to date with the deltas of the relations --
//! lo que cambio se invalida SOLO donde cambio, y lo que esta al dia se
//! lee sin recorrer nada.
//!
//! # The watermark
//!
//! The view says UP TO WHICH version of the deltas it is complete. A
//! reader that needs the truth of the moment compares that version with
//! the ledger's, and the lag is EXPOSED, never hidden: un resultado
//! viejo presentado como actual es una falsa seguridad.

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

/// How fresh an answer is. La frescura se EXPLICA, nunca se disfraza:
/// una vista al dia es Current, y una respuesta calculada ahora por el
/// recorrido autoritativo dice cual era el watermark atrasado que
/// sustituyo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Freshness {
    /// The view was complete up to the version of the ledger.
    Current,
    /// The answer was computed NOW by the authoritative walk: la vista
    /// estaba atrasada o no estaba.
    Authoritative { view_watermark: u64 },
}

/// One subject of the answer, with the path that explains it: las
/// relaciones y la evidencia que lo trajeron. El que pregunta puede
/// comprobar el camino.
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

/// The answer of the impact, typed and agent-readable. Es de SOLO
/// LECTURA por construccion: no hay metodo que mutar.
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

/// Why serving an answer failed. Un desajuste de cuenta NO es una
/// respuesta vacia: es una puerta cerrada.
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

/// The READ-ONLY port of the impact. El consumidor pregunta, y la
/// respuesta dice de donde sale y hasta que version es completa; el
/// consumidor NO puede mutar nada por esta puerta.
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
/// evidence that supports each. Es la procedencia de la vista: el que
/// pregunta puede comprobar el camino, y el que invalida sabe que
/// cambios tocan este advisory.
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
    /// The nodes the walk traversed. Un edge NUEVO no esta en ninguna
    /// relacion previa: lo que invalida un edge nuevo es que sus
    /// extremos pasan por la ruta.
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
