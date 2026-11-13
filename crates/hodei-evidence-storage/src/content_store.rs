//! The store of the raw content of the evidence.
//!
//! The content is kept by its digest, so the same content is kept once, and
//! it is kept because the evidence has to be verifiable later: a claim
//! without the bytes it claims cannot be checked again.
//!
//! This adapter keeps the content in memory, and it says so. The decision
//! about where the big payloads live --in the database, in an object store,
//! or somewhere else-- is the SPIKE-009 of the plan, and until that spike
//! is resolved nothing pretends to be the answer. What matters now is that
//! the contract is right: the content is addressed by its digest, and it can
//! be read back and checked against that digest.

use hodei_evidence::{ContentDigest, ContentStore, StoreError};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// The store of the raw content, in memory.
#[derive(Debug, Clone, Default)]
pub struct InMemoryContentStore {
    content: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl InMemoryContentStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many different contents are kept.
    pub fn len(&self) -> usize {
        self.content.read().expect("the content lock").len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait::async_trait]
impl ContentStore for InMemoryContentStore {
    /// Keeps the content and returns its digest.
    ///
    /// The digest is the identity: the same content is kept once, and
    /// keeping it again is not an error.
    async fn put(&self, content: Vec<u8>) -> Result<ContentDigest, StoreError> {
        let digest = ContentDigest::of_bytes(&content);
        self.content
            .write()
            .expect("the content lock")
            .insert(digest.to_string(), content);
        Ok(digest)
    }

    /// Reads the content of a digest.
    async fn get(&self, digest: &ContentDigest) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .content
            .read()
            .expect("the content lock")
            .get(&digest.to_string())
            .cloned())
    }

    /// The size of a content, in bytes, without reading it.
    async fn size(&self, digest: &ContentDigest) -> Result<Option<u64>, StoreError> {
        Ok(self
            .content
            .read()
            .expect("the content lock")
            .get(&digest.to_string())
            .map(|content| content.len() as u64))
    }
}
