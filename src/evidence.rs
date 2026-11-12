//! The reader of the accepted evidence, from the point of view of the
//! service.
//!
//! The trust capsule is assembled from the evidence that is there, and the
//! service reads that evidence through a port. The port is narrower than the
//! port of the domain on purpose: the service reads, and the writes happen
//! in the ingestion.

use async_trait::async_trait;
use hodei_evidence::domain::evidence::EvidenceRecord;
use kernel::Hrn;

/// The reader of the accepted evidence of a subject.
#[async_trait]
pub trait EvidenceReader: Send + Sync {
    /// The pieces of evidence that speak about a subject, of an account.
    ///
    /// The account is part of the question: the evidence of another
    /// account never comes back.
    async fn evidence_of_subject(
        &self,
        account: &str,
        subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError>;
}

/// Why the evidence could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceStoreError {
    /// The store is not available.
    #[error("the evidence store is not available: {0}")]
    Unavailable(String),
}

/// A reader that never has any evidence.
///
/// It is the reader of a deployment that has not wired the evidence
/// context: the capsule of a subject then says that there is no evidence,
/// which is the truth, and the endpoint does not fail for a reason that is
/// not the caller's.
#[derive(Debug, Clone, Default)]
pub struct EmptyEvidenceReader;

#[async_trait]
impl EvidenceReader for EmptyEvidenceReader {
    async fn evidence_of_subject(
        &self,
        _account: &str,
        _subject: &Hrn,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError> {
        Ok(Vec::new())
    }
}
