use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::domain::context::PolicyEvaluationContext;

/// Contrato para obligaciones que se ejecutan cuando una política lo requiere.
pub trait Obligation {
    fn enforce(&self, ctx: &PolicyEvaluationContext) -> Result<()>;
}

/// Ejemplo de obligación: registrar auditoría
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditObligation {
    pub log_event: String,
}

impl Obligation for AuditObligation {
    fn enforce(&self, _ctx: &PolicyEvaluationContext) -> Result<()> {
        // La implementación concreta se hará en la capa de aplicación/infrastructure
        Ok(())
    }
}
