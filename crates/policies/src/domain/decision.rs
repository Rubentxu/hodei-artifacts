use serde::{Deserialize, Serialize};

/// Resultado de una evaluación de políticas
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub obligations: Vec<ObligationEffect>,
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    pub message: String,
    pub code: Option<String>,
}

/// Efectos a aplicar tras una decisión (obligations)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObligationEffect {
    Audit { log_event: String },
}

impl PolicyDecision {
    pub fn allow() -> Self {
        Self { allowed: true, reasons: vec![], obligations: vec![], advice: vec![] }
    }
    pub fn deny(reason: impl Into<String>) -> Self {
        Self { allowed: false, reasons: vec![reason.into()], obligations: vec![], advice: vec![] }
    }
}
