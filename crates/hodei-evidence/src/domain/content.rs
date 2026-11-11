//! The reference to a piece of content.
//!
//! The evidence that Hodei accepts is a document produced elsewhere: an
//! SBOM, an attestation, a build log. Its payload can be large and it can
//! contain things that must never be stored -- an access token, a private
//! key, a credential in a build log.
//!
//! So the contract carries a **reference** to the content, not the content
//! itself: a digest that identifies it and a pointer that says where it
//! lives. The raw bytes stay in the content store, under the control of
//! whoever owns them, and Hodei keeps the fact of their existence, their
//! digest and where to find them.
//!
//! That is not a promise about redaction; it is the shape of the type. A
//! record of evidence cannot carry a secret because it has nowhere to put it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The algorithm that produced a digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DigestAlgorithm {
    /// SHA-256, the algorithm the artifact identity of Hodei uses.
    Sha256,
    /// SHA-512.
    Sha512,
}

impl fmt::Display for DigestAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sha256 => f.write_str("sha256"),
            Self::Sha512 => f.write_str("sha512"),
        }
    }
}

/// A content digest: the algorithm and the value it produced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest {
    /// The algorithm.
    pub algorithm: DigestAlgorithm,
    /// The value, in lowercase hexadecimal.
    pub value: String,
}

impl ContentDigest {
    /// A SHA-256 digest.
    pub fn sha256(value: impl Into<String>) -> Self {
        Self {
            algorithm: DigestAlgorithm::Sha256,
            value: value.into().to_ascii_lowercase(),
        }
    }

    /// Whether the digest is well formed: a non-empty value of hexadecimal
    /// characters, and the length the algorithm produces.
    pub fn is_well_formed(&self) -> bool {
        if self.value.is_empty() || !self.value.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
        match self.algorithm {
            DigestAlgorithm::Sha256 => self.value.len() == 64,
            DigestAlgorithm::Sha512 => self.value.len() == 128,
        }
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.value)
    }
}

/// A reference to a piece of content: what it is, and where it lives.
///
/// The `object_ref` is opaque on purpose. It may be a key in an object
/// store, a path, or a pointer to an external system. Hodei does not
/// interpret it, and that is what keeps the context free of any knowledge
/// about where the bytes are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    /// The digest of the content. Two references with the same digest are
    /// the same content.
    pub digest: ContentDigest,
    /// Where the content lives, if it lives somewhere Hodei knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<String>,
}

impl ContentRef {
    /// A reference to content that lives somewhere, by digest.
    pub fn new(digest: ContentDigest, object_ref: impl Into<String>) -> Self {
        Self {
            digest,
            object_ref: Some(object_ref.into()),
        }
    }

    /// A reference to content whose location is not known yet.
    pub fn by_digest(digest: ContentDigest) -> Self {
        Self {
            digest,
            object_ref: None,
        }
    }
}

/// The error of a malformed content reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// The digest is not well formed.
    #[error("the digest is not well formed: {0}")]
    MalformedDigest(String),
    /// The reference has no digest at all.
    #[error("a content reference needs a digest")]
    MissingDigest,
}

impl ContentRef {
    /// Checks that the reference can be used.
    pub fn validate(&self) -> Result<(), ContentError> {
        if self.digest.value.is_empty() {
            return Err(ContentError::MissingDigest);
        }
        if !self.digest.is_well_formed() {
            return Err(ContentError::MalformedDigest(self.digest.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sha256_digest_is_well_formed_only_with_sixty_four_hexadecimal_characters() {
        assert!(ContentDigest::sha256("a".repeat(64)).is_well_formed());
        assert!(!ContentDigest::sha256("a".repeat(63)).is_well_formed());
        assert!(!ContentDigest::sha256("z".repeat(64)).is_well_formed());
        assert!(!ContentDigest::sha256(String::new()).is_well_formed());
    }

    #[test]
    fn the_digest_is_written_in_the_canonical_form() {
        let digest = ContentDigest::sha256("A".repeat(64));
        assert_eq!(digest.value, "a".repeat(64));
        assert_eq!(digest.to_string(), format!("sha256:{}", "a".repeat(64)));
    }

    #[test]
    fn a_reference_without_a_well_formed_digest_is_refused() {
        let reference = ContentRef::by_digest(ContentDigest::sha256("short"));
        assert!(matches!(
            reference.validate(),
            Err(ContentError::MalformedDigest(_))
        ));

        let reference = ContentRef::by_digest(ContentDigest::sha256(String::new()));
        assert!(matches!(
            reference.validate(),
            Err(ContentError::MissingDigest)
        ));
    }
}
