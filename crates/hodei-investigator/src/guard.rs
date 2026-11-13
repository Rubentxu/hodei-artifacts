//! La guardia de capacidades: lo que el investigador puede hacer y lo que
//! necesita aprobacion.
//!
//! El investigador **lee**. Leer no necesita permiso: es lo que se le ha
//! pedido. Pero cualquier otra cosa --abrir un PR, tocar un despliegue,
//! reevaluar una decision-- **necesita la aprobacion de una persona
//! autorizada**, y la aprobacion queda en la traza.
//!
//! Y mientras no haya aprobacion, la accion **sigue pendiente**: no se
//! ejecuta, no se programa y no se olvida. Una accion a la espera de una
//! aprobacion que nadie ve es una accion que se ejecuta sola un dia.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lo que el investigador quiere hacer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedAction {
    /// Que clase de accion es.
    pub kind: ActionKind,
    /// Que dice la accion, en palabras.
    pub description: String,
    /// De quien es la decision que la accion tomaria.
    pub subject: String,
    /// Quien la pide.
    pub requested_by: String,
    /// Cuando se pide.
    pub requested_at: DateTime<Utc>,
}

/// Que clase de accion es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Leer. No necesita aprobacion.
    Read,
    /// Escribir en el grafo o en las decisiones. Necesita aprobacion.
    Mutate,
    /// Correr algo. Necesita aprobacion.
    Execute,
    /// Proponer una promocion. Necesita aprobacion.
    Promote,
}

/// Lo que una accion puede hacer.
impl ActionKind {
    /// Si la accion necesita aprobacion.
    ///
    /// La lectura no: es lo que se le pidio. Todo lo demas, si.
    pub fn needs_approval(&self) -> bool {
        !matches!(self, Self::Read)
    }
}

/// El estado de una accion propuesta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionState {
    /// A la espera de una aprobacion.
    Pending {
        /// Quien puede aprobar.
        approver: String,
    },
    /// Aprobada.
    Approved {
        /// Quien la aprobo.
        by: String,
        /// Cuando.
        at: DateTime<Utc>,
    },
    /// Denegada.
    Denied {
        /// Quien la denego.
        by: String,
        /// Cuando.
        at: DateTime<Utc>,
        /// Por que.
        reason: String,
    },
}

/// Una accion propuesta y su estado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracedAction {
    /// La accion.
    pub action: ProposedAction,
    /// Su estado.
    pub state: ActionState,
}

impl TracedAction {
    /// Una accion que espera aprobacion.
    pub fn pending(action: ProposedAction, approver: impl Into<String>) -> Self {
        Self {
            action,
            state: ActionState::Pending {
                approver: approver.into(),
            },
        }
    }

    /// La accion sigue esperando.
    pub fn is_pending(&self) -> bool {
        matches!(self.state, ActionState::Pending { .. })
    }

    /// La accion esta aprobada.
    pub fn is_approved(&self) -> bool {
        matches!(self.state, ActionState::Approved { .. })
    }
}

/// La guardia de capacidades.
///
/// La guardia **no** ejecuta nada: registra lo que se pide y deja claro
/// que hace falta una aprobacion. Y la aprobacion la da una persona, con
/// su nombre y su momento, y eso queda en la traza.
#[derive(Debug, Clone, Default)]
pub struct CapabilityGuard {
    actions: Vec<TracedAction>,
    approvers: Vec<String>,
}

impl CapabilityGuard {
    /// Una guardia sin aprobadores: nadie puede aprobar nada.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Una guardia con quien puede aprobar.
    pub fn with_approvers(approvers: Vec<String>) -> Self {
        Self {
            actions: Vec::new(),
            approvers,
        }
    }

    /// Registra una accion propuesta.
    ///
    /// Una accion de lectura queda **aprobada** de entrada, porque leer es
    /// lo que se le ha pedido. Todo lo demos queda **pendiente**.
    pub fn propose(&mut self, action: ProposedAction) -> &TracedAction {
        if action.kind.needs_approval() {
            let approver = self
                .approvers
                .first()
                .cloned()
                .unwrap_or_else(|| "nadie".to_string());
            self.actions.push(TracedAction::pending(action, approver));
        } else {
            self.actions.push(TracedAction {
                action,
                state: ActionState::Approved {
                    by: "la propia peticion".to_string(),
                    at: Utc::now(),
                },
            });
        }
        self.actions.last().expect("la accion se acaba de anadir")
    }

    /// Aprueba una accion, si quien aprueba puede.
    pub fn approve(&mut self, index: usize, by: &str, at: DateTime<Utc>) -> Result<(), GuardError> {
        let approver = self
            .approvers
            .iter()
            .find(|one| one.as_str() == by)
            .ok_or(GuardError::NotAnApprover(by.to_string()))?;
        let action = self
            .actions
            .get_mut(index)
            .ok_or(GuardError::NoSuchAction(index))?;
        if !action.is_pending() {
            return Err(GuardError::NotPending);
        }
        action.state = ActionState::Approved {
            by: approver.clone(),
            at,
        };
        Ok(())
    }

    /// Deniega una accion.
    pub fn deny(
        &mut self,
        index: usize,
        by: &str,
        at: DateTime<Utc>,
        reason: impl Into<String>,
    ) -> Result<(), GuardError> {
        let approver = self
            .approvers
            .iter()
            .find(|one| one.as_str() == by)
            .ok_or(GuardError::NotAnApprover(by.to_string()))?;
        let action = self
            .actions
            .get_mut(index)
            .ok_or(GuardError::NoSuchAction(index))?;
        if !action.is_pending() {
            return Err(GuardError::NotPending);
        }
        action.state = ActionState::Denied {
            by: approver.clone(),
            at,
            reason: reason.into(),
        };
        Ok(())
    }

    /// Las acciones registradas.
    pub fn actions(&self) -> &[TracedAction] {
        &self.actions
    }

    /// Las que siguen esperando.
    pub fn pending(&self) -> Vec<&TracedAction> {
        self.actions
            .iter()
            .filter(|action| action.is_pending())
            .collect()
    }
}

/// Por que la guardia no pudo hacer lo que se le pedia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// Quien aprueba no puede aprobar.
    NotAnApprover(String),
    /// No hay esa accion.
    NoSuchAction(usize),
    /// La accion no estaba esperando.
    NotPending,
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnApprover(name) => {
                write!(formatter, "{name} is not an approver")
            }
            Self::NoSuchAction(index) => write!(formatter, "there is no action {index}"),
            Self::NotPending => write!(formatter, "the action is not pending"),
        }
    }
}
