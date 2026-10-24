//! DTOs for Get Policy feature

use kernel::Hrn;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;
use serde::{Deserialize, Serialize};

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

    /// Los principales a los que está adjunta la política
    ///
    /// El adjunto es lo que hace que la política sea *efectiva* para un
    /// principal, así que se reporta para que el consumidor pueda confirmar
    /// lo que quedó persistido.
    pub attached_principals: Vec<String>,
}
