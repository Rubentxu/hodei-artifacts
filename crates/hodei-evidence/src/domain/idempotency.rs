//! The key that makes a repeated delivery the same delivery.
//!
//! Every ingestion route provides or derives a key, and the key is what
//! tells Hodei whether it is looking at the same evidence twice or at a new
//! revision of it. The composite of the plan is:
//!
//! ```text
//! producer + external_evidence_id + payload_digest + normalizer_major
//! ```
//!
//! The consequences of that composite are the whole point:
//!
//! - The same producer, the same external identifier, the same content and
//!   the same normalizer **is the same delivery**. Accepting it twice
//!   changes nothing.
//! - The same producer and the same external identifier with **different
//!   content** is a different key, and therefore a revision. The history is
//!   not overwritten: the new revision is a new fact, and the old one stays.
//! - The same content read by a **different major version of the normalizer**
//!   is a different key, because it is a different reading of that content.
//!
//! The key is derived, not generated: it is a function of facts that are
//! already in the record, so it can be recomputed by anyone who has them.

use crate::domain::content::ContentDigest;
use crate::domain::evidence::{Producer, SourceRef};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The key of a delivery.
///
/// The key is **structured**, not a string that has to be taken apart later:
/// the four facts it is made of are fields, so comparing two keys is a
/// comparison of facts and not of a text format that could be parsed wrong.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey {
    producer_id: String,
    external_id: String,
    content_digest: ContentDigest,
    normalizer_major: String,
}

impl IdempotencyKey {
    /// Derives the key of a delivery.
    ///
    /// The producer, the external identifier of the evidence, the digest of
    /// the content and the major version of the normalizer.
    pub fn derive(
        producer: &Producer,
        source: &SourceRef,
        content_digest: &ContentDigest,
        normalizer_major: &str,
    ) -> Self {
        Self {
            producer_id: producer.id.clone(),
            external_id: source.external_id.clone().unwrap_or_default(),
            content_digest: content_digest.clone(),
            normalizer_major: normalizer_major.to_string(),
        }
    }

    /// The producer the delivery comes from.
    pub fn producer_id(&self) -> &str {
        &self.producer_id
    }

    /// The external identifier of the evidence, empty when it has none.
    pub fn external_id(&self) -> &str {
        &self.external_id
    }

    /// The digest of the content that was delivered.
    pub fn content_digest(&self) -> &ContentDigest {
        &self.content_digest
    }

    /// The major version of the normalizer that read the content.
    pub fn normalizer_major(&self) -> &str {
        &self.normalizer_major
    }

    /// Whether this key is the key of the same delivery as the given one.
    pub fn is_same_delivery(&self, other: &IdempotencyKey) -> bool {
        self == other
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}",
            self.producer_id, self.external_id, self.content_digest, self.normalizer_major
        )
    }
}

/// What a repeated delivery means.
///
/// The distinction is the one the plan draws: re-delivering the same
/// evidence is not a new fact, and re-delivering the same external
/// identifier with different content is a new revision of it. Neither is an
/// overwrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redelivery {
    /// The same delivery: the acceptance already exists and nothing changes.
    SameDelivery,
    /// A new revision: the same external identity, different content.
    NewRevision,
}

impl IdempotencyKey {
    /// What a second delivery with the given key means.
    ///
    /// The comparison is by facts: the producer, the external identity, the
    /// content and the reading of the content. Two deliveries with the same
    /// producer, the same external identity and a different content are a
    /// revision of the same thing; anything else is simply another delivery.
    pub fn classify_redelivery(
        &self,
        previous: &IdempotencyKey,
        producer: &Producer,
        source: &SourceRef,
        content_digest: &ContentDigest,
    ) -> Redelivery {
        if self == previous {
            return Redelivery::SameDelivery;
        }
        let same_producer = self.producer_id == producer.id && previous.producer_id == producer.id;
        let same_external = self.external_id == previous.external_id
            && self.external_id == source.external_id.clone().unwrap_or_default();
        let different_content =
            self.content_digest != *content_digest || previous.content_digest != *content_digest;
        if same_producer && same_external && different_content {
            return Redelivery::NewRevision;
        }
        Redelivery::SameDelivery
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn producer() -> Producer {
        Producer::new("connector", "pipelinek").with_version("1.2.3")
    }

    fn source() -> SourceRef {
        SourceRef::by_external_id("run-481")
    }

    fn digest(value: &str) -> ContentDigest {
        ContentDigest::sha256(value.repeat(64))
    }

    #[test]
    fn the_key_is_derived_from_the_facts_of_the_delivery() {
        let key = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "1");
        assert_eq!(key.producer_id(), "pipelinek");
        assert_eq!(key.external_id(), "run-481");
        assert_eq!(key.content_digest(), &digest("a"));
        assert_eq!(key.normalizer_major(), "1");
        assert_eq!(
            key.to_string(),
            format!("pipelinek/run-481/{}/1", digest("a"))
        );
    }

    #[test]
    fn the_same_facts_are_the_same_delivery() {
        let first = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "1");
        let second = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "1");
        assert_eq!(first, second);
        assert_eq!(
            first.classify_redelivery(&second, &producer(), &source(), &digest("a")),
            Redelivery::SameDelivery
        );
    }

    #[test]
    fn the_same_external_identity_with_other_content_is_a_revision() {
        let first = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "1");
        let second = IdempotencyKey::derive(&producer(), &source(), &digest("b"), "1");
        assert_ne!(first, second);
        assert_eq!(
            first.classify_redelivery(&second, &producer(), &source(), &digest("b")),
            Redelivery::NewRevision,
            "the same external identity with different content is a revision"
        );
    }

    #[test]
    fn another_major_version_of_the_normalizer_is_another_reading() {
        let first = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "1");
        let second = IdempotencyKey::derive(&producer(), &source(), &digest("a"), "2");
        assert_ne!(first, second);
    }
}
