//! El alcance de la recuperacion: lo que se puede mirar y lo que no.
//!
//! Una investigacion se hace sobre un subgrafo, y el subgrafo se pide
//! **expresamente**: quien pregunta (verificado), el sujeto y los limites.
//! Una consulta que se salga de lo pedido no es una consulta: es una fuga,
//! y la fuga de una cuenta a otra es el peor fallo que puede tener este
//! sistema.
//!
//! Por eso el alcance se comprueba **dos veces**: una cuando se recupera
//! (el que recupera autoriza ANTES de leer) y otra cuando se usa (el que
//! investiga comprueba lo que le han dado). La segunda comprobacion es la
//! que importa: un recuperador que se equivoca no puede filtrar nada.
//!
//! Y las cuentas NO se comparan con texto: se comparan los **componentes
//! canonicos del HRN**. Un `contains` o un `split` ad hoc es una
//! comprobacion que un HRN parecido engana; `account_id()` no.

use async_trait::async_trait;
use hodei_evidence::domain::graph::NodeKind;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

use crate::read_set::ReadSet;
use crate::redact::redact;

/// Cuanto se fia de un texto.
///
/// Un texto de evidencia es **dato**, nunca instruccion. Una descripcion
/// de SBOM la escribe el que publica el paquete, y un README lo escribe
/// quien quiera: los dos son texto de un atacante en cuanto el atacante
/// es quien publica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// El texto es evidencia: se lee, se cita, no se obedece.
    Untrusted,
    /// El texto lo escribio el sistema: eso si es instruccion.
    Trusted,
}

/// El presupuesto de una recuperacion.
///
/// Una investigacion es un recurso, y el contexto que se le da al modelo
/// tiene un coste: cuantas evidencias se cargan, cuantas relaciones
/// entran y cuantos bytes de contenido se bajan. Los limites son
/// **declarados**, y lo que no cabe se OMITE, no se trunca: no se baja
/// un blob entero para cortarlo despues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBudget {
    /// Cuantas evidencias se cargan, como maximo.
    pub max_objects: usize,
    /// Cuantos hechos de relacion entran, como maximo.
    pub max_relations: usize,
    /// Cuantos bytes de contenido se bajan, en total.
    pub max_bytes: u64,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        Self {
            max_objects: 100,
            max_relations: 400,
            max_bytes: 256 * 1024,
        }
    }
}

/// El alcance de una recuperacion.
///
/// El actor es la identidad **verificada** que pide la investigacion, y
/// la cuenta NO se dice: se toma del sujeto por su componente canonico.
/// Un actor de cuenta A y un sujeto de cuenta B no forman un alcance: lo
/// rechazan las dos comprobaciones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalScope {
    /// Quien pregunta, verificado.
    pub actor: Hrn,
    /// El sujeto del que se parte.
    pub subject: Hrn,
    /// Cuantos nodos puede tocar la recuperacion.
    pub max_nodes: usize,
    /// A cuanta profundidad puede bajar.
    pub max_depth: usize,
    /// Cuanto puede gastar la recuperacion.
    #[serde(default)]
    pub budget: RetrievalBudget,
}

impl RetrievalScope {
    /// Un alcance con los limites por defecto.
    pub fn new(actor: Hrn, subject: Hrn) -> Self {
        Self {
            actor,
            subject,
            max_nodes: 200,
            max_depth: 4,
            budget: RetrievalBudget::default(),
        }
    }

    /// La cuenta de la investigacion, por el componente canonico del
    /// sujeto. Nunca se dice como texto libre.
    pub fn the_account(&self) -> &str {
        self.subject.account_id()
    }

    /// Si el alcance cruza cuentas: el actor y el sujeto de cuentas
    /// distintas no forman un alcance.
    pub fn crosses_accounts(&self) -> bool {
        self.actor.account_id() != self.subject.account_id()
    }
}

/// El estado de un hecho recuperado.
///
/// Las distinciones SON el contrato: un contenido ausente (Missing), un
/// almacen que no contesto (Unavailable), un texto que no se pudo
/// decodificar (Undecodable) y un hecho que el presupuesto dejo fuera
/// (OmittedByBudget) son cuatro cosas distintas, y ninguna de ellas se
/// disfraza de texto inventado ni de ausencia silenciosa de hechos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// El contenido se leyo y se entrega.
    Loaded,
    /// La evidencia existe y su contenido no esta.
    Missing,
    /// El almacen no pudo contestar.
    Unavailable,
    /// El contenido existe y no se pudo decodificar como texto.
    Undecodable,
    /// El presupuesto lo dejo fuera ANTES de leerse.
    OmittedByBudget,
}

impl Default for FactStatus {
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self::Loaded
    }
}

/// Cuanto de lo que hay se entrego.
///
/// Una recuperacion parcial se DECLARA: un scope conocido pero
/// incompleto no puede presentarse como respuesta exhaustiva.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// Se entrego todo lo que hay.
    #[default]
    Complete,
    /// Se entrego una parte, y se dice cuanta se dejo fuera.
    Partial {
        /// Cuantos hechos se dejaron fuera por el presupuesto.
        omitted: usize,
    },
}

/// Un hecho recuperado, con su procedencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedFact {
    /// El identificador de la evidencia.
    ///
    /// Es lo que se cita, y una cita que no resuelve no vale.
    pub evidence_id: String,
    /// El texto. Es DATO, nunca instruccion.
    pub text: String,
    /// El camino del grafo por el que se llego al hecho.
    pub path: Vec<Hrn>,
    /// De que clase de nodo es el sujeto del camino.
    pub kind: NodeKind,
    /// Cuanto se fia del texto.
    pub trust: TrustLevel,
    /// El estado del hecho. Por defecto, cargado: los hechos que venian
    /// sin estado son hechos leidos.
    #[serde(default)]
    pub status: FactStatus,
}

impl ScopedFact {
    /// Un hecho de evidencia, que es dato y no instruccion. El texto se
    /// REDACTA aqui mismo: lo que sale hacia el modelo no lleva
    /// secretos.
    pub fn untrusted(
        evidence_id: impl Into<String>,
        text: impl Into<String>,
        path: Vec<Hrn>,
    ) -> Self {
        Self {
            evidence_id: evidence_id.into(),
            text: redact(&text.into()),
            path,
            kind: NodeKind::Evidence,
            trust: TrustLevel::Untrusted,
            status: FactStatus::Loaded,
        }
    }
}

/// Lo que una recuperacion entrega: los hechos, CUANTO de lo que hay y
/// el manifiesto de lo que se leyo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedContext {
    /// Los hechos, en el orden de su clave estable.
    pub facts: Vec<ScopedFact>,
    /// Si la recuperacion fue completa o se omitio algo.
    pub coverage: Coverage,
    /// El manifiesto de las lecturas: identificadores, digests y
    /// generaciones, sin contenido crudo.
    pub read_set: ReadSet,
}

/// Por que la recuperacion no pudo hacerse.
///
/// Un rechazo de autorizacion es EXPLICITO: nunca una respuesta vacia
/// que parezca cero evidencia, y nunca un texto que haya que adivinar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalError {
    /// Quien pregunta no puede leer el recurso. La puerta esta cerrada.
    NotAuthorized {
        /// Quien preguntaba.
        actor: String,
        /// El recurso que no puede leer.
        resource: String,
    },
    /// El mundo cambio mientras se leia: la generacion de una fuente al
    /// empezar la lectura ya no es la del final. No hay snapshot
    /// coherente que presentar, y una lectura obsoleta se dice
    /// obsoleta.
    Stale { source: String, expected: u64, found: u64 },
    /// El almacen no pudo atender. Negar es cerrar: si no se pudo
    /// comprobar el permiso, no se lee.
    Unavailable(String),
}

impl std::fmt::Display for RetrievalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized { actor, resource } => {
                write!(formatter, "{actor} is not allowed to read {resource}")
            }
            Self::Stale {
                source,
                expected,
                found,
            } => write!(
                formatter,
                "the world changed while reading: {source} was at generation {expected} and is at {found}"
            ),
            Self::Unavailable(reason) => {
                write!(formatter, "the retrieval is unavailable: {reason}")
            }
        }
    }
}

/// El permiso de lectura de la evidencia.
///
/// El Authorizer compartido llega por este puerto: cada recurso que se
/// lee se autoriza ANTES de leerse, y una denegacion o un error es una
/// puerta cerrada, nunca una lectura silenciosa. La IA no es autoridad:
/// este puerto lo implementa el motor de politicas determinista.
#[async_trait]
pub trait EvidenceReadAuthorizer: Send + Sync {
    /// Ok si el actor puede leer el recurso; Err con el por que si no.
    async fn can_read(&self, actor: &Hrn, resource: &Hrn) -> Result<(), String>;
}

/// Por que un hecho no cabe en el alcance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeViolation {
    /// El sujeto del hecho no es el sujeto de la pregunta.
    AnotherSubject,
    /// El camino es mas profundo de lo permitido.
    TooDeep(usize),
    /// Hay mas hechos de los que caben.
    TooMany(usize),
    /// El camino pasa por un nodo de otra cuenta.
    AnotherAccount,
}

impl std::fmt::Display for ScopeViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnotherSubject => {
                write!(formatter, "the fact is about another subject")
            }
            Self::TooDeep(depth) => {
                write!(
                    formatter,
                    "the path is {depth} hops deep and the scope allows fewer"
                )
            }
            Self::TooMany(count) => {
                write!(
                    formatter,
                    "there are {count} facts and the scope allows fewer"
                )
            }
            Self::AnotherAccount => {
                write!(
                    formatter,
                    "the path leaves the account of the investigation"
                )
            }
        }
    }
}

/// Comprueba que los hechos caben en el alcance.
///
/// Esta es la comprobacion que **no se fia** de quien recupero: el
/// servicio la hace sobre lo que le dieron, y por eso un recuperador que
/// se equivoca no puede meter nada. Las cuentas se comparan por los
/// componentes canonicos del HRN, nunca por texto.
pub fn check_the_scope(scope: &RetrievalScope, facts: &[ScopedFact]) -> Result<(), ScopeViolation> {
    // Un alcance que ya cruza cuentas no es un alcance: nada de lo que
    // traiga es de la cuenta del que pregunta.
    if scope.crosses_accounts() {
        return Err(ScopeViolation::AnotherAccount);
    }

    if facts.len() > scope.max_nodes {
        return Err(ScopeViolation::TooMany(facts.len()));
    }

    for fact in facts {
        if fact.path.is_empty() {
            return Err(ScopeViolation::AnotherSubject);
        }
        if fact.path.len() > scope.max_depth + 1 {
            return Err(ScopeViolation::TooDeep(fact.path.len() - 1));
        }
        // El camino tiene que empezar por el sujeto de la pregunta y no
        // puede salirse de la cuenta: componente canonico contra
        // componente canonico.
        if fact.path.first() != Some(&scope.subject) {
            return Err(ScopeViolation::AnotherSubject);
        }
        for node in &fact.path {
            if node.account_id() != scope.subject.account_id() {
                return Err(ScopeViolation::AnotherAccount);
            }
        }
    }

    Ok(())
}
