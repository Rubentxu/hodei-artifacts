//! El investigador: el que pregunta, comprueba y publica.
//!
//! El recorrido es el de una investigacion que se puede auditar:
//!
//! 1. Se **recupera** dentro de un alcance, y lo que se recupera se
//!    **comprueba** contra el alcance otra vez, porque un recuperador que
//!    se equivoca no puede meter nada.
//! 2. Se **pregunta** al modelo con el contexto delimitado y declarado
//!    como dato.
//! 3. La respuesta se **valida**: tiene que ser estructurada, y cada
//!    afirmacion tiene que citar evidencia que se resolvió de verdad.
//! 4. Una afirmacion que no vale **no se publica**: se rechaza la
//!    respuesta entera, porque una respuesta a medias es peor que ninguna.
//! 5. Lo que el modelo propone **no se ejecuta**: se registra y espera la
//!    aprobacion de una persona.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use kernel::Hrn;

use crate::claim::{AIClaim, ClaimError, ClaimKind, InvestigationTrace, check_the_claim};
use crate::guard::{ActionKind, CapabilityGuard, ProposedAction};
use crate::model::{ModelProvider, PROMPT_VERSION, the_prompt};
use crate::scope::{
    EvidenceReadAuthorizer, RetrievalError, RetrievalScope, ScopeViolation, ScopedFact,
    check_the_scope,
};

/// De donde se recupera la evidencia.
///
/// El recuperador es el BORDE de autorizacion: autoriza ANTES de leer,
/// y una denegacion es explicita, no una respuesta vacia.
#[async_trait::async_trait]
pub trait EvidenceRetriever: Send + Sync {
    /// El subgrafo del sujeto, dentro del alcance.
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<Vec<ScopedFact>, RetrievalError>;
}

/// De donde salen las afirmaciones de alcanzabilidad.
///
/// La alcanzabilidad es un hecho del grafo de codigo, y llega por su
/// propio camino: no es evidencia del artefacto, es evidencia del simbolo.
/// Por eso tiene su puerto, y por eso el investigador puede investigar un
/// caso sin alcanzabilidad (y entonces no dice nada de ella).
#[async_trait::async_trait]
pub trait ReachabilityFacts: Send + Sync {
    /// Los hechos de alcanzabilidad del sujeto.
    async fn facts_of(&self, subject: &Hrn) -> Result<Vec<ScopedFact>, String>;
}

/// Por que la investigacion no se pudo hacer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvestigationError {
    /// Quien pregunta no puede leer lo que pide. La puerta esta cerrada
    /// ANTES de recuperar nada.
    NotAuthorized { actor: String, resource: String },
    /// La recuperacion se salio del alcance.
    OutOfScope(ScopeViolation),
    /// La respuesta del modelo no se pudo leer.
    Unreadable(String),
    /// Una afirmacion no valido.
    InvalidClaim(ClaimError),
    /// El modelo no contesto.
    NoAnswer(String),
}

impl std::fmt::Display for InvestigationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized { actor, resource } => {
                write!(formatter, "{actor} is not allowed to read {resource}")
            }
            Self::OutOfScope(violation) => {
                write!(formatter, "the retrieval left the scope: {violation}")
            }
            Self::Unreadable(reason) => {
                write!(formatter, "the answer could not be read: {reason}")
            }
            Self::InvalidClaim(error) => {
                write!(formatter, "a claim is not valid: {error}")
            }
            Self::NoAnswer(reason) => write!(formatter, "there is no answer: {reason}"),
        }
    }
}

/// Lo que salio de una investigacion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Investigation {
    /// Las afirmaciones validas.
    pub claims: Vec<AIClaim>,
    /// La traza: quien, con que prompt, sobre que contexto y cuando.
    pub trace: InvestigationTrace,
}

/// El investigador.
pub struct Investigator {
    retriever: Arc<dyn EvidenceRetriever>,
    model: Arc<dyn ModelProvider>,
    guard: Arc<std::sync::Mutex<CapabilityGuard>>,
}

impl Investigator {
    /// El investigador con su recuperador, su modelo y su guardia.
    pub fn new(
        retriever: Arc<dyn EvidenceRetriever>,
        model: Arc<dyn ModelProvider>,
        guard: CapabilityGuard,
    ) -> Self {
        Self {
            retriever,
            model,
            guard: Arc::new(std::sync::Mutex::new(guard)),
        }
    }

    /// Investiga una pregunta sobre un sujeto.
    ///
    /// Devuelve las afirmaciones **validas** y la traza. Si alguna
    /// afirmacion no vale, **no** devuelve nada: una respuesta con una
    /// afirmacion fabricada dentro es una respuesta en la que no se puede
    /// apoyar nada.
    pub async fn investigate(
        &self,
        investigation: &str,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError> {
        // 0. El alcance que ya cruza cuentas se cierra ANTES de pedir
        // nada: un actor de cuenta A y un sujeto de cuenta B no forman
        // una investigacion, y el rechazo sale antes de recuperar.
        if scope.crosses_accounts() {
            return Err(InvestigationError::NotAuthorized {
                actor: scope.actor.to_string(),
                resource: scope.subject.to_string(),
            });
        }

        // 1. La recuperacion, y su comprobacion. La denegacion del
        // borde es explicita, y un almacen caido cierra, no abre.
        let facts = match self.retriever.retrieve(scope).await {
            Ok(facts) => facts,
            Err(RetrievalError::NotAuthorized { actor, resource }) => {
                return Err(InvestigationError::NotAuthorized { actor, resource });
            }
            Err(RetrievalError::Unavailable(reason)) => {
                return Err(InvestigationError::NoAnswer(reason));
            }
        };
        check_the_scope(scope, &facts).map_err(InvestigationError::OutOfScope)?;

        // 2. La pregunta, con el contexto delimitado.
        let request = the_prompt(question, &facts);
        let response = self
            .model
            .complete(&request)
            .await
            .map_err(InvestigationError::NoAnswer)?;

        // 3. La validacion de la respuesta estructurada.
        let claims = the_claims(&response.text)?;

        // Y de cada afirmacion, que las citas resuelvan de verdad.
        let known: Vec<String> = facts.iter().map(|fact| fact.evidence_id.clone()).collect();
        for claim in &claims {
            check_the_claim(claim, &known, the_allowed_kinds())
                .map_err(InvestigationError::InvalidClaim)?;
        }

        // 4. La traza.
        let trace = InvestigationTrace::new(
            investigation,
            response.provider,
            response.model,
            response.model_version,
            PROMPT_VERSION,
            known,
            now,
        );

        Ok(Investigation { claims, trace })
    }

    /// Registra una accion que el investigador quiere hacer.
    ///
    /// No la ejecuta: la deja esperando la aprobacion de una persona, y
    /// eso es todo lo que puede hacer.
    pub fn propose(&self, action: ProposedAction) -> crate::guard::TracedAction {
        let mut guard = self.guard.lock().expect("la guardia no esta envenenada");
        guard.propose(action).clone()
    }

    /// La guardia.
    pub fn guard(&self) -> &Arc<std::sync::Mutex<CapabilityGuard>> {
        &self.guard
    }
}

/// Los tipos de afirmacion que se admiten.
///
/// Los cuatro del dominio. La restriccion no es de los tipos sino de lo
/// que se hace con ellos: los hechos se comprueban contra las
/// herramientas, y las inferencias se declaran como tales.
fn the_allowed_kinds() -> &'static [ClaimKind] {
    &[
        ClaimKind::Fact,
        ClaimKind::Inference,
        ClaimKind::Unknown,
        ClaimKind::Recommendation,
    ]
}

/// Lee la respuesta estructurada del modelo.
///
/// Si no se puede leer, **no** se publica nada.
fn the_claims(text: &str) -> Result<Vec<AIClaim>, InvestigationError> {
    let parsed: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|error| InvestigationError::Unreadable(error.to_string()))?;
    let claims: Vec<AIClaim> = serde_json::from_value(parsed)
        .map_err(|error| InvestigationError::Unreadable(error.to_string()))?;
    if claims.is_empty() {
        return Err(InvestigationError::Unreadable(
            "the answer has no claims".to_string(),
        ));
    }
    Ok(claims)
}

/// La accion de leer, que no necesita aprobacion.
pub fn a_read(subject: impl Into<String>, requested_by: impl Into<String>) -> ProposedAction {
    ProposedAction {
        kind: ActionKind::Read,
        description: "read the evidence of the subject".to_string(),
        subject: subject.into(),
        requested_by: requested_by.into(),
        requested_at: Utc::now(),
    }
}
