//! The simulation store: the adapters that read the real state.
//!
//! The real graph is read and **never written**: the port
//! [`SimulationGraph`] has no write method, and so there is
//! nowhere to write through. The adapter here is the piece that
//! keeps that promise in the real world, and for that reason it also says the
//! VERSION of the graph it reads: a simulation that does not say which
//! generation of the world it was computed on cannot be audited.

pub mod dgraph;
pub mod evaluator;

pub use dgraph::{
    AnchorError, DgraphConformance, DgraphSimulationAnchor, DgraphSimulationGraph,
    SIMULATION_ANCHOR_TYPE, SIMULATION_SCHEMA_DQL, SimulationAnchor, anchor_key,
};
pub use evaluator::CapsuleDecisionEvaluator;
