//! Los puertos de la simulacion.
//!
//! La simulacion **lee** y no escribe. No hay un puerto de escritura, y
//! esa es la garantia de aislamiento: no se puede escribir en el estado
//! real desde una simulacion porque no existe por donde hacerlo.
//!
//! Y lo que se lee lleva su VERSION: un puerto que no dice sobre que
//! generacion del mundo leyo no permite comprobar que el mundo no se
//! haya movido mientras se evaluaba, y una simulacion que no lo
//! comprueba devuelve un diff sobre un mundo que puede no existir.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::read_set::ReadSet;
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
    /// Es una guarda GRUESA: dice si el grafo ha cambiado de tamano, y
    /// no si las relaciones son las mismas. Un cambio que sustituye una
    /// relacion por otra no lo nota, y por eso la simulacion no se
    /// apoya solo en esto.
    ///
    /// El recuento es **de la cuenta**, no del sujeto: cuenta todas las
    /// relaciones de la cuenta. Una escritura que no toca los sujetos
    /// que la simulacion lee tambien lo mueve, y la simulacion se dice
    /// obsoleta. Es a proposito --fallar cerrado--, pero conviene saber
    ///lo antes de usarlo en produccion.
    async fn relation_count(&self) -> Result<usize, String>;

    /// Todas las relaciones del grafo.
    ///
    /// Es una lectura cara y por eso existe aparte: el estado del grafo
    /// se compara con lo que se leyo, y comparar necesita el estado
    /// entero, no solo el de los sujetos afectados.
    async fn all_relations(&self) -> Result<Vec<RelationFact>, String>;

    /// La generacion del grafo de la cuenta: la version que su historia
    /// ha alcanzado.
    ///
    /// Es un contador monotono, y lo que se compara antes y despues de
    /// simular para saber si el mundo se ha movido. El numero de
    /// relaciones no sirve para eso: un recuento no es una version.
    ///
    /// Y es de la **CUENTA**, no del sujeto: es la historia del grafo
    /// entero. Una escritura ajena al conjunto que la simulacion lee
    /// tambien invalida la simulacion.
    async fn generation(&self) -> Result<u64, String>;

    /// La cuenta a la que pertenece el grafo.
    ///
    /// La simulacion se hace sobre UNA cuenta, y la base que declara el
    /// fork tiene que ser de esa misma cuenta: una base de otra cuenta
    /// no es una base, es una respuesta sobre el mundo de otro.
    fn account(&self) -> String;
}

/// Quien decide.
///
/// La decision se pide con la politica **del fork** y con los hechos
/// hipoteticos, para que la decision de despues sea la de un mundo donde
/// los hechos hipoteticos pasaron.
///
/// El instante llega desde fuera: el nucleo de decision no lee el reloj
/// de pared. Un reloj inyectado es lo que hace que la misma base y los
/// mismos hechos den el mismo resultado, y una simulacion reproducible
/// es la que se puede auditar.
#[async_trait]
pub trait DecisionEvaluator: Send + Sync {
    /// La decision de un sujeto, en el instante que se le indica.
    ///
    /// La decision lleva **lo que leyo** para decidir: sin eso, la base
    /// de la simulacion diria sobre que mundo se calculo sin decir que
    /// evidencia lo decidio, y una evidencia que entra en mitad de la
    /// simulacion se atribuiria al hecho hipotetico cuando la causa fue
    /// otra.
    async fn decide(
        &self,
        subject: &Hrn,
        policy: &TrustPolicy,
        facts: &[Hypothetical],
        now: DateTime<Utc>,
    ) -> Result<TheDecision, String>;

    /// Lo que el evaluador lee DE VERDAD para decidir de un sujeto.
    ///
    /// Es la lectura que se repite al final de la simulacion para
    /// comprobar que la evidencia que consumio la decision sigue siendo
    /// la misma. Sin ella, una evidencia que entra mientras se evalua
    /// seria invisible: no toca el grafo, no cambia el numero de
    /// relaciones y no mueve la generacion.
    async fn the_reads_of(&self, subject: &Hrn) -> Result<ReadSet, String>;
}
