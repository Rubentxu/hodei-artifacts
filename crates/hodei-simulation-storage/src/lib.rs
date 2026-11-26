//! El almacen de la simulacion: los adaptadores que leen el estado real.
//!
//! El grafo real se lee y **no se escribe**: el puerto
//! [`SimulationGraph`] no tiene un metodo de escritura, y por eso no
//! existe por donde escribir. El adaptador de aqui es la pieza que
//! cumple esa promesa en el mundo real, y por eso ademas dice la
//! VERSION del grafo que lee: una simulacion que no dice sobre que
//! generacion del mundo se calculo no se puede auditar.

pub mod dgraph;
pub mod evaluator;

pub use dgraph::{
    AnchorError, DgraphConformance, DgraphSimulationAnchor, DgraphSimulationGraph,
    SIMULATION_ANCHOR_TYPE, SIMULATION_SCHEMA_DQL, SimulationAnchor, anchor_key,
};
pub use evaluator::CapsuleDecisionEvaluator;
