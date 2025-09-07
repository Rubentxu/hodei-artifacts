// crates/shared/src/security/resources.rs

use std::collections::HashMap;

/// Un trait para cualquier entidad de dominio que pueda ser representada
/// como un recurso o principal en un sistema de autorización.
/// Esta interfaz genérica permite flexibilidad en la implementación de infraestructura.
pub trait HodeiResource<IdType, AttrType> {
    /// Devuelve el identificador único de la entidad en formato adecuado para el sistema de autorización.
    fn resource_id(&self) -> IdType;

    /// Devuelve un mapa de los atributos de la entidad para evaluación de políticas.
    /// Los valores están en formato que puede interpretar el motor de autorización específico.
    fn resource_attributes(&self) -> HashMap<String, AttrType>;

    /// Devuelve una lista de identificadores de recursos padres para herencia de políticas.
    /// Permite la jerarquía de autorización con tipos específicos del motor de autorización.
    fn resource_parents(&self) -> Vec<IdType>;
}