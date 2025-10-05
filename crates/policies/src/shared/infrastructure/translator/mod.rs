//! # Cedar Translator Module
//!
//! This module provides translation functions from kernel's agnostic types
//! to Cedar policy engine types. It completely encapsulates Cedar as an
//! implementation detail.
//!
//! ## Architecture
//!
//! ```text
//! Kernel (Agnostic)        Translator           Cedar (Internal)
//! ─────────────────────────────────────────────────────────────
//! HodeiEntity          →   translate_entity  →  cedar_policy::Entity
//! AttributeValue       →   translate_attr    →  RestrictedExpression
//! AttributeName        →   String            →  String
//! Hrn                  →   String            →  EntityUid
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use policies::shared::infrastructure::translator::translate_to_cedar_entity;
//! use kernel::HodeiEntity;
//!
//! let user: &dyn HodeiEntity = &my_user;
//! let cedar_entity = translate_to_cedar_entity(user)?;
//! ```
//!
//! ## Error Handling
//!
//! All translation functions return `Result<T, TranslatorError>` to handle
//! invalid data gracefully (malformed HRNs, unsupported types, etc.).

use cedar_policy::{Entity, EntityUid, RestrictedExpression};
use kernel::domain::AttributeValue;
use kernel::{HodeiEntity, Hrn};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during translation from agnostic to Cedar types
#[derive(Debug, Error, Clone)]
pub enum TranslatorError {
    /// The attribute value cannot be translated to Cedar format
    #[error("Invalid attribute value: {0}")]
    InvalidAttributeValue(String),

    /// The entity cannot be translated (missing required data, etc.)
    #[error("Invalid entity: {0}")]
    InvalidEntity(String),

    /// The type is not supported by Cedar
    #[error("Unsupported type: {0}")]
    UnsupportedType(String),

    /// The HRN cannot be parsed into a valid Cedar EntityUid
    #[error("Invalid HRN format: {0}")]
    InvalidHrn(String),

    /// Cedar internal error during translation
    #[error("Cedar internal error: {0}")]
    CedarError(String),
}

// ============================================================================
// Attribute Value Translation
// ============================================================================

/// Translates an agnostic `AttributeValue` to Cedar's `RestrictedExpression`
///
/// This function recursively translates all supported attribute types including
/// nested structures (Sets and Records).
///
/// # Arguments
///
/// * `value` - The agnostic attribute value from the kernel
///
/// # Returns
///
/// A Cedar `RestrictedExpression` ready for use in policy evaluation
///
/// # Errors
///
/// Returns `TranslatorError` if:
/// - The value contains unsupported types
/// - Nested structures are malformed
/// - Entity references have invalid HRN format
///
/// # Examples
///
/// ```rust,ignore
/// use kernel::AttributeValue;
/// use policies::shared::infrastructure::translator::translate_attribute_value;
///
/// // Primitive types
/// let bool_val = AttributeValue::bool(true);
/// let cedar_bool = translate_attribute_value(&bool_val)?;
///
/// // Collections
/// let set_val = AttributeValue::set(vec![
///     AttributeValue::long(1),
///     AttributeValue::long(2),
/// ]);
/// let cedar_set = translate_attribute_value(&set_val)?;
/// ```
pub fn translate_attribute_value(
    value: &AttributeValue,
) -> Result<RestrictedExpression, TranslatorError> {
    match value {
        AttributeValue::Bool(b) => Ok(RestrictedExpression::new_bool(*b)),

        AttributeValue::Long(n) => Ok(RestrictedExpression::new_long(*n)),

        AttributeValue::String(s) => Ok(RestrictedExpression::new_string(s.clone())),

        AttributeValue::Set(values) => {
            // Recursively translate each value in the set
            let cedar_values: Result<Vec<_>, _> =
                values.iter().map(translate_attribute_value).collect();

            let cedar_values = cedar_values?;

            Ok(RestrictedExpression::new_set(cedar_values))
        }

        AttributeValue::Record(map) => {
            // Recursively translate each value in the record
            let mut cedar_map: HashMap<String, RestrictedExpression> = HashMap::new();

            for (key, value) in map {
                let cedar_value = translate_attribute_value(value)?;
                cedar_map.insert(key.to_string(), cedar_value);
            }

            RestrictedExpression::new_record(cedar_map.into_iter())
                .map_err(|e| TranslatorError::CedarError(e.to_string()))
        }

        AttributeValue::EntityRef(hrn_str) => {
            // Parse HRN string to Cedar EntityUid
            let uid = parse_hrn_to_entity_uid(hrn_str)?;

            Ok(RestrictedExpression::new_entity_uid(uid))
        }
    }
}

// ============================================================================
// Entity Translation
// ============================================================================

/// Translates an agnostic `HodeiEntity` to Cedar's `Entity`
///
/// This function performs the complete translation of a domain entity
/// to Cedar's internal representation, including:
/// - Converting the HRN to EntityUid
/// - Translating all attributes
/// - Setting up parent relationships
///
/// # Arguments
///
/// * `entity` - A reference to any type implementing `HodeiEntity`
///
/// # Returns
///
/// A Cedar `Entity` ready for registration in the entity store
///
/// # Errors
///
/// Returns `TranslatorError` if:
/// - The entity's HRN is malformed
/// - Any attribute cannot be translated
/// - Parent HRNs are invalid
///
/// # Examples
///
/// ```rust,ignore
/// use policies::shared::infrastructure::translator::translate_to_cedar_entity;
///
/// let user = User::new(hrn, "Alice", "alice@example.com");
/// let cedar_entity = translate_to_cedar_entity(&user)?;
/// ```
pub fn translate_to_cedar_entity(entity: &dyn HodeiEntity) -> Result<Entity, TranslatorError> {
    // 1. Get entity data
    let hrn = entity.hrn();
    let attributes = entity.attributes();
    let parent_hrns = entity.parent_hrns();

    // 2. Convert HRN to Cedar EntityUid
    let uid = parse_hrn_to_entity_uid(&hrn.to_string())?;

    // 3. Translate attributes
    let mut cedar_attrs: HashMap<String, RestrictedExpression> = HashMap::new();

    for (name, value) in attributes {
        let cedar_value = translate_attribute_value(&value)?;
        cedar_attrs.insert(name.to_string(), cedar_value);
    }

    // 4. Translate parent HRNs
    let cedar_parents: Result<Vec<EntityUid>, _> = parent_hrns
        .iter()
        .map(|hrn| parse_hrn_to_entity_uid(&hrn.to_string()))
        .collect();

    let cedar_parents = cedar_parents?;

    // 5. Construct Cedar Entity
    // Note: Entity::new might have different signature, need to check Cedar API
    Entity::new(
        uid,
        cedar_attrs,
        std::collections::HashSet::from_iter(cedar_parents),
    )
    .map_err(|e| TranslatorError::CedarError(format!("Failed to create entity: {}", e)))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parses an HRN string to Cedar's EntityUid
///
/// HRN format: `hrn:<provider>:<service>:<region>:<type>/<id>`
/// Cedar EntityUid format: `<type>::"<id>"`
///
/// # Examples
///
/// ```text
/// HRN: hrn:aws:iam:us-east-1:123456789012:user/alice
/// Cedar: Iam::User::"alice"
/// ```
fn parse_hrn_to_entity_uid(hrn_str: &str) -> Result<EntityUid, TranslatorError> {
    // Parse the HRN
    let hrn = Hrn::from_string(hrn_str)
        .ok_or_else(|| TranslatorError::InvalidHrn(format!("Failed to parse HRN: {}", hrn_str)))?;

    // HRN already has a method to generate Cedar EntityUid string
    let entity_uid_str = hrn.entity_uid_string();

    // Parse into Cedar EntityUid using FromStr trait
    EntityUid::from_str(&entity_uid_str)
        .map_err(|e| TranslatorError::InvalidHrn(format!("Failed to create EntityUid: {}", e)))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{
        AttributeName, AttributeType, AttributeValue, ResourceTypeName, ServiceName,
    };
    use kernel::{HodeiEntity, HodeiEntityType, Hrn};
    use std::collections::HashMap;

    // Test entity implementation
    struct TestUser {
        hrn: Hrn,
        name: String,
        email: String,
        age: i64,
        active: bool,
    }

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").unwrap()
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").unwrap()
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (AttributeName::new("name").unwrap(), AttributeType::string()),
                (
                    AttributeName::new("email").unwrap(),
                    AttributeType::string(),
                ),
                (AttributeName::new("age").unwrap(), AttributeType::long()),
                (AttributeName::new("active").unwrap(), AttributeType::bool()),
            ]
        }
    }

    impl HodeiEntity for TestUser {
        fn hrn(&self) -> &Hrn {
            &self.hrn
        }

        fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
            let mut attrs = HashMap::new();
            attrs.insert(
                AttributeName::new("name").unwrap(),
                AttributeValue::string(&self.name),
            );
            attrs.insert(
                AttributeName::new("email").unwrap(),
                AttributeValue::string(&self.email),
            );
            attrs.insert(
                AttributeName::new("age").unwrap(),
                AttributeValue::long(self.age),
            );
            attrs.insert(
                AttributeName::new("active").unwrap(),
                AttributeValue::bool(self.active),
            );
            attrs
        }
    }

    // ========================================================================
    // Attribute Value Translation Tests
    // ========================================================================

    #[test]
    fn translate_bool_value() {
        let value = AttributeValue::bool(true);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_long_value() {
        let value = AttributeValue::long(42);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_string_value() {
        let value = AttributeValue::string("test");
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_empty_set() {
        let value = AttributeValue::empty_set();
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_set_with_values() {
        let value = AttributeValue::set(vec![
            AttributeValue::long(1),
            AttributeValue::long(2),
            AttributeValue::long(3),
        ]);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_empty_record() {
        let value = AttributeValue::empty_record();
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_record_with_values() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), AttributeValue::string("Alice"));
        map.insert("age".to_string(), AttributeValue::long(30));
        map.insert("active".to_string(), AttributeValue::bool(true));

        let value = AttributeValue::record(map);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_nested_record() {
        let mut inner = HashMap::new();
        inner.insert("city".to_string(), AttributeValue::string("Madrid"));
        inner.insert("postal_code".to_string(), AttributeValue::string("28001"));

        let mut outer = HashMap::new();
        outer.insert("name".to_string(), AttributeValue::string("Alice"));
        outer.insert("address".to_string(), AttributeValue::record(inner));

        let value = AttributeValue::record(outer);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn translate_nested_set_in_record() {
        let mut map = HashMap::new();
        map.insert(
            "tags".to_string(),
            AttributeValue::set(vec![
                AttributeValue::string("admin"),
                AttributeValue::string("developer"),
            ]),
        );

        let value = AttributeValue::record(map);
        let result = translate_attribute_value(&value);
        assert!(result.is_ok());
    }

    // ========================================================================
    // Entity Translation Tests
    // ========================================================================

    #[test]
    fn translate_entity_with_all_attributes() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "us-east-1".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let user = TestUser {
            hrn,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
            active: true,
        };

        let result = translate_to_cedar_entity(&user);
        assert!(result.is_ok());

        let entity = result.unwrap();
        // Verify entity has correct structure
        assert_eq!(entity.uid().type_name().to_string(), "Iam::User");
    }

    // ========================================================================
    // HRN Parsing Tests
    // ========================================================================

    #[test]
    fn parse_valid_hrn() {
        let hrn_str = "hrn:aws:iam:us-east-1:123456789012:user/alice";
        let result = parse_hrn_to_entity_uid(hrn_str);
        assert!(result.is_ok());
    }

    #[test]
    fn parse_invalid_hrn() {
        let hrn_str = "invalid-hrn-format";
        let result = parse_hrn_to_entity_uid(hrn_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TranslatorError::InvalidHrn(_)
        ));
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[test]
    fn error_display_formats() {
        let err1 = TranslatorError::InvalidAttributeValue("test".to_string());
        assert!(err1.to_string().contains("Invalid attribute value"));

        let err2 = TranslatorError::InvalidEntity("test".to_string());
        assert!(err2.to_string().contains("Invalid entity"));

        let err3 = TranslatorError::InvalidHrn("test".to_string());
        assert!(err3.to_string().contains("Invalid HRN format"));
    }
}
