//! The reader of the accepted evidence, over the store that already exists.
//!
//! The adapter is a bridge and not a new store: it reads through the port
//! of the domain and it does not decide anything. The account is part of
//! the question, so the evidence of another account never comes back.

use async_trait::async_trait;
use hodei_evidence::domain::evidence::EvidenceRecord;
use hodei_evidence::ports::EvidenceStore;
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Connection;
use surrealdb::Surreal;

use crate::evidence::{EvidenceReader, EvidenceStoreError};

/// The reader of the accepted evidence, over SurrealDB.
pub struct SurrealEvidenceReader<C: Connection> {
    store: Arc<hodei_evidence_storage::SurrealEvidenceStore<C>>,
}

impl<C: Connection> SurrealEvidenceReader<C> {
    /// A reader over a store.
    pub fn new(store: Arc<hodei_evidence_storage::SurrealEvidenceStore<C>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl<C: Connection> EvidenceReader for SurrealEvidenceReader<C> {
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

/// The reader of the accepted evidence, over a database.
pub fn reader_over<C: Connection>(db: Arc<Surreal<C>>) -> Arc<SurrealEvidenceReader<C>> {
    Arc::new(SurrealEvidenceReader::new(Arc::new(
        hodei_evidence_storage::SurrealEvidenceStore::new(db),
    )))
}
