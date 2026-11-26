//! The content integrity probe.
//!
//! The catalog asserts that an asset has certain bytes and a certain length. That
//! assertion is a promise, and a promise nobody checks is a lie: a blob that is
//! not where the catalog says it is gets served as if it were healthy, and
//! corruption is mistaken for absence.
//!
//! The probe checks the **cheap** thing: the length the byte store actually
//! holds. Recomputing the digest on every read would mean reading the whole
//! content on every request, and deep verification --that the bytes are the ones
//! their digest says-- is the scrub's job, which it does when it can and not on
//! every request.
//!
//! What the probe does NOT do is decide anything: it answers with a fact --
//! this length, or not there-- and the catalog decides what it means. That is why
//! it lives in the application, which is where the catalog and the byte store
//! already know each other and are wired together.

use std::sync::Arc;

use async_trait::async_trait;

use hodei_catalog::{BlobIntegrityProbe, BlobLocation, BlobProbeError, DigestScope};
use hodei_hub::blob::{BlobError, BlobStore};

/// The probe that asks the byte store.
///
/// The store knows the length of a blob by its digest, and that is what is
/// asked: the length, which is metadata, and not the content.
pub struct BlobStoreProbe {
    /// The byte store.
    store: Arc<dyn BlobStore>,
}

impl BlobStoreProbe {
    /// A probe over the given store.
    pub fn new(store: Arc<dyn BlobStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl BlobIntegrityProbe for BlobStoreProbe {
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError> {
        match location.scope {
            // The global scope: the store keeps one blob per digest, and
            // the question is about that digest.
            DigestScope::Global => self
                .store
                .length(&location.digest)
                .await
                .map_err(the_probe_error),
            // The partitioned scope: the byte store does not yet have a slot
            // per account. The probe does not say "not there" --that would be
            // claiming the content is broken-- nor "I don't know" with the same
            // answer as a disk failure: it says it **cannot know**, which is the
            // truth. The catalog translates that as unavailable, and the client
            // retries instead of deleting something that was not
            // theirs.
            DigestScope::AccountPartitioned => Err(BlobProbeError::Unavailable {
                detail: "the content store keeps one blob per digest and has no \
                         account-partitioned slot yet"
                    .to_string(),
            }),
        }
    }
}

/// A store failure, translated into what the probe can say.
///
/// A `NotFound` means the blob is not there: the store does know that. Everything
/// else --a disk that does not respond, a permission that prevents reading-- means
/// the store **could not** answer, and the probe cannot claim anything about the
/// integrity of a blob it could not look at.
fn the_probe_error(error: BlobError) -> BlobProbeError {
    match error {
        BlobError::NotFound => BlobProbeError::Missing,
        other => BlobProbeError::Unavailable {
            detail: other.to_string(),
        },
    }
}
