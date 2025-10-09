//! Data Transfer Objects for the register_entity_type feature

use serde::{Deserialize, Serialize};

/// Command for registering an entity type
///
/// This command encapsulates the data needed to register an entity type
/// in the Cedar schema builder. It serves as the input contract for the
/// use case when using the command-based interface.
///
/// Note: In practice, direct generic registration via `use_case.register::<E>()`
/// is preferred for type safety, but this command exists to satisfy the
/// port trait interface for architectural consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterEntityTypeCommand {
    /// The name of the entity type being registered
    pub entity_name: String,

    /// Optional description of what this entity represents
    pub description: Option<String>,

    /// Optional list of attributes for this entity type
    pub attributes: Option<Vec<EntityAttribute>>,
}

/// Represents an attribute definition for an entity type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityAttribute {
    /// The name of the attribute
    pub name: String,

    /// The type of the attribute (e.g., "String", "Long", "Boolean")
    pub attribute_type: String,

    /// Whether this attribute is required
    pub required: bool,
}

impl RegisterEntityTypeCommand {
    /// Create a new entity type registration command
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The name of the entity to register
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::register_entity_type::dto::RegisterEntityTypeCommand;
    ///
    /// let command = RegisterEntityTypeCommand::new("User".to_string());
    /// ```
    pub fn new(entity_name: String) -> Self {
        Self {
            entity_name,
            description: None,
            attributes: None,
        }
    }

    /// Create a new entity type registration command with description
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The name of the entity to register
    /// * `description` - A description of what this entity represents
    pub fn with_description(entity_name: String, description: String) -> Self {
        Self {
            entity_name,
            description: Some(description),
            attributes: None,
        }
    }

    /// Create a new entity type registration command with attributes
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The name of the entity to register
    /// * `attributes` - List of attributes for this entity type
    pub fn with_attributes(entity_name: String, attributes: Vec<EntityAttribute>) -> Self {
        Self {
            entity_name,
            description: None,
            attributes: Some(attributes),
        }
    }

    /// Create a complete entity type registration command
    ///
    /// # Arguments
    ///
    /// * `entity_name` - The name of the entity to register
    /// * `description` - A description of what this entity represents
    /// * `attributes` - List of attributes for this entity type
    pub fn complete(
        entity_name: String,
        description: String,
        attributes: Vec<EntityAttribute>,
    ) -> Self {
        Self {
            entity_name,
            description: Some(description),
            attributes: Some(attributes),
        }
    }
}

impl EntityAttribute {
    /// Create a new entity attribute
    pub fn new(name: String, attribute_type: String, required: bool) -> Self {
        Self {
            name,
            attribute_type,
            required,
        }
    }

    /// Create a required attribute
    pub fn required(name: String, attribute_type: String) -> Self {
        Self::new(name, attribute_type, true)
    }

    /// Create an optional attribute
    pub fn optional(name: String, attribute_type: String) -> Self {
        Self::new(name, attribute_type, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_command() {
        let command = RegisterEntityTypeCommand::new("User".to_string());
        assert_eq!(command.entity_name, "User");
        assert!(command.description.is_none());
        assert!(command.attributes.is_none());
    }

    #[test]
    fn test_command_with_description() {
        let command = RegisterEntityTypeCommand::with_description(
            "User".to_string(),
            "Represents a user in the system".to_string(),
        );
        assert_eq!(command.entity_name, "User");
        assert_eq!(
            command.description,
            Some("Represents a user in the system".to_string())
        );
        assert!(command.attributes.is_none());
    }

    #[test]
    fn test_command_with_attributes() {
        let attributes = vec![
            EntityAttribute::required("email".to_string(), "String".to_string()),
            EntityAttribute::optional("age".to_string(), "Long".to_string()),
        ];

        let command = RegisterEntityTypeCommand::with_attributes("User".to_string(), attributes);
        assert_eq!(command.entity_name, "User");
        assert!(command.description.is_none());
        assert!(command.attributes.is_some());
        assert_eq!(command.attributes.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_complete_command() {
        let attributes = vec![EntityAttribute::required(
            "email".to_string(),
            "String".to_string(),
        )];

        let command = RegisterEntityTypeCommand::complete(
            "User".to_string(),
            "A system user".to_string(),
            attributes,
        );
        assert_eq!(command.entity_name, "User");
        assert_eq!(command.description, Some("A system user".to_string()));
        assert_eq!(command.attributes.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_entity_attribute_required() {
        let attr = EntityAttribute::required("email".to_string(), "String".to_string());
        assert_eq!(attr.name, "email");
        assert_eq!(attr.attribute_type, "String");
        assert!(attr.required);
    }

    #[test]
    fn test_entity_attribute_optional() {
        let attr = EntityAttribute::optional("phone".to_string(), "String".to_string());
        assert_eq!(attr.name, "phone");
        assert_eq!(attr.attribute_type, "String");
        assert!(!attr.required);
    }
}
