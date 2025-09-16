// crates/shared/src/resource.rs

use std::collections::HashMap;

use serde_json::Value;

use crate::hrn::Hrn;

/// Trait común para recursos autorizables del dominio.
/// Fija los tipos a `Hrn` y `serde_json::Value` para integrarse con el motor de políticas.
pub trait HodeiResource {
    /// Identificador único del recurso en formato HRN.
    fn resource_id(&self) -> Hrn;

    /// Atributos del recurso en un mapa serializable.
    fn resource_attributes(&self) -> HashMap<String, Value>;

    /// Lista de HRN de recursos padre para herencia/álbol de autorización.
    fn resource_parents(&self) -> Vec<Hrn>;
}
