//! Value Objects for the shared domain
//!
//! This module defines "newtype" types that encapsulate domain primitives
//! with specific validation and semantics. These Value Objects are agnostic
//! to any policy engine (such as Cedar) and represent fundamental
//! concepts of Hodei's domain language.
//!
//! # Design Principles
//!
//! - **Validation on construction**: Constructors validate the format
//! - **Immutability**: Once created, they cannot be modified
//! - **Type safe**: The compiler guarantees correct usage
//! - **Agnostic**: No dependencies on external infrastructure
//!
//! # Examples
//!
//! ```
//! use kernel::domain::value_objects::{ServiceName, ResourceTypeName, AttributeName};
//!
//! // Valid construction
//! let service = ServiceName::new("iam").unwrap();
//! let resource_type = ResourceTypeName::new("User").unwrap();
//! let attr = AttributeName::new("email").unwrap();
//!
//! // Access to the inner value
//! assert_eq!(service.as_str(), "iam");
//! assert_eq!(resource_type.as_str(), "User");
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use thiserror::Error;

// ============================================================================
// Validation Errors
// ============================================================================

/// Errors that may occur when creating Value Objects
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The value is empty when content is required
    #[error("Value cannot be empty")]
    EmptyValue,

    /// The format does not comply with the domain rules
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// The value exceeds the maximum allowed length
    #[error("Value too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },

    /// The value does not match the expected pattern
    #[error("Invalid pattern: {reason}")]
    InvalidPattern { reason: String },
}

// ============================================================================
// ServiceName - Service name in kebab-case
// ============================================================================

/// Represents the name of a service (logical namespace)
///
/// # Expected Format
///
/// - Lowercase kebab-case (e.g. "iam", "organizations", "supply-chain")
/// - Only alphanumeric characters and hyphens
/// - Cannot start or end with a hyphen
/// - Maximum length: 64 characters
///
/// # Examples
///
/// ```
/// use kernel::domain::value_objects::ServiceName;
///
/// // Valid
/// assert!(ServiceName::new("iam").is_ok());
/// assert!(ServiceName::new("supply-chain").is_ok());
/// assert!(ServiceName::new("hodei-organizations").is_ok());
///
/// // Invalid
/// assert!(ServiceName::new("").is_err());           // Empty
/// assert!(ServiceName::new("IAM").is_err());        // Uppercase
/// assert!(ServiceName::new("-iam").is_err());       // Starts with hyphen
/// assert!(ServiceName::new("iam_service").is_err()); // Underscore
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServiceName(String);

impl ServiceName {
    /// Maximum allowed length for a service name
    pub const MAX_LENGTH: usize = 64;

    /// Creates a new ServiceName with validation
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if:
    /// - The value is empty
    /// - It contains disallowed characters
    /// - It exceeds the maximum length
    /// - It does not follow the kebab-case format
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validate non-empty
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validate length
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validate kebab-case format
        Self::validate_kebab_case(&value)?;

        Ok(Self(value))
    }

    /// Validates that the string follows the kebab-case format
    fn validate_kebab_case(value: &str) -> Result<(), ValidationError> {
        // Cannot start or end with a hyphen
        if value.starts_with('-') || value.ends_with('-') {
            return Err(ValidationError::InvalidPattern {
                reason: "Cannot start or end with hyphen".to_string(),
            });
        }

        // Only lowercase, digits and hyphens
        for (i, ch) in value.chars().enumerate() {
            match ch {
                'a'..='z' | '0'..='9' => continue,
                '-' => {
                    // Do not allow consecutive hyphens
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

    /// Gets the value as &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the Value Object and returns the inner String
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
// ResourceTypeName - Resource type name in PascalCase
// ============================================================================

/// Represents the name of a resource type
///
/// # Expected Format
///
/// - PascalCase (e.g. "User", "Group", "ServiceControlPolicy")
/// - Only alphanumeric characters
/// - Must start with an uppercase letter
/// - Maximum length: 64 characters
///
/// # Examples
///
/// ```
/// use kernel::domain::value_objects::ResourceTypeName;
///
/// // Valid
/// assert!(ResourceTypeName::new("User").is_ok());
/// assert!(ResourceTypeName::new("Group").is_ok());
/// assert!(ResourceTypeName::new("ServiceControlPolicy").is_ok());
///
/// // Invalid
/// assert!(ResourceTypeName::new("").is_err());        // Empty
/// assert!(ResourceTypeName::new("user").is_err());    // Lowercase
/// assert!(ResourceTypeName::new("User-Group").is_err()); // Hyphen
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceTypeName(String);

impl ResourceTypeName {
    /// Maximum allowed length for a resource type name
    pub const MAX_LENGTH: usize = 64;

    /// Creates a new ResourceTypeName with validation
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if:
    /// - The value is empty
    /// - It does not start with an uppercase letter
    /// - It contains non-alphanumeric characters
    /// - It exceeds the maximum length
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validate non-empty
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validate length
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validate PascalCase format
        Self::validate_pascal_case(&value)?;

        Ok(Self(value))
    }

    /// Validates that the string follows the PascalCase format
    fn validate_pascal_case(value: &str) -> Result<(), ValidationError> {
        // Must start with an uppercase letter
        if let Some(first) = value.chars().next()
            && !first.is_uppercase()
        {
            return Err(ValidationError::InvalidPattern {
                reason: "Must start with uppercase letter".to_string(),
            });
        }

        // Only alphanumerics
        if !value.chars().all(|ch| ch.is_alphanumeric()) {
            return Err(ValidationError::InvalidFormat(
                "Only alphanumeric characters allowed (PascalCase)".to_string(),
            ));
        }

        Ok(())
    }

    /// Gets the value as &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the Value Object and returns the inner String
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
// AttributeName - Attribute name in snake_case or camelCase
// ============================================================================

/// Represents the name of an entity attribute
///
/// # Expected Format
///
/// - snake_case or camelCase (e.g. "email", "created_at", "isActive")
/// - Only alphanumeric characters and underscore
/// - Cannot start with a digit
/// - Maximum length: 64 characters
///
/// # Examples
///
/// ```
/// use kernel::domain::value_objects::AttributeName;
///
/// // Valid
/// assert!(AttributeName::new("email").is_ok());
/// assert!(AttributeName::new("created_at").is_ok());
/// assert!(AttributeName::new("isActive").is_ok());
/// assert!(AttributeName::new("user_id").is_ok());
///
/// // Invalid
/// assert!(AttributeName::new("").is_err());           // Empty
/// assert!(AttributeName::new("123name").is_err());    // Starts with a digit
/// assert!(AttributeName::new("user-name").is_err());  // Hyphen
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttributeName(String);

impl AttributeName {
    /// Maximum allowed length for an attribute name
    pub const MAX_LENGTH: usize = 64;

    /// Creates a new AttributeName with validation
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if:
    /// - The value is empty
    /// - It starts with a digit
    /// - It contains disallowed characters
    /// - It exceeds the maximum length
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        // Validate non-empty
        if value.is_empty() {
            return Err(ValidationError::EmptyValue);
        }

        // Validate length
        if value.len() > Self::MAX_LENGTH {
            return Err(ValidationError::TooLong {
                max: Self::MAX_LENGTH,
                actual: value.len(),
            });
        }

        // Validate format
        Self::validate_identifier(&value)?;

        Ok(Self(value))
    }

    /// Validates that the string is a valid identifier
    fn validate_identifier(value: &str) -> Result<(), ValidationError> {
        // Cannot start with a digit
        if let Some(first) = value.chars().next()
            && first.is_numeric()
        {
            return Err(ValidationError::InvalidPattern {
                reason: "Cannot start with a number".to_string(),
            });
        }

        // Only alphanumerics and underscore
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

    /// Gets the value as &str
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the Value Object and returns the inner String
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
    // ServiceName tests
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
    // ResourceTypeName tests
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
    // AttributeName tests
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
    // Serialization tests
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

    // ========================================================================
    // Additional tests: PartialEq and Eq
    // ========================================================================

    #[test]
    fn service_name_equality() {
        let name1 = ServiceName::new("iam").unwrap();
        let name2 = ServiceName::new("iam").unwrap();
        let name3 = ServiceName::new("organizations").unwrap();

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
    }

    #[test]
    fn resource_type_name_equality() {
        let name1 = ResourceTypeName::new("User").unwrap();
        let name2 = ResourceTypeName::new("User").unwrap();
        let name3 = ResourceTypeName::new("Group").unwrap();

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
    }

    #[test]
    fn attribute_name_equality() {
        let name1 = AttributeName::new("email").unwrap();
        let name2 = AttributeName::new("email").unwrap();
        let name3 = AttributeName::new("username").unwrap();

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
    }

    // ========================================================================
    // Additional tests: Hash
    // ========================================================================

    #[test]
    fn service_name_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ServiceName::new("iam").unwrap());
        set.insert(ServiceName::new("iam").unwrap()); // Duplicate
        set.insert(ServiceName::new("organizations").unwrap());

        assert_eq!(set.len(), 2); // Only two unique
    }

    #[test]
    fn resource_type_name_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ResourceTypeName::new("User").unwrap());
        set.insert(ResourceTypeName::new("User").unwrap()); // Duplicate
        set.insert(ResourceTypeName::new("Group").unwrap());

        assert_eq!(set.len(), 2); // Only two unique
    }

    #[test]
    fn attribute_name_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(AttributeName::new("email").unwrap());
        set.insert(AttributeName::new("email").unwrap()); // Duplicate
        set.insert(AttributeName::new("username").unwrap());

        assert_eq!(set.len(), 2); // Only two unique
    }

    // ========================================================================
    // Additional tests: Clone
    // ========================================================================

    #[test]
    fn service_name_clone() {
        let original = ServiceName::new("iam").unwrap();
        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.as_str(), cloned.as_str());
    }

    #[test]
    fn resource_type_name_clone() {
        let original = ResourceTypeName::new("User").unwrap();
        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.as_str(), cloned.as_str());
    }

    #[test]
    fn attribute_name_clone() {
        let original = AttributeName::new("email").unwrap();
        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.as_str(), cloned.as_str());
    }

    // ========================================================================
    // Additional tests: AsRef<str>
    // ========================================================================

    #[test]
    fn service_name_as_ref() {
        let name = ServiceName::new("iam").unwrap();
        let str_ref: &str = name.as_ref();
        assert_eq!(str_ref, "iam");
    }

    #[test]
    fn resource_type_name_as_ref() {
        let name = ResourceTypeName::new("User").unwrap();
        let str_ref: &str = name.as_ref();
        assert_eq!(str_ref, "User");
    }

    #[test]
    fn attribute_name_as_ref() {
        let name = AttributeName::new("email").unwrap();
        let str_ref: &str = name.as_ref();
        assert_eq!(str_ref, "email");
    }

    // ========================================================================
    // Additional tests: into_inner
    // ========================================================================

    #[test]
    fn service_name_into_inner() {
        let name = ServiceName::new("iam").unwrap();
        let inner = name.into_inner();
        assert_eq!(inner, "iam");
    }

    #[test]
    fn resource_type_name_into_inner() {
        let name = ResourceTypeName::new("User").unwrap();
        let inner = name.into_inner();
        assert_eq!(inner, "User");
    }

    #[test]
    fn attribute_name_into_inner() {
        let name = AttributeName::new("email").unwrap();
        let inner = name.into_inner();
        assert_eq!(inner, "email");
    }

    // ========================================================================
    // Additional tests: validation edge cases
    // ========================================================================

    #[test]
    fn service_name_single_character() {
        let name = ServiceName::new("a").unwrap();
        assert_eq!(name.as_str(), "a");
    }

    #[test]
    fn service_name_all_numbers_valid() {
        // Digits are allowed in kebab-case
        let result = ServiceName::new("123");
        assert!(result.is_ok());
    }

    #[test]
    fn service_name_special_characters_fail() {
        assert!(ServiceName::new("iam@service").is_err());
        assert!(ServiceName::new("iam.service").is_err());
        assert!(ServiceName::new("iam service").is_err());
    }

    #[test]
    fn resource_type_name_single_character() {
        let name = ResourceTypeName::new("U").unwrap();
        assert_eq!(name.as_str(), "U");
    }

    #[test]
    fn resource_type_name_all_uppercase() {
        let name = ResourceTypeName::new("IAM").unwrap();
        assert_eq!(name.as_str(), "IAM");
    }

    #[test]
    fn resource_type_name_starts_with_lowercase_middle_uppercase() {
        let result = ResourceTypeName::new("userGroup");
        assert!(result.is_err());
    }

    #[test]
    fn attribute_name_single_character() {
        let name = AttributeName::new("a").unwrap();
        assert_eq!(name.as_str(), "a");
    }

    #[test]
    fn attribute_name_all_uppercase() {
        let name = AttributeName::new("EMAIL").unwrap();
        assert_eq!(name.as_str(), "EMAIL");
    }

    #[test]
    fn attribute_name_mixed_underscore_camelcase() {
        let name = AttributeName::new("user_IdValue").unwrap();
        assert_eq!(name.as_str(), "user_IdValue");
    }

    // ========================================================================
    // Additional tests: length boundary cases
    // ========================================================================

    #[test]
    fn service_name_almost_max_length() {
        let name = "a".repeat(ServiceName::MAX_LENGTH - 1);
        let result = ServiceName::new(&name);
        assert!(result.is_ok());
    }

    #[test]
    fn resource_type_name_almost_max_length() {
        let mut name = String::from("A");
        name.push_str(&"a".repeat(ResourceTypeName::MAX_LENGTH - 2));
        let result = ResourceTypeName::new(&name);
        assert!(result.is_ok());
    }

    #[test]
    fn attribute_name_almost_max_length() {
        let name = "a".repeat(AttributeName::MAX_LENGTH - 1);
        let result = AttributeName::new(&name);
        assert!(result.is_ok());
    }

    // ========================================================================
    // Additional tests: ValidationError details
    // ========================================================================

    #[test]
    fn validation_error_too_long_shows_values() {
        let long_name = "a".repeat(ServiceName::MAX_LENGTH + 5);
        let result = ServiceName::new(&long_name);

        match result {
            Err(ValidationError::TooLong { max, actual }) => {
                assert_eq!(max, ServiceName::MAX_LENGTH);
                assert_eq!(actual, ServiceName::MAX_LENGTH + 5);
            }
            _ => panic!("Expected TooLong error"),
        }
    }

    #[test]
    fn validation_error_invalid_pattern_has_reason() {
        let result = ServiceName::new("-iam");

        match result {
            Err(ValidationError::InvalidPattern { reason }) => {
                assert!(!reason.is_empty());
            }
            _ => panic!("Expected InvalidPattern error"),
        }
    }

    #[test]
    fn validation_error_invalid_format_has_reason() {
        let result = ServiceName::new("IAM");

        match result {
            Err(ValidationError::InvalidFormat(reason)) => {
                assert!(!reason.is_empty());
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    // ========================================================================
    // Additional tests: Display with different cases
    // ========================================================================

    #[test]
    fn service_name_display_preserves_format() {
        let name = ServiceName::new("supply-chain-v2").unwrap();
        assert_eq!(format!("{}", name), "supply-chain-v2");
    }

    #[test]
    fn resource_type_name_display_preserves_case() {
        let name = ResourceTypeName::new("ServiceControlPolicy").unwrap();
        assert_eq!(format!("{}", name), "ServiceControlPolicy");
    }

    #[test]
    fn attribute_name_display_preserves_format() {
        let name = AttributeName::new("created_at_timestamp").unwrap();
        assert_eq!(format!("{}", name), "created_at_timestamp");
    }
}
