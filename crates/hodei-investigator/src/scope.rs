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
}

impl RetrievalScope {
    /// Un alcance con los limites por defecto.
    pub fn new(actor: Hrn, subject: Hrn) -> Self {
        Self {
            actor,
            subject,
            max_nodes: 200,
            max_depth: 4,
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
}

impl ScopedFact {
    /// Un hecho de evidencia, que es dato y no instruccion.
    pub fn untrusted(
        evidence_id: impl Into<String>,
        text: impl Into<String>,
        path: Vec<Hrn>,
    ) -> Self {
        Self {
            evidence_id: evidence_id.into(),
            text: text.into(),
            path,
            kind: NodeKind::Evidence,
            trust: TrustLevel::Untrusted,
        }
    }
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
