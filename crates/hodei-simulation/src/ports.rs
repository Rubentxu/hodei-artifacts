//! The ports of the simulation.
//!
//! The simulation **reads** and does not write. There is no write port, and
//! that is the isolation guarantee: it is not possible to write to the real state
//! from a simulation because there is no way to do it.
//!
//! And what is read carries its VERSION: a port that does not say which
//! generation of the world it read does not allow checking that the world has not
//! moved while it was being evaluated, and a simulation that does not
//! check it returns a diff on a world that may not exist.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::read_set::ReadSet;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;

use crate::diff::TheDecision;
use crate::hypothetical::{Hypothetical, RelationFact};

/// The graph that the simulation reads.
///
/// The real graph is **never written** from here. The port has no
/// write method, and a port without writes cannot be used
/// to write.
#[async_trait]
pub trait SimulationGraph: Send + Sync {
    /// The relations of a subject: those that leave it and those that arrive
    /// at it.
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String>;

    /// How many relations the graph has.
    ///
    /// It is a COARSE guard: it says whether the graph has changed size, and
    /// not whether the relations are the same. A change that replaces one
    /// relation with another does not notice it, and for that reason the simulation does not
    /// rely only on this.
    ///
    /// The count is **per account**, not per subject: it counts all the
    /// relations of the account. A write that does not touch the subjects
    /// that the simulation reads also moves it, and the simulation declares itself
    /// stale. It is on purpose --failing closed--, but it is worth knowing
    /// before using it in production.
    async fn relation_count(&self) -> Result<usize, String>;

    /// All the relations of the graph.
    ///
    /// It is an expensive read and for that reason it exists separately: the state of the graph
    /// is compared with what was read, and comparing needs the whole
    /// state, not just that of the affected subjects.
    async fn all_relations(&self) -> Result<Vec<RelationFact>, String>;

    /// The generation of the account's graph: the version that its history
    /// has reached.
    ///
    /// It is a monotonic counter, and it is what is compared before and after
    /// simulating to know whether the world has moved. The number of
    /// relations is no use for that: a count is not a version.
    ///
    /// And it belongs to the **ACCOUNT**, not the subject: it is the history of the whole
    /// graph. A write foreign to the set that the simulation reads
    /// also invalidates the simulation.
    async fn generation(&self) -> Result<u64, String>;

    /// The account the graph belongs to.
    ///
    /// The simulation is done over ONE account, and the base that the
    /// fork declares has to be from that same account: a base from another account
    /// is not a base, it is an answer about someone else's world.
    fn account(&self) -> String;
}

/// Whoever decides.
///
/// The decision is requested with the **fork's** policy and with the hypothetical
/// facts, so that the later decision is that of a world where the
/// hypothetical facts happened.
///
/// The instant arrives from outside: the decision core does not read the wall
/// clock. An injected clock is what makes the same base and the
/// same facts give the same result, and a reproducible simulation
/// is one that can be audited.
#[async_trait]
pub trait DecisionEvaluator: Send + Sync {
    /// The decision of a subject, at the instant it is given.
    ///
    /// The decision carries **what it read** in order to decide: without that, the base
    /// of the simulation would say on which world it was computed without saying which
    /// evidence decided it, and evidence that enters in the middle of the
    /// simulation would be attributed to the hypothetical fact when the cause was
    /// another.
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
        now: DateTime<Utc>,
    ) -> Result<TheDecision, String>;

    /// What the evaluator ACTUALLY reads in order to decide about a subject.
    ///
    /// It is the read that is repeated at the end of the simulation to
    /// check that the evidence consumed by the decision is still
    /// the same. Without it, evidence that enters while evaluating
    /// would be invisible: it does not touch the graph, does not change the number of
    /// relations and does not move the generation.
    async fn the_reads_of(&self, subject: &Hrn) -> Result<ReadSet, String>;
}
