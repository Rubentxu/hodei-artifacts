//! The ports of the risk: where the relations of the risk graph come from.
//!
//! The traversal is a pure function over the relations, and the port is
//! what says where the relations come from. That way the question of the
//! blast radius can be tested without a database, and the adapter only has
//! to say which the relations are.

use crate::traversal::Edge;
use async_trait::async_trait;
use kernel::Hrn;

/// Where the relations of the risk graph come from.
#[async_trait]
pub trait RiskRelationSource: Send + Sync {
    /// The relations of an account.
    async fn edges(&self, tenant: &Hrn) -> Result<Vec<Edge>, RiskRelationError>;
}

/// Why the relations could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskRelationError {
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for RiskRelationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "the risk relations are not available: {message}")
            }
        }
    }
}

impl std::error::Error for RiskRelationError {}
