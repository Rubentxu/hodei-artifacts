//! El investigador con IA: una explicacion sobre el grafo que se pueda
//! auditar.
//!
//! M8 es el hito mas exigente del roadmap, y su exigencia es una sola
//! cosa: **una explicacion con citas de evidencia y caminos verificables,
//! nunca decisiones autoautorizadas**.
//!
//! # La arquitectura es la garantia
//!
//! Un investigador con IA que puede actuar es un investigador que puede
//! equivocarse con consecuencias. Por eso aqui:
//!
//! - El **modelo no tiene por donde actuar**. Su puerto devuelve texto, y
//!   el texto se **valida** antes de publicarse. No hay un camino del
//!   modelo a las decisiones de confianza.
//! - Lo que el modelo dice se **comprueba con las herramientas**: una
//!   afirmacion de tipo HECHO necesita una cita que resuelva, y una cita
//!   que no resuelve **rechaza la respuesta entera**.
//! - La **inferencia se declara como inferencia**, y lo que no se sabe se
//!   dice que no se sabe. Una seguridad inventada es una invitacion a no
//!   comprobar nada.
//! - La **recuperacion tiene alcance**: la cuenta, el sujeto y los
//!   limites, y el alcance se comprueba dos veces (al recuperar y al
//!   usar), porque un recuperador que se equivoca no puede meter nada.
//! - Las **acciones que no son de lectura esperan aprobacion humana**, y
//!   la aprobacion queda en la traza con su nombre y su momento.
//!
//! # El texto de los terceros es DATO
//!
//! Una descripcion de SBOM la escribe quien publica el paquete. Un README
//! lo escribe quien quiera. Los dos son texto de un atacante en cuanto el
//! atacante es quien publica. Por eso el contexto va **delimitado y
//! declarado como dato**, y por eso el corpus de regresion de inyeccion
//! existe: los delimitadores **no** son la garantia (un modelo puede
//! obedecer un texto delimitado), la garantia es que lo que dice el modelo
//! se comprueba y que el modelo no tiene por donde actuar.

// # La conclusion del SPIKE-011
//
//! La salida del spike es: *la funcionalidad de IA solo procede si no puede
//! alterar el estado de confianza autoritativo y si la calidad de su
//! fundamento llega al umbral acordado*.
//!
//! Las dos condiciones se cumplen **por construccion**, y no por promesa:
//!
//! 1. **No puede alterar el estado de confianza.** El modelo no tiene por
//!    donde actuar: su puerto devuelve texto y el texto se comprueba. No
//!    hay un camino del modelo a las decisiones, y las decisiones las
//!    sigue tomando el motor de politicas determinista.
//! 2. **El fundamento se comprueba.** Un HECHO necesita una cita que
//!    resuelva, una cita que no resuelve **rechaza la respuesta
//!    entera**, y una respuesta a medias es peor que ninguna: se rechaza
//!    o no se publica.
//!
//! Y hay una tercera condicion, que es la que el rojo-team del spike
//! («una descripcion de SBOM es un texto controlado por el atacante»)
//! deja escrita: el texto de los terceros es DATO delimitado, y el corpus
//! de regresion demuestra que un texto de evidencia no dirige la
//! respuesta. Los delimitadores **no** son la garantia --un modelo puede
//! obedecer un texto delimitado--: la garantia es que lo que dice el
//! modelo se comprueba con las herramientas y que el modelo no tiene por
//! donde actuar.
//
//! El umbral de calidad se mide en la UAT de M8, y el modo de proveedor
//! es de fixture: el spike es sobre la **seguridad** del investigador, no
//! sobre la calidad de un modelo comercial, y esa calidad no se puede
//! medir sin el proveedor.

#![forbid(unsafe_code)]

pub mod claim;
pub mod corpus;
pub mod guard;
pub mod investigator;
pub mod model;
pub mod redact;
pub mod scope;

pub use claim::{AIClaim, ClaimError, ClaimKind, InvestigationTrace};
pub use guard::{
    ActionKind, ActionState, CapabilityGuard, GuardError, ProposedAction, TracedAction,
};
pub use investigator::{EvidenceRetriever, Investigation, InvestigationError, Investigator};
pub use model::{FixtureProvider, ModelProvider, ModelRequest, ModelResponse, PROMPT_VERSION};
pub use scope::{
    Coverage, EvidenceReadAuthorizer, FactStatus, RetrievalBudget, RetrievalError, RetrievalScope,
    RetrievedContext, ScopeViolation, ScopedFact, TrustLevel, check_the_scope,
};
