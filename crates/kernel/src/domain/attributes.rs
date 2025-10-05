//! Tipos agnósticos para representar atributos de entidades
//!
//! Este módulo define estructuras de datos para representar valores de atributos
//! de forma completamente independiente de cualquier motor de políticas externo
//! (como Cedar). Los tipos aquí definidos pueden ser traducidos a formatos
//! específicos por capas de adaptación cuando sea necesario.
//!
//! # Principios de Diseño
//!
//! - **Agnóstico**: Sin dependencias de infraestructura externa
//! - **Auto-contenido**: Representa todos los tipos de datos necesarios
//! - **Recursivo**: Soporta estructuras anidadas (Sets, Records)
//! - **Serializable**: Compatible con JSON y otros formatos
//!
//! # Ejemplos
//!
//! ```
//! use kernel::domain::attributes::AttributeValue;
//! use std::collections::HashMap;
//!
//! // Primitivos
//! let email = AttributeValue::String("user@example.com".to_string());
//! let age = AttributeValue::Long(30);
//! let active = AttributeValue::Bool(true);
//!
//! // Colecciones
//! let tags = AttributeValue::Set(vec![
//!     AttributeValue::String("admin".to_string()),
//!     AttributeValue::String("developer".to_string()),
//! ]);
//!
//! // Records (objetos anidados)
//! let mut address = HashMap::new();
//! address.insert("city".to_string(), AttributeValue::String("Madrid".to_string()));
//! address.insert("postal_code".to_string(), AttributeValue::String("28001".to_string()));
//! let address_record = AttributeValue::Record(address);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ============================================================================
// AttributeValue - Representación agnóstica de valores
// ============================================================================

/// Representa el valor de un atributo de entidad de forma agnóstica
///
/// Este enum puede representar cualquier tipo de dato que una entidad
/// pueda necesitar para describir sus atributos, sin acoplarse a ningún
/// motor de políticas específico.
///
/// # Variantes
///
/// - `Bool`: Valor booleano (true/false)
/// - `Long`: Entero de 64 bits con signo
/// - `String`: Cadena de texto UTF-8
/// - `Set`: Conjunto (lista ordenada) de valores del mismo tipo
/// - `Record`: Mapa clave-valor (objeto anidado)
/// - `EntityRef`: Referencia a otra entidad por su identificador
///
/// # Notas sobre Serialización
///
/// Los valores se serializan de forma directa a JSON:
/// - `Bool` → `true` / `false`
/// - `Long` → número entero
/// - `String` → string JSON
/// - `Set` → array JSON
/// - `Record` → objeto JSON
/// - `EntityRef` → string con formato especial
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AttributeValue {
    /// Valor booleano
    #[serde(rename = "bool")]
    Bool(bool),

    /// Entero de 64 bits con signo
    #[serde(rename = "long")]
    Long(i64),

    /// Cadena de texto UTF-8
    #[serde(rename = "string")]
    String(String),

    /// Conjunto de valores (homogéneos o heterogéneos)
    ///
    /// En la práctica, los motores de políticas suelen requerir
    /// homogeneidad, pero este tipo lo permite para flexibilidad.
    #[serde(rename = "set")]
    Set(Vec<AttributeValue>),

    /// Registro (objeto anidado) con pares clave-valor
    #[serde(rename = "record")]
    Record(HashMap<String, AttributeValue>),

    /// Referencia a otra entidad
    ///
    /// El string debe contener el identificador completo de la entidad
    /// (por ejemplo, un HRN serializado)
    #[serde(rename = "entity_ref")]
    EntityRef(String),
}

impl AttributeValue {
    /// Crea un AttributeValue::Bool
    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    /// Crea un AttributeValue::Long
    pub fn long(value: i64) -> Self {
        Self::Long(value)
    }

    /// Crea un AttributeValue::String
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// Crea un AttributeValue::Set vacío
    pub fn empty_set() -> Self {
        Self::Set(Vec::new())
    }

    /// Crea un AttributeValue::Set con valores
    pub fn set(values: Vec<AttributeValue>) -> Self {
        Self::Set(values)
    }

    /// Crea un AttributeValue::Record vacío
    pub fn empty_record() -> Self {
        Self::Record(HashMap::new())
    }

    /// Crea un AttributeValue::Record con pares clave-valor
    pub fn record(values: HashMap<String, AttributeValue>) -> Self {
        Self::Record(values)
    }

    /// Crea un AttributeValue::EntityRef
    pub fn entity_ref(id: impl Into<String>) -> Self {
        Self::EntityRef(id.into())
    }

    /// Verifica si es un Bool
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Verifica si es un Long
    pub fn is_long(&self) -> bool {
        matches!(self, Self::Long(_))
    }

    /// Verifica si es un String
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Verifica si es un Set
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    /// Verifica si es un Record
    pub fn is_record(&self) -> bool {
        matches!(self, Self::Record(_))
    }

    /// Verifica si es un EntityRef
    pub fn is_entity_ref(&self) -> bool {
        matches!(self, Self::EntityRef(_))
    }

    /// Intenta obtener el valor como Bool
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    /// Intenta obtener el valor como Long
    pub fn as_long(&self) -> Option<i64> {
        if let Self::Long(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    /// Intenta obtener el valor como String
    pub fn as_string(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Intenta obtener el valor como Set
    pub fn as_set(&self) -> Option<&[AttributeValue]> {
        if let Self::Set(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Intenta obtener el valor como Record
    pub fn as_record(&self) -> Option<&HashMap<String, AttributeValue>> {
        if let Self::Record(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Intenta obtener el valor como EntityRef
    pub fn as_entity_ref(&self) -> Option<&str> {
        if let Self::EntityRef(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Retorna el nombre del tipo como string (útil para debugging)
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Bool",
            Self::Long(_) => "Long",
            Self::String(_) => "String",
            Self::Set(_) => "Set",
            Self::Record(_) => "Record",
            Self::EntityRef(_) => "EntityRef",
        }
    }
}

impl fmt::Display for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", v),
            Self::Long(v) => write!(f, "{}", v),
            Self::String(v) => write!(f, "\"{}\"", v),
            Self::Set(values) => {
                write!(f, "[")?;
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::Record(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\": {}", k, v)?;
                }
                write!(f, "}}")
            }
            Self::EntityRef(id) => write!(f, "EntityRef(\"{}\")", id),
        }
    }
}

// ============================================================================
// Conversiones convenientes desde tipos Rust nativos
// ============================================================================

impl From<bool> for AttributeValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for AttributeValue {
    fn from(value: i64) -> Self {
        Self::Long(value)
    }
}

impl From<i32> for AttributeValue {
    fn from(value: i32) -> Self {
        Self::Long(value as i64)
    }
}

impl From<String> for AttributeValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for AttributeValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Vec<AttributeValue>> for AttributeValue {
    fn from(values: Vec<AttributeValue>) -> Self {
        Self::Set(values)
    }
}

impl From<HashMap<String, AttributeValue>> for AttributeValue {
    fn from(map: HashMap<String, AttributeValue>) -> Self {
        Self::Record(map)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_value_bool() {
        let value = AttributeValue::bool(true);
        assert!(value.is_bool());
        assert_eq!(value.as_bool(), Some(true));
        assert_eq!(value.type_name(), "Bool");
    }

    #[test]
    fn attribute_value_long() {
        let value = AttributeValue::long(42);
        assert!(value.is_long());
        assert_eq!(value.as_long(), Some(42));
        assert_eq!(value.type_name(), "Long");
    }

    #[test]
    fn attribute_value_string() {
        let value = AttributeValue::string("hello");
        assert!(value.is_string());
        assert_eq!(value.as_string(), Some("hello"));
        assert_eq!(value.type_name(), "String");
    }

    #[test]
    fn attribute_value_set() {
        let value = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::long(2),
            AttributeValue::long(3),
        ]);
        assert!(value.is_set());
        assert_eq!(value.as_set().unwrap().len(), 3);
        assert_eq!(value.type_name(), "Set");
    }

    #[test]
    fn attribute_value_empty_set() {
        let value = AttributeValue::empty_set();
        assert!(value.is_set());
        assert_eq!(value.as_set().unwrap().len(), 0);
    }

    #[test]
    fn attribute_value_record() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), AttributeValue::string("Alice"));
        map.insert("age".to_string(), AttributeValue::long(30));

        let value = AttributeValue::record(map);
        assert!(value.is_record());
        assert_eq!(value.as_record().unwrap().len(), 2);
        assert_eq!(value.type_name(), "Record");
    }

    #[test]
    fn attribute_value_empty_record() {
        let value = AttributeValue::empty_record();
        assert!(value.is_record());
        assert_eq!(value.as_record().unwrap().len(), 0);
    }

    #[test]
    fn attribute_value_entity_ref() {
        let value = AttributeValue::entity_ref("hrn:aws:iam:us-east-1:123456789012:user/alice");
        assert!(value.is_entity_ref());
        assert_eq!(
            value.as_entity_ref(),
            Some("hrn:aws:iam:us-east-1:123456789012:user/alice")
        );
        assert_eq!(value.type_name(), "EntityRef");
    }

    #[test]
    fn attribute_value_nested_structures() {
        let mut inner_record = HashMap::new();
        inner_record.insert("city".to_string(), AttributeValue::string("Madrid"));
        inner_record.insert("postal_code".to_string(), AttributeValue::string("28001"));

        let mut outer_record = HashMap::new();
        outer_record.insert("name".to_string(), AttributeValue::string("Alice"));
        outer_record.insert("address".to_string(), AttributeValue::record(inner_record));

        let value = AttributeValue::record(outer_record);

        // Verificar estructura anidada
        let record = value.as_record().unwrap();
        let address = record.get("address").unwrap();
        assert!(address.is_record());

        let address_record = address.as_record().unwrap();
        assert_eq!(
            address_record.get("city").unwrap().as_string(),
            Some("Madrid")
        );
    }

    #[test]
    fn attribute_value_display() {
        assert_eq!(AttributeValue::bool(true).to_string(), "true");
        assert_eq!(AttributeValue::long(42).to_string(), "42");
        assert_eq!(AttributeValue::string("test").to_string(), "\"test\"");
    }

    #[test]
    fn attribute_value_from_conversions() {
        let _: AttributeValue = true.into();
        let _: AttributeValue = 42i64.into();
        let _: AttributeValue = 42i32.into();
        let _: AttributeValue = "hello".into();
        let _: AttributeValue = String::from("hello").into();
    }

    #[test]
    fn attribute_value_serialization_bool() {
        let value = AttributeValue::bool(true);
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_serialization_long() {
        let value = AttributeValue::long(42);
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_serialization_string() {
        let value = AttributeValue::string("hello");
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_serialization_set() {
        let value = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::long(2),
            AttributeValue::long(3),
        ]);
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_serialization_record() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), AttributeValue::string("Alice"));
        map.insert("age".to_string(), AttributeValue::long(30));

        let value = AttributeValue::record(map);
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_type_checks() {
        let bool_val = AttributeValue::bool(true);
        assert!(bool_val.is_bool());
        assert!(!bool_val.is_long());
        assert!(!bool_val.is_string());

        let long_val = AttributeValue::long(42);
        assert!(!long_val.is_bool());
        assert!(long_val.is_long());
        assert!(!long_val.is_string());
    }

    #[test]
    fn attribute_value_as_methods_return_none_for_wrong_type() {
        let bool_val = AttributeValue::bool(true);
        assert_eq!(bool_val.as_bool(), Some(true));
        assert_eq!(bool_val.as_long(), None);
        assert_eq!(bool_val.as_string(), None);
        assert_eq!(bool_val.as_set(), None);
        assert_eq!(bool_val.as_record(), None);
        assert_eq!(bool_val.as_entity_ref(), None);
    }
}
