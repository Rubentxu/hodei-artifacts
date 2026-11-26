//! The pinned base: over which world the simulation is evaluated.
//!
//! A simulation that does not say over which version of the truth it was
//! computed is an answer without a surname: the same "what would happen if"
//! can be true or false depending on what changed between the
//! question and the answer. And a simulation that relies on a world
//! that no longer exists is not an old simulation: it is a **wrong**
//! simulation, because it answers for a state that nobody has.
//!
//! # The two parts of the seal
//!
//! The base is sealed twice over, and both are needed:
//!
//! - The graph **generation** says whether the graph's HISTORY has
//!   advanced. It is a monotonic counter, cheap to read, and it is what
//!   the investigator uses for the same purpose.
//! - The **content seal** says whether what was READ has moved. The
//!   generation is not enough: the graph is also written through paths that
//!   do not go through its delta history, and a relation replaced by
//!   another keeps the number of relations. Only the content
//!   tells one from the other.
//!
//! And the content seal is compared by its digest, not by the count:
//! the number of relations is not a version, it is a tally, and a
//! tally does not say the relations are the same.
//!
//! # The clock
//!
//! The base carries the instant at which it was evaluated. The clock is not the
//! system's: it is received by whoever simulates, and so two simulations with the
//! same base and the same clock give the same result. A decision
//! core that reads the wall clock is not reproducible, and a
//! simulation that is not reproducible cannot be audited.
//!
//! # The scope of the generation
//!
//! The generation is **the account's**, not the subject's: it is the counter of
//! the history of the whole graph. A write that does not touch the set
//! the simulation reads also invalidates it. This is on purpose --failing
//! closed rather than returning an answer about a world that
//! moved--, but it is worth knowing: the simulation is stricter than
//! it seems.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::read_set::{ReadKind, ReadSet, ReadStamp};
use hodei_trust_policy::TrustPolicy;
use serde::{Deserialize, Serialize};

use crate::hypothetical::RelationFact;

/// The base over which a simulation is evaluated.
///
/// It is what is pinned BEFORE simulating and what is checked AFTER:
/// if the world has moved, the simulation is declared stale instead of
/// returning a diff that nobody knows which world it holds for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SimulationBase {
    /// The account the base belongs to.
    pub account: String,
    /// The instant at which it was evaluated.
    pub evaluated_at: DateTime<Utc>,
    /// The manifest of what was read: the generations of the sources
    /// and the seals of each resource.
    ///
    /// It covers the relations read by the SIMULATION, the policy against
    /// which it decides and the evidence the decision consumes.
    pub read_set: ReadSet,
    /// The seal of the content read: the digest of the relations the
    /// simulation read, with their evidence.
    ///
    /// It is the seal of what the simulation read from the GRAPH. What is
    /// read to decide has its own check --evidence is
    /// compared by its seals-- and does not enter here: a digest that mixed
    /// the graph with the evidence would make an evidence change
    /// look like a graph change.
    pub seal: String,
    /// The ledger position that declares the starting point, if
    /// known.
    ///
    /// It goes APART from the manifest generations on purpose: the
    /// generations are the versions that have been VERIFIED against their
    /// source, and this position cannot be verified here --the
    /// simulation has no ledger port to check it
    /// against--. Declaring it as a generation would make it pass for
    /// checked.
    #[serde(default)]
    pub declared_ledger_position: Option<u64>,
}

impl SimulationBase {
    /// An empty base of an account.
    pub fn new(account: impl Into<String>, evaluated_at: DateTime<Utc>) -> Self {
        Self {
            account: account.into(),
            evaluated_at,
            read_set: ReadSet::empty(),
            seal: String::new(),
            declared_ledger_position: None,
        }
    }

    /// Records the generation of a source.
    pub fn with_generation(mut self, source: impl Into<String>, version: u64) -> Self {
        self.read_set.generation(source, version);
        self
    }

    /// Records the relations read: their seals and the seal of their
    /// content.
    ///
    /// The two go together on purpose: a base that sealed the
    /// relations but not their content could not check whether what was read
    /// is still the same, which is exactly what the content seal
    /// checks.
    pub fn with_relations(mut self, relations: &[RelationFact]) -> Self {
        for relation in relations {
            self.read_set.read(ReadStamp::relation(
                relation.identity(),
                relation.evidence.clone().unwrap_or_default(),
            ));
        }
        // The seal is computed over the MANIFEST and not over a separate
        // list: that way it covers everything the base has read, even if it is read
        // in several batches, and it cannot get out of sync with the
        // seals that name it.
        self.seal = the_seal_of_the_read_set(&self.read_set);
        self
    }

    /// Records the policy against which it decides.
    pub fn with_policy(mut self, policy: &TrustPolicy) -> Self {
        self.read_set
            .read(ReadStamp::policy(policy.id.clone(), policy.version.clone()));
        self
    }

    /// Records what has been read to DECIDE.
    ///
    /// It is the part of the base that is not the graph's: the evidence that the
    /// decision core consumes. Without it the base would say over which
    /// world the answer was computed without saying which evidence decided it, and a diff
    /// into which new evidence enters would be attributed to the
    /// hypothetical fact when the cause was another.
    pub fn with_reads(mut self, reads: &ReadSet) -> Self {
        for stamp in &reads.stamps {
            self.read_set.read(stamp.clone());
        }
        self
    }

    /// The fingerprint of the reads manifest: the digest of what was read,
    /// without rereading all of it.
    pub fn fingerprint(&self) -> String {
        self.read_set.fingerprint()
    }

    /// Whether the world's GENERATION is no longer the one that was read.
    ///
    /// The comparison is against the GRAPH generation, the only source
    /// that can be checked here. And a base that has not recorded a
    /// generation does not say the world moved: it says nothing, which is not
    /// the same.
    pub fn is_the_generation_stale(&self, generation: u64) -> bool {
        self.read_set
            .generations
            .iter()
            .find(|(source, _)| source == "graph")
            .map(|(_, expected)| *expected != generation)
            .unwrap_or(false)
    }

    /// Whether the CONTENT read is no longer the one that was read.
    ///
    /// The seal compares the digest of what was read with the digest of what was
    /// just read, and not the number of relations: a number is not a
    /// version.
    pub fn is_the_content_stale(&self, seal: &str) -> bool {
        self.seal != seal
    }

    /// Whether the EVIDENCE read to decide is no longer the same.
    ///
    /// The evidence does not touch the graph: it enters the evidence store and
    /// changes neither the number of relations nor the generation. That is why it
    /// needs its own check, or evidence that enters
    /// while evaluating would go unnoticed -- and the change of
    /// decision would be attributed to the hypothetical fact when the cause was the
    /// evidence that came in.
    pub fn is_the_evidence_stale(&self, reads: &ReadSet) -> bool {
        the_evidence_of(&self.read_set) != the_evidence_of(reads)
    }
}

/// The seals of the evidence of a manifest, in canonical order.
///
/// They are compared as SETS: the evidence that was read and the one just
/// read are the same if their seals are the same, regardless
/// of the order in which they were read.
fn the_evidence_of(read_set: &ReadSet) -> Vec<ReadStamp> {
    let mut stamps: Vec<ReadStamp> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Evidence)
        .cloned()
        .collect();
    stamps.sort();
    stamps
}

/// The fingerprint of the evidence read: the digest of its seals.
///
/// It is what an evidence `StaleBase` says: which evidence was read and
/// which evidence was found, without dumping the contents.
pub fn the_evidence_fingerprint(read_set: &ReadSet) -> String {
    ContentDigest::of_bytes(format!("{:?}", the_evidence_of(read_set)).as_bytes()).to_string()
}

/// The seal of the content read: the digest of the relations, with their
/// evidence.
///
/// The digest is computed over the SORTED list of relations, and not
/// over the order in which the graph gave them: two reads of the same
/// graph give the same seal, whatever order they are read in.
pub fn the_seal_of(relations: &[RelationFact]) -> String {
    let mut read_set = ReadSet::empty();
    for relation in relations {
        read_set.read(ReadStamp::relation(
            relation.identity(),
            relation.evidence.clone().unwrap_or_default(),
        ));
    }
    the_seal_of_the_read_set(&read_set)
}

/// The seal of the relations of a manifest.
///
/// ONLY the relations are sealed: the policy and the generations are
/// other sources, with their own verification, and putting them in the same
/// digest would make a policy change look like a change of
/// content.
///
/// Each line carries the LENGTH of its two parts. Without lengths, two
/// different reads could give the same text --a relation whose
/// evidence carries a newline inside-- and the digest of two
/// different worlds would be the same. A seal that does not distinguish
/// checks nothing.
fn the_seal_of_the_read_set(read_set: &ReadSet) -> String {
    let mut lines: Vec<String> = read_set
        .stamps
        .iter()
        .filter(|stamp| stamp.kind == ReadKind::Relation)
        .map(|stamp| {
            format!(
                "{}:{}:{}:{}",
                stamp.resource.len(),
                stamp.resource,
                stamp.verified_by.len(),
                stamp.verified_by
            )
        })
        .collect();
    lines.sort();
    ContentDigest::of_bytes(lines.join("\n").as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_evidence::domain::graph::RelationType;
    use kernel::Hrn;

    /// The subject of the test.
    fn subject() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
    }

    /// The component it depends on.
    fn component() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
    }

    /// The seal does not depend on the ORDER in which the relations are read: two
    /// reads of the same graph give the same seal.
    #[test]
    fn the_seal_does_not_depend_on_the_order_of_the_reads() {
        let first = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let second = RelationFact::new(RelationType::Contains, component(), subject())
            .with_evidence("evd-2");

        let one = the_seal_of(&[first.clone(), second.clone()]);
        let other = the_seal_of(&[second, first]);
        assert_eq!(one, other);
    }

    /// And the seal DOES change when what was read changes: a relation
    /// replaced by another leaves a different seal, even if the number of
    /// relations stays the same.
    #[test]
    fn the_seal_changes_when_a_relation_is_replaced() {
        let before = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let after = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-2");

        assert_ne!(
            the_seal_of(&[before]),
            the_seal_of(&[after]),
            "el mismo numero de relaciones, distinto sello"
        );
    }

    /// And also when what changes is the relation itself.
    #[test]
    fn the_seal_changes_when_the_relation_changes() {
        let other = Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2")
            .expect("un HRN");
        let before = RelationFact::new(RelationType::DependsOn, subject(), component());
        let after = RelationFact::new(RelationType::DependsOn, subject(), other);

        assert_ne!(the_seal_of(&[before]), the_seal_of(&[after]));
    }

    /// Two DIFFERENT reads cannot give the same seal: the seal
    /// encodes with lengths, and without lengths an evidence with a
    /// newline inside could pass for two relations.
    #[test]
    fn the_seal_tells_two_different_reads_apart() {
        let relation = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");

        // A relation with the evidence "x\n<other>|y" and two relations
        // with "x" and "y": without lengths, the text is the same.
        let disguised = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence(format!("x\n{}|y", relation.identity()));

        let one = the_seal_of(&[relation.clone(), disguised.clone()]);
        let other = the_seal_of(std::slice::from_ref(&relation));
        assert_ne!(
            one, other,
            "una lectura con una evidencia disfrazada no es la misma lectura"
        );
    }

    /// The base is stale when the generation moved or when the
    /// content read changed.
    #[test]
    fn a_base_is_stale_when_the_generation_or_the_content_moved() {
        let relation = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let seal = the_seal_of(std::slice::from_ref(&relation));
        let base = SimulationBase::new("acme", Utc::now())
            .with_generation("graph", 7)
            .with_relations(std::slice::from_ref(&relation));

        assert!(
            !base.is_the_generation_stale(7),
            "la misma generacion no la vuelve obsoleta"
        );
        assert!(
            base.is_the_generation_stale(8),
            "una generacion nueva la vuelve obsoleta"
        );
        assert!(
            !base.is_the_content_stale(&seal),
            "el mismo contenido no la vuelve obsoleta"
        );
        assert!(
            base.is_the_content_stale("otro-sello"),
            "un contenido nuevo la vuelve obsoleta"
        );
    }

    /// The seal covers EVERYTHING the base has read, even if it is read in
    /// several batches: a base that sealed only the last batch would leave
    /// what was read before unwatched, and the seal would lie about its scope.
    #[test]
    fn the_seal_covers_everything_the_base_read() {
        let first = RelationFact::new(RelationType::DependsOn, subject(), component())
            .with_evidence("evd-1");
        let second = RelationFact::new(RelationType::Contains, component(), subject())
            .with_evidence("evd-2");

        let base = SimulationBase::new("acme", Utc::now())
            .with_relations(std::slice::from_ref(&first))
            .with_relations(std::slice::from_ref(&second));

        assert_eq!(
            base.seal,
            the_seal_of(&[first.clone(), second.clone()]),
            "el sello cubre las dos tandas"
        );
        assert!(
            base.is_the_content_stale(&the_seal_of(std::slice::from_ref(&first))),
            "y si se lee solo la primera, el contenido se movio"
        );
    }

    /// The base keeps what was read to DECIDE: the evidence that
    /// the decision core consumes is part of the base, because without
    /// it the base does not say over which world the answer was computed.
    #[test]
    fn the_base_keeps_what_the_decision_read() {
        let mut reads = ReadSet::empty();
        reads.read(ReadStamp::evidence("evd-1", "sha256:aaa"));

        let base = SimulationBase::new("acme", Utc::now()).with_reads(&reads);

        assert!(
            base.read_set
                .stamps
                .iter()
                .any(|stamp| stamp.kind == ReadKind::Evidence),
            "la evidencia leida esta en el manifiesto de la base"
        );
    }
}
