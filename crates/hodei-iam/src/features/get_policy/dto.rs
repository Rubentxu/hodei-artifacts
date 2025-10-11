//! DTOs for Get Policy feature

use kernel::Hrn;
use serde::{Deserialize, Serialize};
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Query para obtener una política IAM por su HRN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPolicyQuery {
    /// HRN de la política a obtener
    pub policy_hrn: Hrn,
}

impl ActionTrait for GetPolicyQuery {
    fn name() -> &'static str {
        "GetPolicy"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Policy".to_string()
    }
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

