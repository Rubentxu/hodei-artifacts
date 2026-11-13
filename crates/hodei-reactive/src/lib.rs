//! El runtime de las reacciones de seguridad de Hodei.
//!
//! Cuando un riesgo nuevo toca un artefacto desplegado, la decision de
//! confianza tiene que cambiar **sin que nadie la pida**. Este crate es
//! la parte del sistema que se entera solo.
//!
//! # Lo que el runtime hace, y nada mas
//!
//! - **Suscribir**: un registro de comportamientos, cada uno con lo que
//!   le interesa.
//! - **Guardar**: la condicion se comprueba; si no se cumple, la reaccion
//!   no corre y el motivo queda en la traza.
//! - **Registrar**: la ejecucion es durable, con su cadena de causalidad,
//!   su numero de intentos y su traza.
//! - **Acotar**: un presupuesto corta la cadena cuando se excede, y el
//!   corte queda en la traza.
//!
//! # Lo que un manejador de eventos ordinario no tiene
//!
//! La idempotencia por clave, el presupuesto de bucles, la traza durable
//! y la reconstruccion sin repetir los efectos externos. Un manejador
//! ordinario es una llamada a una funcion: no tiene clave, ni presupuesto,
//! ni traza que sobreviva a un reinicio. Y la exit gate del hito lo pide
//! literal: **observable, acotado, idempotente y reconstruible**.

#![forbid(unsafe_code)]

pub mod behavior;
pub mod events;
pub mod execution;
pub mod intent;
pub mod reactions;
pub mod runtime;

pub use behavior::{
    Behavior, BehaviorError, BehaviorOutcome, Effect, GuardOutcome, Reaction, TraceEntry,
};
pub use events::{Event, EventKind, PayloadPredicate, Subscription};
pub use execution::{Budget, BudgetState, ExecutionOutcome, ExecutionRecord};
pub use intent::{
    InMemoryIntentOutbox, Intent, IntentExecutor, IntentOrigin, IntentOutbox, Plan,
    PlanningBehavior, ReadScope, Receipt,
};
pub use reactions::{
    BlastRadiusSource, DecisionRequester, DomainIntentExecutor, ExceptionExpiryReaction,
    ImpactedSubject, PlannedExceptionExpiryReaction, PlannedVulnerabilityReaction, ReasonCode,
    Reevaluation, SecurityCaseSink, VulnerabilityReaction,
};
pub use runtime::{
    DispatchReport, ExecutionStore, InMemoryExecutionStore, ReactiveRuntime, RuntimeError,
};
