//! Value Objects para el dominio compartido
//!
//! Este módulo define tipos "newtype" que encapsulan primitivos del dominio
//! con validación y semántica específica. Estos Value Objects son agnósticos
//! a cualquier motor de políticas (como Cedar) y representan conceptos
//! fundamentales del lenguaje de dominio de Hodei.
//!
//! # Principios de Diseño
//!
//! - **Validación en construcción**: Los constructores validan el formato
//! - **Inmutabilidad**: Una vez creados, no pueden modificarse
//! - **Tipo seguro**: El compilador garantiza el uso correcto
//! - **Agnóstico**: Sin dependencias de infraestructura externa
//!
//! # Ejemplos
//!
//! ```
//! use kernel::domain::value_objects::{ServiceName, ResourceTypeName, AttributeName};
//!
//! // Construcción válida
//! let service = ServiceName::new("iam").unwrap();
//! let resource_type = ResourceTypeName::new("User").unwrap();
//! let attr = AttributeName::new("email").unwrap();
//!
//! // Acceso al valor interno
//! assert_eq!(service.as_str(), "iam");
//! assert_eq!(resource_type.as_str(), "User");
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use thiserror::Error;

// ============================================================================
// Errores de Validación
// ============================================================================

/// Errores que pueden ocurrir al crear Value Objects
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// El valor está vacío cuando se requiere contenido
    #[error("Value cannot be empty")]
    EmptyValue,

    /// El formato no cumple con las reglas del dominio
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// El valor excede la longitud máxima permitida
    #[error("Value too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },

    /// El valor no cumple con el patrón esperado
    #[error("Invalid pattern: {reason}")]
    InvalidPattern { reason: String },
}

// ============================================================================
// ServiceName - Nombre de servicio en kebab-case
// ============================================================================

/// Representa el nombre de un servicio (namespace lógico)
///
/// # Formato Esperado
///
/// - Lowercase kebab-case (ej: "iam", "organizations", "supply-chain")
/// - Solo caracteres alfanuméricos y guiones
/// - No puede empezar ni terminar con guión
/// - Longitud máxima: 64 caracteres
///
/// # Ejemplos
///
/// ```
/// use kernel::domain::value_objects::ServiceName;
///
/// // Válidos
/// assert!(ServiceName::new("iam").is_ok());
/// assert!(ServiceName::new("supply-chain").is_ok());
/// assert!(ServiceName::new("hodei-organizations").is_ok());
///
/// // Inválidos
/// assert!(ServiceName::new("").is_err());           // Vacío
/// assert!(ServiceName::new("IAM").is_err());        // Mayúsculas
/// assert!(ServiceName::new("-iam").is_err());       // Empieza con guión
/// assert!(ServiceName::new("iam_service").is_err()); // Underscore
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServiceName(String);

impl ServiceName {
    /// Longitud máxima permitida para un nombre de servicio
    pub const MAX_LENGTH: usize = 64;

    /// Crea un nuevo ServiceName con validación
    ///
    /// # Errores
    ///
    /// Retorna `ValidationError` si:
    /// - El valor está vacío
    /// - Contiene caracteres no permitidos
    /// - Excede la longitud máxima
    /// - No sigue el formato kebab-case
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validar no vacío
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validar longitud
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validar formato kebab-case
        Self::validate_kebab_case(&value)?;

        Ok(Self(value))
    }

    /// Valida que el string siga el formato kebab-case
    fn validate_kebab_case(value: &str) -> Result<(), ValidationError> {
        // No puede empezar o terminar con guión
        if value.starts_with('-') || value.ends_with('-') {
            return Err(ValidationError::InvalidPattern {
                reason: "Cannot start or end with hyphen".to_string(),
            });
        }

        // Solo lowercase, números y guiones
        for (i, ch) in value.chars().enumerate() {
            match ch {
                'a'..='z' | '0'..='9' => continue,
                '-' => {
                    // No permitir guiones consecutivos
                    if i > 0 && value.chars().nth(i - 1) == Some('-') {
                        return Err(ValidationError::InvalidPattern {
                            reason: "Cannot have consecutive hyphens".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(ValidationError::InvalidFormat(format!(
                        "Invalid character '{}' at position {}. Only lowercase letters, numbers, and hyphens allowed",
                        ch, i
                    )));
                }
            }
        }

        Ok(())
    }

    /// Obtiene el valor como &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume el Value Object y retorna el String interno
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for ServiceName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ServiceName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServiceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// ResourceTypeName - Nombre de tipo de recurso en PascalCase
// ============================================================================

/// Representa el nombre de un tipo de recurso
///
/// # Formato Esperado
///
/// - PascalCase (ej: "User", "Group", "ServiceControlPolicy")
/// - Solo caracteres alfanuméricos
/// - Debe empezar con mayúscula
/// - Longitud máxima: 64 caracteres
///
/// # Ejemplos
///
/// ```
/// use kernel::domain::value_objects::ResourceTypeName;
///
/// // Válidos
/// assert!(ResourceTypeName::new("User").is_ok());
/// assert!(ResourceTypeName::new("Group").is_ok());
/// assert!(ResourceTypeName::new("ServiceControlPolicy").is_ok());
///
/// // Inválidos
/// assert!(ResourceTypeName::new("").is_err());        // Vacío
/// assert!(ResourceTypeName::new("user").is_err());    // Minúscula
/// assert!(ResourceTypeName::new("User-Group").is_err()); // Guión
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceTypeName(String);

impl ResourceTypeName {
    /// Longitud máxima permitida para un nombre de tipo de recurso
    pub const MAX_LENGTH: usize = 64;

    /// Crea un nuevo ResourceTypeName con validación
    ///
    /// # Errores
    ///
    /// Retorna `ValidationError` si:
    /// - El valor está vacío
    /// - No empieza con mayúscula
    /// - Contiene caracteres no alfanuméricos
    /// - Excede la longitud máxima
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validar no vacío
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validar longitud
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validar formato PascalCase
        Self::validate_pascal_case(&value)?;

        Ok(Self(value))
    }

    /// Valida que el string siga el formato PascalCase
    fn validate_pascal_case(value: &str) -> Result<(), ValidationError> {
        // Debe empezar con mayúscula
        if let Some(first) = value.chars().next()
            && !first.is_uppercase()
        {
            return Err(ValidationError::InvalidPattern {
                reason: "Must start with uppercase letter".to_string(),
            });
        }

        // Solo alfanuméricos
        if !value.chars().all(|ch| ch.is_alphanumeric()) {
            return Err(ValidationError::InvalidFormat(
                "Only alphanumeric characters allowed (PascalCase)".to_string(),
            ));
        }

        Ok(())
    }

    /// Obtiene el valor como &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume el Value Object y retorna el String interno
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for ResourceTypeName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ResourceTypeName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// AttributeName - Nombre de atributo en snake_case o camelCase
// ============================================================================

/// Representa el nombre de un atributo de entidad
///
/// # Formato Esperado
///
/// - snake_case o camelCase (ej: "email", "created_at", "isActive")
/// - Solo caracteres alfanuméricos y underscore
/// - No puede empezar con número
/// - Longitud máxima: 64 caracteres
///
/// # Ejemplos
///
/// ```
/// use kernel::domain::value_objects::AttributeName;
///
/// // Válidos
/// assert!(AttributeName::new("email").is_ok());
/// assert!(AttributeName::new("created_at").is_ok());
/// assert!(AttributeName::new("isActive").is_ok());
/// assert!(AttributeName::new("user_id").is_ok());
///
/// // Inválidos
/// assert!(AttributeName::new("").is_err());           // Vacío
/// assert!(AttributeName::new("123name").is_err());    // Empieza con número
/// assert!(AttributeName::new("user-name").is_err());  // Guión
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttributeName(String);

impl AttributeName {
    /// Longitud máxima permitida para un nombre de atributo
    pub const MAX_LENGTH: usize = 64;

    /// Crea un nuevo AttributeName con validación
    ///
    /// # Errores
    ///
    /// Retorna `ValidationError` si:
    /// - El valor está vacío
    /// - Empieza con número
    /// - Contiene caracteres no permitidos
    /// - Excede la longitud máxima
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validar no vacío
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validar longitud
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validar formato
        Self::validate_identifier(&value)?;

        Ok(Self(value))
    }

    /// Valida que el string sea un identificador válido
    fn validate_identifier(value: &str) -> Result<(), ValidationError> {
        // No puede empezar con número
        if let Some(first) = value.chars().next()
            && first.is_numeric()
        {
            return Err(ValidationError::InvalidPattern {
                reason: "Cannot start with a number".to_string(),
            });
        }

        // Solo alfanuméricos y underscore
        for (i, ch) in value.chars().enumerate() {
            if !ch.is_alphanumeric() && ch != '_' {
                return Err(ValidationError::InvalidFormat(format!(
                    "Invalid character '{}' at position {}. Only letters, numbers, and underscores allowed",
                    ch, i
                )));
            }
        }

        Ok(())
    }

    /// Obtiene el valor como &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume el Value Object y retorna el String interno
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for AttributeName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for AttributeName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AttributeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Tests de ServiceName
    // ========================================================================

    #[test]
    fn service_name_valid_simple() {
        let name = ServiceName::new("iam").unwrap();
        assert_eq!(name.as_str(), "iam");
    }

    #[test]
    fn service_name_valid_kebab_case() {
        let name = ServiceName::new("supply-chain").unwrap();
        assert_eq!(name.as_str(), "supply-chain");
    }

    #[test]
    fn service_name_valid_with_numbers() {
        let name = ServiceName::new("service-v2").unwrap();
        assert_eq!(name.as_str(), "service-v2");
    }

    #[test]
    fn service_name_empty_fails() {
        let result = ServiceName::new("");
        assert!(matches!(result, Err(ValidationError::EmptyValue)));
    }

    #[test]
    fn service_name_uppercase_fails() {
        let result = ServiceName::new("IAM");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn service_name_starts_with_hyphen_fails() {
        let result = ServiceName::new("-iam");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn service_name_ends_with_hyphen_fails() {
        let result = ServiceName::new("iam-");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn service_name_consecutive_hyphens_fails() {
        let result = ServiceName::new("iam--service");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn service_name_underscore_fails() {
        let result = ServiceName::new("iam_service");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn service_name_too_long_fails() {
        let long_name = "a".repeat(ServiceName::MAX_LENGTH + 1);
        let result = ServiceName::new(long_name);
        assert!(matches!(result, Err(ValidationError::TooLong { .. })));
    }

    #[test]
    fn service_name_max_length_succeeds() {
        let max_name = "a".repeat(ServiceName::MAX_LENGTH);
        let result = ServiceName::new(max_name);
        assert!(result.is_ok());
    }

    #[test]
    fn service_name_display() {
        let name = ServiceName::new("iam").unwrap();
        assert_eq!(format!("{}", name), "iam");
    }

    #[test]
    fn service_name_deref() {
        let name = ServiceName::new("iam").unwrap();
        assert_eq!(&*name, "iam");
    }

    // ========================================================================
    // Tests de ResourceTypeName
    // ========================================================================

    #[test]
    fn resource_type_name_valid_single_word() {
        let name = ResourceTypeName::new("User").unwrap();
        assert_eq!(name.as_str(), "User");
    }

    #[test]
    fn resource_type_name_valid_pascal_case() {
        let name = ResourceTypeName::new("ServiceControlPolicy").unwrap();
        assert_eq!(name.as_str(), "ServiceControlPolicy");
    }

    #[test]
    fn resource_type_name_valid_with_numbers() {
        let name = ResourceTypeName::new("User123").unwrap();
        assert_eq!(name.as_str(), "User123");
    }

    #[test]
    fn resource_type_name_empty_fails() {
        let result = ResourceTypeName::new("");
        assert!(matches!(result, Err(ValidationError::EmptyValue)));
    }

    #[test]
    fn resource_type_name_lowercase_fails() {
        let result = ResourceTypeName::new("user");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn resource_type_name_with_hyphen_fails() {
        let result = ResourceTypeName::new("User-Group");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn resource_type_name_with_underscore_fails() {
        let result = ResourceTypeName::new("User_Group");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn resource_type_name_too_long_fails() {
        let long_name = "A".repeat(ResourceTypeName::MAX_LENGTH + 1);
        let result = ResourceTypeName::new(long_name);
        assert!(matches!(result, Err(ValidationError::TooLong { .. })));
    }

    #[test]
    fn resource_type_name_display() {
        let name = ResourceTypeName::new("User").unwrap();
        assert_eq!(format!("{}", name), "User");
    }

    // ========================================================================
    // Tests de AttributeName
    // ========================================================================

    #[test]
    fn attribute_name_valid_snake_case() {
        let name = AttributeName::new("created_at").unwrap();
        assert_eq!(name.as_str(), "created_at");
    }

    #[test]
    fn attribute_name_valid_camel_case() {
        let name = AttributeName::new("isActive").unwrap();
        assert_eq!(name.as_str(), "isActive");
    }

    #[test]
    fn attribute_name_valid_simple() {
        let name = AttributeName::new("email").unwrap();
        assert_eq!(name.as_str(), "email");
    }

    #[test]
    fn attribute_name_valid_with_numbers() {
        let name = AttributeName::new("user_id_123").unwrap();
        assert_eq!(name.as_str(), "user_id_123");
    }

    #[test]
    fn attribute_name_empty_fails() {
        let result = AttributeName::new("");
        assert!(matches!(result, Err(ValidationError::EmptyValue)));
    }

    #[test]
    fn attribute_name_starts_with_number_fails() {
        let result = AttributeName::new("123name");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPattern { .. })
        ));
    }

    #[test]
    fn attribute_name_with_hyphen_fails() {
        let result = AttributeName::new("user-name");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn attribute_name_with_space_fails() {
        let result = AttributeName::new("user name");
        assert!(matches!(result, Err(ValidationError::InvalidFormat(_))));
    }

    #[test]
    fn attribute_name_too_long_fails() {
        let long_name = "a".repeat(AttributeName::MAX_LENGTH + 1);
        let result = AttributeName::new(long_name);
        assert!(matches!(result, Err(ValidationError::TooLong { .. })));
    }

    #[test]
    fn attribute_name_display() {
        let name = AttributeName::new("email").unwrap();
        assert_eq!(format!("{}", name), "email");
    }

    // ========================================================================
    // Tests de Serialización
    // ========================================================================

    #[test]
    fn service_name_serialization() {
        let name = ServiceName::new("iam").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, r#""iam""#);

        let deserialized: ServiceName = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, name);
    }

    #[test]
    fn resource_type_name_serialization() {
        let name = ResourceTypeName::new("User").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, r#""User""#);

        let deserialized: ResourceTypeName = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, name);
    }

    #[test]
    fn attribute_name_serialization() {
        let name = AttributeName::new("email").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, r#""email""#);

        let deserialized: AttributeName = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, name);
    }
}
