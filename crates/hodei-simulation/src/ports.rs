//! Los puertos de la simulacion.
//!
//! La simulacion **lee** y no escribe. No hay un puerto de escritura, y
//! esa es la garantia de aislamiento: no se puede escribir en el estado
//! real desde una simulacion porque no existe por donde hacerlo.

use async_trait::async_trait;
use hodei_trust_policy::TrustPolicy;
use kernel::Hrn;

use crate::diff::TheDecision;
use crate::hypothetical::{Hypothetical, RelationFact};

/// El grafo que la simulacion lee.
///
/// El grafo real **no se escribe nunca** desde aqui. El puerto no tiene
/// un metodo de escritura, y un puerto sin escritura no se puede usar
/// para escribir.
#[async_trait]
pub trait SimulationGraph: Send + Sync {
    /// Las relaciones de un sujeto: las que parten de el y las que llegan
    /// a el.
    async fn relations_of(&self, subject: &Hrn) -> Result<Vec<RelationFact>, String>;

    /// Cuantas relaciones tiene el grafo.
    ///
    /// Sirve para comprobar el aislamiento: si al terminar la simulacion
    /// el grafo tiene las mismas relaciones, el fork no toco nada.
    async fn relation_count(&self) -> Result<usize, String>;
}

/// Quien decide.
///
/// La decision se pide con la politica **del fork** y con los hechos
/// hipoteticos, para que la decision de despues sea la de un mundo donde
/// los hechos hipoteticos pasaron.
#[async_trait]
pub trait DecisionEvaluator: Send + Sync {
    /// La decision de un sujeto.
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
    ) -> Result<TheDecision, String>;
}
