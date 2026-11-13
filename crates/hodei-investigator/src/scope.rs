//! El alcance de la recuperacion: lo que se puede mirar y lo que no.
//!
//! Una investigacion se hace sobre un subgrafo, y el subgrafo se pide
//! **expresamente**: la cuenta, el sujeto y los limites. Una consulta que
//! se salga de lo pedido no es una consulta: es una fuga, y la fuga de una
//! cuenta a otra es el peor fallo que puede tener este sistema.
//!
//! Por eso el alcance se comprueba **dos veces**: una cuando se recupera
//! (el que recupera dice que no se sale) y otra cuando se usa (el que
//! investiga comprueba lo que le han dado). La segunda comprobacion es la
//! que importa: un recuperador que se equivoca no puede filtrar nada.

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalScope {
    /// La cuenta a la que pertenece la investigacion.
    pub account: String,
    /// El sujeto del que se parte.
    pub subject: Hrn,
    /// Cuantos nodos puede tocar la recuperacion.
    pub max_nodes: usize,
    /// A cuanta profundidad puede bajar.
    pub max_depth: usize,
}

impl RetrievalScope {
    /// Un alcance con los limites por defecto.
    pub fn new(account: impl Into<String>, subject: Hrn) -> Self {
        Self {
            account: account.into(),
            subject,
            max_nodes: 200,
            max_depth: 4,
        }
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
    /// El camino del grafo por el que se lleg\u00f3 al hecho.
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
/// se equivoca no puede meter nada.
pub fn check_the_scope(scope: &RetrievalScope, facts: &[ScopedFact]) -> Result<(), ScopeViolation> {
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
        // puede salirse de la cuenta.
        if fact.path.first() != Some(&scope.subject) {
            return Err(ScopeViolation::AnotherSubject);
        }
        for node in &fact.path {
            if !node.to_string().contains(&format!(":{}:", scope.account)) {
                return Err(ScopeViolation::AnotherAccount);
            }
        }
    }

    Ok(())
}
