//! El servicio que compone la respuesta contrafactual.
//!
//! El recorrido es corto y falla cerrado:
//!
//! 1. Primero se **simula**: si la hipotesis no es valida --de otra
//!    cuenta, caducada, contra un mundo que ya no existe o sin version
//!    comprobable--, se rechaza ANTES de recuperar nada. No se filtraria
//!    dato alguno del mundo que la hipotesis decia tocar, y una
//!    respuesta a medias sobre un mundo equivocado es peor que ningun
//!    rechazo.
//! 2. Despues se **investiga**: la causa, con sus citas y su sello de
//!    lecturas.
//! 3. Se comprueba el **sello compartido**: las fuentes que los dos
//!    manifiestos nombran tienen que estar en la misma version. Si no,
//!    la investigacion y la simulacion no hablan del mismo mundo y la
//!    respuesta se rechaza como sello incoherente.
//! 4. Se compone el informe: etiquetas epistemicas, referencias y los
//!    dos contratos interiores intactos.
//!
//! Y en ningun momento se escribe: la simulacion no tiene por donde
//! escribir, y la investigacion solo lee. Lo que se propone se queda
//! esperando aprobacion humana en la guardia del investigador.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hodei_investigator::investigator::{InvestigationError, Investigator};
use hodei_investigator::scope::RetrievalScope;
use hodei_simulation::service::SimulationService;
use hodei_simulation::{SimulationError, SimulationFork};

use crate::report::{CounterfactualReport, build_report, the_seal_coherence};

/// Por que la hipotesis no merecio respuesta.
///
/// El motivo es una CLASE, no un detalle: los mensajes de rechazo no
/// llevan cuentas, HRNs ni identificadores ajenos, porque rechazar una
/// hipotesis de otro mundo no es una ocasion para describir ese otro
/// mundo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// La hipotesis pertenece a otra cuenta.
    OtherAccount,
    /// La hipotesis ha vivido mas de lo permitido.
    Expired,
    /// La hipotesis es mas grande de lo permitido.
    TooLarge,
    /// El mundo se movio mientras se evaluaba: la base ya no existe.
    StaleWorld,
    /// La base no declara una version comprobable.
    UnverifiableBase,
    /// El mundo no se pudo leer.
    Unavailable,
    /// La investigacion y la simulacion no coinciden en la version de
    /// una fuente compartida.
    IncoherentSeal,
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OtherAccount => write!(formatter, "the hypothesis belongs to another account"),
            Self::Expired => write!(formatter, "the hypothesis has expired"),
            Self::TooLarge => write!(formatter, "the hypothesis is too large"),
            Self::StaleWorld => write!(formatter, "the world of the hypothesis no longer exists"),
            Self::UnverifiableBase => {
                write!(
                    formatter,
                    "the base of the hypothesis declares no checkable version"
                )
            }
            Self::Unavailable => write!(formatter, "the world could not be read"),
            Self::IncoherentSeal => {
                write!(
                    formatter,
                    "the reads of the investigation and the base of the simulation do not agree on a shared source version"
                )
            }
        }
    }
}

/// Por que no hay respuesta contrafactual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterfactualError {
    /// La hipotesis se ha rechazado, con su clase y sin sus datos.
    Rejected {
        /// La clase del rechazo.
        reason: RejectionReason,
    },
    /// La investigacion no ha podido responder: su propio error lo
    /// dice, con el alcance de quien pregunta.
    Investigation(InvestigationError),
}

impl std::fmt::Display for CounterfactualError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected { reason } => write!(formatter, "the hypothesis is rejected: {reason}"),
            Self::Investigation(error) => {
                write!(formatter, "the investigation could not answer: {error}")
            }
        }
    }
}

impl std::error::Error for CounterfactualError {}

/// El servicio de la investigacion contrafactual.
pub struct CounterfactualService {
    investigator: Arc<Investigator>,
    simulation: Arc<SimulationService>,
}

impl CounterfactualService {
    /// El servicio sobre un investigador y un simulador.
    pub fn new(investigator: Arc<Investigator>, simulation: Arc<SimulationService>) -> Self {
        Self {
            investigator,
            simulation,
        }
    }

    /// Responde a la pregunta bajo la hipotesis del fork.
    ///
    /// El `now` es el mismo reloj para las dos mitades: es el que hace
    /// que el sello de una respuesta se pueda comparar con el de la
    /// otra, y el que la deja reproducible.
    pub async fn answer(
        &self,
        counterfactual: &str,
        scope: &RetrievalScope,
        question: &str,
        fork: &SimulationFork,
        now: DateTime<Utc>,
    ) -> Result<CounterfactualReport, CounterfactualError> {
        // 1. Primero la hipotesis: un rechazo no gasta lecturas.
        let simulation = self
            .simulation
            .simulate_result(fork, now)
            .await
            .map_err(the_rejection)?;

        // 2. Despues la causa.
        let investigation = self
            .investigator
            .investigate(counterfactual, scope, question, now)
            .await
            .map_err(CounterfactualError::Investigation)?;

        // 3. Y las dos tienen que hablar del mismo mundo.
        let seal = the_seal_coherence(&investigation.report.read_set, &simulation.base.sources);
        if !seal.coherent {
            return Err(CounterfactualError::Rejected {
                reason: RejectionReason::IncoherentSeal,
            });
        }

        // 4. Componer, sin decidir.
        Ok(build_report(
            counterfactual,
            question,
            &investigation.report,
            &simulation,
            seal,
        ))
    }
}

/// El rechazo de un fallo de simulacion: la clase, nunca el detalle.
///
/// Los mensajes de `SimulationError` llevan cuentas e identidades para
/// quien simula; aqui se cambia el detalle por la clase porque quien
/// recibe el rechazo no tiene porque ver el mundo de otra cuenta.
fn the_rejection(error: SimulationError) -> CounterfactualError {
    let reason = match error {
        SimulationError::NotAllowed(_) => RejectionReason::OtherAccount,
        SimulationError::Expired(_) => RejectionReason::Expired,
        SimulationError::TooLarge(_) => RejectionReason::TooLarge,
        SimulationError::StaleBase { .. } => RejectionReason::StaleWorld,
        SimulationError::NoVersion(_) => RejectionReason::UnverifiableBase,
        SimulationError::Unavailable(_) | SimulationError::ReadFailed(_) => {
            RejectionReason::Unavailable
        }
    };
    CounterfactualError::Rejected { reason }
}
