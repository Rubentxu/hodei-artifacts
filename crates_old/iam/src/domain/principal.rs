use serde::{Deserialize, Serialize};
use crate::domain::{User, ServiceAccount};

/// Representa a la entidad que realiza una acción (el "principal" en ABAC).
/// Esto permite al motor de políticas tratar de forma unificada a usuarios y
/// cuentas de servicio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Principal {
    User(User),
    ServiceAccount(ServiceAccount),
}
