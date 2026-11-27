//! The reader of the accepted evidence, over the Dgraph evidence store.
//!
//! Like its embedded counterpart, the adapter is a bridge and not a new
//! store: it reads through the port of the domain and decides nothing. The
//! account is part of the question (the store filters by it), and a store
//! that cannot answer is an error, never an empty list: an empty answer
//! would read as "no evidence" and a trust decision would be taken on it.

use async_trait::async_trait;
use hodei_evidence::domain::evidence::EvidenceRecord;
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence_storage::DgraphEvidenceStore;
use kernel::Hrn;
use std::sync::Arc;

use crate::evidence::{EvidenceReader, EvidenceStoreError};

/// The reader of the accepted evidence, over Dgraph.
pub struct DgraphEvidenceReader {
    store: Arc<DgraphEvidenceStore>,
}

impl DgraphEvidenceReader {
    /// A reader over a store.
    pub fn new(store: Arc<DgraphEvidenceStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl EvidenceReader for DgraphEvidenceReader {
    async fn evidence_of_subject(
        &self,
        account: &str,
        subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError> {
        self.store
            .evidence_of_subject(account, subject)
            .await
            .map_err(|error| EvidenceStoreError::Unavailable(error.to_string()))
    }
}

/// The reader of the accepted evidence, over a store.
pub fn reader_over(store: Arc<DgraphEvidenceStore>) -> Arc<DgraphEvidenceReader> {
    Arc::new(DgraphEvidenceReader::new(store))
}

/// The evidence reader of the graph profile (HSP-314 F3b-1): the reader is
/// [`DgraphEvidenceReader`] over the evidence store of the SAME alpha the
/// rest of the profile uses. The store is opened through the versioned
/// schema guard, so a server this binary cannot speak to stops the boot.
pub async fn wire_graph_evidence_reader(
    state: crate::app_state::AppState,
    graph: &crate::profile::GraphAuthority,
) -> Result<crate::app_state::AppState, Box<dyn std::error::Error + Send + Sync>> {
    let store = DgraphEvidenceStore::open(graph.client.clone())
        .await
        .map_err(|error| format!("the evidence reader could not open: {error}"))?;
    let store = Arc::new(store);
    // The same store admits execution receipts (HSP-37): one authority for
    // what is read and what is written.
    let ingestor = Arc::new(crate::execution_receipt::ReceiptIngestor::new(
        store.clone(),
        graph.client.clone(),
    ));
    Ok(state
        .with_evidence_store(reader_over(store))
        .with_receipt_ingestor(ingestor))
}
