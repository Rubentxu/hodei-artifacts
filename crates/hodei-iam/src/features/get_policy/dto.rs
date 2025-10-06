//! DTOs for Get Policy feature

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Query para obtener una política IAM por su HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    /// HRN de la política a obtener
    pub policy_hrn: Hrn,
}

/// Vista de una política IAM
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyView {
    /// HRN único de la política
    pub hrn: Hrn,

    /// Nombre de la política
    pub name: String,

    /// Contenido de la política en formato Cedar
    pub content: String,

    /// Descripción opcional de la política
    pub description: Option<String>,
}

