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

    // ========================================================================
    // Tests adicionales: Estructuras anidadas profundas
    // ========================================================================

    #[test]
    fn attribute_value_deeply_nested_records() {
        let mut level3 = HashMap::new();
        level3.insert("value".to_string(), AttributeValue::long(42));

        let mut level2 = HashMap::new();
        level2.insert("nested".to_string(), AttributeValue::record(level3));

        let mut level1 = HashMap::new();
        level1.insert("data".to_string(), AttributeValue::record(level2));

        let root = AttributeValue::record(level1);

        // Navegar por la estructura
        let record1 = root.as_record().unwrap();
        let data = record1.get("data").unwrap();
        let record2 = data.as_record().unwrap();
        let nested = record2.get("nested").unwrap();
        let record3 = nested.as_record().unwrap();
        let value = record3.get("value").unwrap();

        assert_eq!(value.as_long(), Some(42));
    }

    #[test]
    fn attribute_value_deeply_nested_sets() {
        let level4 = AttributeValue::set(vec![AttributeValue::long(1), AttributeValue::long(2)]);
        let level3 = AttributeValue::set(vec![level4]);
        let level2 = AttributeValue::set(vec![level3]);
        let level1 = AttributeValue::set(vec![level2]);

        assert!(level1.is_set());
        let set1 = level1.as_set().unwrap();
        assert_eq!(set1.len(), 1);

        let set2 = set1[0].as_set().unwrap();
        assert_eq!(set2.len(), 1);

        let set3 = set2[0].as_set().unwrap();
        assert_eq!(set3.len(), 1);

        let set4 = set3[0].as_set().unwrap();
        assert_eq!(set4.len(), 2);
    }

    #[test]
    fn attribute_value_mixed_nested_structures() {
        // Set que contiene Records que contienen Sets
        let mut inner_record = HashMap::new();
        inner_record.insert(
            "tags".to_string(),
            AttributeValue::set(vec![
                AttributeValue::string("admin"),
                AttributeValue::string("developer"),
            ]),
        );

        let set_of_records = AttributeValue::set(vec![
            AttributeValue::record(inner_record.clone()),
            AttributeValue::record(inner_record),
        ]);

        let outer_set = set_of_records.as_set().unwrap();
        assert_eq!(outer_set.len(), 2);

        let first_record = outer_set[0].as_record().unwrap();
        let tags = first_record.get("tags").unwrap();
        assert_eq!(tags.as_set().unwrap().len(), 2);
    }

    // ========================================================================
    // Tests adicionales: Sets con tipos mixtos
    // ========================================================================

    #[test]
    fn attribute_value_heterogeneous_set() {
        let mixed_set = AttributeValue::set(vec![
            AttributeValue::long(42),
            AttributeValue::string("hello"),
            AttributeValue::bool(true),
        ]);

        let items = mixed_set.as_set().unwrap();
        assert_eq!(items.len(), 3);
        assert!(items[0].is_long());
        assert!(items[1].is_string());
        assert!(items[2].is_bool());
    }

    #[test]
    fn attribute_value_empty_collections() {
        let empty_set = AttributeValue::empty_set();
        assert_eq!(empty_set.as_set().unwrap().len(), 0);

        let empty_record = AttributeValue::empty_record();
        assert_eq!(empty_record.as_record().unwrap().len(), 0);
    }

    // ========================================================================
    // Tests adicionales: Igualdad y clonación
    // ========================================================================

    #[test]
    fn attribute_value_equality() {
        let val1 = AttributeValue::long(42);
        let val2 = AttributeValue::long(42);
        let val3 = AttributeValue::long(43);

        assert_eq!(val1, val2);
        assert_ne!(val1, val3);
    }

    #[test]
    fn attribute_value_equality_complex() {
        let mut map1 = HashMap::new();
        map1.insert("name".to_string(), AttributeValue::string("Alice"));
        map1.insert("age".to_string(), AttributeValue::long(30));

        let mut map2 = HashMap::new();
        map2.insert("name".to_string(), AttributeValue::string("Alice"));
        map2.insert("age".to_string(), AttributeValue::long(30));

        let record1 = AttributeValue::record(map1);
        let record2 = AttributeValue::record(map2);

        assert_eq!(record1, record2);
    }

    #[test]
    fn attribute_value_clone() {
        let original = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::string("test"),
        ]);

        let cloned = original.clone();
        assert_eq!(original, cloned);

        // Verificar que es una copia profunda
        assert_eq!(cloned.as_set().unwrap().len(), 2);
    }

    #[test]
    fn attribute_value_clone_deep_structure() {
        let mut inner = HashMap::new();
        inner.insert("value".to_string(), AttributeValue::long(100));

        let mut outer = HashMap::new();
        outer.insert("inner".to_string(), AttributeValue::record(inner));

        let original = AttributeValue::record(outer);
        let cloned = original.clone();

        assert_eq!(original, cloned);
    }

    // ========================================================================
    // Tests adicionales: Display para estructuras complejas
    // ========================================================================

    #[test]
    fn attribute_value_display_set() {
        let set = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::long(2),
            AttributeValue::long(3),
        ]);
        let display = format!("{}", set);
        assert_eq!(display, "[1, 2, 3]");
    }

    #[test]
    fn attribute_value_display_empty_set() {
        let set = AttributeValue::empty_set();
        let display = format!("{}", set);
        assert_eq!(display, "[]");
    }

    #[test]
    fn attribute_value_display_entity_ref() {
        let entity_ref = AttributeValue::entity_ref("hrn:partition:service::account:resource/id");
        let display = format!("{}", entity_ref);
        assert!(display.contains("EntityRef"));
        assert!(display.contains("hrn:partition:service::account:resource/id"));
    }

    // ========================================================================
    // Tests adicionales: Serialización con casos edge
    // ========================================================================

    #[test]
    fn attribute_value_serialization_empty_collections() {
        let empty_set = AttributeValue::empty_set();
        let json = serde_json::to_string(&empty_set).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(empty_set, deserialized);

        let empty_record = AttributeValue::empty_record();
        let json = serde_json::to_string(&empty_record).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(empty_record, deserialized);
    }

    #[test]
    fn attribute_value_serialization_entity_ref() {
        let entity_ref = AttributeValue::entity_ref("hrn:aws:iam::123:user/alice");
        let json = serde_json::to_string(&entity_ref).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(entity_ref, deserialized);
    }

    #[test]
    fn attribute_value_serialization_nested_structure() {
        let mut inner = HashMap::new();
        inner.insert("city".to_string(), AttributeValue::string("Madrid"));

        let mut outer = HashMap::new();
        outer.insert("address".to_string(), AttributeValue::record(inner));
        outer.insert("active".to_string(), AttributeValue::bool(true));

        let value = AttributeValue::record(outer);
        let json = serde_json::to_string(&value).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(value, deserialized);
    }

    #[test]
    fn attribute_value_serialization_heterogeneous_set() {
        let mixed = AttributeValue::set(vec![
            AttributeValue::long(42),
            AttributeValue::string("test"),
            AttributeValue::bool(false),
        ]);

        let json = serde_json::to_string(&mixed).unwrap();
        let deserialized: AttributeValue = serde_json::from_str(&json).unwrap();
        assert_eq!(mixed, deserialized);
    }

    // ========================================================================
    // Tests adicionales: Conversiones From con edge cases
    // ========================================================================

    #[test]
    fn attribute_value_from_empty_string() {
        let value: AttributeValue = "".into();
        assert_eq!(value.as_string(), Some(""));
    }

    #[test]
    fn attribute_value_from_negative_numbers() {
        let value: AttributeValue = (-42i64).into();
        assert_eq!(value.as_long(), Some(-42));

        let value2: AttributeValue = (-100i32).into();
        assert_eq!(value2.as_long(), Some(-100));
    }

    #[test]
    fn attribute_value_from_max_min_values() {
        let max_val: AttributeValue = i64::MAX.into();
        assert_eq!(max_val.as_long(), Some(i64::MAX));

        let min_val: AttributeValue = i64::MIN.into();
        assert_eq!(min_val.as_long(), Some(i64::MIN));
    }

    #[test]
    fn attribute_value_from_empty_vec() {
        let empty_vec: Vec<AttributeValue> = vec![];
        let value: AttributeValue = empty_vec.into();
        assert!(value.is_set());
        assert_eq!(value.as_set().unwrap().len(), 0);
    }

    #[test]
    fn attribute_value_from_empty_hashmap() {
        let empty_map: HashMap<String, AttributeValue> = HashMap::new();
        let value: AttributeValue = empty_map.into();
        assert!(value.is_record());
        assert_eq!(value.as_record().unwrap().len(), 0);
    }

    // ========================================================================
    // Tests adicionales: type_name para diferentes casos
    // ========================================================================

    #[test]
    fn attribute_value_type_name_for_all_variants() {
        assert_eq!(AttributeValue::bool(true).type_name(), "Bool");
        assert_eq!(AttributeValue::long(42).type_name(), "Long");
        assert_eq!(AttributeValue::string("test").type_name(), "String");
        assert_eq!(AttributeValue::empty_set().type_name(), "Set");
        assert_eq!(AttributeValue::empty_record().type_name(), "Record");
        assert_eq!(AttributeValue::entity_ref("id").type_name(), "EntityRef");
    }

    // ========================================================================
    // Tests adicionales: Records con claves especiales
    // ========================================================================

    #[test]
    fn attribute_value_record_with_special_keys() {
        let mut map = HashMap::new();
        map.insert("key-with-hyphens".to_string(), AttributeValue::long(1));
        map.insert("key_with_underscores".to_string(), AttributeValue::long(2));
        map.insert("keyWithCamelCase".to_string(), AttributeValue::long(3));
        map.insert("key.with.dots".to_string(), AttributeValue::long(4));

        let record = AttributeValue::record(map);
        let rec = record.as_record().unwrap();

        assert_eq!(rec.get("key-with-hyphens").unwrap().as_long(), Some(1));
        assert_eq!(rec.get("key_with_underscores").unwrap().as_long(), Some(2));
        assert_eq!(rec.get("keyWithCamelCase").unwrap().as_long(), Some(3));
        assert_eq!(rec.get("key.with.dots").unwrap().as_long(), Some(4));
    }

    #[test]
    fn attribute_value_record_unicode_keys() {
        let mut map = HashMap::new();
        map.insert("名前".to_string(), AttributeValue::string("田中"));
        map.insert("città".to_string(), AttributeValue::string("Roma"));

        let record = AttributeValue::record(map);
        let rec = record.as_record().unwrap();

        assert_eq!(rec.get("名前").unwrap().as_string(), Some("田中"));
        assert_eq!(rec.get("città").unwrap().as_string(), Some("Roma"));
    }

    // ========================================================================
    // Tests adicionales: Verificación de construcción de helpers
    // ========================================================================

    #[test]
    fn attribute_value_constructor_consistency() {
        // Verificar que los constructores helper funcionan igual que las variantes directas
        assert_eq!(AttributeValue::bool(true), AttributeValue::Bool(true));
        assert_eq!(AttributeValue::long(42), AttributeValue::Long(42));
        assert_eq!(
            AttributeValue::string("test"),
            AttributeValue::String("test".to_string())
        );
    }
}
