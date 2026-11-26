//! The store of the risk: the relations of the risk graph and the
//! questions that are asked of them.
//!
//! The relations live in the graph of trust, because that is what the
//! graph is for: an artefact that holds a component, a component that
//! depends on another, and a component that a vulnerability is about. The
//! adapter here reads them and gives them back in the shape the traversal
//! wants.

#![forbid(unsafe_code)]

pub mod dgraph_cases;
pub mod dgraph_view;
pub mod impact_view;
pub mod subject_risk;

/// Why the risk relations could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "the risk store is not available: {message}")
            }
        }
    }
}

impl std::error::Error for RiskError {}

/// The port of the blast radius: how far a vulnerability reaches in the
/// graph of an account.
///
/// The reactive cycle asks this question through the port, so it does not
/// care which engine holds the graph. Each adapter keeps the semantics of
/// its own walk; the Dgraph one refuses a walk that hit a bound instead of
/// answering with a partial radius.
#[async_trait::async_trait]
pub trait BlastRadiusReader: Send + Sync {
    /// The radius of `vulnerability` in the graph of `tenant`.
    async fn blast_radius_of(
        &self,
        tenant: &kernel::Hrn,
        vulnerability: &kernel::Hrn,
    ) -> Result<hodei_risk::case::BlastRadius, RiskError>;
}
