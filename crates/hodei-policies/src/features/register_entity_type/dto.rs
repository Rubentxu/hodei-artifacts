//! Data Transfer Objects for the register_entity_type feature

use crate::internal::engine::builder::to_cedar_type;
use kernel::HodeiEntityType;
use serde::{Deserialize, Serialize};

/// Command for registering an entity type
///
/// This command carries everything the Cedar schema builder needs to declare
/// an entity type: the fully qualified type name, an optional description and
/// the attribute list.
///
/// The qualified name is mandatory because a Cedar entity without a namespace
/// cannot be referenced by the `appliesTo` clause of any action; a leaf name
/// such as `"User"` is therefore rejected by the use case rather than
/// silently registered in the empty namespace.
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::register_entity_type::dto::RegisterEntityTypeCommand;
///
/// let command = RegisterEntityTypeCommand::new("Iam::User".to_string());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterEntityTypeCommand {
    /// The fully qualified entity type being registered (`Namespace::Entity`)
    pub entity_type: String,

    /// Optional description of what this entity represents
    pub description: Option<String>,

    /// The attributes of this entity type, in the Cedar type vocabulary
    pub attributes: Vec<EntityAttribute>,
}

/// Represents an attribute definition for an entity type
///
/// The attribute type is expressed in the Cedar vocabulary (`String`, `Long`,
/// `Bool`, `Set<…>`, `__cedar::Entity`). It is produced by the single
/// translation point in the crate, so the same kernel attribute type can
/// never be spelled two different ways.
///
/// There is deliberately **no** `required` flag: Cedar's schema language has
/// no required-attribute syntax, so such a flag would describe a state the
/// generated schema cannot represent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityAttribute {
    /// The name of the attribute
    pub name: String,

    /// The type of the attribute in the Cedar vocabulary
    pub attribute_type: String,
}

impl RegisterEntityTypeCommand {
    /// Create a new entity type registration command
    ///
    /// # Arguments
    ///
    /// * `entity_type` - The fully qualified name of the entity to register
    pub fn new(entity_type: String) -> Self {
        Self {
            entity_type,
            description: None,
            attributes: Vec::new(),
        }
    }

    /// Create a new entity type registration command with description
    ///
    /// # Arguments
    ///
    /// * `entity_type` - The fully qualified name of the entity to register
    /// * `description` - A description of what this entity represents
    pub fn with_description(entity_type: String, description: String) -> Self {
        Self {
            entity_type,
            description: Some(description),
            attributes: Vec::new(),
        }
    }

    /// Create a new entity type registration command with attributes
    ///
    /// # Arguments
    ///
    /// * `entity_type` - The fully qualified name of the entity to register
    /// * `attributes` - Attributes of this entity type
    pub fn with_attributes(entity_type: String, attributes: Vec<EntityAttribute>) -> Self {
        Self {
            entity_type,
            description: None,
            attributes,
        }
    }

    /// Create a complete entity type registration command
    ///
    /// # Arguments
    ///
    /// * `entity_type` - The fully qualified name of the entity to register
    /// * `description` - A description of what this entity represents
    /// * `attributes` - Attributes of this entity type
    pub fn complete(
        entity_type: String,
        description: String,
        attributes: Vec<EntityAttribute>,
    ) -> Self {
        Self {
            entity_type,
            description: Some(description),
            attributes,
        }
    }

    /// Build the command from a Rust type that describes the entity
    ///
    /// This is the bridge between the typed domain and the command interface:
    /// the type name, the attribute names and the attribute types are read from
    /// the type itself, so the caller cannot register a type whose schema does
    /// not match the domain.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The entity type to describe, must implement `HodeiEntityType`
    pub fn from_type<T: HodeiEntityType>() -> Self {
        let attributes = T::attributes_schema()
            .iter()
            .map(|(name, attribute_type)| EntityAttribute {
                name: name.as_str().to_string(),
                attribute_type: to_cedar_type(attribute_type),
            })
            .collect();

        Self {
            entity_type: T::entity_type_name(),
            description: None,
            attributes,
        }
    }
}

impl EntityAttribute {
    /// Create a new entity attribute
    pub fn new(name: String, attribute_type: String) -> Self {
        Self {
            name,
            attribute_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::domain::{AttributeName, AttributeType, ResourceTypeName, ServiceName};

    #[derive(Debug)]
    struct TestUser;

    impl HodeiEntityType for TestUser {
        fn service_name() -> ServiceName {
            ServiceName::new("iam").expect("valid service name")
        }

        fn resource_type_name() -> ResourceTypeName {
            ResourceTypeName::new("User").expect("valid resource type")
        }

        fn is_principal_type() -> bool {
            true
        }

        fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
            vec![
                (
                    AttributeName::new("name").expect("valid name"),
                    AttributeType::string(),
                ),
                (
                    AttributeName::new("tags").expect("valid name"),
                    AttributeType::set(AttributeType::string()),
                ),
            ]
        }
    }

    #[test]
    fn test_new_command() {
        let command = RegisterEntityTypeCommand::new("Iam::User".to_string());
        assert_eq!(command.entity_type, "Iam::User");
        assert!(command.description.is_none());
        assert!(command.attributes.is_empty());
    }

    #[test]
    fn test_command_with_description() {
        let command = RegisterEntityTypeCommand::with_description(
            "Iam::User".to_string(),
            "Represents a user in the system".to_string(),
        );
        assert_eq!(command.entity_type, "Iam::User");
        assert_eq!(
            command.description,
            Some("Represents a user in the system".to_string())
        );
        assert!(command.attributes.is_empty());
    }

    #[test]
    fn test_command_with_attributes() {
        let attributes = vec![
            EntityAttribute::new("email".to_string(), "String".to_string()),
            EntityAttribute::new("age".to_string(), "Long".to_string()),
        ];

        let command =
            RegisterEntityTypeCommand::with_attributes("Iam::User".to_string(), attributes);
        assert_eq!(command.entity_type, "Iam::User");
        assert!(command.description.is_none());
        assert_eq!(command.attributes.len(), 2);
    }

    #[test]
    fn test_complete_command() {
        let attributes = vec![EntityAttribute::new(
            "email".to_string(),
            "String".to_string(),
        )];

        let command = RegisterEntityTypeCommand::complete(
            "Iam::User".to_string(),
            "A system user".to_string(),
            attributes,
        );
        assert_eq!(command.entity_type, "Iam::User");
        assert_eq!(command.description, Some("A system user".to_string()));
        assert_eq!(command.attributes.len(), 1);
    }

    #[test]
    fn test_entity_attribute() {
        let attr = EntityAttribute::new("email".to_string(), "String".to_string());
        assert_eq!(attr.name, "email");
        assert_eq!(attr.attribute_type, "String");
    }

    #[test]
    fn from_type_reads_the_name_and_the_attributes_from_the_type() {
        let command = RegisterEntityTypeCommand::from_type::<TestUser>();

        assert_eq!(command.entity_type, "Iam::User");
        assert_eq!(command.attributes.len(), 2);
        assert_eq!(command.attributes[0].name, "name");
        assert_eq!(command.attributes[0].attribute_type, "String");
        assert_eq!(command.attributes[1].name, "tags");
        assert_eq!(command.attributes[1].attribute_type, "Set<String>");
    }
}
