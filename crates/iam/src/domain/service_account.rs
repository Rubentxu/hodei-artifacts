use serde::{Deserialize, Serialize};
use shared::ServiceAccountId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceAccount {
    pub id: ServiceAccountId,
    pub name: String,
    // Atributos flexibles para el motor de políticas ABAC (Cedar)
    pub attributes: serde_json::Value,
}
