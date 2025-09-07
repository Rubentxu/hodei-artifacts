//! Persistence adapters for Supply Chain bounded context
//!
//! Implements supply chain data storage operations
//! Following Repository pattern with dependency inversion

use async_trait::async_trait;
use crate::application::ports::SbomRepository;
use crate::domain::model::{SbomId, SbomSummary, Vulnerability};
use crate::error::SupplyChainError;
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

// Placeholder for supply chain persistence implementations
// These will be implemented as concrete adapters for MongoDB, etc.

pub struct SupplyChainStore {
    sbom_summaries: RwLock<HashMap<String, SbomSummary>>,
    vulnerabilities: RwLock<HashMap<Uuid, Vulnerability>>,
}

impl SupplyChainStore {
    pub fn new() -> Self {
        Self {
            sbom_summaries: RwLock::new(HashMap::new()),
            vulnerabilities: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for SupplyChainStore {
    fn default() -> Self {
        Self::new()
    }
}

// Implement the SbomRepository port using an in-memory store
#[async_trait]
impl SbomRepository for SupplyChainStore {
    async fn save_summary(&self, summary: &SbomSummary) -> Result<(), SupplyChainError> {
        let mut map = self.sbom_summaries.write().await;
        map.insert(summary.sbom_id.0.clone(), summary.clone());
        Ok(())
    }

    async fn get_summary(&self, sbom_id: &SbomId) -> Result<Option<SbomSummary>, SupplyChainError> {
        let map = self.sbom_summaries.read().await;
        Ok(map.get(&sbom_id.0).cloned())
    }
}
