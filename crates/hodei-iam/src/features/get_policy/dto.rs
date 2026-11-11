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

    /// Cuándo se creó la política, tal y como quedó persistido
    ///
    /// Es `None` para una fila escrita antes de que existieran las marcas de
    /// tiempo: no hay nada que devolver, y devolver el reloj del proceso sería
    /// inventar un dato.
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Cuándo se actualizó la política por última vez, tal y como quedó
    /// persistido
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
