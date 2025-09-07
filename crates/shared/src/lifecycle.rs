// crates/shared/src/lifecycle.rs

use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use crate::hrn::Hrn;

/// Representa el estado del ciclo de vida de un Agregado, unificado y sin ambigüedad.
/// Es una máquina de estados simple: Active -> Archived -> Deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleState {
    /// El recurso está activo y operativo.
    Active,
    /// El recurso está archivado. Generalmente es de solo lectura y puede ser restaurado.
    Archived { at: OffsetDateTime, by: Hrn },
    /// El recurso ha sido marcado para borrado o borrado lógicamente. Es irrecuperable.
    Deleted { at: OffsetDateTime, by: Hrn },
}

/// Un Value Object que contiene información completa y consistente del ciclo de vida de un Agregado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    /// Fecha y hora de creación del recurso.
    pub created_at: OffsetDateTime,
    /// HRN del principal (User o ApiKey) que creó el recurso.
    pub created_by: Hrn,
    /// Fecha y hora de la última modificación del recurso.
    pub updated_at: OffsetDateTime,
    /// HRN del principal que realizó la última modificación.
    pub updated_by: Hrn,
    /// El estado actual del recurso (Activo, Archivado o Borrado).
    pub state: LifecycleState,
}

impl Lifecycle {
    pub fn new(creator_hrn: Hrn) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            created_at: now,
            created_by: creator_hrn.clone(),
            updated_at: now,
            updated_by: creator_hrn,
            state: LifecycleState::Active,
        }
    }
}