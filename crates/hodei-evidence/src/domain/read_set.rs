//! The read-set: what a read READ, stated so that it can be
//! verified.
//!
//! A read has to be able to prove WHICH evidence, relations
//! and policies it consulted and over WHICH versions of the world. That is a
//! **reads manifest**: identifiers, digests and generations,
//! in a stable order, without raw content and without clocks. It is a
//! neutral primitive: it does not depend on a model, nor on a trust
//! decision, nor on a concrete store.
//!
//! # The three lists
//!
//! What was **retrieved**, what was **given to the model** and what the accepted
//! claims **cited** are three different things, and the
//! manifest tells them apart: a retrieved fact that the budget
//! left out did not reach the model, and a fact that arrived and was not cited does not
//! support anything.
//!
//! # Consistency
//!
//! The world can change while it is being read. The manifest carries the source
//! **generations** (a vector, because there may be no single
//! global cursor), and the check says whether the world against which
//! it was read is still the world of now. A stale read
//! says it is stale: it is never presented as a consistent snapshot.
//!
//! # Where it lives
//!
//! It is a primitive of SHARED DOMAIN, and that is why it lives here and not in
//! the crate that consumes it first: the investigator uses it for
//! its reports and the simulation uses it to pin the base on which it
//! evaluates. A domain that defines the format of its manifest and another that
//! consumes it have to speak of the SAME manifest, or the seal of one
//! verifies nothing of the other.

use serde::{Deserialize, Serialize};

/// What kind of resource was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadKind {
    /// A piece of evidence, verified by the digest of its content.
    Evidence,
    /// A graph relation, verified by its identity.
    Relation,
    /// A trust policy, verified by its version.
    Policy,
}

/// The read of ONE resource, with the metadata that is enough to
/// check its origin. It never carries the content: it carries the digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReadStamp {
    /// What kind of resource it is.
    pub kind: ReadKind,
    /// The identifier of the resource: the evidence id, the identity
    /// of the relation or the policy id.
    pub resource: String,
    /// What the origin is verified with: the content digest, the
    /// identity of the relation or the version of the policy.
    pub verified_by: String,
}

impl ReadStamp {
    /// The seal of a piece of evidence, verified by its digest.
    pub fn evidence(resource: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Evidence,
            resource: resource.into(),
            verified_by: digest.into(),
        }
    }

    /// The seal of a relation, verified by its identity.
    pub fn relation(resource: impl Into<String>, evidence: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Relation,
            resource: resource.into(),
            verified_by: evidence.into(),
        }
    }

    /// The seal of a policy, verified by its version.
    pub fn policy(resource: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Policy,
            resource: resource.into(),
            verified_by: version.into(),
        }
    }
}

/// The manifest of the reads.
///
/// The generations are a VECTOR --(source, version)-- because there may be no
/// single global cursor for everything that is read. The seals go
/// in a stable order: the same set of reads gives the same
/// manifest, whatever order it is read in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadSet {
    /// The version of each source at the time of reading, without clocks.
    pub generations: Vec<(String, u64)>,
    /// The seals of what was read.
    pub stamps: Vec<ReadStamp>,
}

impl ReadSet {
    /// An empty manifest.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Records a read.
    ///
    /// The read is recorded by WHAT WAS VERIFIED, not just by the
    /// resource: the same resource verified in two ways is TWO
    /// reads, and the manifest has to be able to state both. Keeping
    /// only the first would make a manifest look coherent
    /// while the world had already changed, and what is not seen cannot
    /// be checked.
    ///
    /// The same read stated twice --the same resource with the same
    /// verification-- is still one.
    pub fn read(&mut self, stamp: ReadStamp) {
        if !self.stamps.contains(&stamp) {
            self.stamps.push(stamp);
        }
    }

    /// Records the generation of a source.
    pub fn generation(&mut self, source: impl Into<String>, version: u64) {
        let source = source.into();
        if !self.generations.iter().any(|(known, _)| *known == source) {
            self.generations.push((source, version));
        }
    }

    /// The canonical manifest: the seals ordered by their identity,
    /// the generations ordered by their source, without clocks and without
    /// accidental order. The replay of the same set of reads
    /// produces THE SAME manifest.
    pub fn manifest(&self) -> String {
        let mut stamps = self.stamps.clone();
        stamps.sort();
        let mut generations = self.generations.clone();
        generations.sort();
        format!("readset generations={:?} stamps={:?}", generations, stamps)
    }

    /// The fingerprint of the canonical manifest: the digest of what was read,
    /// to compare two manifests without reading them in full.
    pub fn fingerprint(&self) -> String {
        crate::ContentDigest::of_bytes(self.manifest().as_bytes()).to_string()
    }

    /// Whether the world against which it was read is STILL the world of now:
    /// every recorded generation has to still be the one the
    /// source says it has.
    pub fn is_consistent_with(&self, current: &[(String, u64)]) -> bool {
        self.generations.iter().all(|(source, expected)| {
            current
                .iter()
                .any(|(now, version)| now == source && version == expected)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC3: the replay of the same set of reads produces the same
    /// manifest, whatever order it is built in.
    #[test]
    fn the_same_reads_give_the_same_manifest_in_any_order() {
        let mut first = ReadSet::empty();
        first.generation("graph", 12);
        first.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        first.read(ReadStamp::relation("contains|a|b", "evd-1"));
        first.read(ReadStamp::evidence("evd-2", "sha256:bbb"));

        let mut second = ReadSet::empty();
        second.read(ReadStamp::evidence("evd-2", "sha256:bbb"));
        second.generation("graph", 12);
        second.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        second.read(ReadStamp::relation("contains|a|b", "evd-1"));

        assert_eq!(first.manifest(), second.manifest());
        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    /// The same read stated twice is ONE in the manifest.
    #[test]
    fn a_read_said_twice_is_one_in_the_manifest() {
        let mut read_set = ReadSet::empty();
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        assert_eq!(read_set.stamps.len(), 1);
    }

    /// The SAME resource verified in two ways is TWO reads: the
    /// manifest has to be able to state both, because if only the
    /// first is kept a manifest can look coherent while the
    /// world had already changed.
    #[test]
    fn the_same_resource_verified_two_ways_is_two_reads() {
        let mut read_set = ReadSet::empty();
        read_set.read(ReadStamp::policy("production", "1.0.0"));
        read_set.read(ReadStamp::policy("production", "2.0.0"));
        assert_eq!(
            read_set.stamps.len(),
            2,
            "las dos versiones de la politica estan en el manifiesto"
        );
        assert_eq!(read_set.stamps[0].verified_by, "1.0.0");
        assert_eq!(read_set.stamps[1].verified_by, "2.0.0");
    }

    /// AC2: the world can change while it is being read. A manifest whose
    /// generation is no longer the source's is STALE, and says so.
    #[test]
    fn a_manifest_from_an_older_generation_is_stale() {
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", 12);
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));

        let current = vec![("graph".to_string(), 13u64)];
        assert!(
            !read_set.is_consistent_with(&current),
            "la generacion avanzo: la lectura esta obsoleta"
        );

        let same = vec![("graph".to_string(), 12u64)];
        assert!(
            read_set.is_consistent_with(&same),
            "la misma generacion es consistente"
        );
    }

    /// The manifest carries no content: it carries identifiers and digests.
    #[test]
    fn the_manifest_carries_digests_not_content() {
        let mut read_set = ReadSet::empty();
        read_set.read(ReadStamp::evidence(
            "evd-1",
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        let manifest = read_set.manifest();
        assert!(manifest.contains("evd-1"));
        assert!(
            !manifest.contains("the sbom says"),
            "el contenido no entra en el manifiesto"
        );
    }
}
